
use std::{sync::{Arc, atomic::{AtomicUsize, Ordering}, Mutex, mpsc::{self}}, vec, thread, time::Duration, collections::HashMap};
use egui::{RichText, Color32};
use egui_extras::{Size, TableBuilder};
use proc_maps::get_process_maps;
use process_memory::*;
use sysinfo::*;
use needle::BoyerMoore;
use threadpool::ThreadPool;

#[derive(Clone)]
pub struct SearchResult {
    addr: usize,
    freezed: bool
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum SearchValue {
    Short(i16),
    Int(i32),
    Int64(i64),
    Float(f32),
    Double(f64)
}

impl SearchValue {

    pub fn get_bytes(&self) -> Vec<u8> {
        match self {
            SearchValue::Short(v) => v.to_le_bytes().to_vec(),
            SearchValue::Int(v) => v.to_le_bytes().to_vec(),
            SearchValue::Int64(v) => v.to_le_bytes().to_vec(),
            SearchValue::Float(v) => v.to_le_bytes().to_vec(),
            SearchValue::Double(v) => v.to_le_bytes().to_vec(),
        }
    }
    pub fn get_byte_length(&self) -> usize {
        match self {
            SearchValue::Short(_) => 2,
            SearchValue::Int(_) => 4,
            SearchValue::Int64(_) => 8,
            SearchValue::Float(_) => 4,
            SearchValue::Double(_) => 8,
        }
    }

    pub fn from_bytes(&self, bytes: &[u8]) -> SearchValue {
        match self {
            SearchValue::Short(_) => SearchValue::Short(i16::from_le_bytes(bytes.try_into().unwrap_or_default())),
            SearchValue::Int(_) => SearchValue::Int(i32::from_le_bytes(bytes.try_into().unwrap_or_default())),
            SearchValue::Int64(_) => SearchValue::Int64(i64::from_le_bytes(bytes.try_into().unwrap_or_default())),
            SearchValue::Float(_) => SearchValue::Float(f32::from_le_bytes(bytes.try_into().unwrap_or_default())),
            SearchValue::Double(_) => SearchValue::Double(f64::from_le_bytes(bytes.try_into().unwrap_or_default()))
        }
    }

    pub fn from_string(&self, txt: &str) -> Result<SearchValue, &str> {
        match self {
            SearchValue::Short(_) => {
                let parsed = txt.parse::<i16>();
                match parsed {
                    Ok(f) => Ok(SearchValue::Short(f)),
                    Err(_) => Err("Invalid input")
                }
            }
            SearchValue::Int(_) =>  {
                let parsed = txt.parse::<i32>();
                match parsed {
                    Ok(f) => Ok(SearchValue::Int(f)),
                    Err(_) => Err("Invalid input")
                }
            }
            SearchValue::Int64(_) =>  {
                let parsed = txt.parse::<i64>();
                match parsed {
                    Ok(f) => Ok(SearchValue::Int64(f)),
                    Err(_) => Err("Invalid input")
                }
            }
            SearchValue::Float(_) => {
                let parsed = txt.parse::<f32>();
                match parsed {
                    Ok(f) => Ok(SearchValue::Float(f)),
                    Err(_) => Err("Invalid input")
                }
            }
            SearchValue::Double(_) => {
                let parsed = txt.parse::<f64>();
                match parsed {
                    Ok(f) => Ok(SearchValue::Double(f)),
                    Err(_) => Err("Invalid input")
                }
            }
        }
    }

    pub fn to_string(&self) -> String {
        match self {
            SearchValue::Short(v) => v.to_string(),
            SearchValue::Int(v) => v.to_string(),
            SearchValue::Int64(v) => v.to_string(),
            SearchValue::Float(v) => v.to_string(),
            SearchValue::Double(v) => v.to_string(),
        }
    }

    pub fn get_description_text(&self) -> &str {
        match self {
            SearchValue::Short(_) => "short (2 bytes)",
            SearchValue::Int(_) => "int (4 bytes)",
            SearchValue::Int64(_) => "int64 (4 bytes)",
            SearchValue::Float(_) => "float (4 bytes)",
            SearchValue::Double(_) => "double (8 bytes)"
        }
    }

    pub fn get_short_description_text(&self) -> &str {
        match self {
            SearchValue::Short(_) => "short",
            SearchValue::Int(_) => "int32",
            SearchValue::Int64(_) => "int64",
            SearchValue::Float(_) => "float",
            SearchValue::Double(_) => "double"
        }
    }
}

impl SearchResult {
    pub fn new(addr: usize) -> Self {
        Self {
            addr,
            freezed: false
        }
    }
}

pub struct SearchContext {
    description: String,
    search_value_text: String,
    searching: bool,
    total_bytes: usize,
    current_bytes: Arc<AtomicUsize>,
    results: Arc<Mutex<Vec<SearchResult>>>,

