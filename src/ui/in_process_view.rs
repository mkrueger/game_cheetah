use std::{sync::atomic::Ordering, time::Duration};

use i18n_embed_fl::fl;
use process_memory::{TryIntoProcessHandle, copy_address};

use crate::{
    SearchMode, SearchResult, SearchType, SearchValue, UnknownComparison,
    ui::app::{App, AppState, CHANGE_HIGHLIGHT},
};

/// Uniform row height used by the virtualized result table.
const RESULT_ROW_HEIGHT: f32 = 32.0;

/// Above this many hits the table is not worth browsing — narrowing the search
/// is the only useful next step, so the list is collapsed behind a hint.
const BROWSE_RESULT_LIMIT: usize = 1000;

/// A search value is a handful of characters; a full-width field only pushes
/// the type picker to the far edge of the window.
const SEARCH_FIELD_WIDTH: f32 = 220.0;

const FREEZE_ICON: &str = "\u{2744}";
const MEMORY_ICON: &str = "…";
/// The ✕/✗/✘ dingbats are not in the bundled fonts and render as tofu; the
/// multiplication sign is, so it stands in for the close glyph.
const REMOVE_ICON: &str = "\u{00D7}";
/// Width reserved in a cell for its hover icon so nothing shifts when the
/// pointer enters or leaves the row.
const ICON_SLOT_WIDTH: f32 = 26.0;
const CHEAT_TABLE_TOAST_DURATION: Duration = Duration::from_secs(4);

/// Text column heading: left-aligned with its data and bold.
fn header_label(ui: &mut egui::Ui, text: String) {
    ui.label(egui::RichText::new(text).text_style(egui::TextStyle::Button).strong().size(16.0));
}

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
    crate::ui::auto_save::show(app, ui);
    error_bar(app, ui);
    tab_bar(app, ui);
    egui::CentralPanel::default()
        .frame(egui::Frame::central_panel(ui.style()).inner_margin(egui::Margin::symmetric(20, 16)))
        .show(ui, |ui| {
            search_area(app, ui);
        });
    cheat_table_toast(app, ui.ctx());
    crate::ui::address_editor::show(app, ui.ctx());
    crate::ui::pointer_scanner::show(app, ui.ctx());
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
                let accent = ui.visuals().selection.bg_fill;
                ui.label(egui::RichText::new(&app.state.process_name).size(15.0).strong().color(accent));
                ui.label(egui::RichText::new("·").size(14.0).weak());
                ui.label(egui::RichText::new(format!("PID {}", app.state.pid)).size(13.0).weak().monospace());

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let secondary = |label: String| {
                        egui::Button::new(egui::RichText::new(label).size(14.0))
                            .frame(false)
                            .min_size(egui::vec2(0.0, 28.0))
                    };
                    let close = egui::Button::new(egui::RichText::new(fl!(crate::LANGUAGE_LOADER, "close-button")).size(14.0)).min_size(egui::vec2(76.0, 28.0));
                    if ui.add(close).clicked() {
                        app.back_to_main_menu();
                    }
                    if app.enable_persistence {
                        ui.add_space(6.0);
                        ui.separator();
                        ui.add_space(6.0);
                        if ui.add(secondary(fl!(crate::LANGUAGE_LOADER, "load-cheat-table-button"))).clicked() {
                            app.load_cheat_table();
                        }
                        if ui
                            .add_enabled(!app.is_saving_cheat_table(), secondary(fl!(crate::LANGUAGE_LOADER, "save-cheat-table-button")))
                            .on_hover_text(fl!(crate::LANGUAGE_LOADER, "address-save-help"))
                            .clicked()
                        {
                            app.save_cheat_table();
                        }
                    }
                });
            });
        });
}

fn cheat_table_toast(app: &mut App, ctx: &egui::Context) {
    let Some(shown_at) = app.cheat_table_status_at else {
        return;
    };
    let elapsed = shown_at.elapsed();
    let mut dismiss = false;
    let mut reading = false;
    egui::Area::new(egui::Id::new("cheat_table_status_toast"))
        .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-20.0, 104.0))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            ui.set_max_width((ctx.content_rect().width() - 40.0).clamp(180.0, 520.0));
            egui::Frame::popup(ui.style()).inner_margin(egui::Margin::symmetric(12, 8)).show(ui, |ui| {
                let response = crate::ui::notice::show(ui, "cheat_table_notice", &app.cheat_table_status, &app.cheat_table_status_details);
                dismiss = response.dismissed;
                reading = response.hovered || response.expanded;
            });
        });

    // Check after rendering, so hover/expansion protects even an overdue toast.
    if dismiss || (!reading && elapsed >= CHEAT_TABLE_TOAST_DURATION) {
        app.cheat_table_status.clear();
        app.cheat_table_status_details.clear();
        app.cheat_table_status_at = None;
    } else if reading {
        app.cheat_table_status_at = Some(std::time::Instant::now());
        ctx.request_repaint_after(CHEAT_TABLE_TOAST_DURATION);
    } else {
        ctx.request_repaint_after(CHEAT_TABLE_TOAST_DURATION.saturating_sub(elapsed));
    }
}

fn error_bar(app: &mut App, ui: &mut egui::Ui) {
    crate::ui::error_notice::show(app, ui);
}

