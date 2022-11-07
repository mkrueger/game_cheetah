use std::{thread, sync::{mpsc, atomic::Ordering, Arc, Mutex}, collections::HashMap, time::Duration};

use needle::BoyerMoore;
use process_memory::{TryIntoProcessHandle, PutAddress, copy_address};
use threadpool::ThreadPool;
use crate::{SearchContext, Message, MessageCommand, SearchValue, SearchResult, SearchType};
use proc_maps::get_process_maps;
use sysinfo::*;

pub struct GameCheetahEngine {
    pub pid: i32,
    pub process_name: String,
    pub show_process_window: bool,

    pub process_filter: String,
    pub processes: Vec<(u32, String, String)>,

    pub current_search: usize,
    pub searches: Vec<Box<SearchContext>>,
    pub search_threads: ThreadPool,

    pub freeze_sender: mpsc::Sender<Message>,
    pub error_text: String
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
                        },
                        MessageCommand::Freeze => { 
                            freezed_values.insert(msg.addr, msg.value); 
                        },
                        MessageCommand::Unfreeze => { 
                            freezed_values.remove(&msg.addr);
                        },
                    }
                }
                for (addr, value) in &freezed_values {
                    if let Ok (handle) = (pid as process_memory::Pid).try_into_process_handle() {
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
            freeze_sender: tx
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
        if self.searches.get_mut(search_index).unwrap().searching {
            return;
        }
        self.remove_freezes(search_index);
        
        self.searches.get_mut(search_index).unwrap().searching = true;

        match get_process_maps(self.pid.try_into().unwrap()) {
            Ok(maps) => {
                self.searches.get_mut(search_index).unwrap().total_bytes = 0;
                self.searches.get_mut(search_index).unwrap().current_bytes.swap(0, Ordering::SeqCst);
                for map in maps {
                    if cfg!(target_os = "windows") {
                        if let Some(file_name)  = map.filename() {
                            if file_name.starts_with("C:\\WINDOWS\\SysWOW64") {
                                continue;
                            }
                        }
                    } else if cfg!(target_os = "linux") {
                    
                    } else {
                        if !map.is_write() || map.is_exec() || map.filename().is_none() || map.size() < 1 * 1024 * 1024 {
                            continue;
                        }
                        if let Some(file_name)  = map.filename() {
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
                    let search_for_value = current_search.search_type.from_string(&current_search.search_value_text).unwrap();
                    self.error_text.clear();

                    while size > max_block + 3 {
                        self.spawn_thread(search_for_value.clone(), start, max_block + 3, search_index);
                        
                        start += max_block;
                        size -= max_block;
                    }
                    self.spawn_thread(search_for_value, start, size, search_index);
                }
            } 
            Err(err) => {
                eprintln!("error getting process maps for pid {}: {}", self.pid, err);
                self.error_text = format!("error getting process maps for pid {}: {}", self.pid, err);
            }
        }
    }

    pub fn filter_searches(&mut self, search_index: usize) {
        self.remove_freezes(search_index);
        let mut search_context = self.searches.get_mut(search_index).unwrap();
    
        let mut new_results = Vec::new();
        let handle = (self.pid as process_memory::Pid).try_into_process_handle().unwrap();
        let old_results = search_context.results.lock().unwrap().clone();
        for result in &*old_results {
            match result.search_type.from_string(&search_context.search_value_text) {
                Ok(my_int) => {
                    if let Ok(buf) = copy_address(result.addr, result.search_type.get_byte_length(), &handle) {
                        let val = SearchValue(result.search_type, buf);
                        if val.1 == my_int.1 {
                            if result.freezed {
                                self.freeze_sender.send(Message {
                                    msg: MessageCommand::Freeze,
                                    addr: result.addr,
                                    value: val
                                }).unwrap_or_default();
                            }
                            new_results.push(result.clone());
                        }
                    }
                }
                Err(err) => { 
                    eprintln!("Error converting {:?}: {}", result.search_type, err);
                    self.error_text = format!("Error converting {:?}: {}", result.search_type, err);
                }
            }
        }
        search_context.search_results = new_results.len() as i64;
        search_context.results = Arc::new(Mutex::new(new_results));
        search_context.old_results.push(old_results);

    }

    pub fn remove_freezes(&self, search_index: usize) {
        let search_context = self.searches.get(search_index).unwrap();
        GameCheetahEngine::remove_freezes_from(&self.freeze_sender, &search_context.results.lock().unwrap().clone());
    }
    
    pub fn remove_freezes_from(freeze_sender: &mpsc::Sender<Message>, v: &Vec<SearchResult>) {
        for result in v {
            if result.freezed {
                freeze_sender.send(Message::from_addr(MessageCommand::Unfreeze, result.addr)).unwrap_or_default();
            }
        }
    }
    
    pub fn show_process_window(&mut self) {
        let sys = System::new_all();
        self.processes.clear();
        for (pid, process) in sys.processes() {
            self.processes.push((pid.as_u32(), process.name().to_string(), process.cmd().join(" ")));
        }
    }

    fn spawn_thread(&mut self, search_value: SearchValue, start: usize, size: usize, search_index: usize) {
        let search_context = self.searches.get(search_index).unwrap();
        let pid = self.pid;
        let current_bytes = search_context.current_bytes.clone();
        let results = search_context.results.clone();
        self.search_threads.execute(move || {
            let handle = (pid as process_memory::Pid).try_into_process_handle().unwrap();
            if let Ok(memory_data) = copy_address(start, size, &handle) {
                match search_value.0 {
                    SearchType::Guess => {
                        let val = String::from_utf8(search_value.1).unwrap();

                        if let Ok(search_value) = SearchType::Int.from_string(&val) {
                            let search_data =&search_value.1[..];
                            let r = search_memory(&memory_data, search_data, search_value.0, start);
                            if r.len() > 0 {
                                results.lock().unwrap().extend_from_slice(&r);
                            }
                        }
                        if let Ok(search_value) = SearchType::Float.from_string(&val) {
                            let search_data =&search_value.1[..];
                            let r = search_memory(&memory_data, search_data, search_value.0, start);
                            if r.len() > 0 {
                                results.lock().unwrap().extend_from_slice(&r);
                            }
                        }
                        if let Ok(search_value) = SearchType::Double.from_string(&val) {
                            let search_data =&search_value.1[..];
                            let r = search_memory(&memory_data, search_data, search_value.0, start);
                            if r.len() > 0 {
                                results.lock().unwrap().extend_from_slice(&r);
                            }
                        }
                    }
                    _ => {
                        let search_data =&search_value.1[..];
                        let r = search_memory(&memory_data, search_data, search_value.0, start);
                        if r.len() > 0 {
                            results.lock().unwrap().extend_from_slice(&r);
                        }
                    }
                }
            }
            current_bytes.fetch_add(size, Ordering::SeqCst); 
        });
    }
}


fn search_memory(memory_data: &Vec<u8>, search_data: &[u8], search_type: SearchType, start: usize) -> Vec<SearchResult> {
    let mut result = Vec::new();
    let search_bytes = BoyerMoore::new(search_data);
    for i in search_bytes.find_in(&memory_data) {
        result.push(SearchResult::new(i + start, search_type));
    }
    result
}