    old_results:Vec<Vec<SearchResult>>,
    search_results: i64,
    search_type: SearchValue
}

impl SearchContext {
    fn new(description: String) -> Self {
        Self {
            description,
            search_value_text: "".to_owned(),
            searching: false,
            results: Arc::new(Mutex::new(Vec::new())),
            total_bytes: 0,
            current_bytes: Arc::new(AtomicUsize::new(0)),
            search_results: -1,
            search_type: SearchValue::Int(0),
            old_results: Vec::new()
        }
    }

    fn clear_results(&mut self, freeze_sender: &mpsc::Sender<Message>) {
        GameCheetahEngine::remove_freezes_from(freeze_sender, &self.results.lock().unwrap().clone());
        self.results.lock().unwrap().clear();
        self.search_results = -1;
    }
}

pub struct GameCheetahEngine {
    pid: i32,
    process_name: String,
    show_process_window: bool,

    process_filter: String,
    processes: Vec<(u32, String, String)>,

    current_search: usize,
    searches: Vec<SearchContext>,
    search_threads: ThreadPool,

    freeze_sender: mpsc::Sender<Message>
}

enum MessageCommand {
    // Quit,
    Freeze,
    Unfreeze,
    Pid
}

struct Message {
    msg: MessageCommand,
    addr: usize,
    value: SearchValue
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
                        let output_buffer = value.get_bytes();
                        handle.put_address(*addr, &output_buffer).unwrap_or_default();
                    }
                }
                thread::sleep(Duration::from_millis(500));
            }
        });

        Self {
            pid: 0,
            process_name: "".to_owned(),

            show_process_window: false,
            process_filter: "".to_owned(),
            processes: Vec::new(),
            current_search: 0,
            searches: vec![SearchContext::new("Search 1".to_string())],
            search_threads: ThreadPool::new(64),
            freeze_sender: tx
        }
    }
}

impl GameCheetahEngine {
    pub fn new(_: &eframe::CreationContext<'_>) -> Self {
        Default::default()
    }

    fn render_process_window(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        if ctx.input().key_down(egui::Key::Escape) {
            self.show_process_window = false;
            return;
        }
        ui.spacing_mut().item_spacing = egui::Vec2::splat(20.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = egui::Vec2::splat(5.0);

            let i = ui.add(egui::TextEdit::singleline(&mut self.process_filter).hint_text("Filter processes"));
            if ui.memory().focus().is_none() {
                ui.memory().request_focus(i.id);
            }

            ui.spacing_mut().item_spacing = egui::Vec2::splat(25.0);

            if ui.button("ｘ").clicked() {
                self.process_filter.clear();
            }

            if ui.button("Close").clicked() {
                self.show_process_window = false;
                return;
            }
        });
        ui.spacing_mut().item_spacing = egui::Vec2::splat(5.0);

        let table = TableBuilder::new(ui)
            .striped(true)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(Size::initial(80.0).at_least(40.0))
            .column(Size::initial(200.0).at_least(40.0))
            .column(Size::remainder().at_least(60.0));

            table
            .header(20.0, |mut header| {
                header.col(|ui| {
                    ui.heading("Pid");
                });
                header.col(|ui| {
                    ui.heading("Name");
                });
                header.col(|ui| {
                    ui.heading("Command");
                });
            })
            .body(|mut body| {
                let filter = self.process_filter.to_ascii_uppercase();

                for (pid, process_name, cmd) in &self.processes {
                    if filter.len() > 0 && (
                        !process_name.to_ascii_uppercase().contains(filter.as_str()) && 
                        !cmd.to_ascii_uppercase().contains(filter.as_str()) && 
                        !pid.to_string().contains(filter.as_str())) {
                        continue;
                    }
                    let row_height = 17.0;
                    body.row(row_height, |mut row| {
                        row.col(|ui| {
                            if ui.selectable_label(false, pid.to_string()).clicked() {
                                self.pid = *pid as i32;
                                self.freeze_sender.send(Message {
                                    msg: MessageCommand::Pid,
                                    addr: *pid as usize,
                                    value: SearchValue::Int(0)
                                }).unwrap_or_default();
                                self.process_name = process_name.clone();
                                self.show_process_window = false;
                            }
                        });

                        row.col(|ui| {
                            ui.label(process_name);
                        });
                        row.col(|ui| {
                            ui.label(cmd);
                        });
                    });
                }
            });
    }
}

