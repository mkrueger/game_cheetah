use crate::{FreezeMessage, MessageCommand, SearchContext, SearchMode, SearchResult, SearchType, SearchValue, UnknownComparison};
use crossbeam_channel::{select, tick};
use i18n_embed_fl::fl;
use memchr::memmem;
use once_cell::sync::Lazy;
use proc_maps::get_process_maps;
use process_memory::{PutAddress, TryIntoProcessHandle, copy_address};
use rayon::prelude::*;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use std::{
    cmp::min,
    collections::HashMap,
    thread,
    time::{Duration, SystemTime},
};
use sysinfo::*;

/// Process memory reader using /proc/[pid]/mem for zero-copy reads on Linux.
/// This is more efficient than process_vm_readv for multiple reads as we keep the file open.
#[cfg(target_os = "linux")]
pub struct ProcessMemReader {
    file: std::fs::File,
}

#[cfg(target_os = "linux")]
impl ProcessMemReader {
    pub fn new(pid: process_memory::Pid) -> std::io::Result<Self> {
        use std::fs::OpenOptions;
        let path = format!("/proc/{}/mem", pid);
        let file = OpenOptions::new().read(true).open(&path)?;
        Ok(Self { file })
    }

    /// Read memory at the given address using pread (no seeking required, thread-safe)
    pub fn read_at(&self, address: usize, size: usize) -> std::io::Result<Vec<u8>> {
        use std::io::Error;
        use std::os::unix::io::AsRawFd;

        let mut buffer = vec![0u8; size];
        let fd = self.file.as_raw_fd();

        let result = unsafe { libc::pread(fd, buffer.as_mut_ptr() as *mut libc::c_void, size, address as libc::off_t) };

        if result == -1 {
            Err(Error::last_os_error())
        } else if (result as usize) < size {
            buffer.truncate(result as usize);
            Ok(buffer)
        } else {
            Ok(buffer)
        }
    }

    /// Read directly into an existing buffer (avoids allocation)
    pub fn read_into(&self, address: usize, buffer: &mut [u8]) -> std::io::Result<usize> {
        use std::io::Error;
        use std::os::unix::io::AsRawFd;

        let fd = self.file.as_raw_fd();

        let result = unsafe { libc::pread(fd, buffer.as_mut_ptr() as *mut libc::c_void, buffer.len(), address as libc::off_t) };

        if result == -1 { Err(Error::last_os_error()) } else { Ok(result as usize) }
    }
}

/// Fast memory read using process_vm_readv on Linux (fastest method).
/// Falls back to /proc/[pid]/mem + pread on error.
#[cfg(target_os = "linux")]
fn fast_read_memory(pid: process_memory::Pid, address: usize, size: usize) -> Result<Vec<u8>, std::io::Error> {
    use std::fs::OpenOptions;
    use std::io::Error;
    use std::os::unix::io::AsRawFd;

    // Try process_vm_readv first (fastest based on benchmarks: ~670 MB/s vs ~540 MB/s)
    let mut buffer = vec![0u8; size];

    let local_iov = libc::iovec {
        iov_base: buffer.as_mut_ptr() as *mut libc::c_void,
        iov_len: size,
    };

    let remote_iov = libc::iovec {
        iov_base: address as *mut libc::c_void,
        iov_len: size,
    };

    let result = unsafe { libc::process_vm_readv(pid as libc::pid_t, &local_iov as *const libc::iovec, 1, &remote_iov as *const libc::iovec, 1, 0) };

    if result > 0 {
        if (result as usize) < size {
            buffer.truncate(result as usize);
        }
        return Ok(buffer);
    }

    // Fallback to /proc/[pid]/mem with pread
    let path = format!("/proc/{}/mem", pid);
    if let Ok(file) = OpenOptions::new().read(true).open(&path) {
        let mut buffer = vec![0u8; size];
        let fd = file.as_raw_fd();

        let result = unsafe { libc::pread(fd, buffer.as_mut_ptr() as *mut libc::c_void, size, address as libc::off_t) };

        if result > 0 {
            if (result as usize) < size {
                buffer.truncate(result as usize);
            }
            return Ok(buffer);
        }
    }

    // Final fallback to copy_address
    match pid.try_into_process_handle() {
        Ok(handle) => copy_address(address, size, &handle).map_err(|e| Error::other(e.to_string())),
        Err(e) => Err(Error::other(e.to_string())),
    }
}

#[cfg(not(target_os = "linux"))]
#[cfg(not(target_os = "linux"))]
fn fast_read_memory(pid: process_memory::Pid, address: usize, size: usize) -> Result<Vec<u8>, std::io::Error> {
    use std::io::{Error, ErrorKind};

    match pid.try_into_process_handle() {
        Ok(handle) => copy_address(address, size, &handle).map_err(|e| Error::new(ErrorKind::Other, e.to_string())),
        Err(e) => Err(Error::new(ErrorKind::Other, e.to_string())),
    }
}

#[derive(Debug, Clone)]
pub struct ProcessInfo {
    pub pid: process_memory::Pid,
    pub name: String,
    pub cmd: String,
    pub user: String,
    pub memory: usize,
}

pub struct GameCheetahEngine {
    pub pid: process_memory::Pid,
    pub process_name: String,
    pub show_process_window: bool,
    pub show_about_dialog: bool,

    pub process_filter: String,
    pub last_process_update: SystemTime,
    pub processes: Vec<ProcessInfo>,

    pub current_search: usize,
    pub searches: Vec<Box<SearchContext>>,
    // Removed: pub search_threads: ThreadPool,
    pub freeze_sender: crossbeam_channel::Sender<FreezeMessage>,
    pub error_text: String,
    pub show_results: bool,
    pub set_focus: bool,

    pub(crate) edit_address: usize,
}

impl Default for GameCheetahEngine {
    fn default() -> Self {
        let (tx, rx) = crossbeam_channel::unbounded::<FreezeMessage>();
        thread::spawn(move || {
            let mut freezed_values: HashMap<usize, SearchValue> = HashMap::new();
            let mut pid: i32 = 0;
            let ticker = tick(Duration::from_millis(125));
            loop {
                // Drain all pending messages quickly
                while let Ok(msg) = rx.try_recv() {
                    match msg.msg {
                        MessageCommand::Pid => {
                            pid = msg.addr as i32;
                            if pid == 0 {
                                freezed_values.clear();
                            }
                        }
                        MessageCommand::Freeze => {
                            freezed_values.insert(msg.addr, msg.value);
                        }
                        MessageCommand::Unfreeze => {
                            freezed_values.remove(&msg.addr);
                        }
                    }
                }
                // Wait either for next tick or a new message
                select! {
                    recv(ticker) -> _ => {
                        if pid != 0
                            && let Ok(handle) = (pid as process_memory::Pid).try_into_process_handle() {
                                for (addr, value) in &freezed_values {
                                    let _ = handle.put_address(*addr, &value.1);
                                }
                            }
                    },
                    recv(rx) -> msg => {
                        if let Ok(msg) = msg {
                            match msg.msg {
                                MessageCommand::Pid => {
                                    pid = msg.addr as i32;
                                    if pid == 0 { freezed_values.clear(); }
                                }
                                MessageCommand::Freeze => { freezed_values.insert(msg.addr, msg.value); }
                                MessageCommand::Unfreeze => { freezed_values.remove(&msg.addr); }
                            }
                        } else {
                            break; // channel closed
                        }
                    }
                }
            }
        });

        Self {
            pid: 0,
            process_name: "".to_owned(),
            error_text: String::new(),
            show_process_window: false,
            process_filter: "".to_owned(),
            processes: Vec::new(),
            last_process_update: SystemTime::now(),
            current_search: 0,
            searches: vec![Box::new(SearchContext::new(fl!(crate::LANGUAGE_LOADER, "first-search-label")))],
            // Removed: search_threads: ThreadPool::new(16),
            freeze_sender: tx,
            show_results: false,
            show_about_dialog: false,
            set_focus: true,
            edit_address: 0,
        }
    }
}

impl GameCheetahEngine {
    pub fn new_search(&mut self) {
        let ctx = SearchContext::new(
            fl!(crate::LANGUAGE_LOADER, "search-label", search = (1 + self.searches.len()).to_string())
                .chars()
                .filter(|c| c.is_ascii())
                .collect::<String>(),
        );
        self.current_search = self.searches.len();
        self.searches.push(Box::new(ctx));
    }

