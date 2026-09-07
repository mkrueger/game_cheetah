//! Bounded, read-only reverse pointer search. Candidates are possibilities,
//! never proof of an object's type or persistence across game versions.
use std::{
    collections::HashSet,
    ops::Range,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use crossbeam_channel::{Receiver, TryRecvError};
use i18n_embed_fl::fl;
use process_memory::{CopyAddress, TryIntoProcessHandle};
use serde::{Deserialize, Serialize};

use crate::{AddressSpec, ModuleCatalog, PointerWidth, SearchResult, SearchType, search_task::SearchTask};

const MAX_POINTERS: usize = 4_000_000;
const MAX_NODES: usize = 50_000;
const MAX_EDGES: usize = 2_000_000;
const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanOptions {
    pub pointer_width: PointerWidth,
    pub depth: usize,
    pub max_offset: usize,
    pub scan_mib: usize,
    pub max_candidates: usize,
    pub aligned_only: bool,
    pub include_readonly: bool,
    pub negative_offsets: bool,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            pointer_width: PointerWidth::Bits64,
            depth: 4,
            max_offset: 0x1000,
            scan_mib: 512,
            max_candidates: 256,
            aligned_only: true,
            include_readonly: false,
            negative_offsets: false,
        }
    }
}

impl ScanOptions {
    pub fn validate(&self) -> Result<(), String> {
        if !(1..=6).contains(&self.depth) || self.max_offset > 0x10000 || !(1..=1024).contains(&self.scan_mib) || !(1..=512).contains(&self.max_candidates) {
            return Err(fl!(crate::LANGUAGE_LOADER, "pointer-scan-options-error"));
        }
        Ok(())
    }
}

#[derive(Default)]
pub struct ScanProgress {
    pub bytes: AtomicUsize,
    pub pointers: AtomicUsize,
    pub depth: AtomicUsize,
    pub candidates: AtomicUsize,
}

#[derive(Debug, Default)]
pub struct ScanReport {
    pub candidates: Vec<AddressSpec>,
    pub limited: bool,
    pub failed_reads: usize,
}

pub struct ScanInput<'a> {
    pub scan_regions: &'a [Range<usize>],
    pub readable_regions: &'a [Range<usize>],
    pub modules: &'a ModuleCatalog,
    pub target: SearchResult,
    pub options: &'a ScanOptions,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Offset {
    negative: bool,
    magnitude: usize,
}

impl Offset {
    fn text(self) -> String {
        format!("{}0x{:X}", if self.negative { "-" } else { "" }, self.magnitude)
    }
}

struct Node {
    location: usize,
    offsets: Vec<Offset>,
    visited: Vec<usize>,
}

fn cancelled(stopped: &impl Fn() -> bool) -> Result<(), String> {
    if stopped() {
        Err(fl!(crate::LANGUAGE_LOADER, "pointer-scan-cancelled"))
    } else {
        Ok(())
    }
}