impl eframe::App for GameCheetahEngine {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            if self.show_process_window {
                self.render_process_window(ui, ctx);
                return;
            }

            ui.spacing_mut().item_spacing = egui::Vec2::splat(12.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = egui::Vec2::splat(5.0);
                ui.label("Process:");
    
                if ui.button(if self.pid != 0 {
                    format!("{} ({})", self.process_name, self.pid)
                } else {
                    "<no process set>".to_string()
                }).clicked() {
                    self.show_process_window = !self.show_process_window;
    
                    if self.show_process_window {
                        self.show_process_window();
                        return;
                    }
                }
    
                if ui.button("ｘ").clicked() {
                    self.pid = 0;
                    self.freeze_sender.send(Message {
                        msg: MessageCommand::Pid,
                        addr: 0,
                        value: SearchValue::Int(0)
                    }).unwrap_or_default();
                    self.searches.clear();
                    self.searches.push(SearchContext::new("default".to_string()));
                    self.process_filter.clear();
                }
            });

            if self.pid > 0 {
                if self.searches.len() > 1 {
                    ui.spacing_mut().item_spacing = egui::Vec2::splat(1.0);

                    ui.separator();
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = egui::Vec2::splat(8.0);

                        for i in 0..self.searches.len() {
                            if ui.selectable_label(self.current_search == i, self.searches[i].description.clone()).clicked() {
                                self.current_search = i;
                            }
                        }
                        if ui.button("-").clicked() {
                            self.remove_freezes(self.current_search);
                            self.searches.remove(self.current_search);
                            if self.current_search >= self.searches.len() - 1 {
                                self.current_search -= 1;
                            }
                            return;
                        }
                        if ui.button("+").clicked() {
                            self.new_search();
                            return;
                        }
                    });
                    ui.separator();
                    ui.spacing_mut().item_spacing = egui::Vec2::splat(8.0);

                    ui.add_space(8.0);
                }
                self.render_content(ui, ctx, self.current_search);
            }            
        });
    }
}