    pub fn initial_search(&mut self, search_index: usize) {
        // Validate index
        let Some(search_context) = self.searches.get(search_index) else {
            self.error_text = format!("Invalid search index {search_index}");
            return;
        };
        if !matches!(search_context.searching, SearchMode::None) {
            return;
        }

        // Remove freezes first
        self.remove_freezes(search_index);

        // Set searching state (need mutable borrow)
        if let Some(ctx_mut) = self.searches.get_mut(search_index) {
            ctx_mut.searching = SearchMode::Memory;
            ctx_mut.total_bytes = 0;
            ctx_mut.current_bytes.swap(0, Ordering::SeqCst);
        } else {
            self.error_text = format!("Invalid search index {search_index}");
            return;
        }

        // Extract needed data (release mutable borrow)
        let (search_type, search_value_text) = {
            let ctx = &self.searches[search_index];
            (ctx.search_type, ctx.search_value_text.clone())
        };

        if search_type == SearchType::String {
            self.error_text.clear();

            // Precompute overlaps for chunking so we don't miss boundary-crossing matches
            let utf8_len = search_value_text.len();
            let utf16_len = utf8_len.saturating_mul(2);
            // For UTF-8, need len-1 overlap; for UTF-16LE, need 2*len - 2 overlap (one u16 less)
            let overlap = std::cmp::max(utf8_len.saturating_sub(1), utf16_len.saturating_sub(2));

            match get_process_maps(self.pid) {
                Ok(maps) => {
                    let mut regions = Vec::new();
                    if let Some(ctx_mut) = self.searches.get_mut(search_index) {
                        for map in maps {
                            if skip_memory_region(&map) {
                                continue;
                            }

                            let mut size = map.size();
                            let mut start = map.start();
                            ctx_mut.total_bytes += size;

                            const MAX_BLOCK: usize = 50 * 1024 * 1024;
                            let chunk_plus = MAX_BLOCK.saturating_add(overlap);
                            while size > chunk_plus {
                                regions.push((start, chunk_plus));
                                start += MAX_BLOCK;
                                size = size.saturating_sub(MAX_BLOCK);
                            }
                            regions.push((start, size));
                        }
                    } else {
                        self.error_text = format!("Search context vanished for index {search_index}");
                        return;
                    }

                    self.spawn_string_search(search_value_text, regions, search_index);
                }
                Err(err) => {
                    eprintln!("error getting process maps for pid {}: {}", self.pid, err);
                    self.error_text = format!("Error getting process maps for pid {}: {}", self.pid, err);
                }
            }
            return;
        }

        // Parse target value
        let search_for_value = match search_type.from_string(&search_value_text) {
            Ok(v) => v,
            Err(e) => {
                self.error_text = format!("Parse error: {e}");
                return;
            }
        };
        self.error_text.clear();

        match get_process_maps(self.pid) {
            Ok(maps) => {
                let mut regions = Vec::new();
                if let Some(ctx_mut) = self.searches.get_mut(search_index) {
                    for map in maps {
                        if skip_memory_region(&map) {
                            continue;
                        }

                        let mut size = map.size();
                        let mut start = map.start();
                        ctx_mut.total_bytes += size;

                        const MAX_BLOCK: usize = 50 * 1024 * 1024;
                        while size > MAX_BLOCK + 7 {
                            regions.push((start, MAX_BLOCK + 7));
                            start += MAX_BLOCK;
                            size -= MAX_BLOCK;
                        }
                        regions.push((start, size));
                    }
                } else {
                    self.error_text = format!("Search context vanished for index {search_index}");
                    return;
                }

                self.spawn_parallel_search(search_for_value, regions, search_index);
            }
            Err(err) => {
                eprintln!("error getting process maps for pid {}: {}", self.pid, err);
                self.error_text = format!("Error getting process maps for pid {}: {}", self.pid, err);
            }
        }
    }

    pub fn filter_searches(&mut self, search_index: usize) {
        self.remove_freezes(search_index);
        let Some(search_context) = self.searches.get_mut(search_index) else {
            self.error_text = format!("Invalid search index {search_index}");
            return;
        };
        search_context.searching = SearchMode::Percent;

        // collect_results now returns Arc<Vec<SearchResult>>
        let old_results = search_context.collect_results();
        search_context.total_bytes = old_results.len();
        search_context.current_bytes.swap(0, Ordering::SeqCst);

        // Need to dereference Arc to clone the underlying Vec for old_results history
        search_context.old_results.push((*old_results).clone());

        let (tx, rx) = crossbeam_channel::unbounded();
        search_context.results_sender = tx;
        search_context.results_receiver = rx;
        search_context.invalidate_cache();

        let max_block = 200 * 1024;
        let chunks: Vec<(usize, usize)> = (0..old_results.len())
            .step_by(max_block)
            .map(|i| (i, min(i + max_block, old_results.len())))
            .collect();

        self.spawn_update_search(search_index, old_results, chunks);
    }

    pub fn remove_freezes(&mut self, search_index: usize) {
        if let Some(search_context) = self.searches.get_mut(search_index) {
            GameCheetahEngine::remove_freezes_from(&self.freeze_sender, &mut search_context.freezed_addresses);
        }
    }

    pub fn remove_freezes_from(freeze_sender: &crossbeam_channel::Sender<FreezeMessage>, freezes: &mut std::collections::HashSet<usize>) {
        for result in freezes.iter() {
            freeze_sender
                .send(FreezeMessage::from_addr(MessageCommand::Unfreeze, *result))
                .unwrap_or_default();
        }
        freezes.clear();
    }

    pub fn update_process_data(&mut self) {
        let sys = System::new_all();
        self.last_process_update = SystemTime::now();
        self.processes.clear();

        let Ok(current_pid) = get_current_pid() else {
            self.error_text = "Failed to get current pid".into();
            return;
        };
        let Some(cur_process) = sys.process(current_pid) else {
            self.error_text = "Current process info not found".into();
            return;
        };

        // Group processes by cmd to identify duplicates
        let mut process_groups: HashMap<String, Vec<(&Pid, &Process)>> = HashMap::new();

        for (pid, process) in sys.processes() {
            // Skip processes with no memory or different user
            if process.memory() == 0 || process.user_id() != cur_process.user_id() {
                continue;
            }

            // Skip kernel threads (they usually have no exe)
            if process.exe().is_none() || process.exe() == Some(std::path::Path::new("")) {
                continue;
            }

            // Group by cmd to identify process groups
            let key = if let Some(cmd) = process.cmd().first() {
                cmd.to_string_lossy().to_string()
            } else {
                continue;
            };
            process_groups.entry(key).or_default().push((pid, process));
        }

        // For each group, pick the best representative
        for (_cmd, mut group) in process_groups {
            if group.is_empty() {
                continue;
            }

            // Calculate total memory for the entire process group
            let largest_group_memory: u64 = group.iter().map(|(_, p)| p.memory()).max().unwrap_or(0);

            // Sort by criteria to pick the main process:
            // 1. Lowest PID in the group (usually the parent/main process)
            // 2. Highest memory usage (main process usually uses more)
            group.sort_by(|a, b| {
                // First by PID (ascending - older processes have lower PIDs)
                match a.0.as_u32().cmp(&b.0.as_u32()) {
                    std::cmp::Ordering::Equal => {
                        // Then by memory (descending)
                        b.1.memory().cmp(&a.1.memory())
                    }
                    other => other,
                }
            });

            // Take the first (best) process from the group
            if let Some((pid, process)) = group.first() {
                let pid_u32 = pid.as_u32();
                if let Ok(conv_pid) = pid_u32.try_into() {
                    let user = process.user_id().map(|u| u.to_string()).unwrap_or_default();

                    // Get the process name
                    let name = process.name().to_string_lossy().to_string();

                    // Build command line, and note if there are multiple instances
                    let instance_count = group.len();
                    let cmd = if instance_count > 1 {
                        format!("{:?} [{} processes]", process.cmd(), instance_count)
                    } else {
                        format!("{:?}", process.cmd())
                    };

                    self.processes.push(ProcessInfo {
                        pid: conv_pid,
                        name,
                        cmd,
                        user,
                        memory: largest_group_memory as usize, // Use total group memory instead
                    });
                }
            }
        }
    }

    fn spawn_update_search(&mut self, search_index: usize, old_results: Arc<Vec<SearchResult>>, chunks: Vec<(usize, usize)>) {
        let Some(search_context) = self.searches.get_mut(search_index) else {
            self.error_text = format!("Invalid search index {search_index}");
            return;
        };
        let current_bytes = search_context.current_bytes.clone();
        let pid = self.pid;
        let value_text = search_context.search_value_text.clone();
        let results_sender = search_context.results_sender.clone();
        let search_complete = search_context.search_complete.clone();
        let cache_valid = search_context.cache_valid.clone();

        search_complete.store(false, Ordering::SeqCst);

        std::thread::spawn(move || {
            chunks.par_iter().for_each(|(from, to)| {
                let handle = match pid.try_into_process_handle() {
                    Ok(h) => h,
                    Err(e) => {
                        eprintln!("Failed to get process handle: {e}");
                        current_bytes.fetch_add(to - from, Ordering::SeqCst);
                        return;
                    }
                };

                let chunk = &old_results[*from..*to];
                let updated = update_results(chunk, &value_text, &handle);

                if !updated.is_empty() {
                    let _ = results_sender.send(updated);
                }

                current_bytes.fetch_add(to - from, Ordering::SeqCst);
            });

            cache_valid.store(false, Ordering::Release);
            search_complete.store(true, Ordering::SeqCst);
        });
    }

