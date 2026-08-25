use std::sync::atomic::Ordering;

use i18n_embed_fl::fl;
use process_memory::{TryIntoProcessHandle, copy_address};

use crate::{
    SearchMode, SearchType, SearchValue, UnknownComparison,
    ui::app::{App, AppState, CHANGE_HIGHLIGHT},
};

/// Uniform row height used by the virtualized result table.
const RESULT_ROW_HEIGHT: f32 = 32.0;

/// Above this many hits the table is not worth browsing — narrowing the search
/// is the only useful next step, so the list is collapsed behind a hint.
const BROWSE_RESULT_LIMIT: usize = 1000;

/// A search value is a handful of characters; a full-width field only pushes
/// the type picker to the far edge of the window.
const SEARCH_FIELD_WIDTH: f32 = 320.0;

const FREEZE_ICON: &str = "\u{2744}";
const EDIT_ICON: &str = "\u{270F}";
/// The ✕/✗/✘ dingbats are not in the bundled fonts and render as tofu; the
/// multiplication sign is, so it stands in for the close glyph.
const REMOVE_ICON: &str = "\u{00D7}";
/// Width reserved in a cell for its hover icon so nothing shifts when the
/// pointer enters or leaves the row.
const ICON_SLOT_WIDTH: f32 = 26.0;

/// Row action rendered as a bare glyph. The slot is always reserved; the
/// button only becomes visible and clickable while its row is hovered.
fn icon_button(ui: &mut egui::Ui, visible: bool, glyph: &str, tooltip: String) -> egui::Response {
    let button = egui::Button::new(egui::RichText::new(glyph).size(16.0))
        .frame(false)
        .min_size(egui::vec2(ICON_SLOT_WIDTH - 4.0, 20.0));
    let response = ui.add_visible(visible, button);
    if visible {
        response.on_hover_text(tooltip).on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    }
}

pub fn view_in_process(app: &mut App, ui: &mut egui::Ui) {
    top_bar(app, ui);
    error_bar(app, ui);
    tab_bar(app, ui);
    egui::CentralPanel::default()
        .frame(egui::Frame::central_panel(ui.style()).inner_margin(egui::Margin::symmetric(20, 16)))
        .show(ui, |ui| {
            search_area(app, ui);
        });
}

fn top_bar(app: &mut App, ui: &mut egui::Ui) {
    egui::Panel::top("in_process_top")
        .frame(
            egui::Frame::new()
                .fill(egui::Color32::from_rgb(22, 25, 29))
                .inner_margin(egui::Margin::symmetric(20, 10))
                .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(45, 50, 60))),
        )
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(fl!(crate::LANGUAGE_LOADER, "process-label")).size(14.0).weak());
                ui.add_space(2.0);
                let accent = ui.visuals().selection.bg_fill;
                ui.label(egui::RichText::new(&app.state.process_name).size(15.0).strong().color(accent));
                ui.label(egui::RichText::new(format!("PID {}", app.state.pid)).size(13.0).weak().monospace());

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let btn = |label: String| egui::Button::new(egui::RichText::new(label).size(14.0)).min_size(egui::vec2(0.0, 28.0));
                    if ui.add(btn(fl!(crate::LANGUAGE_LOADER, "close-button"))).clicked() {
                        app.back_to_main_menu();
                    }
                    if ui.add(btn(fl!(crate::LANGUAGE_LOADER, "load-cheat-table-button"))).clicked() {
                        app.load_cheat_table();
                    }
                    if ui.add(btn(fl!(crate::LANGUAGE_LOADER, "save-cheat-table-button"))).clicked() {
                        app.save_cheat_table();
                    }
                    if !app.cheat_table_status.is_empty() {
                        ui.add_space(6.0);
                        ui.label(egui::RichText::new(&app.cheat_table_status).size(13.0).weak());
                    }
                });
            });
        });
}