/// Browser-style horizontal tab bar listing all active searches.
///
/// Layout:
/// * One row of tabs with rounded top corners.
/// * Active tab is filled with the accent colour; inactive tabs are muted.
/// * A small `×` close button is shown only while its tab is hovered.
/// * A trailing tab-height `+` icon button creates a new search.
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
            if app.renaming_search_index.is_none()
                && app.selected_result.is_none()
                && !ui.memory(|m| m.focused().is_some())
                && ui.input(|i| i.key_pressed(egui::Key::F2))
            {
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
                            let tab_hovered = ui.ctx().pointer_hover_pos().is_some_and(|pos| full_rect.contains(pos));

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
                            } else if tab_hovered {
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

                            if let Some(rect) = close_rect
                                && tab_hovered
                            {
                                let close_response = ui
                                    .interact(rect, egui::Id::new(("close_tab", i)), egui::Sense::click())
                                    .on_hover_text(fl!(crate::LANGUAGE_LOADER, "close-tab-hover-text"));
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

                        // Trailing "+" icon action: visually quiet, but the
                        // same height and hit target as a search tab.
                        let plus_size = egui::vec2(TAB_HEIGHT, TAB_HEIGHT);
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
                        plus_response.on_hover_text(fl!(crate::LANGUAGE_LOADER, "open-tab-hover-text"));
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
    let undo_shortcut = ui.memory(|memory| memory.focused().is_none() || memory.has_focus(result_table_focus_id()))
        && !egui::Popup::is_any_open(ui.ctx())
        && ui.input(|input| input.modifiers.command && !input.modifiers.shift && input.key_pressed(egui::Key::Z));
    if undo_shortcut
        && app
            .state
            .searches
            .get(app.state.current_search)
            .is_some_and(|search| matches!(search.searching, SearchMode::None) && !search.old_results.is_empty())
    {
        app.undo_search();
    }

    let search_index = app.state.current_search;
    let Some(search_context) = app.state.searches.get(search_index) else {
        return;
    };
    let mut selected_type = search_context.search_type;
    let mut value_text = search_context.search_value_text.clone();
    let search_results = search_context.get_result_count();
    let is_search_complete = search_context.search_complete.load(Ordering::SeqCst);
    let can_undo = !search_context.old_results.is_empty();
    let has_snapshot = search_context.memory_snapshot.read().is_ok_and(|pages| !pages.is_empty());
    // Loaded tables and failed-search rollbacks need recovery controls even
    // when no scan has completed in this context.
    let has_resettable_state = is_search_complete || search_results > 0 || !search_context.unresolved_addresses.is_empty() || can_undo || has_snapshot;
    let unknown_comparison = search_context.unknown_comparison;
    let searching = search_context.searching;
    let current_bytes = search_context.current_bytes.load(Ordering::Acquire);
    let total_bytes = search_context.total_bytes;
    let has_pointers = search_context.has_pointer_addresses();
    if search_results == 0 && app.selected_result.is_some() {
        app.clear_result_interaction();
    }

    let idle = matches!(searching, SearchMode::None);
    if has_pointers {
        ui.label(egui::RichText::new(fl!(crate::LANGUAGE_LOADER, "pointer-scan-warning")).small().weak());
    }
    let show_type_picker = idle && search_results == 0 && !has_snapshot;
    let mut enter_search = false;
    let mut parse_error = None;

    let accent = ui.visuals().selection.bg_fill;
    let surface = egui::Color32::from_rgb(22, 25, 30);
    let card_stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(55, 62, 72));
    let secondary_btn = |label: String| egui::Button::new(egui::RichText::new(label).size(14.0)).min_size(egui::vec2(0.0, 30.0));

    // Wrap whole input/action groups, never individual words in a count.
    // Keep secondary actions on a separate, predictable status row.
    let card_width = ui.available_width();
    let selected_text_color = ui.visuals().selection.stroke.color;
    let primary_btn = |label: String| {
        egui::Button::new(egui::RichText::new(label).size(14.0).strong().color(selected_text_color))
            .fill(accent)
            .stroke(egui::Stroke::new(1.0, accent))
            .min_size(egui::vec2(110.0, 30.0))
    };
    egui::Frame::new()
        .fill(surface)
        .stroke(card_stroke)
        .corner_radius(egui::CornerRadius::same(8))
        .inner_margin(egui::Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.set_width((card_width - 26.0).max(0.0));
            ui.horizontal_wrapped(|ui| {
                ui.add_enabled_ui(idle, |ui| {
                    ui.horizontal(|ui| {
                        if selected_type == SearchType::Unknown {
                            ui.label(fl!(crate::LANGUAGE_LOADER, "search-type-label"));
                        } else {
                            ui.label(fl!(crate::LANGUAGE_LOADER, "value-label"));
                            let response = ui.add(
                                egui::TextEdit::singleline(&mut value_text)
                                    .id_salt("search_value_input")
                                    .hint_text(fl!(
                                        crate::LANGUAGE_LOADER,
                                        "search-value-label",
                                        valuetype = selected_type.get_short_description_text()
                                    ))
                                    .desired_width(SEARCH_FIELD_WIDTH.min((card_width - 360.0).max(120.0)))
                                    .margin(egui::Margin::symmetric(10, 8)),
                            );
                            if app.search_value_request_focus {
                                response.request_focus();
                                app.search_value_request_focus = false;
                            }
                            if response.changed() {
                                app.state.searches[search_index].search_value_text = value_text.clone();
                            }
                            enter_search = (response.has_focus() || response.lost_focus()) && ui.input(|input| input.key_pressed(egui::Key::Enter));
                        }
                        type_picker(app, ui, show_type_picker, selected_type);
                    });
                });
                // Read the edited values in this frame, so validation and the
                // primary button never lag a keystroke/type selection behind.
                selected_type = app.state.searches[search_index].search_type;
                if !value_text.is_empty() && selected_type != SearchType::Unknown {
                    parse_error = selected_type.from_string(&value_text).err();
                }
                if idle && (selected_type != SearchType::Unknown || !is_search_complete) {
                    let label = if selected_type == SearchType::Unknown {
                        fl!(crate::LANGUAGE_LOADER, "capture-snapshot-button")
                    } else if search_results > 0 && selected_type != SearchType::String {
                        fl!(crate::LANGUAGE_LOADER, "update-button")
                    } else {
                        fl!(crate::LANGUAGE_LOADER, "initial-search-button")
                    };
                    let enabled = selected_type == SearchType::Unknown || (!value_text.is_empty() && parse_error.is_none());
                    if ui.add_enabled(enabled, primary_btn(label)).clicked() || (enabled && enter_search) {
                        app.start_search();
                    }
                }
            });

            if selected_type == SearchType::Unknown {
                ui.add_space(6.0);
                ui.label(egui::RichText::new(fl!(crate::LANGUAGE_LOADER, "unknown-search-description")).size(13.0).weak());
                if idle && is_search_complete {
                    ui.add_space(6.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.label(fl!(crate::LANGUAGE_LOADER, "compare-label"));
                        for (label, comparison) in [
                            (fl!(crate::LANGUAGE_LOADER, "decreased-button"), UnknownComparison::Decreased),
                            (fl!(crate::LANGUAGE_LOADER, "increased-button"), UnknownComparison::Increased),
                            (fl!(crate::LANGUAGE_LOADER, "changed-button"), UnknownComparison::Changed),
                            (fl!(crate::LANGUAGE_LOADER, "unchanged-button"), UnknownComparison::Unchanged),
                        ] {
                            if ui.add_enabled(search_results > 0 || has_snapshot, secondary_btn(label)).clicked() {
                                app.unknown_search(comparison);
                            }
                        }
                    });
                }
            }
            if let Some(err) = &parse_error {
                ui.add_space(4.0);
                ui.colored_label(egui::Color32::from_rgb(220, 120, 120), format!("\u{26A0}  {err}"));
            }
            let edit_error = app.editing_result.as_ref().and_then(|(index, text)| {
                let results = app.state.searches[search_index].collect_results();
                results.get(*index)?.search_type.from_string(text).err().map(|err| (*index, err))
            });
            if let Some((index, err)) = edit_error {
                ui.add_space(4.0);
                ui.colored_label(ui.visuals().error_fg_color, err);
                if ui.button(fl!(crate::LANGUAGE_LOADER, "check-result-type")).clicked() {
                    app.open_memory_editor(index);
                }
            }
            if idle && has_resettable_state {
                ui.add_space(8.0);
                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    if selected_type == SearchType::Unknown && unknown_comparison.is_none() && has_snapshot {
                        ui.add(egui::Label::new(fl!(crate::LANGUAGE_LOADER, "snapshot-ready-label")).wrap_mode(egui::TextWrapMode::Extend));
                    } else {
                        result_count_label(ui, search_results);
                    }
                    if ui.add_enabled(can_undo, secondary_btn(fl!(crate::LANGUAGE_LOADER, "undo-button"))).clicked() {
                        app.undo_search();
                    }
                    if ui
                        .add(secondary_btn(fl!(crate::LANGUAGE_LOADER, "clear-button")))
                        .on_hover_text(fl!(crate::LANGUAGE_LOADER, "reset-search-tooltip"))
                        .clicked()
                    {
                        app.clear_results();
                    }
                    if search_results > 0 && !matches!(selected_type, SearchType::String | SearchType::StringUtf16) {
                        let show = &mut app.state.searches[search_index].show_numeric_filter;
                        if ui
                            .selectable_label(*show, fl!(crate::LANGUAGE_LOADER, "numeric-filter-title"))
                            .on_hover_text(fl!(crate::LANGUAGE_LOADER, "result-filter-combined-hint"))
                            .clicked()
                        {
                            *show = !*show;
                        }
                    }
                });
            }
            if app.state.searches[search_index].show_numeric_filter
                && search_results > 0
                && !matches!(selected_type, SearchType::String | SearchType::StringUtf16)
            {
                ui.add_space(8.0);
                ui.separator();
                numeric_filter_controls(app, ui);
            }
        });

    if app.enable_persistence && app.pointer_scanner.candidates.is_some() && ui.button(fl!(crate::LANGUAGE_LOADER, "pointer-scan-title")).clicked() {
        app.pointer_scanner.open = true;
    }

    // Keep recovery actions above potentially tall unresolved-entry lists.
    if app.enable_persistence {
        crate::ui::address_editor::unresolved_panel(app, ui);
    }

    if !matches!(searching, SearchMode::None) {
        ui.add_space(10.0);
        // Searching in progress
        let progress = if total_bytes == 0 { 0.0 } else { current_bytes as f32 / total_bytes as f32 };
        let label = if searching == SearchMode::Stability {
            fl!(
                crate::LANGUAGE_LOADER,
                "result-filter-stability-progress",
                current = format!("{:.1}", current_bytes as f64 / 1000.0),
                total = format!("{:.0}", total_bytes as f64 / 1000.0)
            )
        } else if searching == SearchMode::Percent {
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
        ui.add(egui::ProgressBar::new(progress).desired_width(ui.available_width()).show_percentage());
        ui.label(egui::RichText::new(label).size(13.0).weak());
        if ui.add(secondary_btn(fl!(crate::LANGUAGE_LOADER, "result-filter-cancel"))).clicked() {
            app.cancel_search();
        }
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
    } else if matches!(searching, SearchMode::None) && (!is_search_complete || selected_type != SearchType::Unknown || unknown_comparison.is_some()) {
        ui.add_space(18.0);
        empty_results_panel(app, ui, is_search_complete);
    }
}

