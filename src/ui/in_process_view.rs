use std::sync::atomic::Ordering;

use i18n_embed_fl::fl;
use process_memory::{TryIntoProcessHandle, copy_address};

use crate::{
    SearchMode, SearchType, SearchValue, UnknownComparison,
    ui::app::{App, AppState, CHANGE_HIGHLIGHT},
};

/// Uniform row height used by the virtualized result table.
const RESULT_ROW_HEIGHT: f32 = 26.0;

pub fn view_in_process(app: &mut App, ui: &mut egui::Ui) {
    top_bar(app, ui);
    error_bar(app, ui);
    side_tabs(app, ui);
    egui::CentralPanel::default().show_inside(ui, |ui| {
        search_area(app, ui);
    });
}

fn top_bar(app: &mut App, ui: &mut egui::Ui) {
    egui::Panel::top("in_process_top").show_inside(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(fl!(crate::LANGUAGE_LOADER, "process-label"));
            ui.colored_label(
                ui.visuals().hyperlink_color,
                format!("{} ({})", app.state.process_name, app.state.pid),
            );

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(fl!(crate::LANGUAGE_LOADER, "close-button")).clicked() {
                    app.back_to_main_menu();
                }
                if ui.button(fl!(crate::LANGUAGE_LOADER, "load-cheat-table-button")).clicked() {
                    app.load_cheat_table();
                }
                if ui.button(fl!(crate::LANGUAGE_LOADER, "save-cheat-table-button")).clicked() {
                    app.save_cheat_table();
                }
                if !app.cheat_table_status.is_empty() {
                    ui.label(egui::RichText::new(&app.cheat_table_status).size(11.0).weak());
                }
            });
        });
    });
}

fn error_bar(app: &mut App, ui: &mut egui::Ui) {
    if let Some(error) = app.state.current_error() {
        let text = error.to_string();
        let mut dismiss = false;
        egui::Panel::top("in_process_error").show_inside(ui, |ui| {
            egui::Frame::group(ui.style())
                .fill(egui::Color32::from_rgba_unmultiplied(160, 50, 50, 30))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.colored_label(egui::Color32::from_rgb(220, 120, 120), "\u{26A0}");
                        ui.colored_label(egui::Color32::from_rgb(220, 120, 120), text);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("\u{00D7}").clicked() {
                                dismiss = true;
                            }
                        });
                    });
                });
        });
        if dismiss {
            app.state.dismiss_error();
        }
    }
}

fn side_tabs(app: &mut App, ui: &mut egui::Ui) {
    egui::Panel::left("search_tabs")
        .resizable(false)
        .default_size(160.0)
        .show_inside(ui, |ui| {
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new(fl!(crate::LANGUAGE_LOADER, "searches-heading"))
                    .strong(),
            );
            ui.separator();

            // Take a snapshot so we can mutate while iterating.
            let count = app.state.searches.len();
            let mut switch_to: Option<usize> = None;
            let mut close: Option<usize> = None;
            let mut rename: Option<usize> = None;
            for i in 0..count {
                let is_active = i == app.state.current_search;
                let name = app.state.searches[i].description.clone();

                ui.horizontal(|ui| {
                    // If this tab is being renamed, show a TextEdit instead.
                    if app.renaming_search_index == Some(i) {
                        let response = ui.add(
                            egui::TextEdit::singleline(&mut app.rename_search_text)
                                .desired_width(ui.available_width() - 24.0),
                        );
                        response.request_focus();
                        if response.lost_focus() {
                            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                                app.cancel_rename_search();
                            } else {
                                app.commit_rename_search();
                            }
                        }
                    } else {
                        let mut btn = egui::Button::new(egui::RichText::new(&name))
                            .min_size(egui::vec2(ui.available_width() - 24.0, 22.0));
                        if is_active {
                            btn = btn.fill(ui.visuals().selection.bg_fill);
                        }
                        let response = ui.add(btn);
                        if response.clicked() {
                            switch_to = Some(i);
                        }
                        if response.double_clicked() {
                            rename = Some(i);
                        }
                    }
                    if count > 1 && ui.small_button("×").clicked() {
                        close = Some(i);
                    }
                });
            }

            ui.add_space(6.0);
            if ui
                .button(fl!(crate::LANGUAGE_LOADER, "add-search-button"))
                .clicked()
            {
                app.new_search();
            }

            if let Some(i) = switch_to {
                app.switch_search(i);
            }
            if let Some(i) = rename {
                app.begin_rename_search(i);
            }
            if let Some(i) = close {
                app.close_search(i);
            }
        });
}

