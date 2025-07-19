use std::{
    cmp::min,
    collections::{HashMap, HashSet},
    mem,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, SystemTime},
};

use crate::{FreezeMessage, MessageCommand, SearchContext, SearchMode, SearchResult, SearchType, SearchValue};
use boyer_moore_magiclen::BMByte;
use i18n_embed_fl::fl;
use proc_maps::get_process_maps;
use process_memory::{PutAddress, TryIntoProcessHandle, copy_address};
use rayon::prelude::*;
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
        let old_results_arc: Arc<Mutex<Vec<SearchResult>>> = mem::replace(&mut search_context.results, Arc::new(Mutex::new(Vec::new())));
        let old_results = old_results_arc.lock().unwrap().clone();
        search_context.total_bytes = old_results.len();
        search_context.current_bytes.swap(0, Ordering::SeqCst);
        search_context.old_results.push(old_results.clone());

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
        let results = search_context.results.clone();
        let search_complete = search_context.search_complete.clone();
        search_complete.store(false, Ordering::SeqCst);

        // Spawn a separate thread to handle the parallel search
        std::thread::spawn(move || {
            // Process chunks in parallel using rayon
            let chunk_results: Vec<Vec<SearchResult>> = chunks
                .par_iter()
                .map(|(from, to)| {
                    let handle = match pid.try_into_process_handle() {
                        Ok(h) => h,
                        Err(e) => {
                            eprintln!("Failed to get process handle: {}", e);
                            current_bytes.fetch_add(to - from, Ordering::SeqCst); // Still update progress
                            return Vec::new();
                        }
                    };

                    let chunk = &old_results[*from..*to];
                    let updated = update_results(chunk, &value_text, &handle);
                    current_bytes.fetch_add(to - from, Ordering::SeqCst); // Update progress atomically
                    updated
                })
                .collect();

            // Merge all results
            let mut all_results = Vec::new();
            for chunk_result in chunk_results {
                all_results.extend(chunk_result);
            }

            results.lock().unwrap().extend(all_results);

            // Mark search as complete
            search_complete.store(true, Ordering::SeqCst);
        });
    }

    fn spawn_parallel_search(&mut self, search_value: SearchValue, regions: Vec<(usize, usize)>, search_index: usize) {
        let search_context = self.searches.get(search_index).unwrap();
        let pid = self.pid;
        let current_bytes = search_context.current_bytes.clone();
        let results = search_context.results.clone();
        let search_complete = search_context.search_complete.clone(); // Clone the completion flag
        search_complete.store(false, Ordering::SeqCst);

        // Spawn a separate thread to handle the parallel search
        std::thread::spawn(move || {
            // Process regions in parallel using rayon
            let region_results: Vec<Vec<SearchResult>> = regions
                .par_iter()
                .map(|(start, size)| {
                    let handle = match (pid as process_memory::Pid).try_into_process_handle() {
                        Ok(h) => h,
                        Err(_) => {
                            current_bytes.fetch_add(*size, Ordering::SeqCst);
                            return Vec::new();
                        }
                    };

                    // Try to read memory, but don't log every failure
                    match copy_address(*start, *size, &handle) {
                        Ok(memory_data) => {
                            let mut local_results = Vec::new();

                            match search_value.0 {
                                SearchType::Guess => {
                                    let val: String = String::from_utf8(search_value.1.clone()).unwrap();

                                    if let Ok(search_value) = SearchType::Int.from_string(&val) {
                                        let search_data = &search_value.1;
                                        let r = search_memory(&memory_data, search_data, SearchType::Int, *start);
                                        local_results.extend(r);
                                    }
                                    if let Ok(search_value) = SearchType::Float.from_string(&val) {
                                        let search_data = &search_value.1;
                                        let r = search_memory(&memory_data, search_data, SearchType::Float, *start);
                                        local_results.extend(r);
                                    }
                                    if let Ok(search_value) = SearchType::Double.from_string(&val) {
                                        let search_data = &search_value.1;
                                        let r = search_memory(&memory_data, search_data, SearchType::Double, *start);
                                        local_results.extend(r);
                                    }
                                }
                                _ => {
                                    let search_data = &search_value.1;
                                    let r = search_memory(&memory_data, search_data, search_value.0, *start);
                                    local_results.extend(r);
                                }
                            }
                            current_bytes.fetch_add(*size, Ordering::SeqCst);
                            local_results
                        }
                        Err(_) => {
                            // Silently skip inaccessible regions - this is normal
                            current_bytes.fetch_add(*size, Ordering::SeqCst);
                            Vec::new()
                        }
                    }
                })
                .collect();

            // Merge all results
            let mut all_results = Vec::new();
            for region_result in region_results {
                all_results.extend(region_result);
            }

            // Update results
            results.lock().unwrap().extend(all_results);

            // Mark search as complete
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

fn search_memory(memory_data: &Vec<u8>, search_data: &Vec<u8>, search_type: SearchType, start: usize) -> Vec<SearchResult> {
    let mut result = Vec::new();
    let search_bytes = BMByte::from(search_data).unwrap();

    for i in search_bytes.find_all_in(memory_data) {
        result.push(SearchResult::new(i + start, search_type));
    }
    result
}