    fn spawn_parallel_search(&mut self, search_data: SearchValue, regions: Vec<(usize, usize)>, search_index: usize) {
        let Some(search_context) = self.searches.get_mut(search_index) else {
            self.error_text = format!("Invalid search index {search_index}");
            return;
        };

        let current_bytes = search_context.current_bytes.clone();
        let pid = self.pid;
        let results_sender = search_context.results_sender.clone();
        let search_complete: Arc<std::sync::atomic::AtomicBool> = search_context.search_complete.clone();
        let cache_valid = search_context.cache_valid.clone();

        search_complete.store(false, Ordering::SeqCst);

        std::thread::spawn(move || {
            // Use thread-local ProcessMemReader for efficient repeated reads
            #[cfg(target_os = "linux")]
            thread_local! {
                static MEM_READER: std::cell::RefCell<Option<(process_memory::Pid, ProcessMemReader)>> = const { std::cell::RefCell::new(None) };
            }

            regions.par_iter().for_each(|(start, size)| {
                let value_text = String::from_utf8(search_data.1.clone()).unwrap_or_default();

                // Try to read memory using the most efficient method available
                #[cfg(target_os = "linux")]
                let memory_result = MEM_READER.with(|reader_cell| {
                    let mut reader_opt = reader_cell.borrow_mut();

                    // Check if we have a valid reader for this pid
                    let needs_new_reader = match &*reader_opt {
                        Some((cached_pid, _)) => *cached_pid != pid,
                        None => true,
                    };

                    if needs_new_reader {
                        *reader_opt = ProcessMemReader::new(pid).ok().map(|r| (pid, r));
                    }

                    if let Some((_, reader)) = &*reader_opt {
                        reader.read_at(*start, *size)
                    } else {
                        fast_read_memory(pid, *start, *size)
                    }
                });

                #[cfg(not(target_os = "linux"))]
                let memory_result = fast_read_memory(pid, *start, *size);

                match memory_result {
                    Ok(memory) => {
                        let results = if matches!(search_data.0, SearchType::Guess) {
                            // For Guess type, try all possible interpretations
                            let mut all_results = Vec::new();

                            // Try as each type
                            for search_type in [
                                // SearchType::Byte,
                                // SearchType::Short,
                                SearchType::Int,
                                // SearchType::Int64,
                                SearchType::Float,
                                SearchType::Double,
                            ] {
                                match search_type.from_string(&value_text) {
                                    Ok(typed_value) => {
                                        let typed_results: Vec<SearchResult> = search_memory(&memory, &typed_value.1, search_type, *start);
                                        all_results.extend(typed_results);
                                    }
                                    Err(e) => {
                                        eprintln!("Failed to parse typed value for {}: {}", search_type, e);
                                    }
                                }
                            }

                            all_results
                        } else {
                            search_memory(&memory, &search_data.1, search_data.0, *start)
                        };

                        if !results.is_empty() {
                            let _ = results_sender.send(results);
                        }
                    }
                    Err(_) => {
                        // Silently skip this region - it's no longer accessible
                        // This is normal for dynamic memory regions
                        // Don't log every failure as it would spam the console
                    }
                }

                current_bytes.fetch_add(*size, Ordering::SeqCst);
            });

            cache_valid.store(false, Ordering::Release);
            search_complete.store(true, Ordering::SeqCst);
        });
    }

    pub(crate) fn select_process(&mut self, process: &ProcessInfo) {
        self.pid = process.pid;
        if let Err(e) = self.freeze_sender.send(FreezeMessage::from_addr(MessageCommand::Pid, process.pid as usize)) {
            self.error_text = format!("Failed to send pid freeze message: {e}");
        }
        self.process_name = process.name.clone();
        self.show_process_window = false;
    }

    pub fn take_memory_snapshot(&mut self, search_index: usize) {
        let Some(search_context) = self.searches.get_mut(search_index) else {
            return;
        };

        search_context.searching = SearchMode::Memory;
        search_context.clear_memory_snapshot();

        match get_process_maps(self.pid) {
            Ok(maps) => {
                let mut total_bytes = 0;
                let mut regions = Vec::new();

                for map in maps {
                    if skip_memory_region(&map) {
                        continue;
                    }

                    total_bytes += map.size();
                    regions.push((map.start(), map.size()));
                }

                search_context.total_bytes = total_bytes;
                search_context.current_bytes.store(0, Ordering::SeqCst);

                self.spawn_snapshot_capture(search_index, regions);
            }
            Err(e) => {
                self.error_text = format!("Failed to get process maps: {}", e);
                // Set search complete even on error so UI doesn't get stuck
                search_context.search_complete.store(true, Ordering::SeqCst);
            }
        }
    }

    fn spawn_snapshot_capture(&mut self, search_index: usize, regions: Vec<(usize, usize)>) {
        let Some(search_context) = self.searches.get_mut(search_index) else {
            return;
        };

        let pid = self.pid;
        let current_bytes = search_context.current_bytes.clone();
        let memory_snapshot = search_context.memory_snapshot.clone();
        let search_complete = search_context.search_complete.clone();

        search_complete.store(false, Ordering::SeqCst);

        std::thread::spawn(move || {
            // Open /proc/[pid]/mem once for all reads
            #[cfg(target_os = "linux")]
            let mem_reader = ProcessMemReader::new(pid).ok();

            for (start, size) in regions.iter() {
                const CHUNK_SIZE: usize = 50 * 1024 * 1024; // 50MB
                let mut region_offset = 0;
                while region_offset < *size {
                    let chunk_size = (*size - region_offset).min(CHUNK_SIZE);
                    let chunk_start = *start + region_offset;

                    // Use ProcessMemReader for efficient reads
                    #[cfg(target_os = "linux")]
                    let memory_result = if let Some(ref reader) = mem_reader {
                        reader.read_at(chunk_start, chunk_size)
                    } else {
                        fast_read_memory(pid, chunk_start, chunk_size)
                    };

                    #[cfg(not(target_os = "linux"))]
                    let memory_result = fast_read_memory(pid, chunk_start, chunk_size);

                    if let Ok(memory) = memory_result {
                        let arc_page: Arc<[u8]> = Arc::<[u8]>::from(memory.into_boxed_slice());
                        if let Ok(mut snap) = memory_snapshot.write() {
                            snap.push((chunk_start, arc_page));
                        }
                    }
                    region_offset += chunk_size;
                    current_bytes.fetch_add(chunk_size, Ordering::SeqCst);
                }
            }

            search_complete.store(true, Ordering::SeqCst);
        });
    }