fn error_bar(app: &mut App, ui: &mut egui::Ui) {
    if let Some(error) = app.state.current_error() {
        let text = error.to_string();
        let mut dismiss = false;
        egui::Panel::top("in_process_error")
            .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(20, 6)))
            .show(ui, |ui| {
                egui::Frame::new()
                    .fill(egui::Color32::from_rgba_unmultiplied(160, 50, 50, 30))
                    .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(160, 70, 70)))
                    .corner_radius(egui::CornerRadius::same(8))
                    .inner_margin(egui::Margin::symmetric(12, 8))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.colored_label(egui::Color32::from_rgb(220, 120, 120), "\u{26A0}");
                            ui.colored_label(egui::Color32::from_rgb(232, 180, 180), text);
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

/// Browser-style horizontal tab bar listing all active searches.
///
/// Layout:
/// * One row of tabs with rounded top corners.
/// * Active tab is filled with the accent colour; inactive tabs are muted.
/// * A small `×` close button is shown on hover (and persistently on the
///   active tab) when there is more than one search.
/// * Trailing `+` action creates a new search.
/// * The bar is horizontally scrollable so a large number of searches still
///   stays usable.
fn tab_bar(app: &mut App, ui: &mut egui::Ui) {
    egui::Panel::top("search_tab_bar")
        .frame(
            egui::Frame::new()
                .fill(egui::Color32::from_rgb(20, 23, 27))
                .inner_margin(egui::Margin {
                    left: 12,
                    right: 12,
                    top: 6,
                    bottom: 0,
                })
                .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(45, 50, 60))),
        )
        .show(ui, |ui| {
            let accent = ui.visuals().selection.bg_fill;
            let count = app.state.searches.len();
            let mut switch_to: Option<usize> = None;
            let mut close: Option<usize> = None;
            let mut close_others: Option<usize> = None;
            let mut rename: Option<usize> = None;
            let mut new_search = false;

            // F2 starts renaming the active tab when nothing else has
            // focus (so the user can't trigger it while typing in the
            // value-input or any other text field).
            if app.renaming_search_index.is_none() && !ui.memory(|m| m.focused().is_some()) && ui.input(|i| i.key_pressed(egui::Key::F2)) {
                rename = Some(app.state.current_search);
            }

            egui::ScrollArea::horizontal()
                .id_salt("tab_bar_scroll")
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        const TAB_HEIGHT: f32 = 32.0;
                        const TAB_RADIUS: u8 = 8;

                        for i in 0..count {
                            let is_active = i == app.state.current_search;
                            let name = app.state.searches[i].description.clone();

                            // Inline rename: a TextEdit replaces the tab so the
                            // bar height stays stable.
                            if app.renaming_search_index == Some(i) {
                                let response = ui.add(
                                    egui::TextEdit::singleline(&mut app.rename_search_text)
                                        .id(egui::Id::new(("rename_search", i)))
                                        .desired_width(140.0)
                                        .margin(egui::Margin::symmetric(8, 6)),
                                );
                                if app.rename_request_focus {
                                    response.request_focus();
                                    app.rename_request_focus = false;
                                }
                                let escape_pressed = ui.input(|input| input.key_pressed(egui::Key::Escape));
                                if escape_pressed {
                                    app.cancel_rename_search();
                                } else if response.lost_focus() {
                                    app.commit_rename_search();
                                }
                                continue;
                            }

                            // Measure the label to compute the tab width.
                            let font = egui::FontId::proportional(14.0);
                            let galley = ui.painter().layout_no_wrap(name.clone(), font.clone(), egui::Color32::WHITE);
                            let label_w = galley.size().x.ceil();
                            let show_close = count > 1;
                            let close_w = if show_close { 18.0 } else { 0.0 };
                            let tab_w = label_w + 24.0 + close_w + if show_close { 4.0 } else { 0.0 };

                            let (full_rect, _) = ui.allocate_exact_size(egui::vec2(tab_w, TAB_HEIGHT), egui::Sense::hover());

                            // Reserve the close button area; the rest is the
                            // clickable tab body.
                            let close_rect = if show_close {
                                Some(egui::Rect::from_center_size(
                                    egui::pos2(full_rect.right() - 11.0, full_rect.center().y),
                                    egui::vec2(16.0, 16.0),
                                ))
                            } else {
                                None
                            };
                            let body_max_x = close_rect.map(|r| r.left() - 2.0).unwrap_or(full_rect.max.x);
                            let body_rect = egui::Rect::from_min_max(full_rect.min, egui::pos2(body_max_x, full_rect.max.y));
                            // `click_and_drag()` covers primary, secondary
                            // (for the context menu) and double click.
                            let body_response = ui.interact(body_rect, egui::Id::new(("tab_body", i)), egui::Sense::click_and_drag());

                            let corners = egui::CornerRadius {
                                nw: TAB_RADIUS,
                                ne: TAB_RADIUS,
                                sw: 0,
                                se: 0,
                            };
                            let (bg, text_color) = if is_active {
                                (accent, egui::Color32::WHITE)
                            } else if body_response.hovered() {
                                (egui::Color32::from_rgb(36, 40, 46), egui::Color32::from_rgb(225, 228, 232))
                            } else {
                                (egui::Color32::from_rgb(28, 31, 36), egui::Color32::from_rgb(180, 184, 190))
                            };
                            ui.painter().rect_filled(full_rect, corners, bg);

                            let label_pos = egui::pos2(full_rect.left() + 12.0, full_rect.center().y);
                            let label_font = if is_active {
                                egui::FontId::new(14.0, egui::FontFamily::Proportional)
                            } else {
                                font.clone()
                            };
                            ui.painter().text(label_pos, egui::Align2::LEFT_CENTER, &name, label_font, text_color);

                            if let Some(rect) = close_rect {
                                let close_response = ui.interact(rect, egui::Id::new(("close_tab", i)), egui::Sense::click());
                                let hovered = close_response.hovered();
                                let close_bg = if hovered {
                                    egui::Color32::from_rgba_unmultiplied(255, 255, 255, 40)
                                } else {
                                    egui::Color32::TRANSPARENT
                                };
                                ui.painter().rect_filled(rect, egui::CornerRadius::same(3), close_bg);
                                let glyph_color = if hovered { egui::Color32::WHITE } else { text_color };
                                ui.painter().text(
                                    rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    "\u{00D7}",
                                    egui::FontId::proportional(14.0),
                                    glyph_color,
                                );
                                if close_response.clicked() {
                                    close = Some(i);
                                }
                            }

                            if body_response.clicked() {
                                switch_to = Some(i);
                            }
                            // Double-click only renames when the tab is
                            // already active — otherwise it would be too
                            // easy to start a rename during a quick
                            // tab switch.
                            if is_active && body_response.double_clicked() {
                                rename = Some(i);
                            }
                            body_response.clone().on_hover_text(fl!(crate::LANGUAGE_LOADER, "rename-search-hint"));

                            // Right-click context menu: Rename / Close /
                            // Close others. "Close" and "Close others"
                            // are only enabled when more than one search
                            // exists.
                            body_response.context_menu(|ui| {
                                if ui.button(fl!(crate::LANGUAGE_LOADER, "rename-search-menu")).clicked() {
                                    rename = Some(i);
                                    ui.close();
                                }
                                ui.separator();
                                let many = count > 1;
                                if ui
                                    .add_enabled(many, egui::Button::new(fl!(crate::LANGUAGE_LOADER, "close-search-menu")))
                                    .clicked()
                                {
                                    close = Some(i);
                                    ui.close();
                                }
                                if ui
                                    .add_enabled(many, egui::Button::new(fl!(crate::LANGUAGE_LOADER, "close-other-searches-menu")))
                                    .clicked()
                                {
                                    close_others = Some(i);
                                    ui.close();
                                }
                            });
                        }

                        ui.add_space(6.0);

                        // Trailing "+" action — looks like a quiet ghost button,
                        // not a tab.
                        let plus_size = egui::vec2(28.0, 28.0);
                        let (plus_rect, _) = ui.allocate_exact_size(plus_size, egui::Sense::hover());
                        let plus_response = ui.interact(plus_rect, egui::Id::new("new_search_tab"), egui::Sense::click());
                        let plus_bg = if plus_response.hovered() {
                            egui::Color32::from_rgb(36, 40, 46)
                        } else {
                            egui::Color32::TRANSPARENT
                        };
                        ui.painter().rect_filled(plus_rect, egui::CornerRadius::same(6), plus_bg);
                        ui.painter().text(
                            plus_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "+",
                            egui::FontId::proportional(18.0),
                            egui::Color32::from_rgb(200, 205, 212),
                        );
                        if plus_response.clicked() {
                            new_search = true;
                        }
                        plus_response.on_hover_text(fl!(crate::LANGUAGE_LOADER, "add-search-button"));
                    });
                });

            if let Some(i) = switch_to {
                app.switch_search(i);
            }
            if let Some(i) = rename {
                app.begin_rename_search(i);
            }
            if let Some(i) = close {
                app.close_search(i);
            }
            if let Some(i) = close_others {
                app.close_other_searches(i);
            }
            if new_search {
                app.new_search();
            }
        });
}