/// The reader must reject partial reads. Production uses ExactProcessReader.
/// Readable regions must be sorted, non-overlapping process mappings.
/// Scan regions may be prioritized independently of their address order.
pub fn scan_with_reader<T: CopyAddress>(reader: &T, input: ScanInput<'_>, progress: &ScanProgress, stopped: impl Fn() -> bool) -> Result<ScanReport, String> {
    let options = input.options;
    options.validate()?;
    cancelled(&stopped)?;
    let value_width = input
        .target
        .search_type
        .fixed_byte_length()
        .ok_or_else(|| fl!(crate::LANGUAGE_LOADER, "pointer-scan-numeric-only"))?;
    if options.pointer_width == PointerWidth::Bits32 && u32::try_from(input.target.addr).is_err() {
        return Err(fl!(crate::LANGUAGE_LOADER, "pointer-overflow"));
    }
    reader
        .copy_address(input.target.addr, &mut vec![0; value_width])
        .map_err(|error| error.to_string())?;
    let width = options.pointer_width.bytes();
    let alignment = if options.aligned_only { width } else { 1 };
    let budget = options.scan_mib * 1024 * 1024;
    let mut pointers = Vec::<(usize, usize)>::new();
    let mut attempted = 0;
    let mut successful = 0;
    let mut report = ScanReport::default();
    // A small overlap also includes unaligned slots crossing a page boundary.
    let mut page = vec![0; 4096 + width - 1];
    'index: for region in input.scan_regions {
        let mut start = region.start;
        while start < region.end {
            cancelled(&stopped)?;
            if attempted >= budget || pointers.len() >= MAX_POINTERS {
                report.limited = true;
                break 'index;
            }
            let logical = (region.end - start).min(4096).min(budget - attempted);
            let len = logical.saturating_add(width - 1).min(region.end - start);
            attempted += logical;
            progress.bytes.store(attempted, Ordering::Relaxed);
            if reader.copy_address(start, &mut page[..len]).is_err() {
                report.failed_reads += 1;
                // Retry without overlap: an inaccessible next page must not
                // discard valid slots entirely contained in this page.
                if len == logical || reader.copy_address(start, &mut page[..logical]).is_err() {
                    start += logical;
                    continue;
                }
                page[logical..len].fill(0);
                // Never treat synthetic overlap bytes as a successful read.
                index_page(&page[..logical], start, logical, alignment, width, input.readable_regions, &mut pointers);
            } else {
                index_page(&page[..len], start, logical, alignment, width, input.readable_regions, &mut pointers);
            }
            successful += 1;
            progress.pointers.store(pointers.len(), Ordering::Relaxed);
            start += logical;
        }
    }
    cancelled(&stopped)?;
    if successful == 0 {
        return Err(fl!(crate::LANGUAGE_LOADER, "search-read-failed"));
    }
    // Bounded sort; cancellation resumes as soon as this library call returns.
    pointers.sort_unstable();
    cancelled(&stopped)?;
    let mut roots: Vec<_> = input
        .modules
        .modules
        .iter()
        .enumerate()
        .filter(|(_, module)| !module.ambiguous)
        .flat_map(|(i, module)| module.ranges.iter().map(move |range| (range.start, range.end, i)))
        .collect();
    roots.sort_unstable();
    let mut frontier = vec![Node {
        location: input.target.addr,
        offsets: Vec::new(),
        visited: vec![input.target.addr],
    }];
    let mut unique = HashSet::new();
    'levels: for depth in 1..=options.depth {
        progress.depth.store(depth, Ordering::Relaxed);
        let mut next = Vec::new();
        let mut edges = 0;
        'nodes: for node in frontier {
            cancelled(&stopped)?;
            let lower = node.location.saturating_sub(options.max_offset);
            let upper = if options.negative_offsets {
                node.location.saturating_add(options.max_offset)
            } else {
                node.location
            };
            let from = pointers.partition_point(|&(value, _)| value < lower);
            let to = pointers.partition_point(|&(value, _)| value <= upper);
            for &(value, location) in &pointers[from..to] {
                edges += 1;
                if edges % 256 == 0 {
                    cancelled(&stopped)?;
                }
                if edges > MAX_EDGES {
                    report.limited = true;
                    break 'nodes;
                }
                if node.visited.contains(&location) {
                    continue;
                }
                let mut offsets = Vec::with_capacity(depth);
                offsets.push(Offset {
                    negative: value > node.location,
                    magnitude: value.abs_diff(node.location),
                });
                offsets.extend_from_slice(&node.offsets);
                let root_index = roots.partition_point(|&(start, _, _)| start <= location);
                if root_index > 0 {
                    let (_, end, module_index) = roots[root_index - 1];
                    if location.checked_add(width).is_some_and(|end_of_pointer| end_of_pointer <= end) {
                        let module = &input.modules.modules[module_index];
                        if let Some(offset) = location.checked_sub(module.base) {
                            let spec = AddressSpec::Pointer {
                                module: module.path.clone(),
                                offset: format!("0x{offset:X}"),
                                offsets: offsets.iter().map(|offset| offset.text()).collect(),
                                pointer_width: options.pointer_width,
                            };
                            if unique.insert((module_index, offset, offsets.clone()))
                                && input
                                    .modules
                                    .resolve_with_reader(&spec, value_width, reader)
                                    .is_ok_and(|resolved| resolved.address == input.target.addr)
                            {
                                report.candidates.push(spec);
                                progress.candidates.store(report.candidates.len(), Ordering::Relaxed);
                                if report.candidates.len() >= options.max_candidates {
                                    report.limited = true;
                                    break 'levels;
                                }
                            }
                        }
                    }
                }
                // Retain different suffixes to the same intermediate location.
                // Collapsing by location alone loses valid alternative chains.
                if depth < options.depth {
                    if next.len() < MAX_NODES {
                        let mut visited = node.visited.clone();
                        visited.push(location);
                        next.push(Node { location, offsets, visited });
                    } else {
                        report.limited = true;
                    }
                }
            }
        }
        if next.is_empty() {
            break;
        }
        frontier = next;
    }
    cancelled(&stopped)?;
    Ok(report)
}