    pub fn unknown_search_compare(&mut self, search_index: usize, comparison: UnknownComparison) {
        let Some(search_context) = self.searches.get_mut(search_index) else {
            return;
        };
        search_context.searching = SearchMode::Percent;

        let old_results = search_context.collect_results();
        if old_results.len() < 100000 {
            search_context.old_results.push((*old_results).clone());
        }

        // Reset channel/cache
        let (tx, rx) = crossbeam_channel::unbounded();
        search_context.results_sender = tx;
        search_context.results_receiver = rx;
        search_context.invalidate_cache();

        let pid = self.pid;
        let current_bytes = search_context.current_bytes.clone();
        let results_sender = search_context.results_sender.clone();
        let search_complete = search_context.search_complete.clone();
        let cache_valid = search_context.cache_valid.clone();
        let memory_snapshot = search_context.memory_snapshot.clone();

        // Progress baseline
        search_context.total_bytes = if old_results.is_empty() {
            if let Ok(snap) = memory_snapshot.read() {
                snap.iter().map(|(_, p)| p.len()).sum()
            } else {
                0
            }
        } else {
            old_results.len()
        };

        current_bytes.store(0, Ordering::SeqCst);
        search_complete.store(false, Ordering::SeqCst);

        std::thread::spawn(move || {
            if pid.try_into_process_handle().is_err() {
                search_complete.store(true, Ordering::SeqCst);
                return;
            }

            const BATCH: usize = 4096;
            if old_results.is_empty() {
                // First pass: compare snapshot vs current, 4/8-byte aligned offsets only
                let pages: Vec<(usize, Arc<[u8]>)> = if let Ok(snap) = memory_snapshot.read() {
                    snap.iter().map(|(b, p)| (*b, Arc::clone(p))).collect()
                } else {
                    Vec::new()
                };
                // Use bounded channel to prevent memory bloat
                let (local_tx, local_rx) = crossbeam_channel::bounded::<Vec<SearchResult>>(100);

                // Spawn consumer thread
                let results_sender_clone = results_sender.clone();
                let consumer = std::thread::spawn(move || {
                    let mut total = 0;
                    while let Ok(batch) = local_rx.recv() {
                        total += batch.len();
                        let _ = results_sender_clone.send(batch);
                    }
                    total
                });
                pages.par_iter().for_each(|(base, old_mem)| {
                    let len = old_mem.len();
                    // Each worker gets its own handle (avoid sharing across threads)
                    let handle = match pid.try_into_process_handle() {
                        Ok(h) => h,
                        Err(_) => {
                            current_bytes.fetch_add(len, Ordering::SeqCst);
                            return;
                        }
                    };

                    if let Ok(new_mem) = copy_address(*base, len, &handle) {
                        let mut local_out = Vec::with_capacity(BATCH);

                        // 4-byte aligned
                        for i in (0..=(len.saturating_sub(4))).step_by(4) {
                            let oldb = &old_mem[i..i + 4];
                            let newb = &new_mem[i..i + 4];

                            // Fast equality prefilter for Unchanged/Changed
                            if matches!(comparison, UnknownComparison::Unchanged | UnknownComparison::Changed) && oldb == newb {
                                if matches!(comparison, UnknownComparison::Unchanged) {
                                    // For floats we still accept exact-equal as unchanged without decoding
                                    local_out.push(SearchResult::new_with_bytes(*base + i, SearchType::Int, newb));
                                    local_out.push(SearchResult::new_with_bytes(*base + i, SearchType::Float, newb));
                                }
                            } else {
                                if compare_values(oldb, newb, SearchType::Int, comparison) {
                                    local_out.push(SearchResult::new_with_bytes(*base + i, SearchType::Int, newb));
                                }
                                if compare_values(oldb, newb, SearchType::Float, comparison) {
                                    local_out.push(SearchResult::new_with_bytes(*base + i, SearchType::Float, newb));
                                }
                            }

                            if local_out.len() >= BATCH {
                                let _ = results_sender.send(std::mem::take(&mut local_out));
                            }
                        }

                        // 8-byte aligned
                        for i in (0..=(len.saturating_sub(8))).step_by(8) {
                            let oldb = &old_mem[i..i + 8];
                            let newb = &new_mem[i..i + 8];

                            if matches!(comparison, UnknownComparison::Unchanged | UnknownComparison::Changed) && oldb == newb {
                                if matches!(comparison, UnknownComparison::Unchanged) {
                                    // Exact-equal treat as unchanged, skip decoding
                                    local_out.push(SearchResult::new_with_bytes(*base + i, SearchType::Double, newb));
                                }
                            } else if compare_values(oldb, newb, SearchType::Double, comparison) {
                                local_out.push(SearchResult::new_with_bytes(*base + i, SearchType::Double, newb));
                            }

                            if local_out.len() >= BATCH {
                                let _ = results_sender.send(std::mem::take(&mut local_out));
                            }
                        }

                        if !local_out.is_empty() {
                            let _ = local_tx.send(local_out);
                        }
                    }

                    current_bytes.fetch_add(len, Ordering::SeqCst);
                });

                drop(local_tx); // Signal completion
                let _ = consumer.join();

                // Clear snapshot after first pass to free memory
                if let Ok(mut snap) = memory_snapshot.write() {
                    snap.clear();
                    snap.shrink_to_fit();
                }
            } else {
                // Subsequent passes: optimize with better data structures
                const PAGE: usize = 4096;

                // Group by address for single-read optimization
                let mut addr_to_types: std::collections::BTreeMap<usize, Vec<(SearchType, [u8; 8])>> = std::collections::BTreeMap::new();

                for r in old_results.iter() {
                    if let Some(oldb) = r.stored_bytes() {
                        let len = r.search_type.get_byte_length();
                        if len <= 8 && oldb.len() >= len {
                            let mut buf = [0u8; 8];
                            buf[..len].copy_from_slice(&oldb[..len]);
                            addr_to_types.entry(r.addr).or_default().push((r.search_type, buf));
                        }
                    }
                }

                // Group addresses by page
                let mut per_page: std::collections::BTreeMap<usize, Vec<usize>> = std::collections::BTreeMap::new();
                for addr in addr_to_types.keys() {
                    per_page.entry(addr & !(PAGE - 1)).or_default().push(*addr);
                }

                let per_page_vec: Vec<(usize, Vec<usize>)> = per_page.into_iter().collect();

                per_page_vec.par_iter().for_each(|(_page_base, addrs)| {
                    // Sort addresses within page for better cache locality
                    let mut sorted_addrs = addrs.clone();
                    sorted_addrs.sort_unstable();

                    // Calculate minimal span
                    let min_addr = *sorted_addrs.first().unwrap();
                    let sorted_addrs_len = sorted_addrs.len();
                    let max_addr = sorted_addrs
                        .iter()
                        .filter_map(|addr| {
                            addr_to_types
                                .get(addr)
                                .and_then(|types| types.iter().map(|(ty, _)| *addr + ty.get_byte_length()).max())
                        })
                        .max()
                        .unwrap_or(min_addr + 8); // Default to at least 8 bytes

                    let span_len = (max_addr - min_addr).min(PAGE * 4); // Cap at 4 pages
                    if span_len == 0 {
                        current_bytes.fetch_add(sorted_addrs.len(), Ordering::Relaxed);
                        return;
                    }

                    let handle = match pid.try_into_process_handle() {
                        Ok(h) => h,
                        Err(_) => {
                            current_bytes.fetch_add(sorted_addrs.len(), Ordering::Relaxed);
                            return;
                        }
                    };

                    if let Ok(buf) = copy_address(min_addr, span_len, &handle) {
                        let mut local_out = Vec::with_capacity(BATCH);

                        for addr in sorted_addrs {
                            if let Some(types) = addr_to_types.get(&addr) {
                                let offset = addr - min_addr;

                                // Read once per address for the largest type
                                let max_len = types.iter().map(|(ty, _)| ty.get_byte_length()).max().unwrap_or(0);
                                if offset + max_len > buf.len() {
                                    continue;
                                }
                                let new_max = &buf[offset..offset + max_len];

                                for (ty, old8) in types {
                                    let len = ty.get_byte_length();
                                    let oldb = &old8[..len];
                                    let newb = &new_max[..len];

                                    // Fast prefilter for equality in Changed/Unchanged
                                    if matches!(comparison, UnknownComparison::Unchanged | UnknownComparison::Changed) {
                                        if oldb == newb {
                                            if matches!(comparison, UnknownComparison::Unchanged) {
                                                local_out.push(SearchResult::new_with_bytes(addr, *ty, newb));
                                            }
                                            continue; // equality handled, skip decoding
                                        } else if matches!(comparison, UnknownComparison::Changed) && !matches!(ty, SearchType::Float | SearchType::Double) {
                                            // For integers, byte-inequality is sufficient for "Changed"
                                            local_out.push(SearchResult::new_with_bytes(addr, *ty, newb));
                                            continue;
                                        }
                                    }

                                    // Fallback to typed comparison (needed for floats/doubles or inc/dec)
                                    if compare_values(oldb, newb, *ty, comparison) {
                                        local_out.push(SearchResult::new_with_bytes(addr, *ty, newb));
                                    }

                                    if local_out.len() >= BATCH {
                                        let _ = results_sender.send(std::mem::take(&mut local_out));
                                    }
                                }
                            }
                        }

                        if !local_out.is_empty() {
                            let _ = results_sender.send(local_out);
                        }
                    }

                    current_bytes.fetch_add(sorted_addrs_len, Ordering::Relaxed);
                });
            }

            cache_valid.store(false, Ordering::Release);
            search_complete.store(true, Ordering::SeqCst);
        });
    }

    fn spawn_string_search(&mut self, search_text: String, regions: Vec<(usize, usize)>, search_index: usize) {
        let Some(search_context) = self.searches.get_mut(search_index) else {
            self.error_text = format!("Invalid search index {search_index}");
            return;
        };

        let current_bytes = search_context.current_bytes.clone();
        let pid = self.pid;
        let results_sender = search_context.results_sender.clone();
        let search_complete = search_context.search_complete.clone();
        let cache_valid = search_context.cache_valid.clone();

        search_complete.store(false, Ordering::SeqCst);

        std::thread::spawn(move || {
            // Use thread-local ProcessMemReader for efficient repeated reads
            #[cfg(target_os = "linux")]
            thread_local! {
                static MEM_READER: std::cell::RefCell<Option<(process_memory::Pid, ProcessMemReader)>> = const { std::cell::RefCell::new(None) };
            }

            regions.par_iter().for_each(|(start, size)| {
                #[cfg(target_os = "linux")]
                let memory_result = MEM_READER.with(|reader_cell| {
                    let mut reader_opt = reader_cell.borrow_mut();

                    let needs_new_reader = match &*reader_opt {
                        Some((cached_pid, _)) => *cached_pid != pid,
                        None => true,
                    };

                    if needs_new_reader {
                        *reader_opt = ProcessMemReader::new(pid).ok().map(|r| (pid, r));
                    }

                    if let Some((_, reader)) = &*reader_opt {
                        reader.read_at(*start, *size)
                    } else {
                        fast_read_memory(pid, *start, *size)
                    }
                });

                #[cfg(not(target_os = "linux"))]
                let memory_result = fast_read_memory(pid, *start, *size);

                match memory_result {
                    Ok(memory) => {
                        let results = search_string_in_memory(&memory, &search_text, *start);

                        if !results.is_empty() {
                            let _ = results_sender.send(results);
                        }
                    }
                    Err(_) => {
                        // Silently skip this region - it's no longer accessible
                    }
                }

                current_bytes.fetch_add(*size, Ordering::SeqCst);
            });

            cache_valid.store(false, Ordering::Release);
            search_complete.store(true, Ordering::SeqCst);
        });
    }

    pub fn is_process_running(&self) -> bool {
        if self.pid == 0 {
            return false;
        }
        const REFRESH_INTERVAL: Duration = Duration::from_millis(500);
        if let Ok(mut system_guard) = SYSTEM.lock() {
            let (system, last_refresh) = &mut *system_guard;
            let now = Instant::now();

            // Only refresh if enough time has passed
            if now.duration_since(*last_refresh) >= REFRESH_INTERVAL {
                system.refresh_processes(ProcessesToUpdate::All, true);
                *last_refresh = now;
            }
            system.process(Pid::from(self.pid as usize)).is_some()
        } else {
            #[cfg(target_os = "linux")]
            {
                std::path::Path::new(&format!("/proc/{}", self.pid)).exists()
            }

            #[cfg(not(any(target_os = "linux")))]
            {
                // On other platforms, assume process is still running if we can't check
                // This is safer than incorrectly reporting it as dead
                true
            }
        }
    }
}

