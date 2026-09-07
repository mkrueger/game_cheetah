// These fixtures intentionally pass slices containing one address range.
#![allow(clippy::single_range_in_vec_init)]

use std::{
    cell::Cell,
    io,
    ops::Range,
    sync::atomic::{AtomicUsize, Ordering},
};

use game_cheetah::{
    AddressSpec, LoadedModule, ModuleCatalog, PointerWidth, SearchResult, SearchType,
    pointer_scan::{CandidateSet, ScanInput, ScanOptions, ScanProgress, ScanReport, scan_with_reader},
};
use process_memory::{Architecture, CopyAddress};

struct Memory {
    bytes: Vec<u8>,
    fail_from: usize,
    reads: Cell<usize>,
}

impl Memory {
    fn new() -> Self {
        Self {
            bytes: vec![0; 0x6000],
            fail_from: usize::MAX,
            reads: Cell::new(0),
        }
    }
    fn pointer(&mut self, location: usize, value: u64, width: PointerWidth) {
        self.bytes[location..location + width.bytes()].copy_from_slice(&value.to_le_bytes()[..width.bytes()]);
    }
}

impl CopyAddress for Memory {
    fn get_pointer_width(&self) -> Architecture {
        Architecture::Arch32Bit
    }
    fn copy_address(&self, address: usize, bytes: &mut [u8]) -> io::Result<()> {
        self.reads.set(self.reads.get() + 1);
        let end = address.checked_add(bytes.len()).ok_or(io::ErrorKind::UnexpectedEof)?;
        if end > self.fail_from {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        bytes.copy_from_slice(self.bytes.get(address..end).ok_or(io::ErrorKind::UnexpectedEof)?);
        Ok(())
    }
}

fn modules() -> ModuleCatalog {
    ModuleCatalog {
        modules: vec![LoadedModule {
            path: "game.exe".into(),
            base: 0x1000,
            ranges: std::iter::once(0x1000..0x3000).collect(),
            ambiguous: false,
        }],
    }
}

fn scan(memory: &Memory, options: &ScanOptions, regions: &[Range<usize>], target: usize) -> ScanReport {
    scan_with_reader(
        memory,
        ScanInput {
            scan_regions: regions,
            readable_regions: &[0x1000..0x6000],
            modules: &modules(),
            target: SearchResult::new(target, SearchType::Int),
            options,
        },
        &ScanProgress::default(),
        || false,
    )
    .unwrap()
}

fn expected(width: PointerWidth, root: &str, offsets: &[&str]) -> AddressSpec {
    AddressSpec::Pointer {
        module: "game.exe".into(),
        offset: root.into(),
        offsets: offsets.iter().map(|s| (*s).into()).collect(),
        pointer_width: width,
    }
}

#[test]
fn discovers_both_target_widths_and_orders_offsets_from_root() {
    for width in [PointerWidth::Bits32, PointerWidth::Bits64] {
        let mut memory = Memory::new();
        memory.pointer(0x1020, 0x4000, width);
        memory.pointer(0x4010, 0x5000, width);
        let options = ScanOptions {
            pointer_width: width,
            depth: 2,
            max_offset: 0x100,
            ..Default::default()
        };
        let before = memory.bytes.clone();
        let report = scan(&memory, &options, &[0x1000..0x6000], 0x5080);
        assert!(report.candidates.contains(&expected(width, "0x20", &["0x10", "0x80"])));
        assert!(!report.limited);
        assert_eq!(memory.bytes, before);
    }
}

#[test]
fn keeps_alternative_suffixes_through_the_same_root() {
    let mut memory = Memory::new();
    let width = PointerWidth::Bits64;
    memory.pointer(0x1020, 0x4000, width);
    memory.pointer(0x4010, 0x5000, width);
    memory.pointer(0x4020, 0x5000, width);
    let options = ScanOptions {
        depth: 2,
        max_offset: 0x80,
        ..Default::default()
    };
    let report = scan(&memory, &options, &[0x1000..0x6000], 0x5080);
    assert!(report.candidates.contains(&expected(width, "0x20", &["0x10", "0x80"])));
    assert!(report.candidates.contains(&expected(width, "0x20", &["0x20", "0x80"])));
}

#[test]
fn unaligned_pointer_crossing_a_page_is_found_only_when_requested() {
    let mut memory = Memory::new();
    memory.pointer(0x1ffd, 0x5000, PointerWidth::Bits64);
    let mut options = ScanOptions {
        depth: 1,
        max_offset: 0,
        ..Default::default()
    };
    assert!(scan(&memory, &options, &[0x1000..0x3000], 0x5000).candidates.is_empty());
    options.aligned_only = false;
    assert!(
        scan(&memory, &options, &[0x1000..0x3000], 0x5000)
            .candidates
            .contains(&expected(PointerWidth::Bits64, "0xFFD", &["0x0"]))
    );
}

#[test]
fn negative_offsets_are_opt_in_and_null_pointers_do_not_create_candidates() {
    let mut memory = Memory::new();
    memory.pointer(0x1020, 0x5080, PointerWidth::Bits64);
    let mut options = ScanOptions {
        depth: 1,
        max_offset: 0x80,
        ..Default::default()
    };
    assert!(scan(&memory, &options, &[0x1000..0x3000], 0x5000).candidates.is_empty());
    options.negative_offsets = true;
    assert_eq!(
        scan(&memory, &options, &[0x1000..0x3000], 0x5000).candidates,
        vec![expected(PointerWidth::Bits64, "0x20", &["-0x80"])]
    );
    memory.pointer(0x1020, 0, PointerWidth::Bits64);
    assert!(scan(&memory, &options, &[0x1000..0x3000], 0x5000).candidates.is_empty());
}

#[test]
fn limits_are_reported_and_cancellation_discards_partial_results() {
    let mut memory = Memory::new();
    memory.pointer(0x1020, 0x5000, PointerWidth::Bits64);
    memory.pointer(0x1040, 0x5000, PointerWidth::Bits64);
    let options = ScanOptions {
        depth: 1,
        max_offset: 0,
        max_candidates: 1,
        ..Default::default()
    };
    let report = scan(&memory, &options, &[0x1000..0x3000], 0x5000);
    assert!(report.limited);
    assert_eq!(report.candidates.len(), 1);
    memory.reads.set(0);
    let result = scan_with_reader(
        &memory,
        ScanInput {
            scan_regions: &[0x1000..0x6000],
            readable_regions: &[0x1000..0x6000],
            modules: &modules(),
            target: SearchResult::new(0x5000, SearchType::Int),
            options: &options,
        },
        &ScanProgress::default(),
        || memory.reads.get() >= 2,
    );
    assert!(result.is_err());
    assert_eq!(memory.reads.get(), 2);
}

#[test]
fn short_reads_fail_closed_without_indexing_unread_bytes() {
    let mut memory = Memory::new();
    memory.pointer(0x1020, 0x1800, PointerWidth::Bits64);
    memory.fail_from = 0x2000;
    let options = ScanOptions {
        depth: 1,
        max_offset: 0,
        ..Default::default()
    };
    let report = scan(&memory, &options, &[0x1000..0x3000], 0x1800);
    assert!(report.failed_reads > 0);
    assert_eq!(report.candidates, vec![expected(PointerWidth::Bits64, "0x20", &["0x0"])]);
}

#[test]
fn invalid_limits_and_ambiguous_roots_are_rejected() {
    let memory = Memory::new();
    for options in [
        ScanOptions {
            depth: 0,
            ..Default::default()
        },
        ScanOptions {
            max_offset: 0x10001,
            ..Default::default()
        },
        ScanOptions {
            scan_mib: 0,
            ..Default::default()
        },
    ] {
        assert!(options.validate().is_err());
    }
    assert_eq!(memory.reads.get(), 0);
    let mut memory = memory;
    memory.pointer(0x1020, 0x5000, PointerWidth::Bits64);
    let mut modules = modules();
    modules.modules[0].ambiguous = true;
    let report = scan_with_reader(
        &memory,
        ScanInput {
            scan_regions: &[0x1000..0x3000],
            readable_regions: &[0x1000..0x6000],
            modules: &modules,
            target: SearchResult::new(0x5000, SearchType::Int),
            options: &ScanOptions::default(),
        },
        &ScanProgress::default(),
        || false,
    )
    .unwrap();
    assert!(report.candidates.is_empty());
}

#[test]
fn byte_budget_is_enforced_even_when_no_pointers_are_found() {
    struct ZeroMemory;
    impl CopyAddress for ZeroMemory {
        fn get_pointer_width(&self) -> Architecture {
            Architecture::Arch64Bit
        }
        fn copy_address(&self, _: usize, bytes: &mut [u8]) -> io::Result<()> {
            bytes.fill(0);
            Ok(())
        }
    }
    let options = ScanOptions {
        scan_mib: 1,
        ..Default::default()
    };
    let progress = ScanProgress::default();
    let report = scan_with_reader(
        &ZeroMemory,
        ScanInput {
            scan_regions: &[0x1000..0x201000],
            readable_regions: &[0x1000..0x201000],
            modules: &modules(),
            target: SearchResult::new(0x5000, SearchType::Int),
            options: &options,
        },
        &progress,
        || false,
    )
    .unwrap();
    assert!(report.limited);
    assert!(report.candidates.is_empty());
    assert_eq!(progress.bytes.load(Ordering::Relaxed), 1024 * 1024);
}

#[cfg(target_pointer_width = "64")]
#[test]
fn scanner_preserves_addresses_above_four_gib() {
    struct HighMemory(Memory);
    const BASE: usize = 0x1_0000_0000;
    impl CopyAddress for HighMemory {
        fn get_pointer_width(&self) -> Architecture {
            Architecture::Arch32Bit
        }
        fn copy_address(&self, address: usize, bytes: &mut [u8]) -> io::Result<()> {
            self.0.copy_address(address.checked_sub(BASE).ok_or(io::ErrorKind::InvalidInput)?, bytes)
        }
    }
    let mut memory = Memory::new();
    memory.pointer(0x1020, (BASE + 0x5000) as u64, PointerWidth::Bits64);
    let mut catalog = modules();
    catalog.modules[0].base += BASE;
    catalog.modules[0].ranges = vec![BASE + 0x1000..BASE + 0x3000];
    let report = scan_with_reader(
        &HighMemory(memory),
        ScanInput {
            scan_regions: &[BASE + 0x1000..BASE + 0x3000],
            readable_regions: &[BASE + 0x1000..BASE + 0x6000],
            modules: &catalog,
            target: SearchResult::new(BASE + 0x5000, SearchType::Int),
            options: &ScanOptions {
                depth: 1,
                max_offset: 0,
                ..Default::default()
            },
        },
        &ScanProgress::default(),
        || false,
    )
    .unwrap();
    assert_eq!(report.candidates, vec![expected(PointerWidth::Bits64, "0x20", &["0x0"])]);
}

#[test]
fn candidate_file_round_trip_preserves_limits_and_rejects_non_pointer_entries() {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let path = std::env::temp_dir().join(format!(
        "pointer-candidates-{}-{}.toml",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let mut set = CandidateSet {
        version: 1,
        executable: "/games/game.exe".into(),
        value_type: SearchType::Int,
        options: ScanOptions::default(),
        candidates: vec![expected(PointerWidth::Bits64, "0x20", &["0x10", "0x80"])],
        limited: true,
    };
    set.save(&path).unwrap();
    let loaded = CandidateSet::load(&path).unwrap();
    assert_eq!(loaded.candidates, set.candidates);
    assert!(loaded.limited);
    assert_eq!(loaded.executable, set.executable);
    set.candidates = vec![AddressSpec::absolute(0x5000)];
    assert!(set.validate().is_err());
    std::fs::write(&path, toml::to_string(&set).unwrap()).unwrap();
    assert!(CandidateSet::load(&path).is_err());
    std::fs::remove_file(path).unwrap();
}

#[cfg(target_os = "linux")]
mod live {
    use super::*;
    use game_cheetah::{
        App,
        pointer_scan::{ProcessIdentity, ScanJob},
    };
    use std::{
        sync::Mutex,
        time::{Duration, Instant},
    };

    static LOCK: Mutex<()> = Mutex::new(());
    static ROOT: AtomicUsize = AtomicUsize::new(1);

    fn wait(job: &ScanJob) -> Result<ScanReport, String> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(result) = job.poll() {
                return result;
            }
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
    }

    #[test]
    fn rescan_matches_new_address_not_value_and_adoption_never_writes_or_freezes() {
        let _lock = LOCK.lock().unwrap();
        let old = Box::new(12345_i32);
        let new = Box::new(12345_i32);
        let identity = ProcessIdentity::capture(std::process::id() as _).unwrap();
        ROOT.store(&*old as *const i32 as usize, Ordering::Release);
        let catalog = ModuleCatalog::for_process(identity.pid).unwrap();
        let AddressSpec::Module { module, offset } = catalog.suggest(&SearchResult::new(&ROOT as *const _ as usize, SearchType::Int64)) else {
            panic!("root must be module-backed")
        };
        let spec = AddressSpec::Pointer {
            module,
            offset,
            offsets: vec!["0x0".into()],
            pointer_width: PointerWidth::Bits64,
        };
        let set = CandidateSet {
            version: 1,
            executable: identity.executable.clone(),
            value_type: SearchType::Int,
            options: ScanOptions::default(),
            candidates: vec![spec.clone()],
            limited: true,
        };
        let target = SearchResult::new(&*new as *const i32 as usize, SearchType::Int);
        // Same value, wrong object: do not retain.
        assert!(
            wait(&ScanJob::start(identity.clone(), target, ScanOptions::default(), Some(set.clone())).unwrap())
                .unwrap()
                .candidates
                .is_empty()
        );
        ROOT.store(target.addr, Ordering::Release);
        let fresh = wait(
            &ScanJob::start(
                identity.clone(),
                target,
                ScanOptions {
                    depth: 1,
                    max_offset: 0,
                    scan_mib: 1,
                    ..Default::default()
                },
                None,
            )
            .unwrap(),
        )
        .unwrap();
        assert!(fresh.candidates.contains(&spec));
        let report = wait(&ScanJob::start(identity.clone(), target, ScanOptions::default(), Some(set.clone())).unwrap()).unwrap();
        assert_eq!(report.candidates, vec![spec.clone()]);
        assert!(report.limited);
        let mut app = App::default();
        app.set_persistence_enabled(true);
        app.state.pid = identity.pid;
        app.state.searches[0].set_cached_results(vec![target]);
        app.begin_pointer_scan(0);
        app.pointer_scanner.candidates = Some(set.clone());
        app.start_pointer_scan(true).unwrap();
        app.pointer_scanner.cancel();
        app.poll_pointer_scan();
        assert!(app.pointer_scanner.job.is_none());
        assert_eq!(app.pointer_scanner.candidates.as_ref().unwrap().candidates, set.candidates);
        app.start_pointer_scan(true).unwrap();
        app.state.pid = 0;
        app.poll_pointer_scan();
        assert!(app.pointer_scanner.job.is_none());
        assert!(app.pointer_scanner.target.is_none());
        assert_eq!(app.pointer_scanner.candidates.as_ref().unwrap().candidates, set.candidates);
        app.state.pid = identity.pid;
        app.begin_pointer_scan(0);
        let mut wrong = set.clone();
        wrong.executable = "/other/game".into();
        assert!(ScanJob::start(identity.clone(), target, ScanOptions::default(), Some(wrong)).is_err());
        let mut recycled = identity;
        recycled.start_time += 1;
        assert!(wait(&ScanJob::start(recycled, target, ScanOptions::default(), Some(set)).unwrap()).is_err());
        ROOT.store(&*old as *const i32 as usize, Ordering::Release);
        assert!(app.adopt_pointer_candidate(0).is_err());
        assert_eq!(app.state.searches.len(), 1);
        ROOT.store(target.addr, Ordering::Release);
        app.adopt_pointer_candidate(0).unwrap();
        assert_eq!(app.state.searches.len(), 2);
        assert_eq!(app.state.searches[1].address_overrides[&(target.addr, SearchType::Int)], spec);
        assert!(app.state.searches.iter().all(|s| s.freezed_addresses.is_empty()));
        assert_eq!((*old, *new), (12345, 12345));
        ROOT.store(0, Ordering::Release);
    }
}
