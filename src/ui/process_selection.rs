use i18n_embed_fl::fl;

use crate::ui::app::{App, AppState};

mod model;
use model::{ProcessRow, build_rows, process_counts, resolve_process};
pub use model::{ProcessRowKey, ProcessSelectionState};

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
    needle.is_empty()
        || haystack
            .as_bytes()
            .windows(needle.len())
            .any(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
}

fn cmp_ascii_case_insensitive(a: &str, b: &str) -> std::cmp::Ordering {
    a.bytes().map(|b| b.to_ascii_lowercase()).cmp(b.bytes().map(|b| b.to_ascii_lowercase()))
}

/// Highlight ASCII-insensitive matches without changing UTF-8 boundaries.
fn highlight_job(ui: &egui::Ui, text: &str, filter: &str, color: egui::Color32, monospace: bool) -> egui::text::LayoutJob {
    let font_id = if monospace { egui::TextStyle::Monospace } else { egui::TextStyle::Body }.resolve(ui.style());
    let normal = egui::TextFormat {
        font_id,
        color,
        ..Default::default()
    };
    let marked = egui::TextFormat {
        color: ui.visuals().selection.stroke.color,
        background: ui.visuals().selection.bg_fill.gamma_multiply(0.55),
        ..normal.clone()
    };
    let mut job = egui::text::LayoutJob::default();
    let mut cursor = 0;
    if !filter.is_empty() {
        for (start, _) in text.char_indices() {
            let end = start + filter.len();
            if start >= cursor && text.is_char_boundary(end) && text.get(start..end).is_some_and(|s| s.eq_ignore_ascii_case(filter)) {
                job.append(&text[cursor..start], 0.0, normal.clone());
                job.append(&text[start..end], 0.0, marked.clone());
                cursor = end;
            }
        }
    }
    job.append(&text[cursor..], 0.0, normal);
    job
}

const ROW_HEIGHT: f32 = 34.0;

pub fn view_process_selection(app: &mut App, ui: &mut egui::Ui) {
    egui::CentralPanel::default()
        .frame(egui::Frame::central_panel(ui.style()).inner_margin(egui::Margin::symmetric(20, 16)))
        .show(ui, |ui| {
            let full = ui.available_rect_before_wrap();
            let footer_height = 64.0;
            let content_rect = egui::Rect::from_min_max(full.min, egui::pos2(full.max.x, full.max.y - footer_height - 12.0));
            let footer_rect = egui::Rect::from_min_max(egui::pos2(full.min.x, full.max.y - footer_height), full.max);
            let mut content = ui.new_child(egui::UiBuilder::new().max_rect(content_rect).layout(egui::Layout::top_down(egui::Align::Min)));
            let mut connect = process_content(app, &mut content);
            let mut footer = ui.new_child(egui::UiBuilder::new().max_rect(footer_rect).layout(egui::Layout::top_down(egui::Align::Min)));
            connect |= process_footer(app, &mut footer);
            if connect && app.app_state == AppState::ProcessSelection {
                app.connect_selected_process();
            }
        });
}

