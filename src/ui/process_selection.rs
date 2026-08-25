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

/// Find all (non-overlapping) byte-ranges where `needle` matches inside
/// `haystack`, using ASCII case-insensitive comparison.
fn find_all_ascii_ci(haystack: &str, needle: &str) -> Vec<(usize, usize)> {
    let mut hits = Vec::new();
    if needle.is_empty() {
        return hits;
    }
    let h = haystack.as_bytes();
    let n = needle.as_bytes();
    if n.len() > h.len() {
        return hits;
    }
    let mut i = 0usize;
    while i + n.len() <= h.len() {
        let mut matches = true;
        for j in 0..n.len() {
            if !h[i + j].eq_ignore_ascii_case(&n[j]) {
                matches = false;
                break;
            }
        }
        if matches {
            hits.push((i, i + n.len()));
            i += n.len();
        } else {
            i += 1;
        }
    }
    hits
}

/// Choose between the body and monospace font.
#[derive(Copy, Clone)]
enum RowFont {
    Body,
    Monospace,
}

/// One rendered line of the process table: either a (possibly aggregated)
/// top-level entry or one member of an expanded group.
struct ProcessRow {
    process: ProcessInfo,
    /// Number of processes represented by this row; `0` for child rows.
    group_size: usize,
    expanded: bool,
    is_child: bool,
}

fn process_matches_filter(process: &ProcessInfo, filter: &str) -> bool {
    if filter.is_empty() {
        return true;
    }
    contains_ignore_ascii_case(&process.name, filter)
        || contains_ignore_ascii_case(&process.cmd, filter)
        || process.pid.to_string().contains(filter)
        || process.instances.iter().any(|child| {
            contains_ignore_ascii_case(&child.name, filter) || contains_ignore_ascii_case(&child.cmd, filter) || child.pid.to_string().contains(filter)
        })
}

/// Build a `LayoutJob` for a table cell. The substring(s) matching the
/// active filter are rendered with a highlighted background, which
/// makes it obvious why a given row matched.
fn highlight_job(ui: &egui::Ui, text: &str, filter: &str, base_color: egui::Color32, font: RowFont) -> egui::text::LayoutJob {
    let font_id = match font {
        RowFont::Body => egui::TextStyle::Body.resolve(ui.style()),
        RowFont::Monospace => egui::TextStyle::Monospace.resolve(ui.style()),
    };
    // Translucent accent fill behind the matched substring + a bright
    // foreground so the match jumps out without being garish.
    let highlight_bg = ui.visuals().selection.bg_fill.gamma_multiply(0.55);
    let highlight_fg = egui::Color32::WHITE;

    let mut job = egui::text::LayoutJob::default();
    let mut push = |s: &str, highlighted: bool| {
        let mut fmt = egui::TextFormat {
            font_id: font_id.clone(),
            color: if highlighted { highlight_fg } else { base_color },
            ..Default::default()
        };
        if highlighted {
            fmt.background = highlight_bg;
        }
        job.append(s, 0.0, fmt);
    };

    let hits = find_all_ascii_ci(text, filter);
    if hits.is_empty() {
        push(text, false);
    } else {
        let mut cursor = 0;
        for (start, end) in hits {
            if start > cursor {
                push(&text[cursor..start], false);
            }
            push(&text[start..end], true);
            cursor = end;
        }
        if cursor < text.len() {
            push(&text[cursor..], false);
        }
    }
    job
}

