use std::{
    cmp::min,
    collections::HashMap,
    mem,
    sync::{atomic::Ordering, mpsc, Arc, Mutex},
    thread,
    time::Duration,
};

use crate::{
    Message, MessageCommand, SearchContext, SearchMode, SearchResult, SearchType, SearchValue,
};
use boyer_moore_magiclen::BMByte;
use proc_maps::get_process_maps;
use process_memory::{copy_address, PutAddress, TryIntoProcessHandle};
use sysinfo::*;
use threadpool::ThreadPool;

pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub cmd: String,
    pub memory: usize,
}

pub struct GameCheetahEngine {
    pub pid: process_memory::Pid,
    pub process_name: String,
    pub show_process_window: bool,

    pub process_filter: String,
    pub processes: Vec<ProcessInfo>,

    pub current_search: usize,
    pub searches: Vec<Box<SearchContext>>,
    pub search_threads: ThreadPool,

    pub freeze_sender: mpsc::Sender<Message>,
    pub error_text: String,
    pub show_results: bool,
}

impl Default for GameCheetahEngine {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel::<Message>();

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
            current_search: 0,
            searches: vec![Box::new(SearchContext::new("Search 1".to_string()))],
            search_threads: ThreadPool::new(16),
            freeze_sender: tx,
            show_results: false,
        }
    }
}

impl GameCheetahEngine {
    pub fn new_search(&mut self) {
        let ctx = SearchContext::new(format!("Search {}", 1 + self.searches.len()));
        self.current_search = self.searches.len();
        self.searches.push(Box::new(ctx));
    }

    pub fn initial_search(&mut self, search_index: usize) {
        if !matches!(
            self.searches.get_mut(search_index).unwrap().searching,
            SearchMode::None
        ) {
            return;
        }
        self.remove_freezes(search_index);

        self.searches.get_mut(search_index).unwrap().searching = SearchMode::Memory;

        match get_process_maps(self.pid) {
            Ok(maps) => {
                self.searches.get_mut(search_index).unwrap().total_bytes = 0;
                self.searches
                    .get_mut(search_index)
                    .unwrap()
                    .current_bytes
                    .swap(0, Ordering::SeqCst);
                for map in maps {
                    if cfg!(target_os = "windows") {
                        if let Some(file_name) = map.filename() {
                            if file_name.starts_with("C:\\WINDOWS\\SysWOW64") {
                                continue;
                            }
                        }
                    } else if cfg!(target_os = "linux") {
                    } else {
                        if !map.is_write()
                            || map.is_exec()
                            || map.filename().is_none()
                            || map.size() < 1024 * 1024
                        {
                            continue;
                        }
                        if let Some(file_name) = map.filename() {
                            if file_name.starts_with("/usr/lib") {
                                continue;
                            }
                        }
                    }
                    let mut size = map.size();
                    let mut start = map.start();
                    self.searches.get_mut(search_index).unwrap().total_bytes += size;

                    let max_block = 10 * 1024 * 1024;

                    let current_search = self.searches.get(search_index).unwrap();
                    let search_for_value = current_search
                        .search_type
                        .from_string(&current_search.search_value_text)
                        .unwrap();
                    self.error_text.clear();

                    while size > max_block + 3 {
                        self.spawn_first_search_thread(
                            search_for_value.clone(),
                            start,
                            max_block + 3,
                            search_index,
                        );

                        start += max_block;
                        size -= max_block;
                    }
                    self.spawn_first_search_thread(search_for_value, start, size, search_index);
                }
            }
            Err(err) => {
                eprintln!("error getting process maps for pid {}: {}", self.pid, err);
                self.error_text =
                    format!("error getting process maps for pid {}: {}", self.pid, err);
            }
        }
    }

    pub fn filter_searches(&mut self, search_index: usize) {
        self.remove_freezes(search_index);
        let search_context = self.searches.get_mut(search_index).unwrap();
        search_context.searching = SearchMode::Percent;
        let old_results_arc: Arc<Mutex<Vec<SearchResult>>> = mem::replace(
            &mut search_context.results,
            Arc::new(Mutex::new(Vec::new())),
        );
        let old_results = old_results_arc.lock().unwrap();
        search_context.total_bytes = old_results.len();
        search_context.current_bytes.swap(0, Ordering::SeqCst);
        search_context.old_results.push(old_results.clone());

        let mut i = 0;
        let max_i: usize = old_results.len();
        let max_block = 200 * 1024;
        while i < max_i {
            let j = min(i + max_block, max_i);
            self.spawn_update_thread(search_index, old_results_arc.clone(), i, j);
            i = j;
        }
    }

    pub fn remove_freezes(&mut self, search_index: usize) {
        let search_context = self.searches.get_mut(search_index).unwrap();
        GameCheetahEngine::remove_freezes_from(
            &self.freeze_sender,
            &mut search_context.freezed_addresses,
        );
    }