fn process_content(app: &mut App, ui: &mut egui::Ui) -> bool {
    ui.heading(egui::RichText::new(fl!(crate::LANGUAGE_LOADER, "process-selection-title")).size(23.0).strong());
    ui.label(egui::RichText::new(fl!(crate::LANGUAGE_LOADER, "process-selection-subtitle")).size(13.0).weak());
    ui.add_space(12.0);

    let mut filter_response = None;
    ui.horizontal(|ui| {
        let width = (ui.available_width() - 38.0 - ui.spacing().item_spacing.x).max(100.0);
        let response = ui.add(
            egui::TextEdit::singleline(&mut app.state.process_filter)
                .id(egui::Id::new("process-filter"))
                .hint_text(fl!(crate::LANGUAGE_LOADER, "filter-processes-hint"))
                .desired_width(width)
                .margin(egui::Margin::symmetric(10, 8)),
        );
        let focus_shortcut = ui.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::F));
        if app.state.set_focus || focus_shortcut {
            response.request_focus();
            app.state.set_focus = false;
        }
        if ui
            .add_enabled(!app.state.process_filter.is_empty(), egui::Button::new("×").min_size(egui::vec2(32.0, 32.0)))
            .on_hover_text(fl!(crate::LANGUAGE_LOADER, "reset-filter-button"))
            .clicked()
        {
            app.state.process_filter.clear();
            response.request_focus();
        }
        filter_response = Some(response);
    });

    // Read the edited value, not last frame's copy. Clear stale selection
    // before drawing the footer so a hidden result cannot be attached to.
    let filter = app.state.process_filter.trim().to_owned();
    app.process_selection.update_filter(&filter);
    let (total, matched, groups) = process_counts(&app.state.processes, &filter);
    ui.add_space(6.0);
    ui.horizontal_wrapped(|ui| {
        let count = if filter.is_empty() {
            fl!(crate::LANGUAGE_LOADER, "process-count-total", total = total)
        } else {
            fl!(crate::LANGUAGE_LOADER, "process-count-filtered", shown = matched, total = total)
        };
        ui.label(egui::RichText::new(count).size(12.0).weak());
        ui.label(egui::RichText::new("·").weak());
        ui.label(
            egui::RichText::new(fl!(crate::LANGUAGE_LOADER, "process-group-count", groups = groups))
                .size(12.0)
                .weak(),
        )
        .on_hover_text(fl!(crate::LANGUAGE_LOADER, "process-group-count-hint"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(egui::RichText::new(fl!(crate::LANGUAGE_LOADER, "process-refresh-status")).size(12.0).weak());
        });
    });
    ui.add_space(8.0);

    let rows = build_rows(
        &app.state.processes,
        &filter,
        app.process_sort_column,
        app.process_sort_direction,
        &app.process_selection,
    );
    app.process_selection.reconcile(&rows);
    let filter_focused = filter_response.as_ref().is_some_and(egui::Response::has_focus);
    let keyboard_allowed = !egui::Popup::is_any_open(ui.ctx())
        && (filter_focused || ui.memory(|memory| memory.focused().is_none() || memory.focused() == app.process_selection.row_focus));
    let mut connect = false;
    if keyboard_allowed {
        for (key, backwards) in [(egui::Key::ArrowDown, false), (egui::Key::ArrowUp, true)] {
            if ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, key)) {
                app.process_selection.move_selection(&rows, backwards);
            }
        }
        if !filter_focused {
            for (key, last) in [(egui::Key::Home, false), (egui::Key::End, true)] {
                if ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, key)) {
                    let row = if last { rows.last() } else { rows.first() };
                    app.process_selection.selected = row.map(|row| row.key.clone());
                    app.process_selection.scroll_to_selection = true;
                }
            }
        }
        if let Some(index) = rows.iter().position(|row| Some(&row.key) == app.process_selection.selected.as_ref()) {
            let row = &rows[index];
            if ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) {
                if row.is_group() {
                    app.process_selection.toggle_group(&row.process.executable, row.expanded, !filter.is_empty());
                } else {
                    connect = true;
                }
            }
            if !filter_focused && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight)) && row.is_group() {
                if row.expanded {
                    app.process_selection.move_selection(&rows, false);
                } else {
                    app.process_selection.toggle_group(&row.process.executable, false, !filter.is_empty());
                }
            }
            if !filter_focused && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft)) {
                if row.is_group() && row.expanded {
                    app.process_selection.toggle_group(&row.process.executable, true, !filter.is_empty());
                } else if let Some(parent) = &row.parent {
                    app.process_selection.selected = Some(ProcessRowKey::Group(parent.clone()));
                    app.process_selection.scroll_to_selection = true;
                }
            }
        }
    }

    if rows.is_empty() {
        ui.add_space(24.0);
        ui.vertical_centered(|ui| {
            ui.heading(fl!(crate::LANGUAGE_LOADER, "no-processes-match"));
            ui.add_space(6.0);
            ui.label(if total == 0 {
                fl!(crate::LANGUAGE_LOADER, "no-processes-available-hint")
            } else {
                fl!(crate::LANGUAGE_LOADER, "no-processes-match-hint")
            });
            if !filter.is_empty() && ui.button(fl!(crate::LANGUAGE_LOADER, "reset-filter-button")).clicked() {
                app.state.process_filter.clear();
                app.state.set_focus = true;
            }
        });
    } else {
        connect |= process_table(
            ui,
            &rows,
            &filter,
            &mut app.process_selection,
            &mut app.process_sort_column,
            &mut app.process_sort_direction,
        );
    }
    connect
}