fn index_page(bytes: &[u8], start: usize, logical: usize, alignment: usize, width: usize, regions: &[Range<usize>], pointers: &mut Vec<(usize, usize)>) {
    let offset = (alignment - start % alignment) % alignment;
    for i in (offset..logical.min(bytes.len().saturating_sub(width - 1))).step_by(alignment) {
        if pointers.len() >= MAX_POINTERS {
            break;
        }
        let raw = if width == 4 {
            u64::from(u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap()))
        } else {
            u64::from_le_bytes(bytes[i..i + 8].try_into().unwrap())
        };
        let Ok(value) = usize::try_from(raw) else { continue };
        let index = regions.partition_point(|region| region.start <= value);
        if value != 0 && index > 0 && value < regions[index - 1].end {
            pointers.push((value, start + i));
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessIdentity {
    pub pid: process_memory::Pid,
    pub start_time: u64,
    pub executable: String,
}

impl ProcessIdentity {
    pub fn capture(pid: process_memory::Pid) -> Result<Self, String> {
        let mut system = sysinfo::System::new();
        let sys_pid = sysinfo::Pid::from(pid as usize);
        system.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[sys_pid]), true);
        let process = system
            .process(sys_pid)
            .filter(|p| !matches!(p.status(), sysinfo::ProcessStatus::Zombie | sysinfo::ProcessStatus::Dead))
            .ok_or_else(|| fl!(crate::LANGUAGE_LOADER, "address-no-process"))?;
        let executable = process
            .exe()
            .and_then(|path| path.to_str())
            .filter(|path| !path.is_empty())
            .ok_or_else(|| fl!(crate::LANGUAGE_LOADER, "pointer-scan-identity-error"))?
            .to_owned();
        Ok(Self {
            pid,
            start_time: process.start_time(),
            executable,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateSet {
    pub version: u32,
    pub executable: String,
    pub value_type: SearchType,
    pub options: ScanOptions,
    pub candidates: Vec<AddressSpec>,
    #[serde(default)]
    pub limited: bool,
}

impl CandidateSet {
    pub fn validate(&self) -> Result<(), String> {
        self.options.validate()?;
        if self.version != 1 || self.executable.is_empty() || self.value_type.fixed_byte_length().is_none() || self.candidates.len() > 512 {
            return Err(fl!(crate::LANGUAGE_LOADER, "pointer-scan-file-error"));
        }
        for spec in &self.candidates {
            spec.validate()?;
            if !spec.is_pointer() {
                return Err(fl!(crate::LANGUAGE_LOADER, "pointer-scan-file-error"));
            }
        }
        Ok(())
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        self.validate()?;
        let text = toml::to_string_pretty(self).map_err(|error| error.to_string())?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        std::fs::write(path, text).map_err(|error| error.to_string())
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        use std::io::Read;
        let file = std::fs::File::open(path).map_err(|error| error.to_string())?;
        let mut text = String::new();
        file.take(MAX_FILE_BYTES + 1).read_to_string(&mut text).map_err(|error| error.to_string())?;
        if text.len() as u64 > MAX_FILE_BYTES {
            return Err(fl!(crate::LANGUAGE_LOADER, "pointer-scan-file-error"));
        }
        let set: Self = toml::from_str(&text).map_err(|error| error.to_string())?;
        set.validate()?;
        Ok(set)
    }
}

pub struct ScanJob {
    _lifetime: SearchTask,
    receiver: Receiver<Result<ScanReport, String>>,
    pub progress: Arc<ScanProgress>,
}

impl ScanJob {
    /// Dropping a job cancels it, discards late results, and retains the old
    /// candidate set owned by the UI. Workers never mutate search tabs.
    pub fn start(identity: ProcessIdentity, target: SearchResult, options: ScanOptions, previous: Option<CandidateSet>) -> Result<Self, String> {
        options.validate()?;
        if target.search_type.fixed_byte_length().is_none() {
            return Err(fl!(crate::LANGUAGE_LOADER, "pointer-scan-numeric-only"));
        }
        if let Some(set) = &previous {
            set.validate()?;
            if set.executable != identity.executable || set.value_type != target.search_type {
                return Err(fl!(crate::LANGUAGE_LOADER, "pointer-scan-wrong-target"));
            }
        }
        let lifetime = SearchTask::new(identity.pid, identity.start_time, identity.executable.clone(), Arc::default());
        let worker = lifetime.worker.clone();
        let progress = Arc::new(ScanProgress::default());
        let thread_progress = progress.clone();
        let (sender, receiver) = crossbeam_channel::bounded(1);
        std::thread::spawn(move || {
            let result = (|| {
                if ProcessIdentity::capture(identity.pid)? != identity {
                    return Err(fl!(crate::LANGUAGE_LOADER, "pointer-scan-wrong-target"));
                }
                let handle = identity.pid.try_into_process_handle().map_err(|error| error.to_string())?;
                let reader = worker.reader(crate::state::memory_reader::ExactProcessReader(&handle));
                let modules = ModuleCatalog::for_process(identity.pid)?;
                reader
                    .copy_address(target.addr, &mut vec![0; target.search_type.fixed_byte_length().unwrap()])
                    .map_err(|error| error.to_string())?;
                let mut report = if let Some(previous) = previous {
                    let mut report = ScanReport {
                        limited: previous.limited,
                        ..Default::default()
                    };
                    for spec in previous.candidates {
                        cancelled(&|| worker.stopped())?;
                        if modules
                            .resolve_with_reader(&spec, target.search_type.fixed_byte_length().unwrap(), &reader)
                            .is_ok_and(|resolved| resolved.address == target.addr)
                        {
                            report.candidates.push(spec);
                            thread_progress.candidates.store(report.candidates.len(), Ordering::Relaxed);
                        }
                    }
                    report
                } else {
                    let maps = proc_maps::get_process_maps(identity.pid).map_err(|error| error.to_string())?;
                    let mut readable: Vec<_> = maps
                        .iter()
                        .filter(|map| map.is_read())
                        .map(|map| map.start()..map.start().saturating_add(map.size()))
                        .collect();
                    let mut scan_regions: Vec<_> = maps
                        .iter()
                        .filter(|map| map.is_read() && (options.include_readonly || map.is_write()))
                        .filter(|map| {
                            !map.filename()
                                .is_some_and(|path| path.starts_with("/dev") || path.to_string_lossy().ends_with(" (deleted)"))
                        })
                        .map(|map| map.start()..map.start().saturating_add(map.size()))
                        .collect();
                    readable.sort_by_key(|range| range.start);
                    // A large low-address heap must not exhaust the budget
                    // before small globals/stacks (often essential links).
                    scan_regions.sort_by_key(|range| (range.len(), range.start));
                    let last_check = std::cell::Cell::new(std::time::Instant::now());
                    // Process identity is checked at both ends. Per-read cancellation
                    // stops immediately when the UI detects exit or switches process.
                    let result = scan_with_reader(
                        &reader,
                        ScanInput {
                            scan_regions: &scan_regions,
                            readable_regions: &readable,
                            modules: &modules,
                            target,
                            options: &options,
                        },
                        &thread_progress,
                        || {
                            if last_check.get().elapsed() >= std::time::Duration::from_secs(1) {
                                last_check.set(std::time::Instant::now());
                                worker.check_process();
                            }
                            worker.stopped()
                        },
                    );
                    result?
                };
                if ProcessIdentity::capture(identity.pid)? != identity {
                    return Err(fl!(crate::LANGUAGE_LOADER, "pointer-scan-wrong-target"));
                }
                // Refresh mappings and every candidate once more before publishing.
                let fresh = ModuleCatalog::for_process(identity.pid)?;
                report.candidates.retain(|spec| {
                    !worker.stopped()
                        && fresh
                            .resolve_with_reader(spec, target.search_type.fixed_byte_length().unwrap(), &reader)
                            .is_ok_and(|resolved| resolved.address == target.addr)
                });
                cancelled(&|| worker.stopped())?;
                Ok(report)
            })();
            let result = if let Some(error) = worker.error() { Err(error.to_string()) } else { result };
            crossbeam_channel::select! {
                recv(worker.cancel) -> _ => {},
                send(sender, result) -> _ => {},
            }
        });
        Ok(Self {
            _lifetime: lifetime,
            receiver,
            progress,
        })
    }

    pub fn poll(&self) -> Option<Result<ScanReport, String>> {
        match self.receiver.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err(fl!(crate::LANGUAGE_LOADER, "pointer-scan-worker-error"))),
        }
    }

    /// Blocking counterpart for command-line diagnostics, never used by the UI.
    pub fn wait(self) -> Result<ScanReport, String> {
        self.receiver.recv().map_err(|_| fl!(crate::LANGUAGE_LOADER, "pointer-scan-worker-error"))?
    }
}
