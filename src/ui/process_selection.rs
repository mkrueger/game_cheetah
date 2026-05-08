use i18n_embed_fl::fl;
use icy_ui::{
    Element, Length, alignment,
    widget::{
        button, column, container, row, rule, scroll_area,
        scrollable::{Direction, Scrollbar},
        text, text_input,
    },
};

use crate::{app::App, message::Message};

const COL_PID: f32 = 80.0;
const COL_NAME: f32 = 260.0;
const COL_MEM: f32 = 140.0;
const COL_CMD: f32 = 1200.0;
const ROW_HEIGHT: f32 = 28.0;

/// Case-insensitive ASCII substring search without allocating.
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

/// Case-insensitive ASCII ordering without allocating.
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

fn header_cell<'a>(label: String, indicator: &str, width: Length, sort: ProcessSortColumn, align_right: bool) -> Element<'a, Message> {
    let txt = text(format!("{label}{indicator}")).size(13).font(icy_ui::Font {
        weight: icy_ui::font::Weight::Semibold,
        ..icy_ui::Font::default()
    });
    let inner = container(txt).width(width).padding([6, 8]).align_y(alignment::Alignment::Center);
    let inner = if align_right { inner.align_x(alignment::Alignment::End) } else { inner };
    button(inner)
        .on_press(Message::SortProcesses(sort))
        .width(width)
        .padding(0)
        .style(|theme: &icy_ui::Theme, status: icy_ui::widget::button::Status| {
            use icy_ui::widget::button::Status;
            button::Style {
                background: Some(match status {
                    Status::Hovered => theme.primary.base.scale_alpha(0.85).into(),
                    _ => theme.primary.base.into(),
                }),
                border: icy_ui::Border::default(),
                text_color: theme.primary.on,
                ..Default::default()
            }
        })
        .into()
}