fn numeric_filter_controls(app: &mut App, ui: &mut egui::Ui) {
    let search = &mut app.state.searches[app.state.current_search];
    let mut apply = false;
    ui.add_enabled_ui(search.searching == SearchMode::None, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut search.numeric_filter_enabled, fl!(crate::LANGUAGE_LOADER, "result-filter-numeric"))
                .on_hover_text(fl!(crate::LANGUAGE_LOADER, "numeric-filter-hint"));
            if search.numeric_filter_enabled {
                egui::ComboBox::from_id_salt("numeric_filter_operator")
                    .width(85.0)
                    .selected_text(search.numeric_comparison.label())
                    .show_ui(ui, |ui| {
                        for comparison in crate::NumericComparison::ALL {
                            ui.selectable_value(&mut search.numeric_comparison, comparison, comparison.label());
                        }
                    });
                ui.add(
                    egui::TextEdit::singleline(&mut search.numeric_filter_lower)
                        .id_salt("numeric_filter_lower")
                        .hint_text(fl!(crate::LANGUAGE_LOADER, "numeric-filter-value"))
                        .desired_width(110.0),
                );
                if search.numeric_comparison == crate::NumericComparison::Between {
                    ui.horizontal(|ui| {
                        ui.label(fl!(crate::LANGUAGE_LOADER, "numeric-filter-and"));
                        ui.add(
                            egui::TextEdit::singleline(&mut search.numeric_filter_upper)
                                .id_salt("numeric_filter_upper")
                                .hint_text(fl!(crate::LANGUAGE_LOADER, "numeric-filter-upper"))
                                .desired_width(110.0),
                        );
                    });
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut search.type_filter_enabled, fl!(crate::LANGUAGE_LOADER, "result-filter-types"))
                .on_hover_text(fl!(crate::LANGUAGE_LOADER, "result-filter-types-hint"));
            if search.type_filter_enabled {
                for (index, ty) in SearchType::NUMERIC_TYPES.into_iter().enumerate() {
                    ui.toggle_value(&mut search.filter_types[index], compact_type_label(ty));
                }
                if ui.small_button(fl!(crate::LANGUAGE_LOADER, "result-filter-types-all")).clicked() {
                    search.filter_types.fill(true);
                }
                if ui.small_button(fl!(crate::LANGUAGE_LOADER, "result-filter-types-none")).clicked() {
                    search.filter_types.fill(false);
                }
            }
        });
        let mut validation = Ok(());
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut search.stable_filter_enabled, fl!(crate::LANGUAGE_LOADER, "result-filter-stable"))
                .on_hover_text(fl!(crate::LANGUAGE_LOADER, "result-filter-stable-hint"));
            if search.stable_filter_enabled {
                ui.add(egui::DragValue::new(&mut search.stable_filter_seconds).range(1..=30).speed(0.1));
                ui.label(fl!(crate::LANGUAGE_LOADER, "result-filter-seconds"));
            }
            validation = search.result_filter().map(|_| ());
            apply = ui
                .add_enabled(
                    validation.is_ok(),
                    egui::Button::new(fl!(crate::LANGUAGE_LOADER, "numeric-filter-apply")).min_size(egui::vec2(0.0, 30.0)),
                )
                .on_hover_text(fl!(crate::LANGUAGE_LOADER, "numeric-filter-description"))
                .clicked();
        });
        if let Err(err) = validation {
            ui.colored_label(ui.visuals().error_fg_color, err);
        }
    });
    if apply {
        app.apply_numeric_filter();
    }
}

