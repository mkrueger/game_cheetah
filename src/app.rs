use std::{sync::{Arc, atomic::{AtomicUsize, Ordering}, Mutex}};
use egui_extras::{Size, TableBuilder};
use proc_maps::get_process_maps;
use process_memory::*;
use sysinfo::*;
use needle::BoyerMoore;
use threadpool::ThreadPool;

pub struct GameCheetahEngine {
    text: String,
    pid: i32,
    process_name: String,
    show_process_window: bool,
    results: Arc<Mutex<Vec<usize>>>,

    filter: String,
    processes: Vec<(u32, String, String)>,
    searching: bool,
    total_bytes: usize,
    current_bytes: Arc<AtomicUsize>,
    search_threads: ThreadPool
}

impl Default for GameCheetahEngine {
    fn default() -> Self {
        Self {
            text: "".to_owned(),
            pid: 0,
            process_name: "".to_owned(),

            show_process_window: false,
            results: Arc::new(Mutex::new(Vec::new())),

            filter: "".to_owned(),
            processes: Vec::new(),
            searching: false,
            search_threads: ThreadPool::new(8),
            total_bytes: 0,
            current_bytes: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl GameCheetahEngine {
    pub fn new(_: &eframe::CreationContext<'_>) -> Self {
        Default::default()
    }

    fn render_process_window(&mut self, ctx: &egui::Context) {
        egui::Window::new("Select process").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.filter).hint_text("Filter processes"));
                if ui.button("ｘ").clicked() {
                    self.filter.clear();
                }
            });
            let table = TableBuilder::new(ui)
                .striped(true)
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                .column(Size::initial(120.0).at_least(40.0))
                .column(Size::initial(60.0).at_least(40.0))
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
                    let filter = self.filter.to_ascii_uppercase();

                    for (pid, process_name, cmd) in &self.processes {
                        if filter.len() > 0 && (!process_name.to_ascii_uppercase().contains(filter.as_str()) || !cmd.to_ascii_uppercase().contains(filter.as_str())) {
                            continue;
                        }
                        let row_height = 18.0;
                        body.row(row_height, |mut row| {
                            row.col(|ui| {
                                if ui.selectable_label(false, pid.to_string()).clicked() {
                                    self.pid = *pid as i32;
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
        });
    }
}

impl eframe::App for GameCheetahEngine {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {

        egui::CentralPanel::default().show(ctx, |ui| {

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
                    }
                }

                if ui.button("ｘ").clicked() {
                    self.pid = 0;
                    self.results.lock().unwrap().clear();
                    self.filter.clear();
                }
            });

            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = egui::Vec2::splat(5.0);
                ui.label("Value:");
                ui.add(egui::TextEdit::singleline(&mut self.text).hint_text("Value text"));
            });

            if self.searching {
                let current_bytes = self.current_bytes.load(Ordering::Relaxed);
                let progress_bar = egui::widgets::ProgressBar::new(current_bytes as f32 / self.total_bytes as f32).show_percentage();

                ui.add(progress_bar);

                if current_bytes >= self.total_bytes {
                    self.searching = false;
                }
            } else {
                if i32::from_str_radix(self.text.as_str(), 10).is_ok()  {
                    let len = self.results.lock().unwrap().len();
                    if len == 0 { 
                        if ui.button("initial search").clicked() {
                            self.initial_search();
                        }
                    } else {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing = egui::Vec2::splat(5.0);
            
                            if ui.button("update").clicked() {
                                self.search();
                            }
                            if ui.button("clear").clicked() {
                                self.results.lock().unwrap().clear();
                            }

                            ui.label(format!("found {} items.", len));
                        });
                        
                        if len > 0 && len < 10 {
                            let table = TableBuilder::new(ui)
                            .striped(true)
                            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                            .column(Size::initial(120.0).at_least(40.0))
                            .column(Size::remainder().at_least(60.0));
                            let row_height = 18.0;
                
                            table
                            .header(20.0, |mut header| {
                                header.col(|ui| {
                                    ui.heading("Address");
                                });
                                header.col(|ui| {
                                    ui.heading("Value");
                                });
                            })
                            .body(|mut body| {
                                for addr in self.results.lock().unwrap().clone() {
                                    body.row(row_height, |mut row| {
                                        row.col(|ui| {
                                            ui.label(format!("0x{:X}", addr));
                                        });

                                        row.col(|ui| {
                                            if let Ok (handle) = (self.pid as process_memory::Pid).try_into_process_handle() {
                                                if let Ok(buf) = copy_address(addr, 4, &handle) {
                                                    let mut val = i32::from_le_bytes(buf.try_into().unwrap());
                                                    if ui.add(egui::DragValue::new(&mut val)).changed() {
                                                        let output_buffer = val.to_le_bytes();
                                                        handle.put_address(addr, &output_buffer).unwrap_or_default();
                                                    }
                                                } else {
                                                    ui.label("<error>");
                                                }
                                            }
                                        });
                                    });
                                }
                            });
                        }


                    }
                }
            }
            if self.show_process_window {
                self.render_process_window(ctx);
            }
        });
    }
}

impl GameCheetahEngine {

    fn initial_search(&mut self) {
        let my_int = i32::from_str_radix(self.text.as_str(), 10).unwrap();
        let b = i32::to_le_bytes(my_int);
        
        self.searching = true;

        if let Ok(maps) = get_process_maps(self.pid) {

            let handle = (self.pid as process_memory::Pid).try_into_process_handle().unwrap();
            self.total_bytes = 0;

            self.current_bytes.swap(0, Ordering::SeqCst);

            for map in maps {
                //let pl = Path::new("");
                //let p = map.filename().unwrap_or(pl);
                // println!("{:x}:{:x} is_exec  {}, is_read {}, is_write {}, filename {}", map.start(), map.size(), map.is_exec(), map.is_read(), map.is_write(), p.to_str().unwrap());
                if !map.is_write() || map.is_exec() {
                    continue;
                }
                let size = map.size();
                self.total_bytes += size;
                
                let results = self.results.clone();
                let current_bytes = self.current_bytes.clone();

                self.search_threads.execute(move || {
                    let n =&b[..];

                    let search_bytes =  BoyerMoore::new(n);

                    if let Ok(buf) = copy_address(map.start(), map.size(), &handle) {
                        if buf.len() < 1 {
                            return;
                        }

                        for i in search_bytes.find_in(&buf) {
                            results.lock().unwrap().push(i + map.start());
                        }
                    }
                    current_bytes.fetch_add(size, Ordering::SeqCst);
                });
            }
        } else {
            println!("error getting process maps.");
        }
    }
    
    fn search(&mut self) {
        let mut new_results = Vec::new();
        let handle = (self.pid as process_memory::Pid).try_into_process_handle().unwrap();
        let my_int = i32::from_str_radix(self.text.as_str(), 10).unwrap();

        for address in self.results.lock().unwrap().clone() {
            if let Ok(buf) = copy_address(address, 4, &handle) {
                let val = i32::from_le_bytes(buf.try_into().unwrap());
                if val == my_int {
                    new_results.push(address);
                }
            }
        }
        self.results = Arc::new(Mutex::new(new_results));
    }

    fn show_process_window(&mut self) {
        let sys = System::new_all();
        self.processes.clear();
        for (pid, process) in sys.processes() {
            self.processes.push((pid.as_u32(), process.name().to_string(), process.cmd().join(" ")));
        }
    }
}