    pub fn remove_freezes_from(
        freeze_sender: &mpsc::Sender<Message>,
        freezes: &mut std::collections::HashSet<usize>,
    ) {
        for result in freezes.iter() {
            freeze_sender
                .send(Message::from_addr(MessageCommand::Unfreeze, *result))
                .unwrap_or_default();
        }
        freezes.clear();
    }

    pub fn show_process_window(&mut self) {
        let sys = System::new_all();
        self.processes.clear();
        for (pid, process) in sys.processes() {
            if process.memory() == 0 {
                continue;
            }
            self.processes.push(ProcessInfo {
                pid: pid.as_u32(),
                name: process.name().to_string(),
                cmd: process.cmd().join(" "),
                memory: process.memory() as usize,
            });
        }
    }

    fn spawn_update_thread(
        &mut self,
        search_index: usize,
        old_results_arc: Arc<Mutex<Vec<SearchResult>>>,
        from: usize,
        to: usize,
    ) {
        if from >= to {
            return;
        }

        let search_context = self.searches.get_mut(search_index).unwrap();
        let current_bytes = search_context.current_bytes.clone();
        let pid = self.pid;
        let value_text = search_context.search_value_text.clone();
        let results: Arc<Mutex<Vec<SearchResult>>> = search_context.results.clone();

        self.search_threads.execute(move || {
            let old_results = match old_results_arc.lock() {
                Ok(old_results) => {
                    // println!("{}-{} max:{}", from, to, old_results.len());
                    old_results[from..to].to_vec()
                }
                Err(err) => {
                    eprintln!("{err}");
                    return;
                }
            };
            let handle = (pid as u32)
                .try_into_process_handle()
                .unwrap();
            let updated_results = update_results(&old_results, &value_text, &handle);
            results.lock().unwrap().extend_from_slice(&updated_results);
            current_bytes.fetch_add(to - from, Ordering::SeqCst);
        });
    }

    fn spawn_first_search_thread(
        &mut self,
        search_value: SearchValue,
        start: usize,
        size: usize,
        search_index: usize,
    ) {
        let search_context = self.searches.get(search_index).unwrap();
        let pid = self.pid;
        let current_bytes = search_context.current_bytes.clone();
        let results: Arc<Mutex<Vec<SearchResult>>> = search_context.results.clone();
        self.search_threads.execute(move || {
            let handle = (pid as process_memory::Pid)
                .try_into_process_handle()
                .unwrap();
            if let Ok(memory_data) = copy_address(start, size, &handle) {
                match search_value.0 {
                    SearchType::Guess => {
                        let val = String::from_utf8(search_value.1).unwrap();

                        if let Ok(search_value) = SearchType::Int.from_string(&val) {
                            let search_data = &search_value.1;
                            let r =
                                search_memory(&memory_data, search_data, SearchType::Int, start);
                            if !r.is_empty() {
                                results.lock().unwrap().extend_from_slice(&r);
                            }
                        }
                        if let Ok(search_value) = SearchType::Float.from_string(&val) {
                            let search_data = &search_value.1;
                            let r =
                                search_memory(&memory_data, search_data, SearchType::Float, start);
                            if !r.is_empty() {
                                results.lock().unwrap().extend_from_slice(&r);
                            }
                        }
                        if let Ok(search_value) = SearchType::Double.from_string(&val) {
                            let search_data = &search_value.1;
                            let r =
                                search_memory(&memory_data, search_data, SearchType::Double, start);
                            if !r.is_empty() {
                                results.lock().unwrap().extend_from_slice(&r);
                            }
                        }
                    }
                    _ => {
                        let search_data = &search_value.1;
                        let r = search_memory(&memory_data, search_data, search_value.0, start);
                        if !r.is_empty() {
                            results.lock().unwrap().extend_from_slice(&r);
                        }
                    }
                }
            }
            current_bytes.fetch_add(size, Ordering::SeqCst);
        });
    }
}

fn update_results<T>(
    old_results: &[SearchResult],
    value_text: &str,
    handle: &T,
) -> Vec<SearchResult>
where
    T: process_memory::CopyAddress
{
    let mut results = Vec::new();
    for result in old_results {
        match result.search_type.from_string(value_text) {
            Ok(my_int) => {
                if let Ok(buf) =
                    copy_address(result.addr, result.search_type.get_byte_length(), handle)
                {
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

fn search_memory(
    memory_data: &Vec<u8>,
    search_data: &Vec<u8>,
    search_type: SearchType,
    start: usize,
) -> Vec<SearchResult> {
    let mut result = Vec::new();
    let search_bytes = BMByte::from(search_data).unwrap();

    for i in search_bytes.find_all_in(memory_data) {
        result.push(SearchResult::new(i + start, search_type));
    }
    result
}