static SYSTEM: Lazy<Arc<Mutex<(System, Instant)>>> = Lazy::new(|| Arc::new(Mutex::new((System::new(), Instant::now() - Duration::from_secs(1)))));

fn skip_memory_region(map: &proc_maps::MapRange) -> bool {
    if map.start() == 0xffffffffff600000 {
        return true;
    }
    if map.size() == 0 {
        return true;
    }
    // Skip kernel vsyscall region

    // Skip regions that are likely to cause issues

    if cfg!(target_os = "windows") {
        if let Some(file_name) = map.filename()
            && file_name.starts_with("C:\\WINDOWS\\")
        {
            return true;
        }
    } else if cfg!(target_os = "linux") {
        if !map.is_write() {
            return true;
        }
        // Skip if not readable at all
        if !map.is_read() {
            return true;
        }

        // Skip kernel space addresses (very high addresses)
        if map.start() > 0x7fffffffffff {
            return true;
        }

        // Skip special regions
        if let Some(file_name) = map.filename() {
            let file_str = file_name.to_string_lossy();
            if file_str.starts_with("/usr/")
                || file_str.starts_with("/lib/")
                || file_str.starts_with("/lib64/")
                || file_str.starts_with("/dev/")
                || file_str.starts_with("/proc/")
                || file_str.starts_with("/sys/")
                || file_str == "[vvar]"
                || file_str == "[vdso]"
                || file_str == "[vsyscall]"
            {
                return true;
            }
        }
    } else {
        if !map.is_write() || map.filename().is_none() || map.size() < 1024 * 1024 {
            return true;
        }
        if let Some(file_name) = map.filename()
            && (file_name.starts_with("/usr/lib") || file_name.starts_with("/System/"))
        {
            return true;
        }
    }
    false
}

fn update_results<T>(old_results: &[SearchResult], value_text: &str, handle: &T) -> Vec<SearchResult>
where
    T: process_memory::CopyAddress,
{
    let mut results = Vec::new();
    for result in old_results {
        match result.search_type.from_string(value_text) {
            Ok(search_value) => {
                if let Ok(buf) = copy_address(result.addr, result.search_type.get_byte_length(), handle) {
                    let matches = match result.search_type {
                        SearchType::Float => {
                            if buf.len() == 4 && search_value.1.len() == 4 {
                                let current = f32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
                                let target = f32::from_le_bytes([search_value.1[0], search_value.1[1], search_value.1[2], search_value.1[3]]);

                                let epsilon = get_epsilon_f32(target);

                                if current.is_finite() && target.is_finite() {
                                    (current - target).abs() <= epsilon
                                } else {
                                    current == target || (current.is_nan() && target.is_nan())
                                }
                            } else {
                                false
                            }
                        }
                        SearchType::Double => {
                            if buf.len() == 8 && search_value.1.len() == 8 {
                                let current = f64::from_le_bytes([buf[0], buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7]]);
                                let target = f64::from_le_bytes([
                                    search_value.1[0],
                                    search_value.1[1],
                                    search_value.1[2],
                                    search_value.1[3],
                                    search_value.1[4],
                                    search_value.1[5],
                                    search_value.1[6],
                                    search_value.1[7],
                                ]);

                                let epsilon = get_epsilon_f64(target);

                                if current.is_finite() && target.is_finite() {
                                    (current - target).abs() <= epsilon
                                } else {
                                    current == target || (current.is_nan() && target.is_nan())
                                }
                            } else {
                                false
                            }
                        }
                        _ => {
                            // For integer types, use exact comparison
                            let val = SearchValue(result.search_type, buf);
                            val.1 == search_value.1
                        }
                    };

                    if matches {
                        results.push(*result);
                    }
                }
            }
            Err(err) => {
                eprintln!("Error converting {:?}: {}", result.search_type, err);
            }
        }
    }
    results
}

pub fn search_memory(memory_data: &[u8], search_data: &[u8], search_type: SearchType, start: usize) -> Vec<SearchResult> {
    let mut result = Vec::new();

    match search_type {
        // For single byte searches, use memchr which is SIMD optimized
        SearchType::Byte => {
            if search_data.len() == 1 {
                let positions = memchr::memchr_iter(search_data[0], memory_data);
                for pos in positions {
                    result.push(SearchResult::new(pos + start, search_type));
                }
            }
        }
        // For aligned integer types, use optimized searching
        SearchType::Short | SearchType::Int | SearchType::Int64 => {
            // Use SIMD-optimized pattern matching for aligned data
            result = search_aligned_integers(memory_data, search_data, search_type, start);
        }
        // For floats, search with epsilon tolerance
        SearchType::Float => {
            if search_data.len() == 4 {
                let target = f32::from_le_bytes([search_data[0], search_data[1], search_data[2], search_data[3]]);
                let epsilon = get_epsilon_f32(target);

                #[cfg(target_arch = "x86_64")]
                {
                    result = search_f32_simd(memory_data, target, epsilon, start);
                }

                #[cfg(not(target_arch = "x86_64"))]
                {
                    // Scan through memory interpreting each position as a potential float
                    if memory_data.len() >= 4 {
                        for i in 0..=memory_data.len() - 4 {
                            let value = f32::from_le_bytes([memory_data[i], memory_data[i + 1], memory_data[i + 2], memory_data[i + 3]]);

                            // Check if value is close enough to target
                            // Also handle special cases like NaN and infinity
                            if value.is_finite() && target.is_finite() {
                                if (value - target).abs() <= epsilon {
                                    result.push(SearchResult::new(start + i, SearchType::Float));
                                }
                            } else if value.is_nan() && target.is_nan() {
                                // Both are NaN
                                result.push(SearchResult::new(start + i, SearchType::Float));
                            } else if value == target {
                                // Handle infinities
                                result.push(SearchResult::new(start + i, SearchType::Float));
                            }
                        }
                    }
                }
            }
        }
        // For doubles, search with epsilon tolerance
        SearchType::Double => {
            if search_data.len() == 8 {
                let target = f64::from_le_bytes([
                    search_data[0],
                    search_data[1],
                    search_data[2],
                    search_data[3],
                    search_data[4],
                    search_data[5],
                    search_data[6],
                    search_data[7],
                ]);

                // Similar epsilon strategy for doubles
                let epsilon = get_epsilon_f64(target);

                #[cfg(target_arch = "x86_64")]
                {
                    result = search_f64_simd(memory_data, target, epsilon, start);
                }

                #[cfg(not(target_arch = "x86_64"))]
                {
                    // Scan through memory interpreting each position as a potential double
                    if memory_data.len() >= 8 {
                        for i in 0..=memory_data.len() - 8 {
                            let value = f64::from_le_bytes([
                                memory_data[i],
                                memory_data[i + 1],
                                memory_data[i + 2],
                                memory_data[i + 3],
                                memory_data[i + 4],
                                memory_data[i + 5],
                                memory_data[i + 6],
                                memory_data[i + 7],
                            ]);

                            // Check if value is close enough to target
                            if value.is_finite() && target.is_finite() {
                                if (value - target).abs() <= epsilon {
                                    result.push(SearchResult::new(start + i, SearchType::Double));
                                }
                            } else if value.is_nan() && target.is_nan() {
                                result.push(SearchResult::new(start + i, SearchType::Double));
                            } else if value == target {
                                // Handle infinities
                                result.push(SearchResult::new(start + i, SearchType::Double));
                            }
                        }
                    }
                }
            }
        }
        SearchType::Guess => {
            // For Guess type, still use exact matching with memmem
            // The epsilon matching is handled when we try Float/Double variants
            let finder = memmem::Finder::new(search_data);
            for pos in finder.find_iter(memory_data) {
                result.push(SearchResult::new(pos + start, search_type));
            }
        }
        SearchType::Unknown => {
            eprintln!("Unknown search type encountered, this should not happen in production code.");
        }
        SearchType::String | SearchType::StringUtf16 => {
            // only done in initial search.
        }
    }

    result
}

fn get_epsilon_f32(_current: f32) -> f32 {
    1.0 - f32::EPSILON
}

fn get_epsilon_f64(_current: f64) -> f64 {
    1.0 - f64::EPSILON
}