fn empty_results_panel(app: &mut App, ui: &mut egui::Ui, search_complete: bool) {
    let (title, hint) = if search_complete {
        (
            fl!(crate::LANGUAGE_LOADER, "empty-results-title"),
            fl!(crate::LANGUAGE_LOADER, "empty-results-hint"),
        )
    } else {
        (
            fl!(crate::LANGUAGE_LOADER, "empty-search-title"),
            fl!(crate::LANGUAGE_LOADER, "empty-search-hint"),
        )
    };

    let available_size = ui.available_size();
    const CONTENT_HEIGHT: f32 = 58.0;
    const VERTICAL_POSITION: f32 = 0.32;
    ui.allocate_ui_with_layout(available_size, egui::Layout::top_down(egui::Align::Center), |ui| {
        ui.add_space(((available_size.y - CONTENT_HEIGHT) * VERTICAL_POSITION).max(20.0));
        ui.label(egui::RichText::new(title).size(20.0).strong());
        ui.add_space(7.0);
        ui.add(egui::Label::new(egui::RichText::new(hint).size(15.0).weak()).wrap());
        if search_complete && app.state.current_error().is_none() {
            ui.horizontal_wrapped(|ui| {
                let can_undo = !app.state.searches[app.state.current_search].old_results.is_empty();
                if can_undo && ui.button(fl!(crate::LANGUAGE_LOADER, "empty-results-undo")).clicked() {
                    app.undo_search();
                }
                if ui.button(fl!(crate::LANGUAGE_LOADER, "error-new-search")).clicked() {
                    app.new_search();
                }
            });
        }
    });
}

