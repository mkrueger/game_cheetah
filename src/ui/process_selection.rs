use i18n_embed_fl::fl;
use iced::{
    Element, Length, alignment,
    widget::{button, column, container, row, scrollable, text, text_input},
};

use crate::{app::App, message::Message};

pub fn view_process_selection(app: &App) -> Element<'_, Message> {
    let filter = app.state.process_filter.to_ascii_uppercase();

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
            // Table header
            container(row![
                container(text(fl!(crate::LANGUAGE_LOADER, "pid-heading")).size(14))
                    .width(Length::Fixed(80.0))
                    .padding(5),
                container(text(fl!(crate::LANGUAGE_LOADER, "name-heading")).size(14))
                    .width(Length::Fixed(250.0))
                    .padding(5),
                container(text(fl!(crate::LANGUAGE_LOADER, "memory-heading")).size(14))
                    .width(Length::Fixed(200.0))
                    .padding(5),
                container(text(fl!(crate::LANGUAGE_LOADER, "command-heading")).size(14))
                    .width(Length::Fill)
                    .padding(5),
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
                    app.state
                        .processes
                        .iter()
                        .filter(|process| {
                            filter.is_empty()
                                || process.name.to_ascii_uppercase().contains(filter.as_str())
                                || process.cmd.to_ascii_uppercase().contains(filter.as_str())
                                || process.pid.to_string().contains(filter.as_str())
                        })
                        .enumerate()
                        .map(|(_index, process)| {
                            let process_clone = process.clone();
                            // let is_even = index % 2 == 0;
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
                                            background: Some(theme.palette().primary.into()), // highlight color
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
                                //if is_even {
                                container::Style::default()
                                /*} else {
                                    container::Style {
                                        background: Some(theme.extended_palette().secondary.weak.color.into()),
                                        ..Default::default()
                                    }
                                }*/
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
