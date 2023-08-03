use egui::{Color32, RichText};
use egui_extras::{Column, TableBuilder};
use i18n_embed_fl::fl;
use process_memory::*;
use std::{
    sync::{atomic::Ordering, Arc, Mutex},
    time::Duration,
};

use crate::{
    GameCheetahEngine, Message, MessageCommand, SearchContext, SearchMode, SearchType, SearchValue,
};

impl GameCheetahEngine {
    pub fn new(_: &eframe::CreationContext<'_>) -> Self {
        Default::default()
    }

    fn render_process_window(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        if ctx.input(|i| i.key_down(egui::Key::Escape)) {
            self.show_process_window = false;
            return;
        }
        ui.spacing_mut().item_spacing = egui::Vec2::splat(20.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = egui::Vec2::splat(5.0);

            let i = ui.add(
                egui::TextEdit::singleline(&mut self.process_filter)
                    .hint_text(fl!(crate::LANGUAGE_LOADER, "filter-processes-hint")),
            );
            if ui.memory(|m| m.focus().is_none()) {
                ui.memory_mut(|m| m.request_focus(i.id));
            }

            ui.spacing_mut().item_spacing = egui::Vec2::splat(25.0);

            if ui.button("ｘ").clicked() {
                self.process_filter.clear();
            }

            if ui
                .button(fl!(crate::LANGUAGE_LOADER, "close-button"))
                .clicked()
            {
                self.show_process_window = false;
            }
        });
        ui.spacing_mut().item_spacing = egui::Vec2::splat(5.0);

        let table = TableBuilder::new(ui)
            .striped(true)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(Column::initial(80.0).at_least(40.0))
            .column(Column::initial(200.0).at_least(40.0))
            .column(Column::initial(80.0).at_least(40.0))
            .column(Column::remainder().at_least(60.0));

        table
            .header(20.0, |mut header| {
                header.col(|ui| {
                    ui.heading(fl!(crate::LANGUAGE_LOADER, "pid-heading"));
                });
                header.col(|ui| {
                    ui.heading(fl!(crate::LANGUAGE_LOADER, "name-heading"));
                });
                header.col(|ui| {
                    ui.heading(fl!(crate::LANGUAGE_LOADER, "memory-heading"));
                });
                header.col(|ui| {
                    ui.heading(fl!(crate::LANGUAGE_LOADER, "command-heading"));
                });
            })
            .body(|mut body| {
                let filter = self.process_filter.to_ascii_uppercase();

                for process in &self.processes {
                    if !filter.is_empty()
                        && (!process.name.to_ascii_uppercase().contains(filter.as_str())
                            && !process.cmd.to_ascii_uppercase().contains(filter.as_str())
                            && !process.pid.to_string().contains(filter.as_str()))
                    {
                        continue;
                    }
                    let row_height = 17.0;
                    body.row(row_height, |mut row| {
                        row.col(|ui| {
                            let r = ui.selectable_label(false, process.pid.to_string());
                            if r.clicked() {
                                self.pid = process.pid;
                                self.freeze_sender
                                    .send(Message::from_addr(
                                        MessageCommand::Pid,
                                        process.pid as usize,
                                    ))
                                    .unwrap_or_default();
                                self.process_name = process.name.clone();
                                self.show_process_window = false;
                            }
                        });

                        row.col(|ui| {
                            ui.label(&process.name);
                        });
                        row.col(|ui| {
                            let bb = gabi::BytesConfig::default();
                            let memory = bb.bytes(process.memory as u64);
                            ui.label(memory.to_string());
                        });
                        row.col(|ui| {
                            ui.label(&process.cmd);
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
                ui.label(fl!(crate::LANGUAGE_LOADER, "process-label"));

                if ui
                    .button(if self.pid != 0 {
                        format!("{} ({})", self.process_name, self.pid)
                    } else {
                        fl!(crate::LANGUAGE_LOADER, "no-processes-label")
                    })
                    .clicked()
                {
                    self.show_process_window = !self.show_process_window;

                    if self.show_process_window {
                        self.show_process_window();
                        return;
                    }
                }

                if ui.button("ｘ").clicked() {
                    self.pid = 0;
                    self.freeze_sender
                        .send(Message::from_addr(MessageCommand::Pid, 0))
                        .unwrap_or_default();
                    self.searches.clear();
                    self.searches.push(Box::new(SearchContext::new(fl!(
                        crate::LANGUAGE_LOADER,
                        "first-search-label"
                    ))));
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
                            let r = ui
                                .selectable_label(
                                    self.current_search == i,
                                    self.searches[i].description.clone(),
                                )
                                .on_hover_text(fl!(crate::LANGUAGE_LOADER, "tab-hover-text"));

                            if r.clicked() {
                                self.current_search = i;
                            }

                            if r.double_clicked() {
                                self.searches[i].rename_mode = true;
                            }
                        }
                        if self.current_search < self.searches.len()
                            && ui
                                .button("-")
                                .on_hover_text(fl!(crate::LANGUAGE_LOADER, "close-tab-hover-text"))
                                .clicked()
                        {
                            self.remove_freezes(self.current_search);
                            self.searches.remove(self.current_search);
                            if self.current_search > 0 {
                                self.current_search -= 1;
                            }
                        }
                        if ui
                            .button("+")
                            .on_hover_text(fl!(crate::LANGUAGE_LOADER, "open-tab-hover-text"))
                            .clicked()
                        {
                            self.new_search();
                        }
                    });
                    ui.separator();
                    ui.spacing_mut().item_spacing = egui::Vec2::splat(8.0);

                    ui.add_space(8.0);
                }
                if !self.error_text.is_empty() {
                    ui.label(self.error_text.clone());
                }

                self.render_content(ui, ctx, self.current_search);
            }
        });
    }
}