fn search_area(app: &mut App, ui: &mut egui::Ui) {
    let search_index = app.state.current_search;
    let Some(search_context) = app.state.searches.get(search_index) else {
        return;
    };
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

    let accent = ui.visuals().selection.bg_fill;
    let surface = egui::Color32::from_rgb(22, 25, 30);
    let card_stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(55, 62, 72));

    // The active tab already shows the current search's name, so no
    // additional heading is required here.

    // === Input card =====================================================
    egui::Frame::new()
        .fill(surface)
        .stroke(card_stroke)
        .corner_radius(egui::CornerRadius::same(10))
        .inner_margin(egui::Margin::symmetric(16, 14))
        .show(ui, |ui| {
            if selected_type == SearchType::Unknown {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(fl!(crate::LANGUAGE_LOADER, "search-type-label")).size(14.0));
                    type_picker(app, ui, show_type_picker, selected_type);
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new(fl!(crate::LANGUAGE_LOADER, "unknown-search-description")).size(13.0).weak());
                });
            } else {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(fl!(crate::LANGUAGE_LOADER, "value-label")).size(14.0));
                    let mut buf = value_text.clone();
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut buf)
                            .hint_text(fl!(
                                crate::LANGUAGE_LOADER,
                                "search-value-label",
                                valuetype = selected_type.get_description_text()
                            ))
                            .desired_width(SEARCH_FIELD_WIDTH)
                            .margin(egui::Margin::symmetric(10, 8))
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
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.add_space(60.0);
                        ui.colored_label(egui::Color32::from_rgb(220, 120, 120), format!("\u{26A0}  {err}"));
                    });
                }
            }
        });

    ui.add_space(12.0);

    // === Action row =====================================================
    let primary_btn = |label: String| {
        egui::Button::new(egui::RichText::new(label).size(14.0).strong().color(egui::Color32::WHITE))
            .fill(accent)
            .stroke(egui::Stroke::new(1.0, accent))
            .min_size(egui::vec2(160.0, 32.0))
    };
    let secondary_btn = |label: String| egui::Button::new(egui::RichText::new(label).size(14.0)).min_size(egui::vec2(0.0, 32.0));
    let results_label = |ui: &mut egui::Ui, count: usize| {
        let text = fl!(crate::LANGUAGE_LOADER, "found-results-label", results = count)
            .chars()
            .filter(|c| c.is_ascii())
            .collect::<String>();
        let mut text = egui::RichText::new(text).size(14.0).strong();
        if count > BROWSE_RESULT_LIMIT {
            text = text.color(ui.visuals().warn_fg_color);
        }
        ui.label(text);
    };

    if !matches!(searching, SearchMode::None) {
        // Searching in progress
        let progress = if total_bytes == 0 { 0.0 } else { current_bytes as f32 / total_bytes as f32 };
        let label = if searching == SearchMode::Percent {
            fl!(crate::LANGUAGE_LOADER, "update-numbers-progress", current = current_bytes, total = total_bytes)
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
        .collect::<String>();
        ui.horizontal(|ui| {
            ui.add(egui::ProgressBar::new(progress).desired_width(ui.available_width() - 240.0).show_percentage());
            ui.label(egui::RichText::new(label).size(13.0).weak());
        });
    } else if !is_search_complete {
        // No search has been started yet
        ui.horizontal(|ui| {
            let enabled = parse_error.is_none() || selected_type == SearchType::Unknown;
            if ui
                .add_enabled(enabled, primary_btn(fl!(crate::LANGUAGE_LOADER, "initial-search-button")))
                .clicked()
            {
                app.start_search();
            }
        });
    } else if selected_type == SearchType::Unknown {
        // Unknown-search with results, show comparison buttons
        ui.horizontal_wrapped(|ui| {
            if ui.add(secondary_btn(fl!(crate::LANGUAGE_LOADER, "decreased-button"))).clicked() {
                app.unknown_search(UnknownComparison::Decreased);
            }
            if ui.add(secondary_btn(fl!(crate::LANGUAGE_LOADER, "increased-button"))).clicked() {
                app.unknown_search(UnknownComparison::Increased);
            }
            if ui.add(secondary_btn(fl!(crate::LANGUAGE_LOADER, "changed-button"))).clicked() {
                app.unknown_search(UnknownComparison::Changed);
            }
            let unchanged_enabled = search_results > 0 || can_undo;
            if ui
                .add_enabled(unchanged_enabled, secondary_btn(fl!(crate::LANGUAGE_LOADER, "unchanged-button")))
                .clicked()
            {
                app.unknown_search(UnknownComparison::Unchanged);
            }
            ui.add_space(8.0);
            ui.separator();
            ui.add_space(8.0);
            if ui.add(secondary_btn(fl!(crate::LANGUAGE_LOADER, "clear-button"))).clicked() {
                app.clear_results();
            }
            if ui.add_enabled(can_undo, secondary_btn(fl!(crate::LANGUAGE_LOADER, "undo-button"))).clicked() {
                app.undo_search();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                results_label(ui, search_results);
            });
        });
    } else {
        // Regular search with results
        ui.horizontal_wrapped(|ui| {
            let enabled = parse_error.is_none();
            if ui.add_enabled(enabled, primary_btn(fl!(crate::LANGUAGE_LOADER, "update-button"))).clicked() {
                app.start_search();
            }
            if ui.add(secondary_btn(fl!(crate::LANGUAGE_LOADER, "clear-button"))).clicked() {
                app.clear_results();
            }
            if ui.add_enabled(can_undo, secondary_btn(fl!(crate::LANGUAGE_LOADER, "undo-button"))).clicked() {
                app.undo_search();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                results_label(ui, search_results);
            });
        });
    }

    if matches!(searching, SearchMode::None) && search_results > 0 {
        ui.add_space(14.0);
        let collapsed = search_results > BROWSE_RESULT_LIMIT && !app.state.show_results;
        if collapsed {
            too_many_results_panel(app, ui);
        } else {
            if search_results > BROWSE_RESULT_LIMIT {
                if ui.add(secondary_btn(fl!(crate::LANGUAGE_LOADER, "hide-results-button"))).clicked() {
                    app.state.show_results = false;
                }
                ui.add_space(8.0);
            }
            result_table(app, ui);
        }
    }
}

