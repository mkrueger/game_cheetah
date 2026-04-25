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

mod memory_reader;
mod simd;
mod string_search;
mod unknown;

#[cfg(target_os = "linux")]
pub use memory_reader::ProcessMemReader;
use memory_reader::fast_read_memory;
use simd::{get_epsilon_f32, get_epsilon_f64, search_aligned_integers};
#[cfg(target_arch = "x86_64")]
use simd::{search_f32_simd, search_f64_simd};
pub use string_search::search_string_in_memory;
pub use unknown::compare_values;

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

        let (tx, rx) = SearchContext::result_channel();
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
        let guess_value_text = if matches!(search_data.0, SearchType::Guess) {
            Some(String::from_utf8(search_data.1.clone()).unwrap_or_default())
        } else {
            None
        };

        search_complete.store(false, Ordering::SeqCst);

        std::thread::spawn(move || {
            // Use thread-local ProcessMemReader for efficient repeated reads
            #[cfg(target_os = "linux")]
            thread_local! {
                static MEM_READER: std::cell::RefCell<Option<(process_memory::Pid, ProcessMemReader)>> = const { std::cell::RefCell::new(None) };
            }

            regions.par_iter().for_each(|(start, size)| {
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
                                match search_type.from_string(guess_value_text.as_deref().unwrap_or_default()) {
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
        let (tx, rx) = SearchContext::result_channel();
        search_context.results_sender = tx;
        search_context.results_receiver = rx;
        search_context.invalidate_cache();

        let pid = self.pid;
        let current_bytes = search_context.current_bytes.clone();
        let results_sender = search_context.results_sender.clone();
        let search_complete = search_context.search_complete.clone();
        let cache_valid = search_context.cache_valid.clone();
        let memory_snapshot = search_context.memory_snapshot.clone();
        let previous_unknown_values = search_context.previous_unknown_values.clone();

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
                        let mut local_out: Vec<SearchResult> = Vec::with_capacity(BATCH);
                        let mut local_prev: Vec<((usize, SearchType), [u8; 8])> = Vec::with_capacity(BATCH);

                        let record = |addr: usize,
                                      ty: SearchType,
                                      newb: &[u8],
                                      local_out: &mut Vec<SearchResult>,
                                      local_prev: &mut Vec<((usize, SearchType), [u8; 8])>| {
                            local_out.push(SearchResult::new(addr, ty));
                            local_prev.push(((addr, ty), pack_bytes(newb)));
                        };

                        // 4-byte aligned
                        for i in (0..=(len.saturating_sub(4))).step_by(4) {
                            let oldb = &old_mem[i..i + 4];
                            let newb = &new_mem[i..i + 4];

                            // Fast equality prefilter for Unchanged/Changed
                            if matches!(comparison, UnknownComparison::Unchanged | UnknownComparison::Changed) && oldb == newb {
                                if matches!(comparison, UnknownComparison::Unchanged) {
                                    // For floats we still accept exact-equal as unchanged without decoding
                                    record(*base + i, SearchType::Int, newb, &mut local_out, &mut local_prev);
                                    record(*base + i, SearchType::Float, newb, &mut local_out, &mut local_prev);
                                }
                            } else {
                                if compare_values(oldb, newb, SearchType::Int, comparison) {
                                    record(*base + i, SearchType::Int, newb, &mut local_out, &mut local_prev);
                                }
                                if compare_values(oldb, newb, SearchType::Float, comparison) {
                                    record(*base + i, SearchType::Float, newb, &mut local_out, &mut local_prev);
                                }
                            }

                            if local_out.len() >= BATCH {
                                let _ = local_tx.send(std::mem::take(&mut local_out));
                            }
                        }

                        // 8-byte aligned
                        for i in (0..=(len.saturating_sub(8))).step_by(8) {
                            let oldb = &old_mem[i..i + 8];
                            let newb = &new_mem[i..i + 8];

                            if matches!(comparison, UnknownComparison::Unchanged | UnknownComparison::Changed) && oldb == newb {
                                if matches!(comparison, UnknownComparison::Unchanged) {
                                    // Exact-equal treat as unchanged, skip decoding
                                    record(*base + i, SearchType::Double, newb, &mut local_out, &mut local_prev);
                                }
                            } else if compare_values(oldb, newb, SearchType::Double, comparison) {
                                record(*base + i, SearchType::Double, newb, &mut local_out, &mut local_prev);
                            }

                            if local_out.len() >= BATCH {
                                let _ = local_tx.send(std::mem::take(&mut local_out));
                            }
                        }

                        if !local_out.is_empty() {
                            let _ = local_tx.send(local_out);
                        }

                        if !local_prev.is_empty()
                            && let Ok(mut map) = previous_unknown_values.write()
                        {
                            map.reserve(local_prev.len());
                            for (k, v) in local_prev {
                                map.insert(k, v);
                            }
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

                // Snapshot the previous-value table once so workers can read
                // without taking a read-lock per address.
                let prev_snapshot: HashMap<(usize, SearchType), [u8; 8]> = match previous_unknown_values.read() {
                    Ok(map) => map.clone(),
                    Err(_) => HashMap::new(),
                };

                // Group by address for single-read optimization
                let mut addr_to_types: std::collections::BTreeMap<usize, Vec<(SearchType, [u8; 8])>> = std::collections::BTreeMap::new();

                for r in old_results.iter() {
                    if let Some(old8) = prev_snapshot.get(&(r.addr, r.search_type)) {
                        addr_to_types.entry(r.addr).or_default().push((r.search_type, *old8));
                    }
                }

                // Group addresses by page
                let mut per_page: std::collections::BTreeMap<usize, Vec<usize>> = std::collections::BTreeMap::new();
                for addr in addr_to_types.keys() {
                    per_page.entry(addr & !(PAGE - 1)).or_default().push(*addr);
                }

                let per_page_vec: Vec<(usize, Vec<usize>)> = per_page.into_iter().collect();

                // Survivors and their fresh bytes are gathered into this map and
                // become the previous-value table for the next pass.
                type PrevMap = HashMap<(usize, SearchType), [u8; 8]>;
                let next_prev: Arc<Mutex<PrevMap>> = Arc::new(Mutex::new(HashMap::with_capacity(old_results.len())));

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
                                .and_then(|types| types.iter().filter_map(|(ty, _)| ty.fixed_byte_length().map(|len| *addr + len)).max())
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
                        let mut local_out: Vec<SearchResult> = Vec::with_capacity(BATCH);
                        let mut local_prev: Vec<((usize, SearchType), [u8; 8])> = Vec::with_capacity(BATCH);

                        for addr in sorted_addrs {
                            if let Some(types) = addr_to_types.get(&addr) {
                                let offset = addr - min_addr;

                                // Read once per address for the largest type
                                let max_len = types.iter().filter_map(|(ty, _)| ty.fixed_byte_length()).max().unwrap_or(0);
                                if offset + max_len > buf.len() {
                                    continue;
                                }
                                let new_max = &buf[offset..offset + max_len];

                                for (ty, old8) in types {
                                    let Some(len) = ty.fixed_byte_length() else {
                                        continue;
                                    };
                                    let oldb = &old8[..len];
                                    let newb = &new_max[..len];

                                    // Fast prefilter for equality in Changed/Unchanged
                                    if matches!(comparison, UnknownComparison::Unchanged | UnknownComparison::Changed) {
                                        if oldb == newb {
                                            if matches!(comparison, UnknownComparison::Unchanged) {
                                                local_out.push(SearchResult::new(addr, *ty));
                                                local_prev.push(((addr, *ty), pack_bytes(newb)));
                                            }
                                            continue; // equality handled, skip decoding
                                        } else if matches!(comparison, UnknownComparison::Changed) && !matches!(ty, SearchType::Float | SearchType::Double) {
                                            // For integers, byte-inequality is sufficient for "Changed"
                                            local_out.push(SearchResult::new(addr, *ty));
                                            local_prev.push(((addr, *ty), pack_bytes(newb)));
                                            continue;
                                        }
                                    }

                                    // Fallback to typed comparison (needed for floats/doubles or inc/dec)
                                    if compare_values(oldb, newb, *ty, comparison) {
                                        local_out.push(SearchResult::new(addr, *ty));
                                        local_prev.push(((addr, *ty), pack_bytes(newb)));
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

                        if !local_prev.is_empty()
                            && let Ok(mut map) = next_prev.lock()
                        {
                            map.reserve(local_prev.len());
                            for (k, v) in local_prev {
                                map.insert(k, v);
                            }
                        }
                    }

                    current_bytes.fetch_add(sorted_addrs_len, Ordering::Relaxed);
                });

                // Replace the previous-value table with the survivors of this pass.
                if let Ok(new_map) = Arc::try_unwrap(next_prev).map(|m| m.into_inner().unwrap_or_default())
                    && let Ok(mut map) = previous_unknown_values.write()
                {
                    *map = new_map;
                }
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

/// Pack up to 8 bytes into a fixed-size buffer suitable for the
/// unknown-search previous-value table.
fn pack_bytes(src: &[u8]) -> [u8; 8] {
    let mut out = [0u8; 8];
    let n = src.len().min(8);
    out[..n].copy_from_slice(&src[..n]);
    out
}

fn update_results<T>(old_results: &[SearchResult], value_text: &str, handle: &T) -> Vec<SearchResult>
where
    T: process_memory::CopyAddress,
{
    let mut results = Vec::new();
    for result in old_results {
        match result.search_type.from_string(value_text) {
            Ok(search_value) => {
                let Some(byte_len) = result.search_type.fixed_byte_length() else {
                    continue;
                };
                if let Ok(buf) = copy_address(result.addr, byte_len, handle) {
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
