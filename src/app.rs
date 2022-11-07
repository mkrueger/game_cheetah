
use std::{sync::{Arc, atomic::{Ordering}, Mutex}};
use egui::{RichText, Color32};
use egui_extras::{Size, TableBuilder};
use process_memory::*;

use crate::{SearchType, SearchValue, SearchContext, Message, MessageCommand, SearchResult, GameCheetahEngine};

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
                                self.freeze_sender.send(Message::from_addr(MessageCommand::Pid, *pid as usize)).unwrap_or_default();
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
                    self.freeze_sender.send(Message::from_addr(MessageCommand::Pid, 0)).unwrap_or_default();
                    self.searches.clear();
                    self.searches.push(Box::new(SearchContext::new("default".to_string())));
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
                if self.error_text.len() > 0 {
                    ui.label(self.error_text.clone());
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
                if let Some(search_context) = self.searches.get_mut(search_index) {
                    ui.add(egui::TextEdit::singleline(&mut search_context.description).hint_text("Search description").interactive(!search_context.searching));
                }
            });
        }

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = egui::Vec2::splat(5.0);
            ui.label("Value:");
            if let Some(search_context) = self.searches.get_mut(search_index) {
                let re = ui.add(egui::TextEdit::singleline(&mut search_context.search_value_text)
                    .hint_text(format!("Search for {} value", search_context.search_type.get_description_text()))
                    .interactive(!search_context.searching)
                );

                let old_value = search_context.search_type.clone();
                egui::ComboBox::from_id_source(1)
                .selected_text(search_context.search_type.get_description_text())
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut search_context.search_type, SearchType::Guess, SearchType::Guess.get_short_description_text());
                    ui.selectable_value(&mut search_context.search_type, SearchType::Short, SearchType::Short.get_short_description_text());
                    ui.selectable_value(&mut search_context.search_type, SearchType::Int, SearchType::Int.get_short_description_text());
                    ui.selectable_value(&mut search_context.search_type, SearchType::Int64, SearchType::Int64.get_short_description_text());
                    ui.selectable_value(&mut search_context.search_type, SearchType::Float, SearchType::Float.get_short_description_text());
                    ui.selectable_value(&mut search_context.search_type, SearchType::Double, SearchType::Double.get_short_description_text());
                });

                if old_value != search_context.search_type {
                    search_context.clear_results(&self.freeze_sender);
                }


                if ui.add_enabled(search_context.old_results.len() > 0, egui::Button::new("Undo")).clicked() {
                    if let Some(old) = search_context.old_results.pop() {
                        search_context.search_results = old.len() as i64;
                        search_context.results = Arc::new(Mutex::new(old));
                    }
                    return;
                }

                if re.lost_focus() && re.ctx.input().key_down(egui::Key::Enter) {
                    let len = self.searches.get(search_index).unwrap().results.lock().unwrap().len();
                    if len == 0 { 
                        self.initial_search(search_index);
                    } else {
                        self.filter_searches(search_index);
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
                        self.filter_searches(search_index);
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
                            if let Ok(buf) = copy_address(result.addr, result.search_type.get_byte_length(), &handle) {
                                let val = SearchValue(result.search_type, buf);
                                let mut value_text  = val.to_string();
                                let old_text = value_text.clone();
                                ui.add(egui::TextEdit::singleline(&mut value_text));
                                if old_text != value_text {
                                    let val = result.search_type.from_string(&value_text);
                                    match val {
                                        Ok(value) => {
                                            handle.put_address(result.addr, &value.1).unwrap_or_default();
                                            if result.freezed {
                                                self.freeze_sender.send(Message {
                                                    msg: MessageCommand::Freeze,
                                                    addr: result.addr,
                                                    value
                                                }).unwrap_or_default();
                                            }
                                        }
                                        Err(err) => {
                                            eprintln!("Error converting {:?}: {}", result.search_type, err);
                                            self.error_text = format!("Error converting {:?}: {}", result.search_type, err);
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
                                if let Ok(buf) = copy_address(result.addr, result.search_type.get_byte_length(), &handle) {
                                    search_context.results.lock().as_mut().unwrap().remove(i);
                                    if b {
                                        self.freeze_sender.send(Message {
                                            msg: MessageCommand::Freeze,
                                            addr: result.addr,
                                            value: SearchValue(result.search_type, buf)
                                        }).unwrap_or_default();
                                    } else {
                                        self.freeze_sender.send(Message::from_addr(MessageCommand::Unfreeze, result.addr)).unwrap_or_default();
                                    }
                                    search_context.results.lock().as_mut().unwrap().insert(i, SearchResult {
                                        addr: result.addr,
                                        search_type: result.search_type,
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
        let current_bytes = search_context.current_bytes.load(Ordering::Acquire);
        let progress_bar = egui::widgets::ProgressBar::new(current_bytes as f32 / search_context.total_bytes as f32).show_percentage();
        let bb = gabi::BytesConfig::default();
        let current_bytes_out = bb.bytes(current_bytes as u64);
        let total_bytes_out = bb.bytes(search_context.total_bytes as u64);
        ui.label(format!("Search {}/{}", current_bytes_out, total_bytes_out));
        ui.add(progress_bar);
        if current_bytes >= search_context.total_bytes {
            search_context.search_results = search_context.results.lock().unwrap().len() as i64;
            search_context.searching = false;
        }
    }

}

