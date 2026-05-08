use i18n_embed_fl::fl;
use icy_ui::{
    Element, Length, alignment,
    widget::{button, checkbox, column, container, row, rule, text},
};

use crate::{app::App, message::Message};

pub fn view_main_window(_app: &App) -> Element<'_, Message> {
    container(
        column![
            // Add title and version at the top
            container(
                column![
                    text(crate::APP_NAME).size(32),
                    button(text("github.com/mkrueger/game_cheetah").size(14))
                        .style(|theme: &icy_ui::Theme, status: icy_ui::widget::button::Status| {
                            use icy_ui::widget::button::Status;
                            match status {
                                Status::Hovered => button::Style {
                                    background: Some(icy_ui::Color::TRANSPARENT.into()),
                                    border: icy_ui::Border::default(),
                                    text_color: theme.accent.base,
                                    ..Default::default()
                                },
                                _ => button::Style {
                                    background: Some(icy_ui::Color::TRANSPARENT.into()),
                                    border: icy_ui::Border::default(),
                                    text_color: theme.secondary.on,
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
                button(text(fl!(crate::LANGUAGE_LOADER, "settings-button")))
                    .on_press(Message::Settings)
                    .padding(10),
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

pub fn view_settings(app: &App) -> Element<'_, Message> {
    let config_dir = crate::config_dir().display().to_string();

    container(
        column![
            container(
                text(fl!(crate::LANGUAGE_LOADER, "settings-title"))
                    .size(24)
                    .style(|theme: &icy_ui::Theme| icy_ui::widget::text::Style {
                        color: Some(theme.accent.base)
                    })
            )
            .width(Length::Fill)
            .align_x(alignment::Alignment::Center),
            rule::horizontal(1),
            column![
                row![
                    checkbox(app.auto_reconnect).on_toggle(|_| Message::ToggleAutoReconnect).size(16),
                    text(fl!(crate::LANGUAGE_LOADER, "automatic-reconnect-label")).size(16),
                ]
                .spacing(8)
                .align_y(alignment::Alignment::Center),
                text(fl!(crate::LANGUAGE_LOADER, "automatic-reconnect-description"))
                    .size(13)
                    .style(|theme: &icy_ui::Theme| icy_ui::widget::text::Style {
                        color: Some(theme.background.on.scale_alpha(0.65)),
                    })
            ]
            .spacing(6),
            column![
                text(fl!(crate::LANGUAGE_LOADER, "config-directory-label")).size(16),
                container(text(config_dir).size(13))
                    .width(Length::Fill)
                    .padding([6, 8])
                    .style(|theme: &icy_ui::Theme| icy_ui::widget::container::Style {
                        background: Some(theme.background.on.scale_alpha(0.06).into()),
                        border: icy_ui::Border {
                            radius: 2.0.into(),
                            width: 1.0,
                            color: theme.primary.divider,
                        },
                        ..Default::default()
                    }),
                text(fl!(crate::LANGUAGE_LOADER, "config-directory-description"))
                    .size(13)
                    .style(|theme: &icy_ui::Theme| icy_ui::widget::text::Style {
                        color: Some(theme.background.on.scale_alpha(0.65)),
                    })
            ]
            .spacing(6),
            container(
                button(text(fl!(crate::LANGUAGE_LOADER, "back-to-main-button")))
                    .on_press(Message::MainMenu)
                    .padding(10)
            )
            .width(Length::Fill)
            .align_x(alignment::Alignment::Center)
        ]
        .spacing(20)
        .padding(crate::DIALOG_PADDING),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .align_x(alignment::Alignment::Center)
    .align_y(alignment::Alignment::Center)
    .into()
}