fn process_table(
    ui: &mut egui::Ui,
    rows: &[ProcessRow<'_>],
    filter: &str,
    selection: &mut ProcessSelectionState,
    sort_column: &mut ProcessSortColumn,
    sort_direction: &mut SortDirection,
) -> bool {
    use egui_extras::{Column, TableBuilder};
    let mut connect = false;
    let mut toggle = None;
    let scroll_to = if std::mem::take(&mut selection.scroll_to_selection) {
        rows.iter().position(|row| Some(&row.key) == selection.selected.as_ref())
    } else {
        None
    };
    // Only the body scrolls vertically: the table header stays visible.
    // At narrow window widths the command column is reached horizontally.
    egui::ScrollArea::horizontal()
        .id_salt("process-horizontal")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let available_width = ui.available_width();
            let mut table = TableBuilder::new(ui)
                .id_salt("process-list-v2")
                .striped(true)
                .resizable(true)
                .sense(egui::Sense::click())
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                .min_scrolled_height(80.0)
                .animate_scrolling(false)
                .column(Column::initial(240.0).at_least(180.0).clip(true))
                .column(Column::initial(85.0).at_least(70.0))
                .column(Column::initial(120.0).at_least(100.0))
                .column(Column::remainder().at_least((available_width - 480.0).max(180.0)));
            if let Some(index) = scroll_to {
                table = table.scroll_to_row(index, None);
            }
            table
                .header(32.0, |mut header| {
                    for (label, column) in [
                        (fl!(crate::LANGUAGE_LOADER, "name-heading"), ProcessSortColumn::Name),
                        (fl!(crate::LANGUAGE_LOADER, "pid-heading"), ProcessSortColumn::Pid),
                        (fl!(crate::LANGUAGE_LOADER, "memory-heading"), ProcessSortColumn::Memory),
                        (fl!(crate::LANGUAGE_LOADER, "command-heading"), ProcessSortColumn::Command),
                    ] {
                        header.col(|ui| {
                            let arrow = if *sort_column == column {
                                if *sort_direction == SortDirection::Ascending { " ⏶" } else { " ⏷" }
                            } else {
                                ""
                            };
                            if ui
                                .add(egui::Button::new(egui::RichText::new(format!("{label}{arrow}")).strong()).frame(false))
                                .on_hover_text(fl!(crate::LANGUAGE_LOADER, "process-sort-hint"))
                                .clicked()
                            {
                                *sort_direction = if *sort_column == column {
                                    if *sort_direction == SortDirection::Ascending {
                                        SortDirection::Descending
                                    } else {
                                        SortDirection::Ascending
                                    }
                                } else if column == ProcessSortColumn::Memory {
                                    SortDirection::Descending
                                } else {
                                    SortDirection::Ascending
                                };
                                *sort_column = column;
                                selection.scroll_to_selection = true;
                            }
                        });
                    }
                })
                .body(|body| {
                    body.rows(ROW_HEIGHT, rows.len(), |mut row| {
                        let item = &rows[row.index()];
                        let process = item.process;
                        let selected = selection.selected.as_ref() == Some(&item.key);
                        row.set_selected(selected);
                        let text_color = |ui: &egui::Ui, normal| {
                            if selected { ui.visuals().selection.stroke.color } else { normal }
                        };
                        let mut disclosure_clicked = false;
                        row.col(|ui| {
                            if item.parent.is_some() {
                                ui.add_space(22.0);
                            }
                            if item.is_group() {
                                let response = ui
                                    .add(
                                        egui::Button::new(
                                            egui::RichText::new(if item.expanded { "⏷" } else { "⏵" }).color(text_color(ui, ui.visuals().text_color())),
                                        )
                                        .frame(false)
                                        .min_size(egui::vec2(18.0, 24.0)),
                                    )
                                    .on_hover_text(fl!(crate::LANGUAGE_LOADER, "process-expand-hint"));
                                if response.clicked() {
                                    disclosure_clicked = true;
                                    toggle = Some((process.executable.clone(), item.expanded));
                                    selection.selected = Some(item.key.clone());
                                }
                            }
                            let badge = if item.is_group() {
                                Some(if filter.is_empty() {
                                    fl!(crate::LANGUAGE_LOADER, "process-group-badge", count = item.group_size)
                                } else {
                                    fl!(
                                        crate::LANGUAGE_LOADER,
                                        "process-group-filtered-badge",
                                        matched = item.matching_children,
                                        total = item.group_size
                                    )
                                })
                            } else {
                                None
                            };
                            let badge_width = badge.as_ref().map_or(0.0, |text| {
                                ui.painter()
                                    .layout_no_wrap(text.clone(), egui::FontId::proportional(11.0), text_color(ui, ui.visuals().weak_text_color()))
                                    .size()
                                    .x
                                    + 12.0
                            });
                            let name_width = (ui.available_width() - badge_width).max(32.0);
                            let name = highlight_job(ui, &process.name, filter, text_color(ui, ui.visuals().strong_text_color()), false);
                            ui.add_sized([name_width, 24.0], egui::Label::new(name).selectable(false).truncate())
                                .on_hover_text(format!("{}\n{}", process.name, process.executable));
                            if let Some(badge) = badge {
                                ui.label(egui::RichText::new(badge).size(11.0).color(text_color(ui, ui.visuals().weak_text_color())));
                            }
                        });
                        row.col(|ui| {
                            if item.is_group() {
                                ui.label(egui::RichText::new("—").color(text_color(ui, ui.visuals().weak_text_color())))
                                    .on_hover_text(fl!(crate::LANGUAGE_LOADER, "process-group-select-hint"));
                            } else {
                                let job = highlight_job(ui, &process.pid.to_string(), filter, text_color(ui, ui.visuals().text_color()), true);
                                ui.add(egui::Label::new(job).selectable(false));
                            }
                        });
                        row.col(|ui| {
                            let memory = gabi::BytesConfig::default().bytes(process.memory as u64).to_string();
                            let memory = if item.is_group() { format!("Σ {memory}") } else { memory };
                            let response = ui.add(
                                egui::Label::new(egui::RichText::new(memory).monospace().color(text_color(ui, ui.visuals().text_color())))
                                    .selectable(false)
                                    .truncate(),
                            );
                            if item.is_group() {
                                response.on_hover_text(fl!(crate::LANGUAGE_LOADER, "process-group-memory-hint"));
                            }
                        });
                        row.col(|ui| {
                            let command = if item.is_group() { &process.executable } else { &process.cmd };
                            let job = highlight_job(ui, command, filter, text_color(ui, ui.visuals().weak_text_color()), false);
                            ui.add(egui::Label::new(job).selectable(false).truncate()).on_hover_text(command);
                        });
                        let response = row.response().on_hover_cursor(egui::CursorIcon::PointingHand);
                        response.context_menu(|ui| {
                            if !item.is_group() && ui.button(fl!(crate::LANGUAGE_LOADER, "process-copy-pid")).clicked() {
                                ui.ctx().copy_text(process.pid.to_string());
                                ui.close();
                            }
                            if ui.button(fl!(crate::LANGUAGE_LOADER, "process-copy-command")).clicked() {
                                ui.ctx()
                                    .copy_text(if item.is_group() { process.executable.clone() } else { process.cmd.clone() });
                                ui.close();
                            }
                        });
                        if !disclosure_clicked && response.clicked() {
                            selection.selected = Some(item.key.clone());
                            selection.message = None;
                            selection.row_focus = Some(response.id);
                            response.request_focus();
                        }
                        if !disclosure_clicked && response.double_clicked() {
                            selection.selected = Some(item.key.clone());
                            if item.is_group() {
                                toggle = Some((process.executable.clone(), item.expanded));
                            } else {
                                connect = true;
                            }
                        }
                    });
                });
        });
    if let Some((key, expanded)) = toggle {
        selection.toggle_group(&key, expanded, !filter.is_empty());
    }
    connect
}