impl GameCheetahEngine {
    fn render_content(&mut self, ui: &mut egui::Ui, _ctx: &egui::Context, search_index: usize) {

        if self.searches.len() > 1 {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = egui::Vec2::splat(5.0);
                ui.label("Name:");
                let search_context = self.searches.get_mut(search_index).unwrap();
                ui.add(egui::TextEdit::singleline(&mut search_context.description).hint_text("Search description").interactive(!search_context.searching));
            });
        }

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = egui::Vec2::splat(5.0);
            ui.label("Value:");
            let search_context = self.searches.get_mut(search_index).unwrap();
            let re = ui.add(egui::TextEdit::singleline(&mut search_context.search_value_text)
                .hint_text(format!("Search for {} value", search_context.search_type.get_description_text()))
                .interactive(!search_context.searching)
            );

            let old_value = search_context.search_type.clone();
            egui::ComboBox::from_id_source(1)
            .selected_text(search_context.search_type.get_description_text())
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut search_context.search_type, SearchValue::Short(0), SearchValue::Short(0).get_short_description_text());
                ui.selectable_value(&mut search_context.search_type, SearchValue::Int(0), SearchValue::Int(0).get_short_description_text());
                ui.selectable_value(&mut search_context.search_type, SearchValue::Int64(0), SearchValue::Int64(0).get_short_description_text());
                ui.selectable_value(&mut search_context.search_type, SearchValue::Float(0.0), SearchValue::Float(0.0).get_short_description_text());
                ui.selectable_value(&mut search_context.search_type, SearchValue::Double(0.0), SearchValue::Double(0.0).get_short_description_text());
            });

            if old_value != search_context.search_type {
                search_context.clear_results(&self.freeze_sender);
            }


            if ui.add_enabled(search_context.old_results.len() > 0, egui::Button::new("Undo")).clicked() {
                let old = search_context.old_results.pop().unwrap();
                search_context.search_results = old.len() as i64;
                search_context.results = Arc::new(Mutex::new(old));
                return;
            }

            if re.lost_focus() && re.ctx.input().key_down(egui::Key::Enter) {
                let len = self.searches.get(search_index).unwrap().results.lock().unwrap().len();
                if len == 0 { 
                    self.initial_search(search_index);
                } else {
                    self.search(search_index);
                }
            } else {
                if ui.memory().focus().is_none() {
                    ui.memory().request_focus(re.id);
                }
            }

            if self.searches.len() <= 1 {
                if ui.button("+").clicked() {
                    self.new_search();
                    return;
                }
            }
        });

        let search_context = self.searches.get(search_index).unwrap();
        if search_context.search_value_text.len() > 0 && search_context.search_type.from_string(&search_context.search_value_text).is_err() {
            ui.label(RichText::new("Invalid number").color(Color32::from_rgb(200, 0, 0)));
        }

        if self.searches.get(search_index).unwrap().searching {
            self.render_search_bar(ui, search_index);
            return;
        }

        if i32::from_str_radix(self.searches.get(search_index).unwrap().search_value_text.as_str(), 10).is_ok()  {
            let len = self.searches.get(search_index).unwrap().search_results;
            if len <= 0 { 
                if ui.button("Initial search").clicked() {
                    self.initial_search(search_index);
                    return;
                }
                if len == 0 {
                    ui.label("No results found.".to_string());
                }
            } else {
                ui.horizontal(|ui| {
                    let search_context = self.searches.get_mut(search_index).unwrap();

                    ui.spacing_mut().item_spacing = egui::Vec2::splat(5.0);
        
                    if ui.button("Update").clicked() {
                        self.search(search_index);
                        return;
                    }
                    if ui.button("Clear").clicked() {
                        search_context.clear_results(&self.freeze_sender);
                        return;
                    }

                    if len == 1 {
                        ui.label(format!("found {} result.", len));
                    } else {
                        ui.label(format!("found {} results.", len));
                    }
                });
        
                if len > 0 && len < 20 {
                    self.render_result_table(ui, search_index);
                }
            }
        }
    }

    fn new_search(&mut self) {
        let ctx = SearchContext::new(format!("Search {}", 1 + self.searches.len()));
        self.current_search = self.searches.len();
        self.searches.push(ctx);
    }

    fn render_result_table(&mut self, ui: &mut egui::Ui, search_index: usize) {

        let search_context = self.searches.get_mut(search_index).unwrap();

        let table = TableBuilder::new(ui)
        .striped(true)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(Size::initial(120.0).at_least(40.0))
        .column(Size::initial(120.0).at_least(40.0))
        .column(Size::remainder().at_least(60.0));
        table
        .header(20.0, |mut header| {
            header.col(|ui| {
                ui.heading("Address");
            });
            header.col(|ui| {
                ui.heading("Value");
            });
            header.col(|ui| {
                 ui.heading("Freezed");
            });
        })
        .body(|mut body| {
            let cloned_results = search_context.results.lock().unwrap().clone();
            let row_height = 17.0;
            for i in 0..cloned_results.len() {
                let result = &cloned_results[i];
                body.row(row_height, |mut row| {
                    row.col(|ui| {
                        ui.label(format!("0x{:X}", result.addr));
                    });
                    row.col(|ui| {
                        if let Ok (handle) = (self.pid as process_memory::Pid).try_into_process_handle() {
                            if let Ok(buf) = copy_address(result.addr, search_context.search_type.get_byte_length(), &handle) {
                                let val = search_context.search_type.from_bytes(&buf);
                                let mut value_text  = val.to_string();
                                let old_text = value_text.clone();
                                ui.add(egui::TextEdit::singleline(&mut value_text));
                                if old_text != value_text {
                                    let val = search_context.search_type.from_string(&value_text);
                                    if val.is_ok() {
                                        let val = val.unwrap();
                                        let output_buffer = val.get_bytes();
                                        handle.put_address(result.addr, &output_buffer).unwrap_or_default();
                                        if result.freezed {
                                            self.freeze_sender.send(Message {
                                                msg: MessageCommand::Freeze,
                                                addr: result.addr,
                                                value: val
                                            }).unwrap_or_default();
                                        }
                                    }
                                }
                            } else {
                                ui.label("<error>");
                            }
                        }
                    });
                    row.col(|ui| {
                        let mut b = result.freezed;
                        if ui.checkbox(&mut b, "").changed() {
                            if let Ok (handle) = (self.pid as process_memory::Pid).try_into_process_handle() {
                                if let Ok(buf) = copy_address(result.addr, search_context.search_type.get_byte_length(), &handle) {
                                    let value = search_context.search_type.from_bytes(&buf);
                                    search_context.results.lock().as_mut().unwrap().remove(i);
                                    if b {
                                        self.freeze_sender.send(Message {
                                            msg: MessageCommand::Freeze,
                                            addr: result.addr,
                                            value
                                        }).unwrap_or_default();
                                    } else {
                                        self.freeze_sender.send(Message {
                                            msg: MessageCommand::Unfreeze,
                                            addr: result.addr,
                                            value
                                        }).unwrap_or_default();
                                    }
                                    search_context.results.lock().as_mut().unwrap().insert(i, SearchResult {
                                        addr: result.addr,
                                        freezed: b
                                    });
                                }
                            }
                        }
                    });
                });
            }
        });
    }

    fn render_search_bar(&mut self, ui: &mut egui::Ui, search_index: usize) {
        let mut search_context = self.searches.get_mut(search_index).unwrap();
        let current_bytes = search_context.current_bytes.load(Ordering::Relaxed);
        let progress_bar = egui::widgets::ProgressBar::new(current_bytes as f32 / search_context.total_bytes as f32).show_percentage();
        let bb = gabi::BytesConfig::default();
        let current_bytes_out = bb.bytes(current_bytes as u64);
        let total_bytes_out = bb.bytes(search_context.total_bytes as u64);
        ui.label( format!("Search {}/{}", current_bytes_out, total_bytes_out));
        ui.add(progress_bar);
        if current_bytes >= search_context.total_bytes {
            search_context.search_results = search_context.results.lock().unwrap().len() as i64;
            search_context.searching = false;
        }
    }

    fn initial_search(&mut self, search_index: usize) {
        if self.searches.get_mut(search_index).unwrap().searching {
            return;
        }
        self.remove_freezes(search_index);
        let my_int = i32::from_str_radix(self.searches.get(search_index).unwrap().search_value_text.as_str(), 10).unwrap();
        let b = i32::to_le_bytes(my_int);
        
        self.searches.get_mut(search_index).unwrap().searching = true;

        if let Ok(maps) = get_process_maps(self.pid.try_into().unwrap()) {
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
                while size > max_block + 3 {
                    self.spawn_thread(b,  start, max_block + 3, search_index);
                    
                    start += max_block;
                    size -= max_block;
                }
                self.spawn_thread(b,  start, size, search_index);
            }
        } else {
            println!("error getting process maps.");
        }
    }

    fn spawn_thread(&mut self, b: [u8; 4], start: usize, mut size: usize, search_index: usize) {
        let search_context = self.searches.get(search_index).unwrap();
        let pid = self.pid;
        let results = search_context.results.clone();
        let current_bytes = search_context.current_bytes.clone();

        self.search_threads.execute(move || {
            let n =&b[..];
            let handle = (pid as process_memory::Pid).try_into_process_handle().unwrap();
 
            let search_bytes = BoyerMoore::new(n);
            if let Ok(buf) = copy_address(start, size, &handle) {
                let mut last_i = 0;
                for i in search_bytes.find_in(&buf) {
                    current_bytes.fetch_add(i - last_i, Ordering::SeqCst); 
                    size -= i - last_i;
                    results.lock().unwrap().push(SearchResult::new(i + start));
                    last_i = i;
                }
            }
            current_bytes.fetch_add(size, Ordering::SeqCst); 
        });
    }
    
    fn search(&mut self, search_index: usize) {
        self.remove_freezes(search_index);
        let mut search_context = self.searches.get_mut(search_index).unwrap();
    
        let mut new_results = Vec::new();
        let handle = (self.pid as process_memory::Pid).try_into_process_handle().unwrap();
        let my_int = search_context.search_type.from_string(&search_context.search_value_text);
        if my_int.is_ok() {
            let my_int = my_int.unwrap();
            let old_results = search_context.results.lock().unwrap().clone();
            for i in 0..old_results.len() {
                let result = &old_results[i];
                if let Ok(buf) = copy_address(result.addr, search_context.search_type.get_byte_length(), &handle) {
                    let val = search_context.search_type.from_bytes(&buf);
                    if val == my_int {
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
            search_context.search_results = new_results.len() as i64;
            search_context.results = Arc::new(Mutex::new(new_results));
            search_context.old_results.push(old_results);
        }
    }

    fn remove_freezes(&self, search_index: usize) {
        let search_context = self.searches.get(search_index).unwrap();
        GameCheetahEngine::remove_freezes_from(&self.freeze_sender, &search_context.results.lock().unwrap().clone());
    }
    
    fn remove_freezes_from(freeze_sender: &mpsc::Sender<Message>, v: &Vec<SearchResult>) {
        for result in v {
            if result.freezed {
                freeze_sender.send(Message {
                    msg: MessageCommand::Unfreeze,
                    addr: result.addr,
                    value: SearchValue::Int(0)
                }).unwrap_or_default();
            }
        }
    }

    fn show_process_window(&mut self) {
        let sys = System::new_all();
        self.processes.clear();
        for (pid, process) in sys.processes() {
            self.processes.push((pid.as_u32(), process.name().to_string(), process.cmd().join(" ")));
        }
    }


}