/// Replaces the table while the result list is too large to read, so the next
/// useful action is visible instead of thousands of rows.
fn too_many_results_panel(app: &mut App, ui: &mut egui::Ui) {
    egui::Frame::new()
        .fill(egui::Color32::from_rgb(22, 25, 30))
        .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(55, 62, 72)))
        .corner_radius(egui::CornerRadius::same(10))
        .inner_margin(egui::Margin::symmetric(16, 14))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("\u{1F50D}").size(20.0));
                ui.add_space(6.0);
                ui.vertical(|ui| {
                    ui.label(egui::RichText::new(fl!(crate::LANGUAGE_LOADER, "too-many-results-hint")).size(14.0));
                    ui.add_space(8.0);
                    if ui
                        .add(
                            egui::Button::new(egui::RichText::new(fl!(crate::LANGUAGE_LOADER, "show-results-button")).size(13.0))
                                .min_size(egui::vec2(0.0, 28.0)),
                        )
                        .clicked()
                    {
                        app.state.show_results = true;
                    }
                });
            });
        });
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
    let is_string = matches!(search_context.search_type, SearchType::String | SearchType::StringUtf16);
    let show_search_types = matches!(search_context.search_type, SearchType::Guess | SearchType::Unknown);
    let string_byte_len = search_context.search_value_text.len();
    let string_char_count = search_context.search_value_text.chars().count();
    let pid = app.state.pid as process_memory::Pid;

    // Build the freezed set + an "all frozen" check once for the whole frame.
    let freezed: std::collections::HashSet<usize> = search_context.freezed_addresses.iter().copied().collect();
    let all_frozen = !results.is_empty() && results.iter().all(|r| freezed.contains(&r.addr));
    let frozen_count = results.iter().filter(|r| freezed.contains(&r.addr)).count();

    // Capture lookups we'll need inside the row closure. Borrow checker:
    // the row closure runs inside TableBuilder::body which holds `ui`, and
    // we mutate `app` (toggle_freeze etc.) only via deferred actions
    // collected here.
    let mut toggle_freeze: Option<usize> = None;
    let mut toggle_freeze_all = false;
    let mut remove_result: Option<usize> = None;
    let mut open_editor: Option<usize> = None;
    let mut begin_edit: Option<(usize, String)> = None;
    let mut live_write: Option<(usize, String)> = None;
    let mut commit_edit: Option<(usize, String)> = None;
    let mut cancel_edit = false;
    // Row-level hover comes from the previous frame: the cells are built before
    // the row response exists, and a frame of lag is invisible at 30 Hz.
    let hovered_row = app.hovered_result_row;
    let mut new_hovered_row: Option<usize> = None;
    // Hover is decided by the pointer against the row rectangle rather than by
    // `Response::hovered()`: the latter goes false as soon as the pointer
    // reaches the icon on top of the row, which would hide the icon, re-hover
    // the row, and flicker forever without ever accepting a click.
    let pointer_pos = ui.ctx().pointer_hover_pos();

    use egui_extras::{Column, TableBuilder};

    // Fixed widths only: a trailing remainder column would just reintroduce
    // the empty strip this table used to carry around.
    let mut builder = TableBuilder::new(ui)
        .striped(true)
        .resizable(true)
        .sense(egui::Sense::hover())
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(Column::initial(150.0).at_least(120.0)) // address + remove icon
        .column(Column::initial(190.0).at_least(140.0)); // value + edit icon
    if show_search_types {
        builder = builder.column(Column::initial(110.0).at_least(80.0));
    }
    if !is_string {
        builder = builder.column(Column::initial(56.0).at_least(48.0));
    }

    builder
        .header(32.0, |mut header| {
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
                    ui.spacing_mut().item_spacing.x = 4.0;
                    let mut checked = all_frozen;
                    if ui.checkbox(&mut checked, "").clicked() {
                        toggle_freeze_all = true;
                    }
                    let heading = fl!(crate::LANGUAGE_LOADER, "freezed-heading");
                    let tooltip = if frozen_count > 0 {
                        format!("{heading} ({frozen_count}/{total_results})")
                    } else {
                        heading
                    };
                    let color = if frozen_count > 0 {
                        ui.visuals().selection.bg_fill
                    } else {
                        ui.visuals().weak_text_color()
                    };
                    ui.label(egui::RichText::new(FREEZE_ICON).size(15.0).color(color)).on_hover_text(tooltip);
                });
            }
        })
        .body(|body| {
            body.rows(RESULT_ROW_HEIGHT, total_results, |mut row| {
                let i = row.index();
                let Some(result) = results.get(i).copied() else {
                    return;
                };
                let is_frozen = freezed.contains(&result.addr);
                let row_hovered = hovered_row == Some(i);

                // Always read fresh raw bytes from the target so the display
                // and the change diff reflect the process's current state.
                // Comparing raw bytes (not formatted strings) skips a String
                // allocation per row per frame, and dovetails with the bulk
                // tracker which works in raw bytes too.
                let raw_bytes: Vec<u8> = if let Some(byte_len) = result.search_type.fixed_byte_length() {
                    match pid.try_into_process_handle() {
                        Ok(handle) => copy_address(result.addr, byte_len, &handle).unwrap_or_default(),
                        Err(_) => Vec::new(),
                    }
                } else if matches!(result.search_type, SearchType::String | SearchType::StringUtf16) {
                    let utf16 = result.search_type == SearchType::StringUtf16;
                    let max_bytes = if utf16 { string_char_count * 2 } else { string_byte_len };
                    read_bytes_from_process(pid, result.addr, max_bytes).unwrap_or_default()
                } else {
                    Vec::new()
                };

                // Diff this frame's fresh bytes against the previous-value
                // record. The LRU bounds growth on million-row searches so
                // only the rows the user is actually looking at stay tracked
                // (the bulk tracker covers the rest in round-robin fashion).
                let prev_bytes = app.value_change_tracker.put(result.addr, raw_bytes.clone());
                if let Some(prev_bytes) = prev_bytes
                    && prev_bytes != raw_bytes
                {
                    app.changed_addresses.insert(result.addr, std::time::Instant::now());
                }

                // Format for display. Done after the diff so unchanged rows
                // skip the alloc-and-decode entirely when bytes are empty.
                let value_text = if raw_bytes.is_empty() {
                    String::new()
                } else if matches!(result.search_type, SearchType::String | SearchType::StringUtf16) {
                    decode_string_bytes(&raw_bytes, result.search_type == SearchType::StringUtf16)
                } else {
                    SearchValue(result.search_type, raw_bytes).to_string()
                };

                // Linear fade from 1.0 right after a change down to 0.0 at
                // `CHANGE_HIGHLIGHT`. Drives the orange backdrop tint — same
                // treatment as the memory editor's per-byte change overlay.
                let change_intensity = app
                    .changed_addresses
                    .get(&result.addr)
                    .map(|t| {
                        let elapsed = t.elapsed();
                        if elapsed >= CHANGE_HIGHLIGHT {
                            0.0
                        } else {
                            1.0 - (elapsed.as_secs_f32() / CHANGE_HIGHLIGHT.as_secs_f32())
                        }
                    })
                    .unwrap_or(0.0);
                let recently_changed = change_intensity > 0.0;

                // Address column
                row.col(|ui| {
                    ui.monospace(format!("0x{:X}", result.addr));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if icon_button(ui, row_hovered, REMOVE_ICON, fl!(crate::LANGUAGE_LOADER, "remove-button")).clicked() {
                            remove_result = Some(i);
                        }
                    });
                });

                // Value column - always rendered as an editable text box.
                // When the user focuses or types into the cell we capture
                // the row into `app.editing_result`; otherwise the box
                // displays the live value freshly read this frame.
                row.col(|ui| {
                    // Stable per-result id: keeps focus across the
                    // display/edit swap and stops one row's editor state
                    // from bleeding into another when results are re-sorted
                    // or scrolled.
                    let value_id = egui::Id::new(("result-value", result.addr, result.search_type));
                    let has_focus = ui.memory(|mem| mem.has_focus(value_id));
                    // Only the focused cell may render from the edit buffer.
                    // Every other row shows what was read from the process
                    // this frame, so a buffer left behind by a failed commit
                    // or a row that scrolled away can't freeze the display.
                    let is_edit_row = matches!(app.editing_result, Some((idx, _)) if idx == i);
                    let editing = is_edit_row && has_focus;
                    if is_edit_row && !has_focus {
                        cancel_edit = true;
                    }
                    let text_color = if recently_changed {
                        Some(egui::Color32::from_rgb(255, 180, 130))
                    } else if is_frozen {
                        Some(ui.visuals().selection.bg_fill)
                    } else {
                        None
                    };
                    let cell_width = (ui.available_width() - ICON_SLOT_WIDTH).clamp(60.0, 150.0);

                    let response = if editing {
                        // Bind the TextEdit straight to the live editing
                        // buffer so keystrokes mutate it in place.
                        let buf = &mut app.editing_result.as_mut().unwrap().1;
                        let r = ui.add(
                            egui::TextEdit::singleline(buf)
                                .id(value_id)
                                .desired_width(cell_width)
                                .text_color_opt(text_color),
                        );
                        if r.changed() {
                            // Live write: try to push every keystroke into
                            // the target process. Invalid intermediate input
                            // is silently ignored so half-typed numbers
                            // don't spam errors.
                            live_write = Some((i, buf.clone()));
                        }
                        if r.lost_focus() {
                            if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                commit_edit = Some((i, buf.clone()));
                            } else {
                                cancel_edit = true;
                            }
                        } else if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                            cancel_edit = true;
                        }
                        r
                    } else {
                        // Display-mode TextEdit. We re-render the live
                        // value every frame; the user transitions to
                        // edit mode the moment they focus or type.
                        let mut buf = value_text.clone();
                        let r = ui.add(
                            egui::TextEdit::singleline(&mut buf)
                                .id(value_id)
                                .desired_width(cell_width)
                                .text_color_opt(text_color),
                        );
                        if r.gained_focus() || r.changed() {
                            begin_edit = Some((i, buf));
                        }
                        r
                    };

                    if change_intensity > 0.0 {
                        // Translucent orange tint that fades out — same colour
                        // and alpha curve as the memory editor's per-byte
                        // change overlay. Painted on top of the value editor
                        // so the cell flashes orange and decays to default.
                        let alpha = (change_intensity * 180.0) as u8;
                        ui.painter()
                            .rect_filled(response.rect.expand(2.0), 3.0, egui::Color32::from_rgba_unmultiplied(255, 150, 60, alpha));
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if icon_button(ui, row_hovered, EDIT_ICON, fl!(crate::LANGUAGE_LOADER, "edit-button")).clicked() {
                            open_editor = Some(i);
                        }
                    });
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

                if let Some(pos) = pointer_pos
                    && row.response().rect.contains(pos)
                {
                    new_hovered_row = Some(i);
                }
            });
        });

    drop(results);

    app.hovered_result_row = new_hovered_row;

    if toggle_freeze_all {
        app.toggle_freeze_all();
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
    // Clicking straight from one value cell into another reports the old
    // row's `lost_focus` and the new row's `gained_focus` in the same frame,
    // so a cancel must never discard the edit that just started.
    let started_edit = begin_edit.is_some();
    if let Some((i, text)) = begin_edit {
        app.editing_result = Some((i, text));
    }
    if let Some((i, text)) = live_write {
        app.try_write_result_value(i, &text);
    }
    if let Some((i, text)) = commit_edit {
        let ok = app.commit_result_value(i, &text);
        if ok {
            app.editing_result = None;
        }
    }
    if cancel_edit && !started_edit {
        app.editing_result = None;
    }

    // Silence unused warning when AppState transitions are handled elsewhere.
    let _ = AppState::InProcess;
}