pub fn view_process_selection(app: &App) -> Element<'_, Message> {
    let filter = app.state.process_filter.as_str();

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

    let mut filtered_processes: Vec<&crate::ProcessInfo> = app
        .state
        .processes
        .iter()
        .filter(|process| {
            filter.is_empty()
                || contains_ignore_ascii_case(&process.name, filter)
                || contains_ignore_ascii_case(&process.cmd, filter)
                || process.pid.to_string().contains(filter)
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

    // Search bar (no extra outer border — text_input draws its own)
    let mut search_row = row![
        container(text("🔍").size(16)).padding([0, 4]).align_y(alignment::Alignment::Center),
        text_input(&fl!(crate::LANGUAGE_LOADER, "filter-processes-hint"), &app.state.process_filter)
            .on_input(Message::FilterChanged)
            .padding([8, 10])
            .size(14)
            .width(Length::Fill),
    ]
    .spacing(6)
    .align_y(alignment::Alignment::Center);
    if !app.state.process_filter.is_empty() {
        search_row = search_row.push(
            button(text("✕").size(13))
                .on_press(Message::FilterChanged(String::new()))
                .padding([6, 10])
                .style(|theme: &icy_ui::Theme, _status| button::Style {
                    background: Some(icy_ui::Color::TRANSPARENT.into()),
                    text_color: theme.background.on.scale_alpha(0.7),
                    border: icy_ui::Border::default(),
                    ..Default::default()
                }),
        );
    }

    let search_bar = container(search_row);

    // Header
    let header = container(
        row![
            header_cell(
                fl!(crate::LANGUAGE_LOADER, "pid-heading"),
                sort_indicator(ProcessSortColumn::Pid),
                Length::Fixed(COL_PID),
                ProcessSortColumn::Pid,
                true,
            ),
            header_cell(
                fl!(crate::LANGUAGE_LOADER, "name-heading"),
                sort_indicator(ProcessSortColumn::Name),
                Length::Fixed(COL_NAME),
                ProcessSortColumn::Name,
                false,
            ),
            header_cell(
                fl!(crate::LANGUAGE_LOADER, "memory-heading"),
                sort_indicator(ProcessSortColumn::Memory),
                Length::Fixed(COL_MEM),
                ProcessSortColumn::Memory,
                true,
            ),
            header_cell(
                fl!(crate::LANGUAGE_LOADER, "command-heading"),
                sort_indicator(ProcessSortColumn::Command),
                Length::Fixed(COL_CMD),
                ProcessSortColumn::Command,
                false,
            ),
        ]
        .spacing(0),
    )
    .style(|theme: &icy_ui::Theme| container::Style {
        background: Some(theme.primary.base.into()),
        ..Default::default()
    });

    // Body
    let body: Element<'_, Message> = if filtered_processes.is_empty() {
        container(
            text(if total == 0 {
                fl!(crate::LANGUAGE_LOADER, "no-processes-loading")
            } else {
                fl!(crate::LANGUAGE_LOADER, "no-processes-match")
            })
            .size(14)
            .style(|theme: &icy_ui::Theme| icy_ui::widget::text::Style {
                color: Some(theme.background.on.scale_alpha(0.6)),
            }),
        )
        .center_x(Length::Fill)
        .padding(40)
        .height(Length::Fill)
        .into()
    } else {
        let rows = filtered_processes;
        let total_rows = rows.len();
        scroll_area()
            .direction(Direction::Both {
                vertical: Scrollbar::default(),
                horizontal: Scrollbar::default(),
            })
            .auto_scroll(true)
            .height(Length::Fill)
            .width(Length::Fill)
            .show_rows(ROW_HEIGHT, total_rows, move |visible_range| {
                use icy_ui::widget::text::Wrapping;
                column(
                    visible_range
                        .filter_map(|i| rows.get(i).map(|p| (i, *p)))
                        .map(|(idx, process)| {
                            let process_clone = process.clone();
                            let bb = gabi::BytesConfig::default();
                            let memory = bb.bytes(process.memory as u64).to_string();
                            let zebra = idx % 2 == 1;
                            button(
                                row![
                                    container(text(process.pid.to_string()).size(13).font(icy_ui::Font::MONOSPACE).wrapping(Wrapping::None))
                                        .width(Length::Fixed(COL_PID))
                                        .padding([4, 8])
                                        .align_x(alignment::Alignment::End)
                                        .align_y(alignment::Alignment::Center)
                                        .clip(true),
                                    container(
                                        text(process.name.as_str())
                                            .size(13)
                                            .font(icy_ui::Font {
                                                weight: icy_ui::font::Weight::Semibold,
                                                ..icy_ui::Font::default()
                                            })
                                            .wrapping(Wrapping::None)
                                    )
                                    .width(Length::Fixed(COL_NAME))
                                    .padding([4, 8])
                                    .align_y(alignment::Alignment::Center)
                                    .clip(true),
                                    container(text(memory).size(13).font(icy_ui::Font::MONOSPACE).wrapping(Wrapping::None))
                                        .width(Length::Fixed(COL_MEM))
                                        .padding([4, 8])
                                        .align_x(alignment::Alignment::End)
                                        .align_y(alignment::Alignment::Center)
                                        .clip(true),
                                    container(text(process.cmd.as_str()).size(12).wrapping(Wrapping::None).style(|theme: &icy_ui::Theme| {
                                        icy_ui::widget::text::Style {
                                            color: Some(theme.background.on.scale_alpha(0.7)),
                                        }
                                    }))
                                    .width(Length::Fixed(COL_CMD))
                                    .padding([4, 8])
                                    .align_y(alignment::Alignment::Center)
                                    .clip(true),
                                ]
                                .height(Length::Fixed(ROW_HEIGHT)),
                            )
                            .style(move |theme: &icy_ui::Theme, status: icy_ui::widget::button::Status| {
                                use icy_ui::widget::button::Status;
                                let base_bg = if zebra {
                                    theme.background.on.scale_alpha(0.04)
                                } else {
                                    icy_ui::Color::TRANSPARENT
                                };
                                match status {
                                    Status::Hovered => button::Style {
                                        background: Some(theme.accent.base.scale_alpha(0.18).into()),
                                        border: icy_ui::Border::default(),
                                        text_color: theme.background.on,
                                        ..Default::default()
                                    },
                                    _ => button::Style {
                                        background: Some(base_bg.into()),
                                        border: icy_ui::Border::default(),
                                        text_color: theme.background.on,
                                        ..Default::default()
                                    },
                                }
                            })
                            .on_press(Message::SelectProcess(process_clone))
                            .padding(0)
                            .into()
                        })
                        .collect::<Vec<Element<'_, Message>>>(),
                )
                .spacing(0)
                .into()
            })
            .into()
    };

    // Footer
    let footer = row![
        text(if filter.is_empty() {
            fl!(crate::LANGUAGE_LOADER, "process-count-total", total = total)
        } else {
            fl!(crate::LANGUAGE_LOADER, "process-count-filtered", shown = shown, total = total)
        })
        .size(12)
        .style(|theme: &icy_ui::Theme| icy_ui::widget::text::Style {
            color: Some(theme.background.on.scale_alpha(0.6)),
        }),
        container(text("")).width(Length::Fill),
        button(text(fl!(crate::LANGUAGE_LOADER, "close-button")))
            .on_press(Message::MainMenu)
            .padding([6, 14]),
    ]
    .spacing(10)
    .align_y(alignment::Alignment::Center);

    container(
        column![
            search_bar,
            container(column![header, rule::horizontal(1), body].spacing(0))
                .height(Length::FillPortion(1))
                .style(|theme: &icy_ui::Theme| container::Style {
                    background: Some(theme.background.base.into()),
                    border: icy_ui::Border {
                        radius: 4.0.into(),
                        width: 1.0,
                        color: theme.primary.divider,
                    },
                    ..Default::default()
                })
                .clip(true),
            footer,
        ]
        .spacing(10)
        .padding(crate::DIALOG_PADDING),
    )
    .into()
}