pub fn view_process_selection(app: &mut App, ui: &mut egui::Ui) {
    // Give the view a bit of horizontal padding to the window edges
    // instead of a hard border around everything. The whole view lives
    // in a single panel so the bottom action bar shares the same fill
    // and the window's rounded corners stay intact.
    egui::CentralPanel::default()
        .frame(egui::Frame::central_panel(ui.style()).inner_margin(egui::Margin::symmetric(18, 12)))
        .show(ui, |ui| {
            const ACTION_BAR_HEIGHT: f32 = 42.0;
            // Gap between the table/content area and the action bar so
            // the close button doesn't sit flush against the table.
            const ACTION_BAR_GAP: f32 = 12.0;

            // Reserve the bottom strip for the action bar so the close
            // button always sits at the same vertical position
            // regardless of whether the table, the loading state or
            // the empty state is displayed.
            let full_rect = ui.available_rect_before_wrap();
            let content_rect = egui::Rect::from_min_max(full_rect.min, egui::pos2(full_rect.max.x, full_rect.max.y - ACTION_BAR_HEIGHT - ACTION_BAR_GAP));
            let action_rect = egui::Rect::from_min_max(egui::pos2(full_rect.min.x, full_rect.max.y - ACTION_BAR_HEIGHT), full_rect.max);

            let mut content_ui = ui.new_child(egui::UiBuilder::new().max_rect(content_rect).layout(egui::Layout::top_down(egui::Align::Min)));
            view_process_selection_inner(app, &mut content_ui);

            let mut action_ui = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(action_rect)
                    .layout(egui::Layout::right_to_left(egui::Align::Center)),
            );
            if action_ui
                .add(egui::Button::new(egui::RichText::new(fl!(crate::LANGUAGE_LOADER, "close-button")).size(15.0)).min_size(egui::vec2(120.0, 32.0)))
                .clicked()
            {
                app.app_state = AppState::MainWindow;
            }
        });
}

