use i18n_embed_fl::fl;
use icy_ui::{
    Element, Length, alignment,
    widget::{button, column, container, row, rule, scrollable, text, text_input},
};

use crate::{app::App, message::Message};

const COL_PID: f32 = 80.0;
const COL_NAME: f32 = 260.0;
const COL_MEM: f32 = 140.0;
const ROW_HEIGHT: f32 = 28.0;

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
    let filter = app.state.process_filter.to_ascii_uppercase();

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

    let mut filtered_processes: Vec<_> = app
        .state
        .processes
        .iter()
        .filter(|process| {
            filter.is_empty()
                || process.name.to_ascii_uppercase().contains(filter.as_str())
                || process.cmd.to_ascii_uppercase().contains(filter.as_str())
                || process.pid.to_string().contains(filter.as_str())
        })
        .cloned()
        .collect();

    match app.process_sort_column {
        ProcessSortColumn::Pid => filtered_processes.sort_by(|a, b| match app.process_sort_direction {
            SortDirection::Ascending => a.pid.cmp(&b.pid),
            SortDirection::Descending => b.pid.cmp(&a.pid),
        }),
        ProcessSortColumn::Name => filtered_processes.sort_by(|a, b| match app.process_sort_direction {
            SortDirection::Ascending => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            SortDirection::Descending => b.name.to_lowercase().cmp(&a.name.to_lowercase()),
        }),
        ProcessSortColumn::Memory => filtered_processes.sort_by(|a, b| match app.process_sort_direction {
            SortDirection::Ascending => a.memory.cmp(&b.memory),
            SortDirection::Descending => b.memory.cmp(&a.memory),
        }),
        ProcessSortColumn::Command => filtered_processes.sort_by(|a, b| match app.process_sort_direction {
            SortDirection::Ascending => a.cmd.to_lowercase().cmp(&b.cmd.to_lowercase()),
            SortDirection::Descending => b.cmd.to_lowercase().cmp(&a.cmd.to_lowercase()),
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
                Length::Fill,
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
        scrollable(column(filtered_processes.iter().enumerate().map(|(idx, process)| {
            let process_clone = process.clone();
            let bb = gabi::BytesConfig::default();
            let memory = bb.bytes(process.memory as u64).to_string();
            let zebra = idx % 2 == 1;
            use icy_ui::widget::text::Wrapping;
            container(
                button(
                    row![
                        container(text(process.pid.to_string()).size(13).font(icy_ui::Font::MONOSPACE).wrapping(Wrapping::None))
                            .width(Length::Fixed(COL_PID))
                            .padding([4, 8])
                            .align_x(alignment::Alignment::End)
                            .align_y(alignment::Alignment::Center)
                            .clip(true),
                        container(
                            text(process.name.clone())
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
                        container(
                            text(process.cmd.clone())
                                .size(12)
                                .wrapping(Wrapping::None)
                                .style(|theme: &icy_ui::Theme| icy_ui::widget::text::Style {
                                    color: Some(theme.background.on.scale_alpha(0.7)),
                                })
                        )
                        .width(Length::Fill)
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
                .width(Length::Fill)
                .padding(0),
            )
            .into()
        })))
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