fn search_area(app: &mut App, ui: &mut egui::Ui) {
    let search_index = app.state.current_search;
    let Some(search_context) = app.state.searches.get(search_index) else {
        return;
    };
    let description = search_context.description.clone();
    let selected_type = search_context.search_type;
    let value_text = search_context.search_value_text.clone();
    let search_results = search_context.get_result_count();
    let is_search_complete = search_context.search_complete.load(Ordering::SeqCst);
    let can_undo = !search_context.old_results.is_empty();
    let searching = search_context.searching;
    let current_bytes = search_context.current_bytes.load(Ordering::Acquire);
    let total_bytes = search_context.total_bytes;

    let parse_error: Option<String> = if !value_text.is_empty() && selected_type != SearchType::Unknown {
        selected_type.from_string(&value_text).err()
    } else {
        None
    };
    let show_type_picker = matches!(searching, SearchMode::None) && search_results == 0;

    // Header
    ui.add_space(4.0);
    ui.heading(
        egui::RichText::new(description)
            .color(ui.visuals().selection.bg_fill)
            .size(20.0),
    );
    ui.separator();

    // Value input + type picker (unless searching Unknown, no input needed)
    if selected_type == SearchType::Unknown {
        ui.horizontal(|ui| {
            ui.label(fl!(crate::LANGUAGE_LOADER, "search-type-label"));
            type_picker(app, ui, show_type_picker, selected_type);
            ui.label(
                egui::RichText::new(fl!(crate::LANGUAGE_LOADER, "unknown-search-description"))
                    .weak(),
            );
        });
    } else {
        ui.horizontal(|ui| {
            ui.label(fl!(crate::LANGUAGE_LOADER, "value-label"));
            let mut buf = value_text.clone();
            let response = ui.add(
                egui::TextEdit::singleline(&mut buf)
                    .hint_text(fl!(
                        crate::LANGUAGE_LOADER,
                        "search-value-label",
                        valuetype = selected_type.get_description_text()
                    ))
                    .desired_width(ui.available_width() - 200.0)
                    .text_color_opt(if parse_error.is_some() {
                        Some(egui::Color32::from_rgb(220, 120, 120))
                    } else {
                        None
                    }),
            );
            if response.changed()
                && let Some(ctx) = app.state.searches.get_mut(search_index)
            {
                ctx.search_value_text = buf;
            }
            if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                app.start_search();
            }
            type_picker(app, ui, show_type_picker, selected_type);
        });
        if let Some(err) = &parse_error {
            ui.horizontal(|ui| {
                ui.add_space(60.0);
                ui.colored_label(egui::Color32::from_rgb(220, 120, 120), format!("⚠ {err}"));
            });
        }
    }

    ui.add_space(8.0);

    // Action row: depends on whether we're searching, have results, etc.
    if !matches!(searching, SearchMode::None) {
        ui.horizontal(|ui| {
            let progress = if total_bytes == 0 {
                0.0
            } else {
                current_bytes as f32 / total_bytes as f32
            };
            ui.add(egui::ProgressBar::new(progress).desired_width(ui.available_width() - 200.0));
            ui.label(
                if searching == SearchMode::Percent {
                    fl!(
                        crate::LANGUAGE_LOADER,
                        "update-numbers-progress",
                        current = current_bytes,
                        total = total_bytes
                    )
                } else {
                    let bb = gabi::BytesConfig::default();
                    fl!(
                        crate::LANGUAGE_LOADER,
                        "search-memory-progress",
                        current = bb.bytes(current_bytes as u64).to_string(),
                        total = bb.bytes(total_bytes as u64).to_string()
                    )
                }
                .chars()
                .filter(|c| c.is_ascii())
                .collect::<String>(),
            );
        });
    } else if !is_search_complete {
        ui.horizontal(|ui| {
            let enabled = parse_error.is_none() || selected_type == SearchType::Unknown;
            if ui
                .add_enabled(
                    enabled,
                    egui::Button::new(fl!(crate::LANGUAGE_LOADER, "initial-search-button")),
                )
                .clicked()
            {
                app.start_search();
            }
        });
    } else if selected_type == SearchType::Unknown {
        ui.horizontal_wrapped(|ui| {
            if ui
                .button(fl!(crate::LANGUAGE_LOADER, "decreased-button"))
                .clicked()
            {
                app.unknown_search(UnknownComparison::Decreased);
            }
            if ui
                .button(fl!(crate::LANGUAGE_LOADER, "increased-button"))
                .clicked()
            {
                app.unknown_search(UnknownComparison::Increased);
            }
            if ui
                .button(fl!(crate::LANGUAGE_LOADER, "changed-button"))
                .clicked()
            {
                app.unknown_search(UnknownComparison::Changed);
            }
            let unchanged_enabled = search_results > 0 || can_undo;
            if ui
                .add_enabled(
                    unchanged_enabled,
                    egui::Button::new(fl!(crate::LANGUAGE_LOADER, "unchanged-button")),
                )
                .clicked()
            {
                app.unknown_search(UnknownComparison::Unchanged);
            }
            ui.separator();
            if ui.button(fl!(crate::LANGUAGE_LOADER, "clear-button")).clicked() {
                app.clear_results();
            }
            if ui
                .add_enabled(can_undo, egui::Button::new(fl!(crate::LANGUAGE_LOADER, "undo-button")))
                .clicked()
            {
                app.undo_search();
            }
            ui.label(
                fl!(crate::LANGUAGE_LOADER, "found-results-label", results = search_results)
                    .chars()
                    .filter(|c| c.is_ascii())
                    .collect::<String>(),
            );
        });
    } else {
        ui.horizontal_wrapped(|ui| {
            let enabled = parse_error.is_none();
            if ui
                .add_enabled(
                    enabled,
                    egui::Button::new(fl!(crate::LANGUAGE_LOADER, "update-button")),
                )
                .clicked()
            {
                app.start_search();
            }
            if ui.button(fl!(crate::LANGUAGE_LOADER, "clear-button")).clicked() {
                app.clear_results();
            }
            if ui
                .add_enabled(can_undo, egui::Button::new(fl!(crate::LANGUAGE_LOADER, "undo-button")))
                .clicked()
            {
                app.undo_search();
            }
            ui.label(
                fl!(crate::LANGUAGE_LOADER, "found-results-label", results = search_results)
                    .chars()
                    .filter(|c| c.is_ascii())
                    .collect::<String>(),
            );
        });
    }

    ui.add_space(8.0);
    ui.separator();

    if matches!(searching, SearchMode::None) && search_results > 0 {
        result_table(app, ui);
    }
}