fn process_footer(app: &mut App, ui: &mut egui::Ui) -> bool {
    ui.separator();
    ui.add_space(6.0);
    let selected = app
        .process_selection
        .selected
        .as_ref()
        .and_then(|key| resolve_process(&app.state.processes, key));
    let enabled = selected.is_some();
    let title = if let Some(process) = selected {
        format!("{} · PID {}", process.name, process.pid)
    } else if matches!(app.process_selection.selected, Some(ProcessRowKey::Group(_))) {
        fl!(crate::LANGUAGE_LOADER, "process-group-select-hint")
    } else {
        fl!(crate::LANGUAGE_LOADER, "process-select-hint")
    };
    let mut connect = false;
    ui.horizontal(|ui| {
        let label_width = (ui.available_width() - 258.0).max(80.0);
        ui.add_sized([label_width, 32.0], egui::Label::new(egui::RichText::new(title.clone()).size(13.0)).truncate())
            .on_hover_text(title);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            connect = ui
                .add_enabled(
                    enabled,
                    egui::Button::new(egui::RichText::new(fl!(crate::LANGUAGE_LOADER, "process-connect-button")).color(egui::Color32::WHITE))
                        .fill(ui.visuals().selection.bg_fill)
                        .min_size(egui::vec2(120.0, 32.0)),
                )
                .clicked();
            if ui
                .add(egui::Button::new(fl!(crate::LANGUAGE_LOADER, "process-cancel-button")).min_size(egui::vec2(110.0, 32.0)))
                .clicked()
            {
                app.app_state = AppState::MainWindow;
            }
        });
    });
    if let Some(message) = &app.process_selection.message {
        ui.label(egui::RichText::new(message).size(11.0).color(ui.visuals().warn_fg_color));
    } else {
        ui.label(egui::RichText::new(fl!(crate::LANGUAGE_LOADER, "process-keyboard-hint")).size(11.0).weak());
    }
    connect
}