// Optimized search for aligned integers using SIMD
fn search_aligned_integers(memory_data: &[u8], search_data: &[u8], search_type: SearchType, start: usize) -> Vec<SearchResult> {
    let mut results = Vec::new();

    match search_type {
        SearchType::Short => {
            if search_data.len() != 2 {
                return results;
            }
            let search_value = u16::from_le_bytes([search_data[0], search_data[1]]);

            #[cfg(target_arch = "x86_64")]
            {
                results.extend(search_u16_simd(memory_data, search_value, start));
            }

            #[cfg(not(target_arch = "x86_64"))]
            {
                // Search aligned positions first (much faster)
                let aligned_data = &memory_data[..memory_data.len() & !1];
                for (i, chunk) in aligned_data.chunks_exact(2).enumerate() {
                    let value = u16::from_le_bytes([chunk[0], chunk[1]]);
                    if value == search_value {
                        results.push(SearchResult::new(start + i * 2, SearchType::Short));
                    }
                }

                // Check unaligned positions (slower, but necessary for completeness)
                if memory_data.len() > 2 {
                    for i in 1..memory_data.len() - 1 {
                        if memory_data[i] == search_data[0] && memory_data[i + 1] == search_data[1] {
                            results.push(SearchResult::new(start + i, SearchType::Short));
                        }
                    }
                }
            }
        }
        SearchType::Int => {
            if search_data.len() != 4 {
                return results;
            }
            let search_value = u32::from_le_bytes([search_data[0], search_data[1], search_data[2], search_data[3]]);

            // Use SIMD on x86_64 if available
            #[cfg(target_arch = "x86_64")]
            {
                results.extend(search_u32_simd(memory_data, search_value, start));
            }

            // Fallback for non-x86_64 or if SIMD didn't find everything
            #[cfg(not(target_arch = "x86_64"))]
            {
                // Search aligned positions first
                let aligned_data = &memory_data[..memory_data.len() & !3];
                for (i, chunk) in aligned_data.chunks_exact(4).enumerate() {
                    let value = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                    if value == search_value {
                        results.push(SearchResult::new(start + i * 4, SearchType::Int));
                    }
                }

                // For unaligned search, use memmem which is SIMD optimized
                let finder = memmem::Finder::new(search_data);
                for pos in finder.find_iter(memory_data) {
                    // Skip aligned positions we already found
                    if pos % 4 != 0 {
                        results.push(SearchResult::new(start + pos, SearchType::Int));
                    }
                }
            }
        }
        SearchType::Int64 => {
            if search_data.len() != 8 {
                return results;
            }
            let search_value = u64::from_le_bytes([
                search_data[0],
                search_data[1],
                search_data[2],
                search_data[3],
                search_data[4],
                search_data[5],
                search_data[6],
                search_data[7],
            ]);

            // Use SIMD on x86_64 if available
            #[cfg(target_arch = "x86_64")]
            {
                results.extend(search_u64_simd(memory_data, search_value, start));
            }

            // Fallback for non-x86_64
            #[cfg(not(target_arch = "x86_64"))]
            {
                // Search aligned positions first
                let aligned_data = &memory_data[..memory_data.len() & !7];
                for (i, chunk) in aligned_data.chunks_exact(8).enumerate() {
                    let value = u64::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5], chunk[6], chunk[7]]);
                    if value == search_value {
                        results.push(SearchResult::new(start + i * 8, SearchType::Int64));
                    }
                }

                // For unaligned search, use memmem
                let finder = memmem::Finder::new(search_data);
                for pos in finder.find_iter(memory_data) {
                    if pos % 8 != 0 {
                        results.push(SearchResult::new(start + pos, SearchType::Int64));
                    }
                }
            }
        }
        _ => {}
    }

    results
}

// For even better performance with explicit SIMD, you can use the `packed_simd` or `std::simd` features
#[cfg(target_arch = "x86_64")]
fn search_u32_simd(memory_data: &[u8], search_value: u32, start: usize) -> Vec<SearchResult> {
    use std::arch::x86_64::*;

    let mut results = Vec::new();

    unsafe {
        // Ensure we have SSE2 support
        if is_x86_feature_detected!("sse2") {
            let search_vec = _mm_set1_epi32(search_value as i32);

            // Process 16 bytes (4 u32s) at a time
            let chunks = memory_data.chunks_exact(16);
            let remainder = chunks.remainder();

            for (chunk_idx, chunk) in chunks.enumerate() {
                let data = _mm_loadu_si128(chunk.as_ptr() as *const __m128i);
                let cmp = _mm_cmpeq_epi32(data, search_vec);
                let mask = _mm_movemask_epi8(cmp);

                if mask != 0 {
                    // Check each u32 in the chunk
                    for i in 0..4 {
                        if (mask >> (i * 4)) & 0xF == 0xF {
                            results.push(SearchResult::new(start + chunk_idx * 16 + i * 4, SearchType::Int));
                        }
                    }
                }
            }

            // Handle remainder with regular search
            if remainder.len() >= 4 {
                for i in 0..=(remainder.len() - 4) {
                    let value = u32::from_le_bytes([remainder[i], remainder[i + 1], remainder[i + 2], remainder[i + 3]]);
                    if value == search_value {
                        let base_offset = memory_data.len() - remainder.len();
                        results.push(SearchResult::new(start + base_offset + i, SearchType::Int));
                    }
                }
            }
        }
    }

    results
}

#[cfg(target_arch = "x86_64")]
fn search_u64_simd(memory_data: &[u8], search_value: u64, start: usize) -> Vec<SearchResult> {
    use std::arch::x86_64::*;

    let mut results = Vec::new();

    unsafe {
        // For u64, we can use different strategies depending on available features
        if is_x86_feature_detected!("avx2") {
            // AVX2 path - process 32 bytes (4 u64s) at a time
            let search_vec = _mm256_set1_epi64x(search_value as i64);

            let chunks = memory_data.chunks_exact(32);
            let remainder = chunks.remainder();

            for (chunk_idx, chunk) in chunks.enumerate() {
                let data = _mm256_loadu_si256(chunk.as_ptr() as *const __m256i);
                let cmp = _mm256_cmpeq_epi64(data, search_vec);
                let mask = _mm256_movemask_epi8(cmp);

                if mask != 0 {
                    // Check each u64 in the chunk
                    for i in 0..4 {
                        if (mask >> (i * 8)) & 0xFF == 0xFF {
                            results.push(SearchResult::new(start + chunk_idx * 32 + i * 8, SearchType::Int64));
                        }
                    }
                }
            }

            // Handle remainder
            if remainder.len() >= 8 {
                for i in 0..=(remainder.len() - 8) {
                    let value = u64::from_le_bytes([
                        remainder[i],
                        remainder[i + 1],
                        remainder[i + 2],
                        remainder[i + 3],
                        remainder[i + 4],
                        remainder[i + 5],
                        remainder[i + 6],
                        remainder[i + 7],
                    ]);
                    if value == search_value {
                        let base_offset = memory_data.len() - remainder.len();
                        results.push(SearchResult::new(start + base_offset + i, SearchType::Int64));
                    }
                }
            }
        } else if is_x86_feature_detected!("sse2") {
            // SSE2 path - process 16 bytes (2 u64s) at a time
            let search_low = _mm_set1_epi32((search_value & 0xFFFFFFFF) as i32);
            let search_high = _mm_set1_epi32((search_value >> 32) as i32);

            let chunks = memory_data.chunks_exact(16);
            let remainder = chunks.remainder();

            for (chunk_idx, chunk) in chunks.enumerate() {
                let data = _mm_loadu_si128(chunk.as_ptr() as *const __m128i);

                // Compare both u64 values in the chunk
                // First u64 (bytes 0-7)
                let data_low = _mm_shuffle_epi32(data, 0b01000100); // Get low 32 bits of both u64s
                let data_high = _mm_shuffle_epi32(data, 0b11101110); // Get high 32 bits of both u64s

                let cmp_low = _mm_cmpeq_epi32(data_low, search_low);
                let cmp_high = _mm_cmpeq_epi32(data_high, search_high);
                let cmp_combined = _mm_and_si128(cmp_low, cmp_high);

                let mask = _mm_movemask_epi8(cmp_combined);

                // Check first u64
                if (mask & 0x00FF) == 0x00FF {
                    let value = u64::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5], chunk[6], chunk[7]]);
                    if value == search_value {
                        results.push(SearchResult::new(start + chunk_idx * 16, SearchType::Int64));
                    }
                }

                // Check second u64
                if (mask & 0xFF00) == 0xFF00 {
                    let value = u64::from_le_bytes([chunk[8], chunk[9], chunk[10], chunk[11], chunk[12], chunk[13], chunk[14], chunk[15]]);
                    if value == search_value {
                        results.push(SearchResult::new(start + chunk_idx * 16 + 8, SearchType::Int64));
                    }
                }
            }

            // Handle remainder
            if remainder.len() >= 8 {
                for i in 0..=(remainder.len() - 8) {
                    let value = u64::from_le_bytes([
                        remainder[i],
                        remainder[i + 1],
                        remainder[i + 2],
                        remainder[i + 3],
                        remainder[i + 4],
                        remainder[i + 5],
                        remainder[i + 6],
                        remainder[i + 7],
                    ]);
                    if value == search_value {
                        let base_offset = memory_data.len() - remainder.len();
                        results.push(SearchResult::new(start + base_offset + i, SearchType::Int64));
                    }
                }
            }
        } else {
            // Fallback to non-SIMD implementation
            for i in 0..=(memory_data.len().saturating_sub(8)) {
                let value = u64::from_le_bytes([
                    memory_data[i],
                    memory_data[i + 1],
                    memory_data[i + 2],
                    memory_data[i + 3],
                    memory_data[i + 4],
                    memory_data[i + 5],
                    memory_data[i + 6],
                    memory_data[i + 7],
                ]);
                if value == search_value {
                    results.push(SearchResult::new(start + i, SearchType::Int64));
                }
            }
        }
    }

    results
}

