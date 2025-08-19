use i18n_embed_fl::fl;
use iced::{
    Element, Length, alignment,
    widget::{button, column, container, text},
};

use crate::{app::App, message::Message};

pub fn view_main_window(_app: &App) -> Element<'_, Message> {
    container(
        column![
            // Add title and version at the top
            container(
                column![
                    text(crate::APP_NAME).size(32),
                    text(format!("v{} © Mike Krüger 2023-2025", crate::VERSION))
                        .size(16)
                        .style(|theme: &iced::Theme| {
                            iced::widget::text::Style {
                                color: Some(theme.extended_palette().secondary.base.color),
                            }
                        }),
                    button(text("github.com/mkrueger/game_cheetah").size(14))
                        .style(|theme: &iced::Theme, status: iced::widget::button::Status| {
                            use iced::widget::button::Status;
                            match status {
                                Status::Hovered => button::Style {
                                    background: Some(iced::Color::TRANSPARENT.into()),
                                    border: iced::Border::default(),
                                    text_color: theme.palette().primary,
                                    ..Default::default()
                                },
                                _ => button::Style {
                                    background: Some(iced::Color::TRANSPARENT.into()),
                                    border: iced::Border::default(),
                                    text_color: theme.extended_palette().secondary.base.color,
                                    ..Default::default()
                                },
                            }
                        })
                        .on_press(Message::OpenGitHub)
                        .padding(5),
                ]
                .spacing(5)
                .width(Length::Fill)
                .align_x(alignment::Alignment::Center)
            )
            .width(Length::Fill)
            .padding(20),
            // Menu buttons
            column![
                button(text(fl!(crate::LANGUAGE_LOADER, "attach-button")).size(24))
                    .on_press(Message::Attach)
                    .padding(10),
                button(text(fl!(crate::LANGUAGE_LOADER, "discuss-button")))
                    .on_press(Message::Discuss)
                    .padding(10),
                button(text(fl!(crate::LANGUAGE_LOADER, "bug-button"))).on_press(Message::ReportBug).padding(10),
                button(text(fl!(crate::LANGUAGE_LOADER, "about-button"))).on_press(Message::About).padding(10),
                button(text(fl!(crate::LANGUAGE_LOADER, "quit-button"))).on_press(Message::Exit).padding(10)
            ]
            .spacing(10)
            .align_x(alignment::Alignment::Center),
        ]
        .spacing(20)
        .align_x(alignment::Alignment::Center),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .align_x(alignment::Alignment::Center)
    .align_y(alignment::Alignment::Center)
    .into()
}
