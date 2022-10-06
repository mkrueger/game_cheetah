
use std::{sync::{Arc, atomic::{AtomicUsize, Ordering}, Mutex, mpsc::{self}}, vec, thread, time::Duration, collections::HashMap};
use egui_extras::{Size, TableBuilder};
use proc_maps::get_process_maps;
use process_memory::*;
use sysinfo::*;
use needle::BoyerMoore;
use threadpool::ThreadPool;

#[derive(Clone)]
pub struct Result {
    addr: usize,
    freezed: bool
}

impl Result {
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
    results: Arc<Mutex<Vec<Result>>>,
    search_results : i64
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
            search_results: -1
        }
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
    value: i32
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
                            pid = msg.value; 
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
                        let output_buffer = (*value as i32).to_le_bytes();
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
                                    addr: 0,
                                    value: *pid as i32
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
                        value: 0
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
                .hint_text("Search for value")
                .interactive(!search_context.searching)
            );
            

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
                        GameCheetahEngine::remove_freezes_from(&self.freeze_sender, &search_context.results.lock().unwrap().clone());
                        search_context.results.lock().unwrap().clear();
                        search_context.search_results = -1;
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
                            if let Ok(buf) = copy_address(result.addr, 4, &handle) {
                                let mut val = i32::from_le_bytes(buf.try_into().unwrap());
                                if ui.add(egui::DragValue::new(&mut val)).changed() {
                                    let output_buffer = val.to_le_bytes();
                                    handle.put_address(result.addr, &output_buffer).unwrap_or_default();
                                    if result.freezed {
                                        self.freeze_sender.send(Message {
                                            msg: MessageCommand::Freeze,
                                            addr: result.addr,
                                            value: val
                                        }).unwrap_or_default();
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
                                if let Ok(buf) = copy_address(result.addr, 4, &handle) {
                                    let value = i32::from_le_bytes(buf.try_into().unwrap());
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
                                    search_context.results.lock().as_mut().unwrap().insert(i, Result {
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
                    if !map.is_write() {
                        continue;
                    }
                    if let Some(file_name)  = map.filename() {
                        if file_name.starts_with("/dev") {
                            continue;
                        }
                        if file_name.as_os_str().to_str().unwrap().starts_with("/memfd") {
                            continue;
                        }
                    }
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
                    results.lock().unwrap().push(Result::new(i + start));
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
        let my_int = i32::from_str_radix(search_context.search_value_text.as_str(), 10).unwrap();

        for result in search_context.results.lock().unwrap().clone() {
            if let Ok(buf) = copy_address(result.addr, 4, &handle) {
                let val = i32::from_le_bytes(buf.try_into().unwrap());
                if val == my_int {
                    if result.freezed {
                        self.freeze_sender.send(Message {
                            msg: MessageCommand::Freeze,
                            addr: result.addr,
                            value: val
                        }).unwrap_or_default();
                    }
                    new_results.push(result);
                }
            }
        }
        search_context.search_results = new_results.len() as i64;
        search_context.results = Arc::new(Mutex::new(new_results));
    }

    fn remove_freezes(&self, search_index: usize) {
        let search_context = self.searches.get(search_index).unwrap();
        GameCheetahEngine::remove_freezes_from(&self.freeze_sender, &search_context.results.lock().unwrap().clone());
    }
    
    fn remove_freezes_from(freeze_sender: &mpsc::Sender<Message>, v: &Vec<Result>) {
        for result in v {
            if result.freezed {
                freeze_sender.send(Message {
                    msg: MessageCommand::Unfreeze,
                    addr: result.addr,
                    value: 0
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