// SIMD-optimized u16 search for x86_64
#[cfg(target_arch = "x86_64")]
fn search_u16_simd(memory_data: &[u8], search_value: u16, start: usize) -> Vec<SearchResult> {
    use std::arch::x86_64::*;

    let mut results = Vec::new();

    unsafe {
        if is_x86_feature_detected!("sse2") {
            let search_vec = _mm_set1_epi16(search_value as i16);

            // Process 16 bytes (8 u16s) at a time
            let chunks = memory_data.chunks_exact(16);
            let remainder = chunks.remainder();

            for (chunk_idx, chunk) in chunks.enumerate() {
                let data = _mm_loadu_si128(chunk.as_ptr() as *const __m128i);
                let cmp = _mm_cmpeq_epi16(data, search_vec);
                let mask = _mm_movemask_epi8(cmp);

                if mask != 0 {
                    // Check each u16 in the chunk
                    for i in 0..8 {
                        if (mask >> (i * 2)) & 0x3 == 0x3 {
                            results.push(SearchResult::new(start + chunk_idx * 16 + i * 2, SearchType::Short));
                        }
                    }
                }
            }

            // Handle remainder with regular search
            if remainder.len() >= 2 {
                for i in 0..=(remainder.len() - 2) {
                    let value = u16::from_le_bytes([remainder[i], remainder[i + 1]]);
                    if value == search_value {
                        let base_offset = memory_data.len() - remainder.len();
                        results.push(SearchResult::new(start + base_offset + i, SearchType::Short));
                    }
                }
            }
        }
    }

    results
}

// SIMD-optimized f32 search for x86_64
#[cfg(target_arch = "x86_64")]
fn search_f32_simd(memory_data: &[u8], target: f32, epsilon: f32, start: usize) -> Vec<SearchResult> {
    use std::arch::x86_64::*;

    let mut results = Vec::new();

    // Handle special cases separately
    if !target.is_finite() {
        // For NaN/Infinity, fall back to scalar
        if memory_data.len() >= 4 {
            for i in 0..=memory_data.len() - 4 {
                let value = f32::from_le_bytes([memory_data[i], memory_data[i + 1], memory_data[i + 2], memory_data[i + 3]]);
                if (value.is_nan() && target.is_nan()) || value == target {
                    results.push(SearchResult::new(start + i, SearchType::Float));
                }
            }
        }
        return results;
    }

    unsafe {
        // Try AVX2 first (processes 8 floats at a time)
        if is_x86_feature_detected!("avx2") {
            let target_vec = _mm256_set1_ps(target);
            let epsilon_vec = _mm256_set1_ps(epsilon);
            let neg_epsilon_vec = _mm256_set1_ps(-epsilon);

            // Process 32 bytes (8 f32s) at a time
            let chunks = memory_data.chunks_exact(32);
            let remainder = chunks.remainder();

            for (chunk_idx, chunk) in chunks.enumerate() {
                let data = _mm256_loadu_ps(chunk.as_ptr() as *const f32);

                let diff = _mm256_sub_ps(data, target_vec);
                let ge_neg_eps = _mm256_cmp_ps(diff, neg_epsilon_vec, _CMP_GE_OQ);
                let le_eps = _mm256_cmp_ps(diff, epsilon_vec, _CMP_LE_OQ);
                let in_range = _mm256_and_ps(ge_neg_eps, le_eps);

                let mask = _mm256_movemask_ps(in_range);

                if mask != 0 {
                    for i in 0..8 {
                        if (mask >> i) & 1 == 1 {
                            let value = f32::from_le_bytes([chunk[i * 4], chunk[i * 4 + 1], chunk[i * 4 + 2], chunk[i * 4 + 3]]);
                            if value.is_finite() && (value - target).abs() <= epsilon {
                                results.push(SearchResult::new(start + chunk_idx * 32 + i * 4, SearchType::Float));
                            }
                        }
                    }
                }
            }

            // Handle remainder with SSE or scalar
            if remainder.len() >= 4 {
                for i in 0..=(remainder.len() - 4) {
                    let value = f32::from_le_bytes([remainder[i], remainder[i + 1], remainder[i + 2], remainder[i + 3]]);
                    if value.is_finite() && (value - target).abs() <= epsilon {
                        let base_offset = memory_data.len() - remainder.len();
                        results.push(SearchResult::new(start + base_offset + i, SearchType::Float));
                    }
                }
            }
        } else if is_x86_feature_detected!("sse") {
            let target_vec = _mm_set1_ps(target);
            let epsilon_vec = _mm_set1_ps(epsilon);
            let neg_epsilon_vec = _mm_set1_ps(-epsilon);

            // Process 16 bytes (4 f32s) at a time
            let chunks = memory_data.chunks_exact(16);
            let remainder = chunks.remainder();

            for (chunk_idx, chunk) in chunks.enumerate() {
                let data = _mm_loadu_ps(chunk.as_ptr() as *const f32);

                // Calculate diff = data - target
                let diff = _mm_sub_ps(data, target_vec);

                // Check if -epsilon <= diff <= epsilon
                let ge_neg_eps = _mm_cmpge_ps(diff, neg_epsilon_vec);
                let le_eps = _mm_cmple_ps(diff, epsilon_vec);
                let in_range = _mm_and_ps(ge_neg_eps, le_eps);

                let mask = _mm_movemask_ps(in_range);

                if mask != 0 {
                    for i in 0..4 {
                        if (mask >> i) & 1 == 1 {
                            // Double-check to handle edge cases and ensure finite
                            let value = f32::from_le_bytes([chunk[i * 4], chunk[i * 4 + 1], chunk[i * 4 + 2], chunk[i * 4 + 3]]);
                            if value.is_finite() && (value - target).abs() <= epsilon {
                                results.push(SearchResult::new(start + chunk_idx * 16 + i * 4, SearchType::Float));
                            }
                        }
                    }
                }
            }

            // Handle remainder
            if remainder.len() >= 4 {
                for i in 0..=(remainder.len() - 4) {
                    let value = f32::from_le_bytes([remainder[i], remainder[i + 1], remainder[i + 2], remainder[i + 3]]);
                    if value.is_finite() && (value - target).abs() <= epsilon {
                        let base_offset = memory_data.len() - remainder.len();
                        results.push(SearchResult::new(start + base_offset + i, SearchType::Float));
                    }
                }
            }
        }
    }

    results
}

// SIMD-optimized f64 search for x86_64
#[cfg(target_arch = "x86_64")]
fn search_f64_simd(memory_data: &[u8], target: f64, epsilon: f64, start: usize) -> Vec<SearchResult> {
    use std::arch::x86_64::*;

    let mut results = Vec::new();

    // Handle special cases separately
    if !target.is_finite() {
        if memory_data.len() >= 8 {
            for i in 0..=memory_data.len() - 8 {
                let value = f64::from_le_bytes([
                    memory_data[i],
                    memory_data[i + 1],
                    memory_data[i + 2],
                    memory_data[i + 3],
                    memory_data[i + 4],
                    memory_data[i + 5],
                    memory_data[i + 6],
                    memory_data[i + 7],
                ]);
                if (value.is_nan() && target.is_nan()) || value == target {
                    results.push(SearchResult::new(start + i, SearchType::Double));
                }
            }
        }
        return results;
    }

    unsafe {
        // Try AVX2 first (processes 4 doubles at a time)
        if is_x86_feature_detected!("avx2") {
            let target_vec = _mm256_set1_pd(target);
            let epsilon_vec = _mm256_set1_pd(epsilon);
            let neg_epsilon_vec = _mm256_set1_pd(-epsilon);

            // Process 32 bytes (4 f64s) at a time
            let chunks = memory_data.chunks_exact(32);
            let remainder = chunks.remainder();

            for (chunk_idx, chunk) in chunks.enumerate() {
                let data = _mm256_loadu_pd(chunk.as_ptr() as *const f64);

                let diff = _mm256_sub_pd(data, target_vec);
                let ge_neg_eps = _mm256_cmp_pd(diff, neg_epsilon_vec, _CMP_GE_OQ);
                let le_eps = _mm256_cmp_pd(diff, epsilon_vec, _CMP_LE_OQ);
                let in_range = _mm256_and_pd(ge_neg_eps, le_eps);

                let mask = _mm256_movemask_pd(in_range);

                if mask != 0 {
                    for i in 0..4 {
                        if (mask >> i) & 1 == 1 {
                            let offset = i * 8;
                            let value = f64::from_le_bytes([
                                chunk[offset],
                                chunk[offset + 1],
                                chunk[offset + 2],
                                chunk[offset + 3],
                                chunk[offset + 4],
                                chunk[offset + 5],
                                chunk[offset + 6],
                                chunk[offset + 7],
                            ]);
                            if value.is_finite() && (value - target).abs() <= epsilon {
                                results.push(SearchResult::new(start + chunk_idx * 32 + offset, SearchType::Double));
                            }
                        }
                    }
                }
            }

            // Handle remainder
            if remainder.len() >= 8 {
                for i in 0..=(remainder.len() - 8) {
                    let value = f64::from_le_bytes([
                        remainder[i],
                        remainder[i + 1],
                        remainder[i + 2],
                        remainder[i + 3],
                        remainder[i + 4],
                        remainder[i + 5],
                        remainder[i + 6],
                        remainder[i + 7],
                    ]);
                    if value.is_finite() && (value - target).abs() <= epsilon {
                        let base_offset = memory_data.len() - remainder.len();
                        results.push(SearchResult::new(start + base_offset + i, SearchType::Double));
                    }
                }
            }
        } else if is_x86_feature_detected!("sse2") {
            let target_vec = _mm_set1_pd(target);
            let epsilon_vec = _mm_set1_pd(epsilon);
            let neg_epsilon_vec = _mm_set1_pd(-epsilon);

            // Process 16 bytes (2 f64s) at a time
            let chunks = memory_data.chunks_exact(16);
            let remainder = chunks.remainder();

            for (chunk_idx, chunk) in chunks.enumerate() {
                let data = _mm_loadu_pd(chunk.as_ptr() as *const f64);

                let diff = _mm_sub_pd(data, target_vec);
                let ge_neg_eps = _mm_cmpge_pd(diff, neg_epsilon_vec);
                let le_eps = _mm_cmple_pd(diff, epsilon_vec);
                let in_range = _mm_and_pd(ge_neg_eps, le_eps);

                let mask = _mm_movemask_pd(in_range);

                if mask != 0 {
                    for i in 0..2 {
                        if (mask >> i) & 1 == 1 {
                            let offset = i * 8;
                            let value = f64::from_le_bytes([
                                chunk[offset],
                                chunk[offset + 1],
                                chunk[offset + 2],
                                chunk[offset + 3],
                                chunk[offset + 4],
                                chunk[offset + 5],
                                chunk[offset + 6],
                                chunk[offset + 7],
                            ]);
                            if value.is_finite() && (value - target).abs() <= epsilon {
                                results.push(SearchResult::new(start + chunk_idx * 16 + offset, SearchType::Double));
                            }
                        }
                    }
                }
            }

            // Handle remainder
            if remainder.len() >= 8 {
                for i in 0..=(remainder.len() - 8) {
                    let value = f64::from_le_bytes([
                        remainder[i],
                        remainder[i + 1],
                        remainder[i + 2],
                        remainder[i + 3],
                        remainder[i + 4],
                        remainder[i + 5],
                        remainder[i + 6],
                        remainder[i + 7],
                    ]);
                    if value.is_finite() && (value - target).abs() <= epsilon {
                        let base_offset = memory_data.len() - remainder.len();
                        results.push(SearchResult::new(start + base_offset + i, SearchType::Double));
                    }
                }
            }
        }
    }

    results
}

