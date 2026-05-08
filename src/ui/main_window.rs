use i18n_embed_fl::fl;
use icy_ui::{
    Element, Length, alignment,
    widget::{button, checkbox, column, container, row, rule, text},
};

use crate::{app::App, message::Message};

const MAIN_MENU_BUTTON_WIDTH: f32 = 280.0;

pub fn view_main_window(_app: &App) -> Element<'_, Message> {
    container(
        column![
            container(
                column![
                    text(crate::APP_NAME).size(32),
                    text(fl!(crate::LANGUAGE_LOADER, "main-menu-subtitle"))
                        .size(14)
                        .style(|theme: &icy_ui::Theme| icy_ui::widget::text::Style {
                            color: Some(theme.background.on.scale_alpha(0.65)),
                        })
                ]
                .spacing(6)
                .width(Length::Fill)
                .align_x(alignment::Alignment::Center)
            )
            .width(Length::Fill)
            .padding([8, 20]),
            column![
                button(text(fl!(crate::LANGUAGE_LOADER, "attach-button")).size(24))
                    .on_press(Message::Attach)
                    .padding([12, 20])
                    .width(Length::Fixed(MAIN_MENU_BUTTON_WIDTH))
                    .style(|theme: &icy_ui::Theme, status: icy_ui::widget::button::Status| {
                        use icy_ui::widget::button::Status;
                        button::Style {
                            background: Some(match status {
                                Status::Hovered => theme.accent.base.scale_alpha(0.9).into(),
                                _ => theme.accent.base.into(),
                            }),
                            text_color: theme.accent.on,
                            border: icy_ui::Border {
                                radius: 4.0.into(),
                                ..Default::default()
                            },
                            ..Default::default()
                        }
                    }),
                button(text(fl!(crate::LANGUAGE_LOADER, "settings-button")))
                    .on_press(Message::Settings)
                    .padding(10)
                    .width(Length::Fixed(MAIN_MENU_BUTTON_WIDTH)),
                button(text(fl!(crate::LANGUAGE_LOADER, "about-button")))
                    .on_press(Message::About)
                    .padding(10)
                    .width(Length::Fixed(MAIN_MENU_BUTTON_WIDTH)),
                button(text(fl!(crate::LANGUAGE_LOADER, "discuss-button")))
                    .on_press(Message::Discuss)
                    .padding(10)
                    .width(Length::Fixed(MAIN_MENU_BUTTON_WIDTH)),
                button(text(fl!(crate::LANGUAGE_LOADER, "bug-button")))
                    .on_press(Message::ReportBug)
                    .padding(10)
                    .width(Length::Fixed(MAIN_MENU_BUTTON_WIDTH)),
                button(text(fl!(crate::LANGUAGE_LOADER, "quit-button")))
                    .on_press(Message::Exit)
                    .padding(10)
                    .width(Length::Fixed(MAIN_MENU_BUTTON_WIDTH))
            ]
            .spacing(10)
            .align_x(alignment::Alignment::Center),
            column![
                text(format!("v{}", crate::VERSION))
                    .size(12)
                    .style(|theme: &icy_ui::Theme| icy_ui::widget::text::Style {
                        color: Some(theme.background.on.scale_alpha(0.55)),
                    }),
                button(text("github.com/mkrueger/game_cheetah").size(13))
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
                                text_color: theme.background.on.scale_alpha(0.55),
                                ..Default::default()
                            },
                        }
                    })
                    .on_press(Message::OpenGitHub)
                    .padding(2),
            ]
            .spacing(2)
            .align_x(alignment::Alignment::Center),
        ]
        .spacing(24)
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

    // Group with a faint card-style background and a subtle divider so each
    // setting reads as one unit instead of free-floating widgets.
    fn section<'a>(content: Element<'a, Message>) -> Element<'a, Message> {
        container(content)
            .width(Length::Fill)
            .padding(16)
            .style(|theme: &icy_ui::Theme| icy_ui::widget::container::Style {
                background: Some(theme.background.on.scale_alpha(0.04).into()),
                border: icy_ui::Border {
                    radius: 4.0.into(),
                    width: 1.0,
                    color: theme.primary.divider,
                },
                ..Default::default()
            })
            .into()
    }

    let auto_reconnect_section = section(
        column![
            row![
                checkbox(app.auto_reconnect).on_toggle(|_| Message::ToggleAutoReconnect).size(16),
                text(fl!(crate::LANGUAGE_LOADER, "automatic-reconnect-label")).size(16).font(icy_ui::Font {
                    weight: icy_ui::font::Weight::Semibold,
                    ..icy_ui::Font::default()
                }),
            ]
            .spacing(8)
            .align_y(alignment::Alignment::Center),
            text(fl!(crate::LANGUAGE_LOADER, "automatic-reconnect-description"))
                .size(13)
                .style(|theme: &icy_ui::Theme| icy_ui::widget::text::Style {
                    color: Some(theme.background.on.scale_alpha(0.65)),
                })
        ]
        .spacing(8)
        .into(),
    );

    let config_dir_section = section(
        column![
            text(fl!(crate::LANGUAGE_LOADER, "config-directory-label")).size(16).font(icy_ui::Font {
                weight: icy_ui::font::Weight::Semibold,
                ..icy_ui::Font::default()
            }),
            row![
                container(text(config_dir).size(13).font(icy_ui::Font::MONOSPACE))
                    .width(Length::Fill)
                    .padding([8, 10])
                    .style(|theme: &icy_ui::Theme| icy_ui::widget::container::Style {
                        background: Some(theme.background.base.into()),
                        border: icy_ui::Border {
                            radius: 2.0.into(),
                            width: 1.0,
                            color: theme.primary.divider,
                        },
                        ..Default::default()
                    }),
                button(text(fl!(crate::LANGUAGE_LOADER, "open-config-directory-button")))
                    .on_press(Message::OpenConfigDir)
                    .padding([8, 12]),
                button(text(fl!(crate::LANGUAGE_LOADER, "copy-config-directory-button")))
                    .on_press(Message::CopyConfigDir)
                    .padding([8, 12]),
            ]
            .spacing(8)
            .align_y(alignment::Alignment::Center),
            text(fl!(crate::LANGUAGE_LOADER, "config-directory-description"))
                .size(13)
                .style(|theme: &icy_ui::Theme| icy_ui::widget::text::Style {
                    color: Some(theme.background.on.scale_alpha(0.65)),
                })
        ]
        .spacing(8)
        .into(),
    );

    container(
        container(
            column![
                container(
                    text(fl!(crate::LANGUAGE_LOADER, "settings-title"))
                        .size(28)
                        .style(|theme: &icy_ui::Theme| icy_ui::widget::text::Style {
                            color: Some(theme.accent.base)
                        })
                )
                .width(Length::Fill)
                .align_x(alignment::Alignment::Center),
                rule::horizontal(1),
                auto_reconnect_section,
                config_dir_section,
                container(
                    button(text(fl!(crate::LANGUAGE_LOADER, "back-to-main-button")))
                        .on_press(Message::MainMenu)
                        .padding([8, 16])
                )
                .width(Length::Fill)
                .align_x(alignment::Alignment::Center)
            ]
            .spacing(16)
            .padding(crate::DIALOG_PADDING),
        )
        .width(Length::Fixed(640.0)),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .align_x(alignment::Alignment::Center)
    .align_y(alignment::Alignment::Center)
    .into()
}