fn result_count_label(ui: &mut egui::Ui, count: usize) {
    let unit = if count == 1 {
        fl!(crate::LANGUAGE_LOADER, "result-unit-singular")
    } else {
        fl!(crate::LANGUAGE_LOADER, "result-unit-plural")
    };
    let mut text = egui::RichText::new(format!("{count} {unit}")).size(14.0).strong();
    if count > BROWSE_RESULT_LIMIT {
        text = text.color(ui.visuals().warn_fg_color);
    }
    egui::Frame::new()
        .fill(ui.visuals().selection.bg_fill.gamma_multiply(0.18))
        .corner_radius(egui::CornerRadius::same(6))
        .inner_margin(egui::Margin::symmetric(10, 5))
        .show(ui, |ui| {
            ui.add(egui::Label::new(text).wrap_mode(egui::TextWrapMode::Extend));
        });
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
    // A String scan discovers both UTF-8 and UTF-16 results. Only individual
    // result rows should label the concrete encoding.
    let label = if current == SearchType::String {
        current.get_short_description_text()
    } else {
        compact_type_label(current)
    };
    if editable {
        let mut selected = current;
        let response = egui::ComboBox::from_id_salt("search_type_picker").selected_text(label).show_ui(ui, |ui| {
            for st in [
                SearchType::Guess,
                SearchType::Unknown,
                SearchType::Byte,
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
        if current == SearchType::Guess {
            response.response.on_hover_text(fl!(crate::LANGUAGE_LOADER, "guess-type-hint"));
        }
        if selected != current
            && let Some(ctx) = app.state.searches.get_mut(app.state.current_search)
        {
            ctx.search_type = selected;
        }
    } else {
        let hint = if current == SearchType::Guess {
            fl!(crate::LANGUAGE_LOADER, "guess-type-hint")
        } else {
            current.get_description_text()
        };
        ui.label(label).on_hover_text(hint);
    }
}

fn compact_type_label(search_type: SearchType) -> String {
    match search_type {
        SearchType::Byte => "UInt8".to_owned(),
        SearchType::Short => "Int16".to_owned(),
        SearchType::Int => "Int32".to_owned(),
        SearchType::Int64 => "Int64".to_owned(),
        SearchType::Float => "Float32".to_owned(),
        SearchType::Double => "Float64".to_owned(),
        SearchType::String => "UTF-8".to_owned(),
        SearchType::StringUtf16 => "UTF-16".to_owned(),
        _ => search_type.get_short_description_text(),
    }
}

fn result_table_focus_id() -> egui::Id {
    egui::Id::new("result_table_focus")
}

// Result caches are sorted by (address, type). Resolve identity, not a stale
// row index, without scanning a potentially million-row result set per frame.
fn selected_result_index(results: &[SearchResult], selected: Option<SearchResult>) -> Option<usize> {
    let selected = selected?;
    results
        .binary_search_by_key(&(selected.addr, selected.search_type as u8), |r| (r.addr, r.search_type as u8))
        .ok()
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
    let string_utf16_byte_len = SearchType::StringUtf16.byte_length_for_text(&search_context.search_value_text).unwrap_or(0);
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
    let mut edit_address: Option<usize> = None;
    let mut scan_pointers: Option<usize> = None;
    let mut begin_edit: Option<(usize, String)> = None;
    let mut live_write: Option<(usize, String)> = None;
    let mut commit_edit: Option<(usize, String)> = None;
    let mut cancel_edit = false;
    let mut new_hovered_row: Option<usize> = None;
    // Cell hover is decided geometrically. A parent `Response::hovered()` goes
    // false when the pointer reaches an icon inside it, causing flicker.
    let pointer_pos = ui.ctx().pointer_hover_pos();
    let ctx = ui.ctx().clone();

    let mut selected_index = selected_result_index(&results, app.selected_result);
    if app.selected_result.is_some() && selected_index.is_none() {
        app.clear_result_interaction();
    }
    let keyboard_allowed = app.editing_result.is_none()
        && !egui::Popup::is_any_open(ui.ctx())
        && ui.memory(|memory| memory.focused().is_none() || memory.has_focus(result_table_focus_id()));
    if keyboard_allowed && !results.is_empty() {
        for key in [egui::Key::ArrowDown, egui::Key::ArrowUp, egui::Key::Home, egui::Key::End] {
            if ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, key)) {
                let index = match key {
                    egui::Key::ArrowDown => selected_index.map_or(0, |i| (i + 1).min(total_results - 1)),
                    egui::Key::ArrowUp => selected_index.map_or(total_results - 1, |i| i.saturating_sub(1)),
                    egui::Key::Home => 0,
                    _ => total_results - 1,
                };
                selected_index = Some(index);
                app.selected_result = Some(results[index]);
                app.result_selection_request_scroll = true;
                ui.memory_mut(|memory| memory.request_focus(result_table_focus_id()));
            }
        }
        if let Some(index) = selected_index {
            if ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Delete)) {
                remove_result = Some(index);
            }
            if ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::F2)) {
                app.result_edit_request_focus = true;
                app.result_selection_request_scroll = true;
            }
            if ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
                app.clear_result_interaction();
                selected_index = None;
            }
        }
    }

    ui.horizontal_wrapped(|ui| {
        if !is_string {
            let text = format!("{frozen_count} {}", fl!(crate::LANGUAGE_LOADER, "result-frozen-count"));
            ui.add(egui::Label::new(egui::RichText::new(text).size(13.0)).wrap_mode(egui::TextWrapMode::Extend));
            ui.separator();
        }
        let (edit_label, edit_hint) = if app.confirm_value_writes {
            (
                fl!(crate::LANGUAGE_LOADER, "result-confirm-edit-label"),
                fl!(crate::LANGUAGE_LOADER, "result-confirm-edit-tooltip"),
            )
        } else {
            (
                fl!(crate::LANGUAGE_LOADER, "result-live-edit-label"),
                fl!(crate::LANGUAGE_LOADER, "result-live-edit-tooltip"),
            )
        };
        ui.add(egui::Label::new(egui::RichText::new(edit_label).size(13.0)).wrap_mode(egui::TextWrapMode::Extend))
            .on_hover_text(edit_hint);
        ui.add(
            egui::Label::new(egui::RichText::new(fl!(crate::LANGUAGE_LOADER, "result-keyboard-hint")).size(12.0).weak()).wrap_mode(egui::TextWrapMode::Extend),
        );
    });
    ui.add_space(6.0);
    // A stable focus target shared by all rows, independent of virtualization.
    ui.interact(
        ui.available_rect_before_wrap(),
        result_table_focus_id(),
        egui::Sense::focusable_noninteractive(),
    );

    use egui_extras::{Column, TableBuilder};

    // Data columns stay compact. A final empty remainder column extends the
    // table's interaction area to the window edge, so wheel scrolling also
    // works over the otherwise-unused space on the right.
    let mut builder = TableBuilder::new(ui)
        .striped(true)
        .resizable(true)
        .sense(egui::Sense::click())
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(Column::initial(150.0).at_least(120.0)) // address + remove icon
        .column(Column::initial(190.0).at_least(140.0)); // value + edit icon
    if show_search_types {
        builder = builder.column(Column::initial(110.0).at_least(80.0));
    }
    if !is_string {
        builder = builder.column(Column::initial(56.0).at_least(48.0));
    }
    builder = builder.column(Column::remainder().resizable(false));
    if app.result_selection_request_scroll {
        if let Some(index) = selected_index {
            builder = builder.scroll_to_row(index, None);
        }
        app.result_selection_request_scroll = false;
    }

    builder
        .header(36.0, |mut header| {
            header.col(|ui| {
                header_label(ui, fl!(crate::LANGUAGE_LOADER, "address-heading"));
            });
            header.col(|ui| {
                header_label(ui, fl!(crate::LANGUAGE_LOADER, "value-heading"));
            });
            if show_search_types {
                header.col(|ui| {
                    header_label(ui, fl!(crate::LANGUAGE_LOADER, "datatype-heading"));
                });
            }
            if !is_string {
                header.col(|ui| {
                    let heading = fl!(crate::LANGUAGE_LOADER, "freezed-heading");
                    let hint = fl!(crate::LANGUAGE_LOADER, "freeze-all-tooltip");
                    // Built here rather than as a Fluent placeable: the loader
                    // wraps substitutions in isolate marks, which the bundled
                    // fonts draw as boxes.
                    let tooltip = if frozen_count > 0 {
                        format!("{heading} ({frozen_count}/{total_results})\n{hint}")
                    } else {
                        format!("{heading}\n{hint}")
                    };
                    let color = if all_frozen {
                        ui.visuals().selection.bg_fill
                    } else if frozen_count > 0 {
                        ui.visuals().selection.bg_fill.gamma_multiply(0.6)
                    } else {
                        ui.visuals().weak_text_color()
                    };
                    ui.with_layout(egui::Layout::centered_and_justified(egui::Direction::LeftToRight), |ui| {
                        if ui
                            .add(egui::Button::new(egui::RichText::new(FREEZE_ICON).size(18.0).strong().color(color)).frame(false))
                            .on_hover_text(tooltip)
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .clicked()
                        {
                            toggle_freeze_all = true;
                        }
                    });
                });
            }
            header.col(|_| {});
        })
        .body(|body| {
            body.rows(RESULT_ROW_HEIGHT, total_results, |mut row| {
                let i = row.index();
                let Some(result) = results.get(i).copied() else {
                    return;
                };
                let is_frozen = freezed.contains(&result.addr);
                let is_selected = selected_index == Some(i);
                row.set_selected(is_selected);

                // Always read fresh raw bytes from the target so the display
                // and the change diff reflect the process's current state.
                // Comparing raw bytes (not formatted strings) skips a String
                // allocation per row per frame, and dovetails with the bulk
                // tracker which works in raw bytes too.
                let pointer_valid = !app.state.searches[search_index]
                    .address_overrides
                    .get(&(result.addr, result.search_type))
                    .is_some_and(crate::AddressSpec::is_pointer)
                    || app
                        .state
                        .validate_result_range(
                            search_index,
                            &result,
                            result
                                .search_type
                                .byte_length_for_text(&app.state.searches[search_index].search_value_text)
                                .unwrap_or(1),
                        )
                        .is_ok();
                let raw_bytes: Vec<u8> = if !pointer_valid {
                    Vec::new()
                } else if let Some(byte_len) = result.search_type.fixed_byte_length() {
                    match pid.try_into_process_handle() {
                        Ok(handle) => copy_address(result.addr, byte_len, &crate::state::memory_reader::ExactProcessReader(&handle)).unwrap_or_default(),
                        Err(_) => Vec::new(),
                    }
                } else if matches!(result.search_type, SearchType::String | SearchType::StringUtf16) {
                    let utf16 = result.search_type == SearchType::StringUtf16;
                    let max_bytes = if utf16 { string_utf16_byte_len } else { string_byte_len };
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
                let readable = !raw_bytes.is_empty();
                let value_text = if !readable {
                    fl!(crate::LANGUAGE_LOADER, "result-unreadable")
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
                let address_text = format!("0x{:X}", result.addr);
                let address_spec = app.state.searches[search_index].address_overrides.get(&(result.addr, result.search_type));
                let address_label = address_spec.map(|spec| spec.label()).unwrap_or_else(|| address_text.clone());
                let (_, address_response) = row.col(|ui| {
                    let cell_hovered = pointer_pos.is_some_and(|pos| ui.max_rect().contains(pos));
                    if is_selected {
                        ui.visuals_mut().override_text_color = Some(ui.visuals().selection.stroke.color);
                    }
                    ui.add_sized(
                        [(ui.available_width() - ICON_SLOT_WIDTH).max(24.0), 24.0],
                        egui::Label::new(egui::RichText::new(&address_label).monospace()).selectable(false).truncate(),
                    )
                    .on_hover_text(format!("{address_label}\n{address_text}"));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if icon_button(ui, cell_hovered, REMOVE_ICON, fl!(crate::LANGUAGE_LOADER, "remove-button")).clicked() {
                            remove_result = Some(i);
                        }
                    });
                });
                if address_response.double_clicked() {
                    open_editor = Some(i);
                }

                // Borderless live value; only hover/focus exposes the editor.
                // When the user focuses or types into the cell we capture
                // the row into `app.editing_result`; otherwise the box
                // displays the live value freshly read this frame.
                row.col(|ui| {
                    let cell_hovered = pointer_pos.is_some_and(|pos| ui.max_rect().contains(pos));
                    if is_selected {
                        ui.visuals_mut().override_text_color = Some(ui.visuals().selection.stroke.color);
                    }
                    // Stable per-result id: keeps focus across the
                    // display/edit swap and stops one row's editor state
                    // from bleeding into another when results are re-sorted
                    // or scrolled.
                    let value_id = egui::Id::new(("result-value", result.addr, result.search_type));
                    let has_focus = ui.memory(|mem| mem.has_focus(value_id));
                    let request_focus = is_selected && app.result_edit_request_focus;
                    if request_focus {
                        app.result_edit_request_focus = false;
                        if readable {
                            app.editing_result = Some((i, value_text.clone()));
                        }
                    }
                    // Only the focused cell may render from the edit buffer.
                    // Every other row shows what was read from the process
                    // this frame, so a buffer left behind by a failed commit
                    // or a row that scrolled away can't freeze the display.
                    let is_edit_row = matches!(app.editing_result, Some((idx, _)) if idx == i);
                    let editing = is_edit_row && (has_focus || request_focus);
                    if is_edit_row && !has_focus && !request_focus {
                        cancel_edit = true;
                    }
                    let text_color = if is_selected {
                        Some(ui.visuals().selection.stroke.color)
                    } else if recently_changed {
                        Some(egui::Color32::from_rgb(255, 180, 130))
                    } else if is_frozen {
                        Some(ui.visuals().selection.bg_fill)
                    } else {
                        None
                    };
                    let cell_width = (ui.available_width() - ICON_SLOT_WIDTH).clamp(60.0, 150.0);

                    let response = if !readable {
                        if is_edit_row {
                            cancel_edit = true;
                        }
                        ui.add(
                            egui::Label::new(egui::RichText::new(&value_text).color(text_color.unwrap_or(ui.visuals().weak_text_color())))
                                .selectable(false)
                                .truncate(),
                        )
                        .on_hover_text(fl!(crate::LANGUAGE_LOADER, "result-unreadable-tooltip"))
                    } else if editing {
                        // Bind the TextEdit straight to the live editing
                        // buffer so keystrokes mutate it in place.
                        let buf = &mut app.editing_result.as_mut().unwrap().1;
                        let r = ui.add(
                            egui::TextEdit::singleline(buf)
                                .id(value_id)
                                .desired_width(cell_width)
                                .text_color_opt(text_color),
                        );
                        if request_focus {
                            r.request_focus();
                        }
                        if r.changed() && !app.confirm_value_writes {
                            // Live write: try to push every keystroke into
                            // the target process. Invalid intermediate input
                            // is silently ignored so half-typed numbers
                            // don't spam errors.
                            live_write = Some((i, buf.clone()));
                        }
                        let enter_pressed = ui.input(|input| input.key_pressed(egui::Key::Enter));
                        let escape_pressed = ui.input(|input| input.key_pressed(egui::Key::Escape));
                        if enter_pressed {
                            commit_edit = Some((i, buf.clone()));
                            r.surrender_focus();
                        } else if escape_pressed {
                            cancel_edit = true;
                            r.surrender_focus();
                        } else if r.lost_focus() {
                            cancel_edit = true;
                        }
                        r
                    } else {
                        // Display-mode TextEdit. We re-render the live
                        // value every frame; the user transitions to
                        // edit mode the moment they focus or type.
                        let mut buf = value_text.clone();
                        let mut editor = egui::TextEdit::singleline(&mut buf)
                            .id(value_id)
                            .desired_width(cell_width)
                            .text_color_opt(text_color);
                        if !cell_hovered && !has_focus {
                            editor = editor.frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(4, 2)));
                        }
                        let hint = if app.confirm_value_writes {
                            fl!(crate::LANGUAGE_LOADER, "result-confirm-edit-tooltip")
                        } else {
                            fl!(crate::LANGUAGE_LOADER, "result-edit-tooltip")
                        };
                        let r = ui.add(editor).on_hover_text(hint);
                        if r.gained_focus() || r.changed() {
                            app.selected_result = Some(result);
                            begin_edit = Some((i, buf));
                        }
                        r
                    };

                    if change_intensity > 0.0 {
                        // A slim marker outside the editor never covers text,
                        // selection or cursor, even at maximum intensity.
                        let alpha = (change_intensity * 255.0) as u8;
                        let marker = egui::Rect::from_min_max(
                            egui::pos2(response.rect.left() - 4.0, response.rect.top()),
                            egui::pos2(response.rect.left() - 2.0, response.rect.bottom()),
                        );
                        ui.painter()
                            .rect_filled(marker, 1.0, egui::Color32::from_rgba_unmultiplied(255, 150, 60, alpha));
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if icon_button(ui, cell_hovered, MEMORY_ICON, fl!(crate::LANGUAGE_LOADER, "open-memory-editor-menu")).clicked() {
                            open_editor = Some(i);
                        }
                    });
                });

                if show_search_types {
                    row.col(|ui| {
                        if is_selected {
                            ui.visuals_mut().override_text_color = Some(ui.visuals().selection.stroke.color);
                        }
                        let mut label = egui::RichText::new(compact_type_label(result.search_type));
                        if is_selected {
                            label = label.color(ui.visuals().selection.stroke.color);
                        }
                        if ui
                            .add(egui::Label::new(label).selectable(false).sense(egui::Sense::click()))
                            .on_hover_text(fl!(crate::LANGUAGE_LOADER, "check-result-type"))
                            .clicked()
                        {
                            open_editor = Some(i);
                        }
                    });
                }

                if !is_string {
                    row.col(|ui| {
                        let tooltip = if is_frozen {
                            fl!(crate::LANGUAGE_LOADER, "unfreeze-result-tooltip")
                        } else {
                            fl!(crate::LANGUAGE_LOADER, "freeze-result-tooltip")
                        };
                        let color = if is_selected {
                            ui.visuals().selection.stroke.color
                        } else if is_frozen {
                            ui.visuals().selection.bg_fill
                        } else {
                            ui.visuals().weak_text_color()
                        };
                        // Keep the control compact and consistently frameless;
                        // the icon color communicates the freeze state.
                        ui.spacing_mut().button_padding = egui::vec2(3.0, 3.0);
                        ui.spacing_mut().interact_size = egui::vec2(24.0, 24.0);
                        let rect = egui::Rect::from_center_size(ui.max_rect().center(), egui::vec2(24.0, 24.0));
                        let button = egui::Button::new(egui::RichText::new(FREEZE_ICON).size(14.0).color(color)).frame(false);
                        if ui
                            .put(rect, button)
                            .on_hover_text(tooltip)
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .clicked()
                        {
                            toggle_freeze = Some(i);
                        }
                    });
                }

                row.col(|_| {});

                let row_response = row.response();
                if row_response.clicked() || row_response.secondary_clicked() || address_response.double_clicked() {
                    app.selected_result = Some(result);
                    ctx.memory_mut(|memory| memory.request_focus(result_table_focus_id()));
                }
                row_response.context_menu(|ui| {
                    if app.enable_persistence {
                        if ui
                            .add_enabled(
                                result.search_type.fixed_byte_length().is_some(),
                                egui::Button::new(fl!(crate::LANGUAGE_LOADER, "pointer-scan-menu")),
                            )
                            .clicked()
                        {
                            scan_pointers = Some(i);
                            ui.close();
                        }
                        if ui.button(fl!(crate::LANGUAGE_LOADER, "address-edit")).clicked() {
                            edit_address = Some(i);
                            ui.close();
                        }
                    }
                    if ui.button(fl!(crate::LANGUAGE_LOADER, "copy-address-menu")).clicked() {
                        ui.ctx().copy_text(address_text.clone());
                        ui.close();
                    }
                    if ui
                        .add_enabled(readable, egui::Button::new(fl!(crate::LANGUAGE_LOADER, "copy-value-menu")))
                        .clicked()
                    {
                        ui.ctx().copy_text(value_text.clone());
                        ui.close();
                    }
                    ui.separator();
                    let freeze_label = if is_frozen {
                        fl!(crate::LANGUAGE_LOADER, "unfreeze-result-tooltip")
                    } else {
                        fl!(crate::LANGUAGE_LOADER, "freeze-result-tooltip")
                    };
                    if ui
                        .add_enabled(result.search_type.fixed_byte_length().is_some(), egui::Button::new(freeze_label))
                        .clicked()
                    {
                        toggle_freeze = Some(i);
                        ui.close();
                    }
                    if ui.button(fl!(crate::LANGUAGE_LOADER, "open-memory-editor-menu")).clicked() {
                        open_editor = Some(i);
                        ui.close();
                    }
                    ui.separator();
                    if ui.button(fl!(crate::LANGUAGE_LOADER, "remove-button")).clicked() {
                        remove_result = Some(i);
                        ui.close();
                    }
                });

                if let Some(pos) = pointer_pos
                    && row_response.rect.contains(pos)
                {
                    new_hovered_row = Some(i);
                }
            });
        });

    drop(results);

    app.hovered_result_row = new_hovered_row;
    if let Some(i) = scan_pointers {
        app.begin_pointer_scan(i);
    }
    if let Some(i) = edit_address {
        app.begin_address_edit(i);
    }

    if toggle_freeze_all {
        app.toggle_freeze_all();
    }
    if let Some(i) = toggle_freeze {
        app.toggle_freeze(i);
    }
    if let Some(i) = remove_result {
        app.remove_result(i);
        return;
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
        } else if app.confirm_value_writes {
            // Enter surrenders TextEdit focus. Keep failed input available for
            // correction rather than discarding it on the next frame.
            if let Some(result) = app.state.searches[search_index].collect_results().get(i) {
                ctx.memory_mut(|memory| memory.request_focus(egui::Id::new(("result-value", result.addr, result.search_type))));
            }
            cancel_edit = false;
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
        for chunk in bytes.as_chunks::<2>().0 {
            let u = u16::from_le_bytes(*chunk);
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