fn view_process_selection_inner(app: &mut App, ui: &mut egui::Ui) {
    // Pre-compute the filtered list so we can show the process count
    // next to the filter input at the top of the dialog.
    let filter = app.state.process_filter.clone();
    let total = app.state.processes.len();

    // Search bar + clear button + live process count.
    // Compute the count label up-front so we can reserve exactly the
    // space it needs at the right side of the row — otherwise the
    // count can overflow into the clear button at narrow widths.
    let shown_after_filter = if filter.is_empty() {
        total
    } else {
        app.state.processes.iter().filter(|process| process_matches_filter(process, &filter)).count()
    };
    let count_text = if filter.is_empty() {
        fl!(crate::LANGUAGE_LOADER, "process-count-total", total = total)
    } else {
        fl!(crate::LANGUAGE_LOADER, "process-count-filtered", shown = shown_after_filter, total = total)
    };

    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("\u{1F50D}").size(15.0));

        // Measure the count label's actual width so it never gets
        // clipped or overlapped by the clear button next to it.
        let count_galley = ui
            .painter()
            .layout_no_wrap(count_text.clone(), egui::TextStyle::Body.resolve(ui.style()), ui.visuals().weak_text_color());
        let count_label_width = count_galley.size().x.ceil() + 4.0;

        let clear_button_width = if app.state.process_filter.is_empty() { 0.0 } else { 36.0 };
        let spacing = ui.spacing().item_spacing.x * 2.0;
        let edit_width = (ui.available_width() - count_label_width - clear_button_width - spacing).max(120.0);
        // The default `TextEdit::singleline` margin (`Margin::symmetric(4, 2)`)
        // is too tight for the 16 px body font — the caret pokes out
        // top and bottom of the rounded frame. Give it explicit
        // breathing room and a touch less corner radius so the input
        // looks consistent with the surrounding chrome.
        let response = ui.add(
            egui::TextEdit::singleline(&mut app.state.process_filter)
                .hint_text(fl!(crate::LANGUAGE_LOADER, "filter-processes-hint"))
                .desired_width(edit_width)
                .margin(egui::Margin::symmetric(10, 8))
                .vertical_align(egui::Align::Center),
        );
        // Autofocus the filter when the view first appears so the user
        // can start typing immediately.
        if app.state.set_focus {
            response.request_focus();
            app.state.set_focus = false;
        }
        if !app.state.process_filter.is_empty() && ui.button(egui::RichText::new("\u{2297}").size(16.0)).clicked() {
            app.state.process_filter.clear();
        }
        // Render the count label in a right-aligned sub-region with
        // exactly the width we measured above. Using
        // `allocate_ui_with_layout` pins the available width so the
        // label can't expand into the clear button.
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width().max(count_label_width), ui.available_height()),
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                ui.label(egui::RichText::new(count_text).weak());
            },
        );
    });

    ui.add_space(8.0);

    let mut filtered_processes: Vec<&ProcessInfo> = app.state.processes.iter().filter(|process| process_matches_filter(process, &filter)).collect();

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

    // Take an owned snapshot so we can safely select inside the loop.
    // Expanded groups contribute one extra row per group member; only those
    // child rows can be attached to, since a search always runs against a
    // single pid.
    let mut process_rows: Vec<ProcessRow> = Vec::with_capacity(filtered_processes.len());
    for process in &filtered_processes {
        let group_size = process.instances.len();
        let expanded = group_size > 1 && app.expanded_process_groups.contains(&process.pid);
        process_rows.push(ProcessRow {
            process: (*process).clone(),
            group_size,
            expanded,
            is_child: false,
        });
        if expanded {
            let mut children: Vec<&ProcessInfo> = process.instances.iter().collect();
            children.sort_by_key(|child| child.pid);
            for child in children {
                process_rows.push(ProcessRow {
                    process: child.clone(),
                    group_size: 0,
                    expanded: false,
                    is_child: true,
                });
            }
        }
    }
    let mut selected: Option<ProcessInfo> = None;
    let mut toggle_group: Option<process_memory::Pid> = None;

    // If there are no rows to show we hide the whole table (incl.
    // headers) and render a prominent empty/loading state instead.
    if process_rows.is_empty() {
        ui.add_space(28.0);
        let is_loading = total == 0;
        let color = if is_loading {
            ui.visuals().weak_text_color()
        } else {
            ui.visuals().warn_fg_color
        };
        let icon = if is_loading { "\u{231B}" } else { "\u{26A0}" };
        let title = if is_loading {
            fl!(crate::LANGUAGE_LOADER, "no-processes-loading")
        } else {
            fl!(crate::LANGUAGE_LOADER, "no-processes-match")
        };
        let hint = if is_loading {
            fl!(crate::LANGUAGE_LOADER, "no-processes-loading-hint")
        } else {
            fl!(crate::LANGUAGE_LOADER, "no-processes-match-hint")
        };
        ui.vertical_centered(|ui| {
            ui.label(egui::RichText::new(icon).size(48.0).color(color));
            ui.add_space(8.0);
            ui.label(egui::RichText::new(title).size(20.0).strong().color(color));
            ui.add_space(4.0);
            ui.label(egui::RichText::new(hint).size(14.0).weak());
            if !is_loading {
                ui.add_space(14.0);
                if ui
                    .add(
                        egui::Button::new(egui::RichText::new(fl!(crate::LANGUAGE_LOADER, "reset-filter-button")).size(14.0)).min_size(egui::vec2(160.0, 30.0)),
                    )
                    .clicked()
                {
                    app.state.process_filter.clear();
                }
            }
        });
    } else {
        // Render the table directly — no wrapping frame around it.
        // The bottom action bar is rendered by the outer function in
        // its own panel, so we can use the full remaining height here.
        let table_height = ui.available_height();
        egui::ScrollArea::both().max_height(table_height).auto_shrink([false, false]).show(ui, |ui| {
            use egui_extras::{Column, TableBuilder};
            let sort_indicator = |column: ProcessSortColumn| -> &'static str {
                if app.process_sort_column == column {
                    match app.process_sort_direction {
                        SortDirection::Ascending => " \u{2B06}",
                        SortDirection::Descending => " \u{2B07}",
                    }
                } else {
                    ""
                }
            };

            let mut new_sort: Option<ProcessSortColumn> = None;

            TableBuilder::new(ui)
                .striped(true)
                .resizable(true)
                // `Sense::click()` makes the whole row interactive: it
                // both enables the automatic hover-row highlight that
                // `egui_extras` paints when a row is interactive and
                // lets us read `row.response().clicked()` to attach.
                .sense(egui::Sense::click())
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                .column(Column::initial(80.0).at_least(60.0))
                .column(Column::initial(220.0).at_least(120.0))
                .column(Column::initial(120.0).at_least(80.0))
                .column(Column::remainder().at_least(200.0))
                .header(32.0, |mut header| {
                    let mut hdr_btn = |h: &mut egui_extras::TableRow<'_, '_>, label: String, col: ProcessSortColumn| {
                        h.col(|ui| {
                            // Custom-painted header cell: a left-aligned
                            // label that fills the entire column width and
                            // senses clicks across the whole strip. We
                            // can't use a normal `Button` here because for
                            // very wide columns (e.g. the remainder
                            // "Kommando" column) the button centers its
                            // text, so the title ends up off-screen when
                            // the table is wider than the viewport.
                            let cell_rect = ui.available_rect_before_wrap();
                            let strip_rect = egui::Rect::from_min_size(cell_rect.min, egui::vec2(cell_rect.width(), 30.0));
                            let response = ui
                                .interact(strip_rect, ui.id().with(("hdr", col as u32)), egui::Sense::click())
                                .on_hover_cursor(egui::CursorIcon::PointingHand);
                            let visuals = ui.style().interact(&response);
                            ui.painter().rect_filled(strip_rect, egui::CornerRadius::ZERO, visuals.weak_bg_fill);
                            ui.painter()
                                .rect_stroke(strip_rect, egui::CornerRadius::ZERO, visuals.bg_stroke, egui::StrokeKind::Inside);
                            let text = format!("{label}{}", sort_indicator(col));
                            let font_id = egui::TextStyle::Button.resolve(ui.style());
                            ui.painter().text(
                                strip_rect.left_center() + egui::vec2(10.0, 0.0),
                                egui::Align2::LEFT_CENTER,
                                text,
                                font_id,
                                visuals.fg_stroke.color,
                            );
                            if response.clicked() {
                                new_sort = Some(col);
                            }
                            // Tell the parent ui that we used this height
                            // so the row sizes correctly.
                            ui.allocate_rect(strip_rect, egui::Sense::hover());
                        });
                    };
                    hdr_btn(&mut header, fl!(crate::LANGUAGE_LOADER, "pid-heading"), ProcessSortColumn::Pid);
                    hdr_btn(&mut header, fl!(crate::LANGUAGE_LOADER, "name-heading"), ProcessSortColumn::Name);
                    hdr_btn(&mut header, fl!(crate::LANGUAGE_LOADER, "memory-heading"), ProcessSortColumn::Memory);
                    hdr_btn(&mut header, fl!(crate::LANGUAGE_LOADER, "command-heading"), ProcessSortColumn::Command);
                })
                .body(|body| {
                    body.rows(30.0, process_rows.len(), |mut row| {
                        let i = row.index();
                        let ProcessRow {
                            process,
                            group_size,
                            expanded,
                            is_child,
                        } = &process_rows[i];
                        let is_group = *group_size > 1;
                        row.col(|ui| {
                            if *is_child {
                                ui.add_space(18.0);
                            } else if is_group {
                                ui.label(egui::RichText::new(if *expanded { "\u{25BC}" } else { "\u{25B6}" }).size(10.0).weak());
                            }
                            let pid_str = process.pid.to_string();
                            let job = highlight_job(ui, &pid_str, &filter, ui.visuals().text_color(), RowFont::Monospace);
                            ui.add(egui::Label::new(job).selectable(false));
                        });
                        row.col(|ui| {
                            let job = highlight_job(ui, &process.name, &filter, ui.visuals().strong_text_color(), RowFont::Body);
                            ui.add(egui::Label::new(job).selectable(false).truncate());
                        });
                        row.col(|ui| {
                            let bb = gabi::BytesConfig::default();
                            ui.add(egui::Label::new(egui::RichText::new(bb.bytes(process.memory as u64).to_string()).monospace()).selectable(false));
                        });
                        row.col(|ui| {
                            let job = highlight_job(ui, &process.cmd, &filter, ui.visuals().weak_text_color(), RowFont::Body);
                            ui.add(egui::Label::new(job).selectable(false).truncate());
                        });
                        // Whole-row click. With
                        // `TableBuilder::sense(Sense::click())` set
                        // above, the aggregate row response already
                        // senses clicks across every cell. A group row
                        // expands/collapses so every member stays
                        // reachable; any other row attaches.
                        let response = row.response().on_hover_cursor(egui::CursorIcon::PointingHand);
                        if response.clicked() || response.double_clicked() {
                            if is_group {
                                toggle_group = Some(process.pid);
                            } else {
                                selected = Some(process.clone());
                            }
                        }
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
    }

    if let Some(pid) = toggle_group
        && !app.expanded_process_groups.remove(&pid)
    {
        app.expanded_process_groups.insert(pid);
    }

    if let Some(process) = selected {
        app.select_process(&process);
    }
}