impl App {
    /// Re-resolve the chosen identity after refreshing. A vanished/recycled
    /// PID must never silently turn into a different attachment.
    pub fn connect_selected_process(&mut self) {
        let Some(key @ ProcessRowKey::Process { .. }) = self.process_selection.selected.clone() else {
            return;
        };
        self.state.update_process_data();
        if let Some(process) = resolve_process(&self.state.processes, &key).cloned() {
            self.select_process(&process);
        } else {
            self.process_selection.selected = None;
            self.process_selection.message = Some(fl!(crate::LANGUAGE_LOADER, "process-selection-gone"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use model::tests::process;

    #[test]
    fn process_arrow_glyphs_exist_in_the_application_font() {
        let ctx = egui::Context::default();
        crate::ui::theme::apply(&ctx);
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            for style in [egui::TextStyle::Body, egui::TextStyle::Button, egui::TextStyle::Small] {
                let font = style.resolve(ui.style());
                for arrow in ['⏶', '⏷', '⏵', '⏴'] {
                    assert!(ui.fonts_mut(|fonts| fonts.has_glyph(&font, arrow)), "Missing arrow {arrow} in {font:?}");
                }
            }
        });
        output.textures_delta.clear();
    }

    #[test]
    fn selected_process_rows_use_selection_color_for_every_cell() {
        for grouped in [false, true] {
            let mut app = app();
            let first = app.state.processes[0].clone();
            let expected = if grouped {
                app.state.processes = vec![crate::ProcessInfo {
                    instances: vec![first.clone(), crate::ProcessInfo { pid: 303, ..first.clone() }],
                    ..first.clone()
                }];
                app.process_selection.selected = Some(ProcessRowKey::Group(first.executable.clone()));
                vec![
                    first.name.clone(),
                    "⏵".to_owned(),
                    fl!(crate::LANGUAGE_LOADER, "process-group-badge", count = 2),
                    "—".to_owned(),
                    format!("Σ {}", gabi::BytesConfig::default().bytes(first.memory as u64)),
                    first.executable.clone(),
                ]
            } else {
                app.process_selection.selected = Some(ProcessRowKey::process(&first));
                vec![
                    first.name,
                    first.pid.to_string(),
                    gabi::BytesConfig::default().bytes(first.memory as u64).to_string(),
                    first.cmd,
                ]
            };
            let ctx = egui::Context::default();
            crate::ui::theme::apply(&ctx);
            let selected_color = egui::Color32::from_rgb(255, 240, 180);
            ctx.global_style_mut(|style| style.visuals.selection.stroke.color = selected_color);
            let size = egui::vec2(1100.0, 720.0);
            frame(&mut app, &ctx, size, vec![]);
            let output = frame(&mut app, &ctx, size, vec![]);
            // The toolbar's total count can have exactly the same text as
            // the group badge. Check only labels in the selected row.
            let row_center_y = text_shapes(&output)
                .iter()
                .find(|(text, _)| text == &expected[0])
                .expect("selected process name must be rendered")
                .1
                .center()
                .y;
            for label in expected {
                let text = output
                    .shapes
                    .iter()
                    .find_map(|shape| match &shape.shape {
                        egui::epaint::Shape::Text(text)
                            if text.galley.job.text == label
                                && (text.galley.rect.translate(text.pos.to_vec2()).center().y - row_center_y).abs() < ROW_HEIGHT / 2.0 =>
                        {
                            Some(text)
                        }
                        _ => None,
                    })
                    .unwrap_or_else(|| panic!("Missing row label: {label}"));
                assert!(
                    text.galley.job.sections.iter().all(|section| {
                        let color = text.override_text_color.unwrap_or_else(|| {
                            if section.format.color == egui::Color32::PLACEHOLDER {
                                text.fallback_color
                            } else {
                                section.format.color
                            }
                        });
                        color == selected_color
                    }),
                    "Wrong selection color: {label}; expected={selected_color:?}, sections={:?}, override={:?}, fallback={:?}",
                    text.galley.job.sections,
                    text.override_text_color,
                    text.fallback_color
                );
            }
        }
    }

    fn frame(app: &mut App, ctx: &egui::Context, size: egui::Vec2, events: Vec<egui::Event>) -> egui::FullOutput {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                events,
                ..Default::default()
            },
            |ui| view_process_selection(app, ui),
        );
        // Headless tests inspect paint shapes, not GPU texture updates.
        output.textures_delta.clear();
        output
    }