/// Read up to `max_bytes` raw bytes from the target process at `addr`.
/// Used as a primitive by both the change-tracker (which diffs raw bytes)
/// and `read_string_from_process` (which decodes them into a UTF-8 / UTF-16
/// string for display). Returns `None` only on read failure.
pub fn read_bytes_from_process(pid: process_memory::Pid, addr: usize, max_bytes: usize) -> Option<Vec<u8>> {
    let handle = pid.try_into_process_handle().ok()?;
    copy_address(addr, max_bytes.max(1), &handle).ok()
}

/// Decode a UTF-8 (or UTF-16LE if `utf16le`) NUL-terminated string out of
/// a raw byte buffer previously read from process memory. Used by both the
/// row renderer and the bulk change-tracker so identical bytes always
/// produce identical display strings.
pub fn decode_string_bytes(bytes: &[u8], utf16le: bool) -> String {
    if utf16le {
        let mut units: Vec<u16> = Vec::with_capacity(bytes.len() / 2);
        for chunk in bytes.chunks_exact(2) {
            let u = u16::from_le_bytes([chunk[0], chunk[1]]);
            if u == 0 {
                break;
            }
            units.push(u);
        }
        String::from_utf16_lossy(&units)
    } else {
        let nul = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
        String::from_utf8_lossy(&bytes[..nul]).into_owned()
    }
}

/// Read a contiguous NUL-terminated UTF-8 or UTF-16LE string from the target
/// process. Returns `None` only if the entire process-memory read fails;
/// otherwise a (possibly empty / lossy) `String` is always returned.
///
/// Public so the change-tracker in [`App`] can use the exact same read path
/// the row renderer falls back to, keeping the cached strings consistent.
pub fn read_string_from_process(pid: process_memory::Pid, addr: usize, utf16le: bool, max_bytes: usize) -> Option<String> {
    let bytes = read_bytes_from_process(pid, addr, max_bytes.max(if utf16le { 2 } else { 1 }))?;
    Some(decode_string_bytes(&bytes, utf16le))
}