impl GameCheetahEngine {
    fn render_content(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, search_index: usize) {
        if self.searches.len() > 1 {
            if let Some(search_context) = self.searches.get_mut(search_index) {
                if search_context.rename_mode {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing = egui::Vec2::splat(5.0);
                        ui.label(fl!(crate::LANGUAGE_LOADER, "name-label"));
                        ui.add(
                            egui::TextEdit::singleline(&mut search_context.description)
                                .hint_text(fl!(crate::LANGUAGE_LOADER, "search-description-label"))
                                .interactive(matches!(search_context.searching, SearchMode::None)),
                        );

                        if ui
                            .button(fl!(crate::LANGUAGE_LOADER, "rename-button"))
                            .clicked()
                        {
                            search_context.rename_mode = false;
                        }
                    });
                }
            }
        }

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = egui::Vec2::splat(5.0);
            ui.label(fl!(crate::LANGUAGE_LOADER, "value-label"));
            if let Some(search_context) = self.searches.get_mut(search_index) {
                let re = ui.add(
                    egui::TextEdit::singleline(&mut search_context.search_value_text)
                        .hint_text(fl!(
                            crate::LANGUAGE_LOADER,
                            "search-value-label",
                            valuetype = search_context.search_type.get_description_text()
                        ))
                        .interactive(matches!(search_context.searching, SearchMode::None)),
                );

                let old_value = search_context.search_type;
                egui::ComboBox::from_id_source(1)
                    .selected_text(search_context.search_type.get_description_text())
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut search_context.search_type,
                            SearchType::Guess,
                            SearchType::Guess.get_short_description_text(),
                        );
                        ui.selectable_value(
                            &mut search_context.search_type,
                            SearchType::Short,
                            SearchType::Short.get_short_description_text(),
                        );
                        ui.selectable_value(
                            &mut search_context.search_type,
                            SearchType::Int,
                            SearchType::Int.get_short_description_text(),
                        );
                        ui.selectable_value(
                            &mut search_context.search_type,
                            SearchType::Int64,
                            SearchType::Int64.get_short_description_text(),
                        );
                        ui.selectable_value(
                            &mut search_context.search_type,
                            SearchType::Float,
                            SearchType::Float.get_short_description_text(),
                        );
                        ui.selectable_value(
                            &mut search_context.search_type,
                            SearchType::Double,
                            SearchType::Double.get_short_description_text(),
                        );
                    });

                if old_value != search_context.search_type {
                    search_context.clear_results(&self.freeze_sender);
                }

                if ui
                    .add_enabled(
                        !search_context.old_results.is_empty(),
                        egui::Button::new(fl!(crate::LANGUAGE_LOADER, "undo-button")),
                    )
                    .clicked()
                {
                    if let Some(old) = search_context.old_results.pop() {
                        search_context.search_results = old.len() as i64;
                        search_context.results = Arc::new(Mutex::new(old));
                    }
                    return;
                }

                if re.lost_focus() && re.ctx.input(|i| i.key_down(egui::Key::Enter)) {
                    let len = self
                        .searches
                        .get(search_index)
                        .unwrap()
                        .results
                        .lock()
                        .unwrap()
                        .len();
                    if len == 0 {
                        self.initial_search(search_index);
                    } else {
                        self.filter_searches(search_index);
                    }
                } else if ui.memory(|m| m.focus().is_none()) {
                    ui.memory_mut(|m| m.request_focus(re.id));
                }

                if self.searches.len() <= 1
                    && ui
                        .button("+")
                        .on_hover_text(fl!(crate::LANGUAGE_LOADER, "open-tab-hover-text"))
                        .clicked()
                {
                    self.new_search();
                }
            }
        });

        let search_context = self.searches.get(search_index).unwrap();
        if !search_context.search_value_text.is_empty()
            && search_context
                .search_type
                .from_string(&search_context.search_value_text)
                .is_err()
        {
            ui.label(
                RichText::new(fl!(crate::LANGUAGE_LOADER, "invalid-number-error"))
                    .color(Color32::from_rgb(200, 0, 0)),
            );
        }

        if !matches!(
            self.searches.get(search_index).unwrap().searching,
            SearchMode::None
        ) {
            self.render_search_bar(ui, search_index);
            ctx.request_repaint_after(Duration::from_millis(200));
            return;
        }

        if self
            .searches
            .get(search_index)
            .unwrap()
            .search_value_text
            .as_str()
            .parse::<i32>()
            .is_ok()
        {
            let len = self.searches.get(search_index).unwrap().search_results;
            if len <= 0 {
                if ui
                    .button(fl!(crate::LANGUAGE_LOADER, "initial-search-button"))
                    .clicked()
                {
                    self.initial_search(search_index);
                    return;
                }
                if len == 0 {
                    ui.label(fl!(crate::LANGUAGE_LOADER, "no-results-label"));
                }
            } else {
                let auto_show_treshold = 20;

                ui.horizontal(|ui| {
                    let search_context = self.searches.get_mut(search_index).unwrap();

                    ui.spacing_mut().item_spacing = egui::Vec2::splat(5.0);

                    if ui
                        .button(fl!(crate::LANGUAGE_LOADER, "update-button"))
                        .clicked()
                    {
                        self.filter_searches(search_index);
                        return;
                    }
                    if ui
                        .button(fl!(crate::LANGUAGE_LOADER, "clear-button"))
                        .clicked()
                    {
                        search_context.clear_results(&self.freeze_sender);
                        return;
                    }
                    if len >= auto_show_treshold {
                        if self.show_results {
                            if ui
                                .button(fl!(crate::LANGUAGE_LOADER, "hide-results-button"))
                                .clicked()
                            {
                                self.show_results = false;
                                return;
                            }
                        } else if ui
                            .button(fl!(crate::LANGUAGE_LOADER, "show-results-button"))
                            .clicked()
                        {
                            self.show_results = true;
                            return;
                        }
                    }

                    if len == 1 {
                        ui.label(fl!(crate::LANGUAGE_LOADER, "found-one-result-label"));
                    } else {
                        ui.label(fl!(
                            crate::LANGUAGE_LOADER,
                            "found-results-label",
                            results = len
                        ));
                    }
                });

                if len > 0 && len < auto_show_treshold || self.show_results {
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
            .column(Column::initial(120.0).at_least(40.0))
            .column(Column::initial(120.0).at_least(40.0))
            .column(Column::remainder().at_least(60.0));
        table
            .header(20.0, |mut header| {
                header.col(|ui| {
                    ui.heading(fl!(crate::LANGUAGE_LOADER, "address-heading"));
                });
                header.col(|ui| {
                    ui.heading(fl!(crate::LANGUAGE_LOADER, "value-heading"));
                });
                header.col(|ui| {
                    ui.heading(fl!(crate::LANGUAGE_LOADER, "freezed-heading"));
                });
            })
            .body(|body| {
                let row_height = 17.0;
                let results = search_context.results.lock().unwrap();
                let num_rows = results.len();

                body.rows(row_height, num_rows, |row_index, mut row| {
                    let result = &results[row_index];
                    row.col(|ui| {
                        ui.label(format!("0x{:X}", result.addr));
                    });
                    row.col(|ui| {
                        if let Ok(handle) =
                            (self.pid as process_memory::Pid).try_into_process_handle()
                        {
                            if let Ok(buf) = copy_address(
                                result.addr,
                                result.search_type.get_byte_length(),
                                &handle,
                            ) {
                                let val = SearchValue(result.search_type, buf);
                                let mut value_text = val.to_string();
                                let old_text = value_text.clone();
                                ui.add(egui::TextEdit::singleline(&mut value_text));
                                if old_text != value_text {
                                    let val = result.search_type.from_string(&value_text);
                                    match val {
                                        Ok(value) => {
                                            handle
                                                .put_address(result.addr, &value.1)
                                                .unwrap_or_default();
                                            if search_context
                                                .freezed_addresses
                                                .contains(&result.addr)
                                            {
                                                self.freeze_sender
                                                    .send(Message {
                                                        msg: MessageCommand::Freeze,
                                                        addr: result.addr,
                                                        value,
                                                    })
                                                    .unwrap_or_default();
                                            }
                                        }
                                        Err(err) => {
                                            eprintln!(
                                                "Error converting {:?}: {}",
                                                result.search_type, err
                                            );
                                            self.error_text = fl!(
                                                crate::LANGUAGE_LOADER,
                                                "conversion-error",
                                                valuetype =
                                                    result.search_type.get_short_description_text(),
                                                message = err
                                            );
                                        }
                                    }
                                }
                            } else {
                                ui.label(fl!(crate::LANGUAGE_LOADER, "generic-error-label"));
                            }
                        }
                    });
                    row.col(|ui| {
                        let mut b = search_context.freezed_addresses.contains(&result.addr);
                        if ui.checkbox(&mut b, "").changed() {
                            if let Ok(handle) =
                                (self.pid as process_memory::Pid).try_into_process_handle()
                            {
                                if let Ok(buf) = copy_address(
                                    result.addr,
                                    result.search_type.get_byte_length(),
                                    &handle,
                                ) {
                                    if b {
                                        search_context.freezed_addresses.insert(result.addr);
                                        self.freeze_sender
                                            .send(Message {
                                                msg: MessageCommand::Freeze,
                                                addr: result.addr,
                                                value: SearchValue(result.search_type, buf),
                                            })
                                            .unwrap_or_default();
                                    } else {
                                        search_context.freezed_addresses.remove(&(result.addr));
                                        self.freeze_sender
                                            .send(Message::from_addr(
                                                MessageCommand::Unfreeze,
                                                result.addr,
                                            ))
                                            .unwrap_or_default();
                                    }
                                }
                            }
                        }
                    });
                });
            });
    }

    fn render_search_bar(&mut self, ui: &mut egui::Ui, search_index: usize) {
        let search_context = self.searches.get_mut(search_index).unwrap();
        let current_bytes = search_context.current_bytes.load(Ordering::Acquire);
        let progress_bar = egui::widgets::ProgressBar::new(
            current_bytes as f32 / search_context.total_bytes as f32,
        )
        .show_percentage();
        match search_context.searching {
            SearchMode::None => {}
            SearchMode::Percent => {
                ui.label(fl!(
                    crate::LANGUAGE_LOADER,
                    "update-numbers-progress",
                    current = current_bytes,
                    total = search_context.total_bytes
                ));
            }
            SearchMode::Memory => {
                let bb = gabi::BytesConfig::default();
                let current_bytes_out = bb.bytes(current_bytes as u64).to_string();
                let total_bytes_out = bb.bytes(search_context.total_bytes as u64).to_string();
                ui.label(fl!(
                    crate::LANGUAGE_LOADER,
                    "search-memory-progress",
                    current = current_bytes_out,
                    total = total_bytes_out
                ));
            }
        }

        ui.add(progress_bar);
        if current_bytes >= search_context.total_bytes {
            search_context.search_results = search_context.results.lock().unwrap().len() as i64;
            search_context.searching = SearchMode::None;
        }
    }
}