    fn text_shapes(output: &egui::FullOutput) -> Vec<(String, egui::Rect)> {
        fn collect(shape: &egui::epaint::Shape, out: &mut Vec<(String, egui::Rect)>) {
            match shape {
                egui::epaint::Shape::Text(text) => out.push((text.galley.job.text.clone(), text.galley.rect.translate(text.pos.to_vec2()))),
                egui::epaint::Shape::Vec(shapes) => {
                    for shape in shapes {
                        collect(shape, out);
                    }
                }
                _ => {}
            }
        }
        let mut result = Vec::new();
        for shape in &output.shapes {
            collect(&shape.shape, &mut result);
        }
        result
    }

    fn app() -> App {
        let mut app = App::default();
        app.app_state = AppState::ProcessSelection;
        app.state.processes = vec![process(101, "TestGame", 1024), process(202, "OtherGame", 512)];
        app
    }

    #[test]
    fn process_selection_layout_keeps_actions_visible_at_supported_sizes() {
        for size in [egui::vec2(640.0, 420.0), egui::vec2(1100.0, 720.0)] {
            let mut app = app();
            let ctx = egui::Context::default();
            crate::ui::theme::apply(&ctx);
            frame(&mut app, &ctx, size, vec![]);
            let output = frame(&mut app, &ctx, size, vec![]);
            let texts = text_shapes(&output);
            let connect = fl!(crate::LANGUAGE_LOADER, "process-connect-button");
            let cancel = fl!(crate::LANGUAGE_LOADER, "process-cancel-button");
            for label in [connect, cancel] {
                let (_, rect) = texts.iter().find(|(text, _)| text == &label).expect("action must be rendered");
                assert!(rect.left() >= 0.0 && rect.right() <= size.x, "{label}: {rect:?}");
                assert!(rect.top() >= 0.0 && rect.bottom() <= size.y, "{label}: {rect:?}");
            }
            assert!(texts.iter().any(|(text, _)| text == "TestGame"));
        }
    }

