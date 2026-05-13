use i18n_embed_fl::fl;

use crate::{
    ProcessInfo,
    ui::app::{App, AppState},
};

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum ProcessSortColumn {
    Pid,
    Name,
    #[default]
    Memory,
    Command,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum SortDirection {
    Ascending,
    #[default]
    Descending,
}

fn contains_ignore_ascii_case(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    let h = haystack.as_bytes();
    let n = needle.as_bytes();
    if n.len() > h.len() {
        return false;
    }
    'outer: for i in 0..=h.len() - n.len() {
        for j in 0..n.len() {
            if !h[i + j].eq_ignore_ascii_case(&n[j]) {
                continue 'outer;
            }
        }
        return true;
    }
    false
}

fn cmp_ascii_case_insensitive(a: &str, b: &str) -> std::cmp::Ordering {
    let ab = a.as_bytes();
    let bb = b.as_bytes();
    let len = ab.len().min(bb.len());
    for i in 0..len {
        let av = ab[i].to_ascii_lowercase();
        let bv = bb[i].to_ascii_lowercase();
        match av.cmp(&bv) {
            std::cmp::Ordering::Equal => continue,
            other => return other,
        }
    }
    ab.len().cmp(&bb.len())
}

pub fn view_process_selection(app: &mut App, ui: &mut egui::Ui) {
    egui::CentralPanel::default().show_inside(ui, |ui| {
        ui.add_space(8.0);

        // Search bar.
        ui.horizontal(|ui| {
            ui.label("🔍");
            let response = ui.add(
                egui::TextEdit::singleline(&mut app.state.process_filter)
                    .hint_text(fl!(crate::LANGUAGE_LOADER, "filter-processes-hint"))
                    .desired_width(ui.available_width() - 100.0),
            );
            // Autofocus the filter when the view first appears so the user
            // can start typing immediately.
            if app.state.set_focus {
                response.request_focus();
                app.state.set_focus = false;
            }
            if !app.state.process_filter.is_empty() && ui.button("✕").clicked() {
                app.state.process_filter.clear();
            }
        });

        ui.add_space(6.0);
        ui.separator();

        let filter = app.state.process_filter.clone();
        let mut filtered_processes: Vec<&ProcessInfo> = app
            .state
            .processes
            .iter()
            .filter(|process| {
                filter.is_empty()
                    || contains_ignore_ascii_case(&process.name, &filter)
                    || contains_ignore_ascii_case(&process.cmd, &filter)
                    || process.pid.to_string().contains(&filter)
            })
            .collect();

        match app.process_sort_column {
            ProcessSortColumn::Pid => filtered_processes.sort_by(|a, b| match app.process_sort_direction {
                SortDirection::Ascending => a.pid.cmp(&b.pid),
                SortDirection::Descending => b.pid.cmp(&a.pid),
            }),
            ProcessSortColumn::Name => filtered_processes.sort_by(|a, b| match app.process_sort_direction {
                SortDirection::Ascending => cmp_ascii_case_insensitive(&a.name, &b.name),
                SortDirection::Descending => cmp_ascii_case_insensitive(&b.name, &a.name),
            }),
            ProcessSortColumn::Memory => filtered_processes.sort_by(|a, b| match app.process_sort_direction {
                SortDirection::Ascending => a.memory.cmp(&b.memory),
                SortDirection::Descending => b.memory.cmp(&a.memory),
            }),
            ProcessSortColumn::Command => filtered_processes.sort_by(|a, b| match app.process_sort_direction {
                SortDirection::Ascending => cmp_ascii_case_insensitive(&a.cmd, &b.cmd),
                SortDirection::Descending => cmp_ascii_case_insensitive(&b.cmd, &a.cmd),
            }),
        }

        let total = app.state.processes.len();
        let shown = filtered_processes.len();

        // Take an owned snapshot so we can safely select inside the loop.
        let process_rows: Vec<ProcessInfo> = filtered_processes.iter().map(|p| (*p).clone()).collect();
        let mut selected: Option<ProcessInfo> = None;

        let table_height = ui.available_height() - 60.0;
        egui::ScrollArea::both()
            .max_height(table_height)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                use egui_extras::{Column, TableBuilder};
                let sort_indicator = |column: ProcessSortColumn| -> &'static str {
                    if app.process_sort_column == column {
                        match app.process_sort_direction {
                            SortDirection::Ascending => " ▲",
                            SortDirection::Descending => " ▼",
                        }
                    } else {
                        ""
                    }
                };

                let mut new_sort: Option<ProcessSortColumn> = None;

                TableBuilder::new(ui)
                    .striped(true)
                    .resizable(true)
                    .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                    .column(Column::initial(80.0).at_least(60.0))
                    .column(Column::initial(220.0).at_least(120.0))
                    .column(Column::initial(120.0).at_least(80.0))
                    .column(Column::remainder().at_least(200.0))
                    .header(22.0, |mut header| {
                        let mut hdr_btn = |h: &mut egui_extras::TableRow<'_, '_>,
                                           label: String,
                                           col: ProcessSortColumn| {
                            h.col(|ui| {
                                if ui
                                    .add(
                                        egui::Button::new(
                                            egui::RichText::new(format!("{label}{}", sort_indicator(col)))
                                                .strong(),
                                        )
                                        .frame(false),
                                    )
                                    .clicked()
                                {
                                    new_sort = Some(col);
                                }
                            });
                        };
                        hdr_btn(
                            &mut header,
                            fl!(crate::LANGUAGE_LOADER, "pid-heading"),
                            ProcessSortColumn::Pid,
                        );
                        hdr_btn(
                            &mut header,
                            fl!(crate::LANGUAGE_LOADER, "name-heading"),
                            ProcessSortColumn::Name,
                        );
                        hdr_btn(
                            &mut header,
                            fl!(crate::LANGUAGE_LOADER, "memory-heading"),
                            ProcessSortColumn::Memory,
                        );
                        hdr_btn(
                            &mut header,
                            fl!(crate::LANGUAGE_LOADER, "command-heading"),
                            ProcessSortColumn::Command,
                        );
                    })
                    .body(|body| {
                        body.rows(22.0, process_rows.len(), |mut row| {
                            let i = row.index();
                            let process = &process_rows[i];
                            row.col(|ui| {
                                ui.monospace(process.pid.to_string());
                            });
                            row.col(|ui| {
                                if ui.add(egui::Label::new(egui::RichText::new(&process.name).strong()).sense(egui::Sense::click()))
                                    .double_clicked()
                                {
                                    selected = Some(process.clone());
                                }
                            });
                            row.col(|ui| {
                                let bb = gabi::BytesConfig::default();
                                ui.monospace(bb.bytes(process.memory as u64).to_string());
                            });
                            row.col(|ui| {
                                let response = ui.add(
                                    egui::Label::new(egui::RichText::new(&process.cmd).weak())
                                        .sense(egui::Sense::click())
                                        .truncate(),
                                );
                                if response.double_clicked() {
                                    selected = Some(process.clone());
                                }
                            });
                        });
                    });

                if let Some(col) = new_sort {
                    if app.process_sort_column == col {
                        app.process_sort_direction = match app.process_sort_direction {
                            SortDirection::Ascending => SortDirection::Descending,
                            SortDirection::Descending => SortDirection::Ascending,
                        };
                    } else {
                        app.process_sort_column = col;
                        app.process_sort_direction = SortDirection::Ascending;
                    }
                }
            });

        if process_rows.is_empty() {
            ui.add_space(40.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    egui::RichText::new(if total == 0 {
                        fl!(crate::LANGUAGE_LOADER, "no-processes-loading")
                    } else {
                        fl!(crate::LANGUAGE_LOADER, "no-processes-match")
                    })
                    .weak(),
                );
            });
        }

        ui.add_space(8.0);
        ui.separator();
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(if filter.is_empty() {
                    fl!(crate::LANGUAGE_LOADER, "process-count-total", total = total)
                } else {
                    fl!(
                        crate::LANGUAGE_LOADER,
                        "process-count-filtered",
                        shown = shown,
                        total = total
                    )
                })
                .weak(),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(fl!(crate::LANGUAGE_LOADER, "close-button")).clicked() {
                    app.app_state = AppState::MainWindow;
                }
            });
        });

        if let Some(process) = selected {
            app.select_process(&process);
        }
    });
}
