use crate::{FreezeMessage, MessageCommand, SearchContext, SearchMode, SearchResult, SearchType, SearchValue};
use crossbeam_channel;
use i18n_embed_fl::fl;
use memchr::memmem;
use proc_maps::get_process_maps;
use process_memory::{PutAddress, TryIntoProcessHandle, copy_address};
use rayon::prelude::*;
use std::sync::{atomic::Ordering, mpsc};
use std::{
    cmp::min,
    collections::{HashMap, HashSet},
    thread,
    time::{Duration, SystemTime},
};
use sysinfo::*;

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
    pub freeze_sender: mpsc::Sender<FreezeMessage>,
    pub error_text: String,
    pub show_results: bool,
    pub set_focus: bool,

    pub(crate) edit_address: usize,
}

impl Default for GameCheetahEngine {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel::<FreezeMessage>();

        thread::spawn(move || {
            let mut freezed_values = HashMap::new();
            let mut pid = 0;
            loop {
                if let Ok(msg) = rx.try_recv() {
                    match msg.msg {
                        // MessageCommand::Quit => { return; },
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
                for (addr, value) in &freezed_values {
                    if let Ok(handle) = (pid as process_memory::Pid).try_into_process_handle() {
                        handle.put_address(*addr, &value.1).unwrap_or_default();
                    }
                }
                thread::sleep(Duration::from_millis(500));
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
        if !matches!(self.searches.get_mut(search_index).unwrap().searching, SearchMode::None) {
            return;
        }
        self.remove_freezes(search_index);

        self.searches.get_mut(search_index).unwrap().searching = SearchMode::Memory;

        match get_process_maps(self.pid) {
            Ok(maps) => {
                let mut regions = Vec::new();
                self.searches.get_mut(search_index).unwrap().total_bytes = 0;
                self.searches.get_mut(search_index).unwrap().current_bytes.swap(0, Ordering::SeqCst);

                for map in maps {
                    // More aggressive filtering
                    if cfg!(target_os = "windows") {
                        if let Some(file_name) = map.filename() {
                            if file_name.starts_with("C:\\WINDOWS\\") {
                                continue;
                            }
                        }
                    } else if cfg!(target_os = "linux") {
                        // Skip non-writable, executable, and system regions
                        if !map.is_write() || map.is_exec() {
                            continue;
                        }

                        // Skip kernel and system libraries
                        if let Some(file_name) = map.filename() {
                            if file_name.starts_with("/usr/")
                                || file_name.starts_with("/lib/")
                                || file_name.starts_with("/lib64/")
                                || file_name.starts_with("/dev/")
                                || file_name.starts_with("/proc/")
                                || file_name.starts_with("/sys/")
                            {
                                continue;
                            }
                        }
                    /*
                    // Skip special memory regions
                    if map.p == "[vvar]" || map.pathname == "[vdso]" || map.pathname == "[vsyscall]" {
                        continue;
                    }*/
                    } else {
                        if !map.is_write() || map.is_exec() || map.filename().is_none() || map.size() < 1024 * 1024 {
                            continue;
                        }
                        if let Some(file_name) = map.filename() {
                            if file_name.starts_with("/usr/lib") || file_name.starts_with("/System/") {
                                continue;
                            }
                        }
                    }

                    let mut size = map.size();
                    let mut start = map.start();
                    self.searches.get_mut(search_index).unwrap().total_bytes += size;

                    const MAX_BLOCK: usize = 10 * 1024 * 1024;

                    while size > MAX_BLOCK + 3 {
                        regions.push((start, MAX_BLOCK + 3));
                        start += MAX_BLOCK;
                        size -= MAX_BLOCK;
                    }
                    regions.push((start, size));
                }

                let current_search = self.searches.get(search_index).unwrap();
                let search_for_value = current_search.search_type.from_string(&current_search.search_value_text).unwrap();
                self.error_text.clear();

                // Use rayon to process regions in parallel
                self.spawn_parallel_search(search_for_value, regions, search_index);
            }
            Err(err) => {
                eprintln!("error getting process maps for pid {}: {}", self.pid, err);
                self.error_text = format!("error getting process maps for pid {}: {}", self.pid, err);
            }
        }
    }

    pub fn filter_searches(&mut self, search_index: usize) {
        self.remove_freezes(search_index);
        let search_context = self.searches.get_mut(search_index).unwrap();
        search_context.searching = SearchMode::Percent;

        // Collect old results
        let old_results = search_context.collect_results();

        search_context.total_bytes = old_results.len();
        search_context.current_bytes.swap(0, Ordering::SeqCst);
        search_context.old_results.push(old_results.clone());

        // Reset the channel for new results
        let (tx, rx) = crossbeam_channel::unbounded();
        search_context.results_sender = tx;
        search_context.results_receiver = rx;
        search_context.result_count.store(0, Ordering::SeqCst);

        search_context.invalidate_cache();

        let max_block = 200 * 1024;
        let chunks: Vec<(usize, usize)> = (0..old_results.len())
            .step_by(max_block)
            .map(|i| (i, min(i + max_block, old_results.len())))
            .collect();

        self.spawn_update_search(search_index, old_results, chunks);
    }

    pub fn remove_freezes(&mut self, search_index: usize) {
        let search_context = self.searches.get_mut(search_index).unwrap();
        GameCheetahEngine::remove_freezes_from(&self.freeze_sender, &mut search_context.freezed_addresses);
    }

    pub fn remove_freezes_from(freeze_sender: &mpsc::Sender<FreezeMessage>, freezes: &mut std::collections::HashSet<usize>) {
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
            return;
        };
        let mut parents = HashSet::new();
        let cur_process = sys.process(current_pid).unwrap();
        for (pid2, process) in sys.processes() {
            if process.memory() == 0 || process.user_id() != cur_process.user_id() {
                continue;
            }
            if let Some(parent) = process.parent() {
                if parents.contains(&parent) {
                    continue;
                }
                parents.insert(parent);
            }

            let pid = pid2.as_u32();
            let user = match process.user_id() {
                Some(user) => user.to_string(),
                None => "".to_string(),
            };
            self.processes.push(ProcessInfo {
                pid: pid.try_into().unwrap(),
                name: process.name().to_string_lossy().to_string(),
                cmd: format!("{:?} ", process.cmd()),
                user,
                memory: process.memory() as usize,
            });
        }
        self.processes.sort_by(|a, b| {
            let cmp = a.name.cmp(&b.name);
            if cmp == std::cmp::Ordering::Equal {
                return a.pid.cmp(&b.pid);
            }
            cmp
        });
    }

    fn spawn_update_search(&mut self, search_index: usize, old_results: Vec<SearchResult>, chunks: Vec<(usize, usize)>) {
        let search_context = self.searches.get_mut(search_index).unwrap();
        let current_bytes = search_context.current_bytes.clone();
        let pid = self.pid;
        let value_text = search_context.search_value_text.clone();
        let results_sender = search_context.results_sender.clone();
        let result_count = search_context.result_count.clone();
        let search_complete = search_context.search_complete.clone();
        let cache_valid = search_context.cache_valid.clone(); // Add this

        search_complete.store(false, Ordering::SeqCst);

        std::thread::spawn(move || {
            chunks.par_iter().for_each(|(from, to)| {
                let handle = match pid.try_into_process_handle() {
                    Ok(h) => h,
                    Err(e) => {
                        eprintln!("Failed to get process handle: {}", e);
                        current_bytes.fetch_add(to - from, Ordering::SeqCst);
                        return;
                    }
                };

                let chunk = &old_results[*from..*to];
                let updated = update_results(chunk, &value_text, &handle);

                if !updated.is_empty() {
                    let count = updated.len();
                    result_count.fetch_add(count, Ordering::SeqCst);
                    let _ = results_sender.send(updated);
                    // DON'T invalidate cache here!
                }

                current_bytes.fetch_add(to - from, Ordering::SeqCst);
            });

            // Invalidate cache only ONCE when search is complete
            cache_valid.store(false, Ordering::Release);
            search_complete.store(true, Ordering::SeqCst);
        });
    }

    fn spawn_parallel_search(&mut self, search_value: SearchValue, regions: Vec<(usize, usize)>, search_index: usize) {
        let search_context = self.searches.get_mut(search_index).unwrap();
        let pid = self.pid;
        let current_bytes = search_context.current_bytes.clone();
        let results_sender = search_context.results_sender.clone();
        let result_count = search_context.result_count.clone();
        let search_complete = search_context.search_complete.clone();
        let cache_valid = search_context.cache_valid.clone();

        search_complete.store(false, Ordering::SeqCst);

        std::thread::spawn(move || {
            regions.par_iter().for_each(|(start, size)| {
                let handle = match (pid as process_memory::Pid).try_into_process_handle() {
                    Ok(h) => h,
                    Err(_) => {
                        current_bytes.fetch_add(*size, Ordering::SeqCst);
                        return;
                    }
                };

                match copy_address(*start, *size, &handle) {
                    Ok(memory_data) => {
                        let local_results = match search_value.0 {
                            SearchType::Guess => {
                                let mut results = Vec::new();
                                let val: String = String::from_utf8(search_value.1.clone()).unwrap();

                                let types = vec![SearchType::Int, SearchType::Float, SearchType::Double];
                                for search_type in types {
                                    if let Ok(search_value) = search_type.from_string(&val) {
                                        let r = search_memory(&memory_data, &search_value.1, search_type, *start);
                                        results.extend(r);
                                    }
                                }
                                results
                            }
                            _ => search_memory(&memory_data, &search_value.1, search_value.0, *start),
                        };

                        if !local_results.is_empty() {
                            let count = local_results.len();
                            result_count.fetch_add(count, Ordering::SeqCst);
                            let _ = results_sender.send(local_results);
                            // DON'T invalidate cache here!
                        }

                        current_bytes.fetch_add(*size, Ordering::SeqCst);
                    }
                    Err(_) => {
                        current_bytes.fetch_add(*size, Ordering::SeqCst);
                    }
                }
            });

            // Invalidate cache only ONCE when search is complete
            cache_valid.store(false, Ordering::Release);
            search_complete.store(true, Ordering::SeqCst);
        });
    }

    pub(crate) fn select_process(&mut self, process: &ProcessInfo) {
        self.pid = process.pid;
        self.freeze_sender
            .send(FreezeMessage::from_addr(MessageCommand::Pid, process.pid as usize))
            .unwrap_or_default();
        self.process_name = process.name.clone();
        self.show_process_window = false;
    }
}

fn update_results<T>(old_results: &[SearchResult], value_text: &str, handle: &T) -> Vec<SearchResult>
where
    T: process_memory::CopyAddress,
{
    let mut results = Vec::new();
    for result in old_results {
        match result.search_type.from_string(value_text) {
            Ok(my_int) => {
                if let Ok(buf) = copy_address(result.addr, result.search_type.get_byte_length(), handle) {
                    let val = SearchValue(result.search_type, buf);
                    if val.1 == my_int.1 {
                        results.push(*result);
                    }
                }
            }
            Err(err) => {
                eprintln!("Error converting {:?}: {}", result.search_type, err);
                //   self.error_text = format!("Error converting {:?}: {}", result.search_type, err);
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
        // For floats and doubles, fall back to boyer-moore or memmem
        SearchType::Float | SearchType::Double | SearchType::Guess => {
            // Use memmem for better performance than boyer-moore
            let finder = memmem::Finder::new(search_data);
            for pos in finder.find_iter(memory_data) {
                result.push(SearchResult::new(pos + start, search_type));
            }
        } /*
          _ => {
              // Fallback to boyer-moore for other types
              let search_bytes = BMByte::from(search_data.to_vec()).unwrap();
              for i in search_bytes.find_all_in(memory_data.to_vec()) {
                  result.push(SearchResult::new(i + start, search_type));
              }
          }*/
    }

    result
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