// Helper function to compare values based on type and comparison
fn float_eps(old: f32) -> f32 {
    // 0.1% relative or 1e-4 absolute minimum
    (old.abs() * 1e-3).max(1e-4)
}
fn double_eps(old: f64) -> f64 {
    // 0.01% relative or 1e-6 absolute minimum
    (old.abs() * 1e-4).max(1e-6)
}

// Helper function to compare values based on type and comparison
pub fn compare_values(old_bytes: &[u8], new_bytes: &[u8], search_type: SearchType, comparison: UnknownComparison) -> bool {
    match search_type {
        SearchType::Byte => {
            let old = old_bytes[0];
            let new = new_bytes[0];
            match comparison {
                UnknownComparison::Decreased => new < old,
                UnknownComparison::Increased => new > old,
                UnknownComparison::Changed => new != old,
                UnknownComparison::Unchanged => new == old,
            }
        }
        SearchType::Short => {
            let old = i16::from_le_bytes([old_bytes[0], old_bytes[1]]);
            let new = i16::from_le_bytes([new_bytes[0], new_bytes[1]]);
            match comparison {
                UnknownComparison::Decreased => new < old,
                UnknownComparison::Increased => new > old,
                UnknownComparison::Changed => new != old,
                UnknownComparison::Unchanged => new == old,
            }
        }
        SearchType::Int => {
            let old = i32::from_le_bytes([old_bytes[0], old_bytes[1], old_bytes[2], old_bytes[3]]);
            let new = i32::from_le_bytes([new_bytes[0], new_bytes[1], new_bytes[2], new_bytes[3]]);
            match comparison {
                UnknownComparison::Decreased => new < old,
                UnknownComparison::Increased => new > old,
                UnknownComparison::Changed => new != old,
                UnknownComparison::Unchanged => new == old,
            }
        }
        SearchType::Int64 => {
            let old = i64::from_le_bytes([
                old_bytes[0],
                old_bytes[1],
                old_bytes[2],
                old_bytes[3],
                old_bytes[4],
                old_bytes[5],
                old_bytes[6],
                old_bytes[7],
            ]);
            let new = i64::from_le_bytes([
                new_bytes[0],
                new_bytes[1],
                new_bytes[2],
                new_bytes[3],
                new_bytes[4],
                new_bytes[5],
                new_bytes[6],
                new_bytes[7],
            ]);
            match comparison {
                UnknownComparison::Decreased => new < old,
                UnknownComparison::Increased => new > old,
                UnknownComparison::Changed => new != old,
                UnknownComparison::Unchanged => new == old,
            }
        }
        SearchType::Float => {
            let old = f32::from_le_bytes([old_bytes[0], old_bytes[1], old_bytes[2], old_bytes[3]]);
            let new = f32::from_le_bytes([new_bytes[0], new_bytes[1], new_bytes[2], new_bytes[3]]);
            if old.is_finite() && new.is_finite() {
                let eps = float_eps(old);
                match comparison {
                    UnknownComparison::Decreased => new < old - eps,
                    UnknownComparison::Increased => new > old + eps,
                    UnknownComparison::Changed => (new - old).abs() > eps,
                    UnknownComparison::Unchanged => (new - old).abs() <= eps,
                }
            } else {
                match comparison {
                    UnknownComparison::Changed => new != old,
                    UnknownComparison::Unchanged => new == old,
                    _ => false,
                }
            }
        }
        SearchType::Double => {
            let old = f64::from_le_bytes([
                old_bytes[0],
                old_bytes[1],
                old_bytes[2],
                old_bytes[3],
                old_bytes[4],
                old_bytes[5],
                old_bytes[6],
                old_bytes[7],
            ]);
            let new = f64::from_le_bytes([
                new_bytes[0],
                new_bytes[1],
                new_bytes[2],
                new_bytes[3],
                new_bytes[4],
                new_bytes[5],
                new_bytes[6],
                new_bytes[7],
            ]);
            if old.is_finite() && new.is_finite() {
                let eps = double_eps(old);
                match comparison {
                    UnknownComparison::Decreased => new < old - eps,
                    UnknownComparison::Increased => new > old + eps,
                    UnknownComparison::Changed => (new - old).abs() > eps,
                    UnknownComparison::Unchanged => (new - old).abs() <= eps,
                }
            } else {
                match comparison {
                    UnknownComparison::Changed => new != old,
                    UnknownComparison::Unchanged => new == old,
                    _ => false,
                }
            }
        }
        _ => false,
    }
}

fn is_ascii_printable(b: u8) -> bool {
    (0x20..=0x7E).contains(&b) || b == b'\t' || b == b'\r' || b == b'\n'
}
fn is_boundary_byte(b: u8) -> bool {
    b == 0 || !is_ascii_printable(b)
}
fn is_printable_u16(u: u16) -> bool {
    // Conservative: ASCII printable + common whitespace
    (0x20..=0x7E).contains(&u) || u == 0x09 || u == 0x0A || u == 0x0D
}

fn encode_utf16_le(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len() * 2);
    for u in s.encode_utf16() {
        out.extend_from_slice(&u.to_le_bytes());
    }
    out
}

// Replace/implement string search with alignment + boundary checks
fn search_string_in_memory(memory_data: &[u8], search_str: &str, base_addr: usize) -> Vec<SearchResult> {
    let mut results = Vec::new();

    // Minimum length to reduce random matches
    if search_str.len() < 3 {
        return results;
    }
    // UTF-8 exact substring with boundary checks
    let utf8_bytes = search_str.as_bytes();
    if !utf8_bytes.is_empty() {
        let finder = memmem::Finder::new(utf8_bytes);
        for pos in finder.find_iter(memory_data) {
            // Boundary before
            let ok_prev = if pos == 0 { true } else { is_boundary_byte(memory_data[pos - 1]) };
            // Boundary after
            let end = pos + utf8_bytes.len();
            let ok_next = if end >= memory_data.len() { true } else { is_boundary_byte(memory_data[end]) };

            if ok_prev && ok_next {
                results.push(SearchResult::new(base_addr + pos, SearchType::String));
            }
        }
    }

    // UTF-16LE exact substring with even alignment and boundary checks
    let utf16le_bytes = encode_utf16_le(search_str);
    if utf16le_bytes.len() >= 2 {
        let finder16 = memmem::Finder::new(&utf16le_bytes);
        for pos in finder16.find_iter(memory_data) {
            // Must be even-aligned in the buffer for UTF-16LE
            if pos % 2 != 0 {
                continue;
            }

            // Boundary before (u16)
            let ok_prev = if pos < 2 {
                true
            } else {
                let prev = u16::from_le_bytes([memory_data[pos - 2], memory_data[pos - 1]]);
                prev == 0 || !is_printable_u16(prev)
            };

            // Boundary after (u16)
            let end = pos + utf16le_bytes.len();
            let ok_next = if end + 1 >= memory_data.len() {
                true
            } else {
                let next = u16::from_le_bytes([memory_data[end], memory_data[end + 1]]);
                next == 0 || !is_printable_u16(next)
            };

            if ok_prev && ok_next {
                results.push(SearchResult::new(base_addr + pos, SearchType::StringUtf16));
            }
        }
    }

    results
}