fn type_picker(app: &mut App, ui: &mut egui::Ui, editable: bool, current: SearchType) {
    if editable {
        let mut selected = current;
        egui::ComboBox::from_id_salt("search_type_picker")
            .selected_text(current.get_description_text())
            .show_ui(ui, |ui| {
                for st in [
                    SearchType::Guess,
                    SearchType::Unknown,
                    SearchType::Short,
                    SearchType::Int,
                    SearchType::Int64,
                    SearchType::Float,
                    SearchType::Double,
                    SearchType::String,
                ] {
                    ui.selectable_value(&mut selected, st, st.get_description_text());
                }
            });
        if selected != current
            && let Some(ctx) = app.state.searches.get_mut(app.state.current_search)
        {
            ctx.search_type = selected;
        }
    } else {
        ui.label(current.get_description_text());
    }
}

fn result_table(app: &mut App, ui: &mut egui::Ui) {
    let search_index = app.state.current_search;
    let Some(search_context) = app.state.searches.get(search_index) else {
        return;
    };
    let results = search_context.collect_results();
    let total_results = results.len();
    let is_string = matches!(
        search_context.search_type,
        SearchType::String | SearchType::StringUtf16
    );
    let show_search_types = matches!(
        search_context.search_type,
        SearchType::Guess | SearchType::Unknown
    );
    let string_byte_len = search_context.search_value_text.len();
    let string_char_count = search_context.search_value_text.chars().count();
    let pid = app.state.pid as process_memory::Pid;
    let hex_display = app.hex_display;

    // Build the freezed set + an "all frozen" check once for the whole frame.
    let freezed: std::collections::HashSet<usize> = search_context.freezed_addresses.iter().copied().collect();
    let all_frozen = !results.is_empty()
        && results.iter().all(|r| freezed.contains(&r.addr));
    let frozen_count = results.iter().filter(|r| freezed.contains(&r.addr)).count();

    // Capture lookups we'll need inside the row closure. Borrow checker:
    // the row closure runs inside TableBuilder::body which holds `ui`, and
    // we mutate `app` (toggle_freeze etc.) only via deferred actions
    // collected here.
    let mut toggle_freeze: Option<usize> = None;
    let mut toggle_freeze_all = false;
    let mut toggle_hex = false;
    let mut remove_result: Option<usize> = None;
    let mut open_editor: Option<usize> = None;
    let mut begin_edit: Option<(usize, String)> = None;
    let mut commit_edit: Option<(usize, String)> = None;
    let mut cancel_edit = false;

    use egui_extras::{Column, TableBuilder};

    let mut builder = TableBuilder::new(ui)
        .striped(true)
        .resizable(true)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(Column::initial(140.0).at_least(100.0)) // address
        .column(Column::initial(140.0).at_least(80.0)); // value
    if show_search_types {
        builder = builder.column(Column::initial(110.0).at_least(80.0));
    }
    if !is_string {
        // Freeze column + buttons column
        builder = builder.column(Column::initial(110.0).at_least(80.0));
    }
    builder = builder.column(Column::remainder().at_least(160.0));

    builder
        .header(24.0, |mut header| {
            header.col(|ui| {
                ui.strong(fl!(crate::LANGUAGE_LOADER, "address-heading"));
            });
            header.col(|ui| {
                ui.strong(fl!(crate::LANGUAGE_LOADER, "value-heading"));
            });
            if show_search_types {
                header.col(|ui| {
                    ui.strong(fl!(crate::LANGUAGE_LOADER, "datatype-heading"));
                });
            }
            if !is_string {
                header.col(|ui| {
                    let mut checked = all_frozen;
                    if ui.checkbox(&mut checked, "").clicked() {
                        toggle_freeze_all = true;
                    }
                    ui.label(fl!(crate::LANGUAGE_LOADER, "freezed-heading"));
                    if frozen_count > 0 {
                        ui.label(
                            egui::RichText::new(format!("({frozen_count}/{total_results})"))
                                .small()
                                .color(ui.visuals().selection.bg_fill),
                        );
                    }
                });
            }
            header.col(|ui| {
                let mut h = hex_display;
                if ui.checkbox(&mut h, fl!(crate::LANGUAGE_LOADER, "hex-toggle-label")).clicked() {
                    toggle_hex = true;
                }
            });
        })
        .body(|body| {
            body.rows(RESULT_ROW_HEIGHT, total_results, |mut row| {
                let i = row.index();
                let Some(result) = results.get(i).copied() else {
                    return;
                };
                let is_frozen = freezed.contains(&result.addr);
                let recently_changed = app
                    .changed_addresses
                    .get(&result.addr)
                    .map(|t| t.elapsed() < CHANGE_HIGHLIGHT)
                    .unwrap_or(false);

                // Read or look up the row's current value text.
                let value_text = if let Some(cached) = app.value_change_tracker.get(&result.addr) {
                    cached.clone()
                } else if let Some(byte_len) = result.search_type.fixed_byte_length() {
                    match pid.try_into_process_handle() {
                        Ok(handle) => match copy_address(result.addr, byte_len, &handle) {
                            Ok(buf) => {
                                let v = SearchValue(result.search_type, buf);
                                if hex_display { v.to_hex_string() } else { v.to_string() }
                            }
                            Err(_) => String::new(),
                        },
                        Err(_) => String::new(),
                    }
                } else if matches!(
                    result.search_type,
                    SearchType::String | SearchType::StringUtf16
                ) {
                    let utf16 = result.search_type == SearchType::StringUtf16;
                    let max_bytes = if utf16 { string_char_count * 2 } else { string_byte_len };
                    read_string_from_process(pid, result.addr, utf16, max_bytes).unwrap_or_default()
                } else {
                    String::new()
                };

                // Address column
                row.col(|ui| {
                    ui.monospace(format!("0x{:X}", result.addr));
                });

                // Value column
                row.col(|ui| {
                    let editing = matches!(app.editing_result, Some((idx, _)) if idx == i);
                    if editing {
                        if let Some((_, buf)) = app.editing_result.as_mut() {
                            let response = ui.add(
                                egui::TextEdit::singleline(buf)
                                    .desired_width(ui.available_width().min(140.0)),
                            );
                            response.request_focus();
                            if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                commit_edit = Some((i, buf.clone()));
                            } else if response.lost_focus()
                                || ui.input(|i| i.key_pressed(egui::Key::Escape))
                            {
                                cancel_edit = true;
                            }
                        }
                    } else {
                        let mut text = egui::RichText::new(&value_text);
                        if is_frozen {
                            text = text.color(ui.visuals().selection.bg_fill);
                        }
                        if recently_changed {
                            text = text.color(egui::Color32::from_rgb(255, 180, 130));
                        }
                        let response = ui.add(
                            egui::Label::new(text)
                                .sense(egui::Sense::click())
                                .truncate(),
                        );
                        if response.clicked() && !is_string {
                            begin_edit = Some((i, value_text.clone()));
                        }
                        if recently_changed {
                            // Subtle backdrop for changed cells
                            ui.painter().rect_filled(
                                response.rect.expand(2.0),
                                2.0,
                                egui::Color32::from_rgba_unmultiplied(255, 180, 130, 24),
                            );
                        }
                    }
                });

                if show_search_types {
                    row.col(|ui| {
                        ui.label(result.search_type.get_description_text());
                    });
                }

                if !is_string {
                    row.col(|ui| {
                        let mut frozen = is_frozen;
                        if ui.checkbox(&mut frozen, "").changed() {
                            toggle_freeze = Some(i);
                        }
                    });
                }

                row.col(|ui| {
                    if ui.small_button(fl!(crate::LANGUAGE_LOADER, "edit-button")).clicked() {
                        open_editor = Some(i);
                    }
                    if ui.small_button(fl!(crate::LANGUAGE_LOADER, "remove-button")).clicked() {
                        remove_result = Some(i);
                    }
                });
            });
        });

    drop(results);

    if toggle_freeze_all {
        app.toggle_freeze_all();
    }
    if toggle_hex {
        app.hex_display = !app.hex_display;
        app.persist_settings();
    }
    if let Some(i) = toggle_freeze {
        app.toggle_freeze(i);
    }
    if let Some(i) = remove_result {
        app.remove_result(i);
    }
    if let Some(i) = open_editor {
        app.open_memory_editor(i);
    }
    if let Some((i, text)) = begin_edit {
        app.editing_result = Some((i, text));
    }
    if let Some((i, text)) = commit_edit {
        let ok = app.commit_result_value(i, &text);
        if ok {
            app.editing_result = None;
        }
    }
    if cancel_edit {
        app.editing_result = None;
    }

    // Silence unused warning when AppState transitions are handled elsewhere.
    let _ = AppState::InProcess;
}

/// Read a contiguous NUL-terminated UTF-8 or UTF-16LE string from the target
/// process. Returns `None` only if the entire process-memory read fails;
/// otherwise a (possibly empty / lossy) `String` is always returned.
///
/// Public so the change-tracker in [`App`] can use the exact same read path
/// the row renderer falls back to, keeping the cached strings consistent.
pub fn read_string_from_process(pid: process_memory::Pid, addr: usize, utf16le: bool, max_bytes: usize) -> Option<String> {
    let handle = pid.try_into_process_handle().ok()?;
    if utf16le {
        let buf = copy_address(addr, max_bytes.max(2), &handle).ok()?;
        let mut units: Vec<u16> = Vec::with_capacity(buf.len() / 2);
        for chunk in buf.chunks_exact(2) {
            let u = u16::from_le_bytes([chunk[0], chunk[1]]);
            if u == 0 {
                break;
            }
            units.push(u);
        }
        Some(String::from_utf16_lossy(&units))
    } else {
        let buf = copy_address(addr, max_bytes.max(1), &handle).ok()?;
        let nul = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        Some(String::from_utf8_lossy(&buf[..nul]).into_owned())
    }
}
