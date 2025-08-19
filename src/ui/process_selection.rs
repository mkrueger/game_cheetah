use i18n_embed_fl::fl;
use iced::{
    Element, Length, alignment,
    widget::{button, column, container, row, scrollable, text, text_input},
};

use crate::{app::App, message::Message};

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum ProcessSortColumn {
    #[default]
    Pid,
    Name,
    Memory,
    Command,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum SortDirection {
    #[default]
    Ascending,
    Descending,
}

pub fn view_process_selection(app: &App) -> Element<'_, Message> {
    let filter = app.state.process_filter.to_ascii_uppercase();

    // Get sort indicator
    let sort_indicator = |column: ProcessSortColumn| -> String {
        if app.process_sort_column == column {
            match app.process_sort_direction {
                SortDirection::Ascending => " ▲".to_string(),
                SortDirection::Descending => " ▼".to_string(),
            }
        } else {
            String::new()
        }
    };

    // Filter processes
    let mut filtered_processes: Vec<_> = app.state
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

    // Sort processes
    match app.process_sort_column {
        ProcessSortColumn::Pid => {
            filtered_processes.sort_by(|a, b| match app.process_sort_direction {
                SortDirection::Ascending => a.pid.cmp(&b.pid),
                SortDirection::Descending => b.pid.cmp(&a.pid),
            });
        }
        ProcessSortColumn::Name => {
            filtered_processes.sort_by(|a, b| match app.process_sort_direction {
                SortDirection::Ascending => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                SortDirection::Descending => b.name.to_lowercase().cmp(&a.name.to_lowercase()),
            });
        }
        ProcessSortColumn::Memory => {
            filtered_processes.sort_by(|a, b| match app.process_sort_direction {
                SortDirection::Ascending => a.memory.cmp(&b.memory),
                SortDirection::Descending => b.memory.cmp(&a.memory),
            });
        }
        ProcessSortColumn::Command => {
            filtered_processes.sort_by(|a, b| match app.process_sort_direction {
                SortDirection::Ascending => a.cmd.to_lowercase().cmp(&b.cmd.to_lowercase()),
                SortDirection::Descending => b.cmd.to_lowercase().cmp(&a.cmd.to_lowercase()),
            });
        }
    }

    // Process selection dialog
    container(
        column![
            // Filter input row with Clear and Close buttons
            row![
                text(fl!(crate::LANGUAGE_LOADER, "process-label")),
                text_input(&fl!(crate::LANGUAGE_LOADER, "filter-processes-hint"), &app.state.process_filter)
                    .on_input(Message::FilterChanged)
                    .padding(10)
                    .width(Length::Fill),
                button(text(fl!(crate::LANGUAGE_LOADER, "clear-button")))
                    .on_press(Message::FilterChanged(String::new()))
                    .padding(5),
                button(text(fl!(crate::LANGUAGE_LOADER, "close-button"))).on_press(Message::MainMenu).padding(5),
            ]
            .spacing(10)
            .align_y(alignment::Alignment::Center),
            // Table header with sortable columns
            container(row![
                button(
                    container(
                        text(format!("{}{}", 
                            fl!(crate::LANGUAGE_LOADER, "pid-heading"),
                            sort_indicator(ProcessSortColumn::Pid)
                        )).size(14)
                    )
                    .width(Length::Fixed(80.0))
                    .padding(5)
                )
                .on_press(Message::SortProcesses(ProcessSortColumn::Pid))
                .style(|theme: &iced::Theme, _status| button::Style {
                    background: Some(theme.extended_palette().background.weak.color.into()),
                    border: iced::Border::default(),
                    text_color: theme.palette().text,
                    ..Default::default()
                })
                .width(Length::Fixed(80.0))
                .padding(0),
                
                button(
                    container(
                        text(format!("{}{}", 
                            fl!(crate::LANGUAGE_LOADER, "name-heading"),
                            sort_indicator(ProcessSortColumn::Name)
                        )).size(14)
                    )
                    .width(Length::Fixed(250.0))
                    .padding(5)
                )
                .on_press(Message::SortProcesses(ProcessSortColumn::Name))
                .style(|theme: &iced::Theme, _status| button::Style {
                    background: Some(theme.extended_palette().background.weak.color.into()),
                    border: iced::Border::default(),
                    text_color: theme.palette().text,
                    ..Default::default()
                })
                .width(Length::Fixed(250.0))
                .padding(0),
                
                button(
                    container(
                        text(format!("{}{}", 
                            fl!(crate::LANGUAGE_LOADER, "memory-heading"),
                            sort_indicator(ProcessSortColumn::Memory)
                        )).size(14)
                    )
                    .width(Length::Fixed(200.0))
                    .padding(5)
                )
                .on_press(Message::SortProcesses(ProcessSortColumn::Memory))
                .style(|theme: &iced::Theme, _status| button::Style {
                    background: Some(theme.extended_palette().background.weak.color.into()),
                    border: iced::Border::default(),
                    text_color: theme.palette().text,
                    ..Default::default()
                })
                .width(Length::Fixed(200.0))
                .padding(0),
                
                button(
                    container(
                        text(format!("{}{}", 
                            fl!(crate::LANGUAGE_LOADER, "command-heading"),
                            sort_indicator(ProcessSortColumn::Command)
                        )).size(14)
                    )
                    .width(Length::Fill)
                    .padding(5)
                )
                .on_press(Message::SortProcesses(ProcessSortColumn::Command))
                .style(|theme: &iced::Theme, _status| button::Style {
                    background: Some(theme.extended_palette().background.weak.color.into()),
                    border: iced::Border::default(),
                    text_color: theme.palette().text,
                    ..Default::default()
                })
                .width(Length::Fill)
                .padding(0),
            ])
            .style(|theme: &iced::Theme| {
                container::Style {
                    background: Some(theme.extended_palette().background.weak.color.into()),
                    ..Default::default()
                }
            }),
            // Table body
            scrollable(
                column(
                    filtered_processes
                        .iter()
                        .enumerate()
                        .map(|(_index, process)| {
                            let process_clone = process.clone();
                            let bb = gabi::BytesConfig::default();
                            let memory = bb.bytes(process.memory as u64).to_string();

                            container(
                                button(row![
                                    container(text(process.pid.to_string()).size(14)).width(Length::Fixed(80.0)).padding(5),
                                    container(text(process.name.clone()).size(14)).width(Length::Fixed(250.0)).padding(5),
                                    container(text(memory).size(14)).width(Length::Fixed(200.0)).padding(5),
                                    container(text(process.cmd.clone()).size(14)).width(Length::Fill).padding(5),
                                ])
                                .style(|theme: &iced::Theme, status: iced::widget::button::Status| {
                                    use iced::widget::button::Status;
                                    match status {
                                        Status::Hovered => button::Style {
                                            background: Some(theme.palette().primary.into()),
                                            border: iced::Border::default(),
                                            text_color: theme.palette().text,
                                            ..Default::default()
                                        },
                                        _ => button::Style {
                                            background: Some(iced::Color::TRANSPARENT.into()),
                                            border: iced::Border::default(),
                                            text_color: theme.palette().text,
                                            ..Default::default()
                                        },
                                    }
                                })
                                .on_press(Message::SelectProcess(process_clone))
                                .width(Length::Fill)
                                .padding(0),
                            )
                            .style(move |_theme: &iced::Theme| {
                                container::Style::default()
                            })
                            .into()
                        })
                        .collect::<Vec<Element<'_, Message>>>()
                )
                .spacing(0)
            )
            .height(Length::FillPortion(1))
        ]
        .spacing(10)
        .padding(crate::DIALOG_PADDING),
    )
    .into()
}