    #[test]
    fn single_click_selects_without_attaching() {
        let mut app = app();
        let ctx = egui::Context::default();
        let size = egui::vec2(1100.0, 720.0);
        frame(&mut app, &ctx, size, vec![]);
        let output = frame(&mut app, &ctx, size, vec![]);
        let texts = text_shapes(&output);
        let pos = texts.iter().find(|(text, _)| text == "TestGame").unwrap().1.center();
        for pressed in [true, false] {
            frame(
                &mut app,
                &ctx,
                size,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
        assert_eq!(app.app_state, AppState::ProcessSelection);
        assert_eq!(app.state.pid, 0);
        assert_eq!(app.process_selection.selected, Some(ProcessRowKey::process(&app.state.processes[0])));
    }

    #[test]
    fn arrow_keys_select_while_filter_has_focus() {
        let mut app = app();
        let ctx = egui::Context::default();
        let size = egui::vec2(1100.0, 720.0);
        frame(&mut app, &ctx, size, vec![]);
        frame(&mut app, &ctx, size, vec![]);
        assert!(ctx.memory(|memory| memory.has_focus(egui::Id::new("process-filter"))));
        frame(
            &mut app,
            &ctx,
            size,
            vec![egui::Event::Key {
                key: egui::Key::ArrowDown,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert_eq!(app.process_selection.selected, Some(ProcessRowKey::process(&app.state.processes[0])));
        assert_eq!(app.app_state, AppState::ProcessSelection);
    }

    #[test]
    fn entering_group_does_not_attach_to_representative() {
        let mut app = app();
        let first = app.state.processes[0].clone();
        let second = crate::ProcessInfo { pid: 303, ..first.clone() };
        app.state.processes = vec![crate::ProcessInfo {
            instances: vec![first.clone(), second],
            ..first
        }];
        let key = ProcessRowKey::Group(app.state.processes[0].executable.clone());
        app.process_selection.selected = Some(key);
        app.connect_selected_process();
        assert_eq!(app.app_state, AppState::ProcessSelection);
        assert_eq!(app.state.pid, 0);
    }

    #[test]
    fn filter_hides_selection_immediately() {
        let mut app = app();
        let ctx = egui::Context::default();
        let size = egui::vec2(1100.0, 720.0);
        frame(&mut app, &ctx, size, vec![]);
        app.process_selection.selected = Some(ProcessRowKey::process(&app.state.processes[0]));
        frame(&mut app, &ctx, size, vec![egui::Event::Text("OtherGame".to_owned())]);
        assert_eq!(app.process_selection.selected, None);
        assert_eq!(app.app_state, AppState::ProcessSelection);
    }

    #[test]
    fn confirming_a_vanished_process_does_not_attach() {
        let mut app = app();
        app.process_selection.selected = Some(ProcessRowKey::Process {
            pid: i32::MAX as _,
            start_time: 100,
        });
        app.connect_selected_process();
        assert_eq!(app.process_selection.selected, None);
        assert!(app.process_selection.message.is_some());
        assert_eq!(app.app_state, AppState::ProcessSelection);
        assert_eq!(app.state.pid, 0);
    }

    #[test]
    fn reopening_selection_requests_filter_focus() {
        let mut app = app();
        app.state.set_focus = false;
        app.process_selection.selected = Some(ProcessRowKey::process(&app.state.processes[0]));
        app.attach_action();
        assert!(app.state.set_focus);
        assert_eq!(app.process_selection.selected, None);
    }

    #[test]
    fn enter_from_filter_confirms_selected_process() {
        let mut app = app();
        app.state.processes[0].pid = i32::MAX as _;
        let ctx = egui::Context::default();
        let size = egui::vec2(1100.0, 720.0);
        frame(&mut app, &ctx, size, vec![]);
        app.process_selection.selected = Some(ProcessRowKey::process(&app.state.processes[0]));
        frame(
            &mut app,
            &ctx,
            size,
            vec![egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        // This deliberately nonexistent PID proves that Enter reached the
        // confirmation/revalidation path without attaching to a real process.
        assert!(app.process_selection.message.is_some());
        assert_eq!(app.process_selection.selected, None);
        assert_eq!(app.app_state, AppState::ProcessSelection);
    }
}
