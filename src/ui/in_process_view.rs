use std::sync::atomic::Ordering;

use i18n_embed_fl::fl;
use iced::{
    Element, Length, alignment,
    widget::{button, checkbox, column, container, horizontal_rule, pick_list, progress_bar, row, scrollable, text, text_input, vertical_rule},
};
use process_memory::{TryIntoProcessHandle, copy_address};

use crate::{SearchMode, SearchType, SearchValue, app::App, message::Message};

fn search_ui(app: &App) -> Element<'_, Message> {
    let search_types = vec![
        SearchType::Guess,
        SearchType::Unknown,
        SearchType::Short,
        SearchType::Int,
        SearchType::Int64,
        SearchType::Float,
        SearchType::Double,
        SearchType::String,
    ];
    let current_search_context = &app.state.searches[app.state.current_search];
    let selected_type = current_search_context.search_type;
    let value_text = &current_search_context.search_value_text;
    let search_results = current_search_context.get_result_count();
    let is_search_complete = current_search_context.search_complete.load(Ordering::SeqCst);
    let can_undo = !current_search_context.old_results.is_empty();
    let show_error = !current_search_context.search_value_text.is_empty()
        && current_search_context
            .search_type
            .from_string(&current_search_context.search_value_text)
            .is_err()
        && selected_type != SearchType::Unknown; // Don't show error for Unknown type
    let auto_show_treshold = 20;

    // Get the current search name for the header
    let current_search_name = current_search_context.description.clone();

    column![
        container(
            text(format!("{}", current_search_name))
                .size(18)
                .font(iced::Font {
                    weight: iced::font::Weight::Bold,
                    ..iced::Font::default()
                })
                .style(|theme: &iced::Theme| iced::widget::text::Style {
                    color: Some(theme.palette().primary),
                })
        ),
        horizontal_rule(1),
        // Conditionally show value input or unknown search info
        if selected_type == SearchType::Unknown {
            row![
                text(fl!(crate::LANGUAGE_LOADER, "search-type-label")),
                pick_list(search_types.clone(), Some(selected_type), |t| Message::SwitchSearchType(t)),
                text(fl!(crate::LANGUAGE_LOADER, "unknown-search-description"))
            ]
            .spacing(10)
            .align_y(alignment::Alignment::Center)
        } else {
            row![
                text(fl!(crate::LANGUAGE_LOADER, "value-label")),
                text_input(
                    &fl!(crate::LANGUAGE_LOADER, "search-value-label", valuetype = selected_type.get_description_text()),
                    value_text
                )
                .on_input(|v| Message::SearchValueChanged(v))
                .on_submit(Message::Search)
                .padding(10)
                .width(Length::Fill),
                pick_list(search_types.clone(), Some(selected_type), |t| Message::SwitchSearchType(t)),
                if show_error {
                    text(fl!(crate::LANGUAGE_LOADER, "invalid-number-error")).style(|theme: &iced::Theme| iced::widget::text::Style {
                        color: Some(theme.palette().danger.into()),
                    })
                } else {
                    text("")
                }
            ]
            .spacing(10)
            .align_y(alignment::Alignment::Center)
        },
        if !matches!(current_search_context.searching, SearchMode::None) {
            let current_bytes = current_search_context.current_bytes.load(Ordering::Acquire);
            column![
                row![
                    progress_bar(0.0..=current_search_context.total_bytes as f32, current_bytes as f32).width(Length::Fill),
                    if current_search_context.searching == SearchMode::Percent {
                        text(
                            fl!(
                                crate::LANGUAGE_LOADER,
                                "update-numbers-progress",
                                current = current_bytes,
                                total = current_search_context.total_bytes
                            )
                            .chars()
                            .filter(|c| c.is_ascii())
                            .collect::<String>(),
                        )
                    } else {
                        let bb = gabi::BytesConfig::default();
                        let current_bytes_out = bb.bytes(current_bytes as u64).to_string();
                        let total_bytes_out = bb.bytes(current_search_context.total_bytes as u64).to_string();
                        text(
                            fl!(
                                crate::LANGUAGE_LOADER,
                                "search-memory-progress",
                                current = current_bytes_out,
                                total = total_bytes_out
                            )
                            .chars()
                            .filter(|c| c.is_ascii())
                            .collect::<String>(),
                        )
                    }
                ]
                .spacing(10.0)
                .align_y(alignment::Alignment::Center)
            ]
        } else {
            if !is_search_complete {
                column![
                    row![{
                        let b = button(text(fl!(crate::LANGUAGE_LOADER, "initial-search-button")));
                        if show_error && selected_type != SearchType::Unknown {
                            b
                        } else {
                            b.on_press(Message::Search)
                        }
                    }]
                    .spacing(10.0)
                ]
            } else {
                // Different buttons for Unknown search type
                if selected_type == SearchType::String {
                    column![
                        row![
                            button(text(fl!(crate::LANGUAGE_LOADER, "initial-search-button"))).on_press(Message::Search),
                            if search_results >= auto_show_treshold {
                                if app.state.show_results {
                                    button(text(fl!(crate::LANGUAGE_LOADER, "hide-results-button"))).on_press(Message::ToggleShowResult)
                                } else {
                                    button(text(fl!(crate::LANGUAGE_LOADER, "show-results-button"))).on_press(Message::ToggleShowResult)
                                }
                            } else {
                                button("").style(|_, _| button::Style::default())
                            },
                            text(
                                fl!(crate::LANGUAGE_LOADER, "found-results-label", results = search_results)
                                    .chars()
                                    .filter(|c| c.is_ascii())
                                    .collect::<String>()
                            )
                        ]
                        .align_y(alignment::Alignment::Center)
                        .spacing(10.0)
                    ]
                } else if selected_type == SearchType::Unknown {
                    column![
                        row![
                            button(text(fl!(crate::LANGUAGE_LOADER, "decreased-button"))).on_press(Message::UnknownSearchDecrease),
                            button(text(fl!(crate::LANGUAGE_LOADER, "increased-button"))).on_press(Message::UnknownSearchIncrease),
                            button(text(fl!(crate::LANGUAGE_LOADER, "changed-button"))).on_press(Message::UnknownSearchChanged),
                            {
                                let b = button(text(fl!(crate::LANGUAGE_LOADER, "unchanged-button")));
                                if search_results > 0 || can_undo {
                                    b.on_press(Message::UnknownSearchUnchanged)
                                } else {
                                    b
                                }
                            },
                            container(vertical_rule(1)).height(Length::Fixed(24.0)),
                            button(text(fl!(crate::LANGUAGE_LOADER, "clear-button"))).on_press(Message::ClearResults),
                            {
                                let b = button(text(fl!(crate::LANGUAGE_LOADER, "undo-button")));
                                if can_undo { b.on_press(Message::Undo) } else { b }
                            },
                            if search_results >= auto_show_treshold {
                                if app.state.show_results {
                                    button(text(fl!(crate::LANGUAGE_LOADER, "hide-results-button"))).on_press(Message::ToggleShowResult)
                                } else {
                                    button(text(fl!(crate::LANGUAGE_LOADER, "show-results-button"))).on_press(Message::ToggleShowResult)
                                }
                            } else {
                                button("").style(|_, _| button::Style::default())
                            }
                        ]
                        .align_y(alignment::Alignment::Center)
                        .spacing(10.0),
                        row![text(if search_results > 0 || can_undo {
                            fl!(crate::LANGUAGE_LOADER, "found-results-label", results = search_results)
                                .chars()
                                .filter(|c| c.is_ascii())
                                .collect::<String>()
                        } else {
                            String::new()
                        })]
                    ]
                } else {
                    // Normal search buttons
                    column![
                        row![
                            {
                                let b = button(text(fl!(crate::LANGUAGE_LOADER, "update-button")));
                                if show_error { b } else { b.on_press(Message::Search) }
                            },
                            button(text(fl!(crate::LANGUAGE_LOADER, "clear-button"))).on_press(Message::ClearResults),
                            {
                                let b = button(text(fl!(crate::LANGUAGE_LOADER, "undo-button")));
                                if can_undo { b.on_press(Message::Undo) } else { b }
                            },
                            if search_results >= auto_show_treshold {
                                if app.state.show_results {
                                    button(text(fl!(crate::LANGUAGE_LOADER, "hide-results-button"))).on_press(Message::ToggleShowResult)
                                } else {
                                    button(text(fl!(crate::LANGUAGE_LOADER, "show-results-button"))).on_press(Message::ToggleShowResult)
                                }
                            } else {
                                button("").style(|_, _| button::Style::default())
                            },
                            text(
                                fl!(crate::LANGUAGE_LOADER, "found-results-label", results = search_results)
                                    .chars()
                                    .filter(|c| c.is_ascii())
                                    .collect::<String>()
                            )
                        ]
                        .align_y(alignment::Alignment::Center)
                        .spacing(10.0)
                    ]
                }
            }
        },
        if search_results > 0 && search_results < auto_show_treshold || app.state.show_results {
            render_result_table(app)
        } else {
            column![].into()
        }
    ]
    .spacing(10)
    .padding(10)
    .into()
}

fn render_result_table(app: &App) -> Element<'_, Message> {
    let current_search_context = &app.state.searches[app.state.current_search];
    // Don't show results if search is still in progress
    if !matches!(current_search_context.searching, SearchMode::None) {
        return column![].into();
    }

    // Collect all results for display
    let results = current_search_context.collect_results();
    let show_search_types =
        matches!(current_search_context.search_type, SearchType::Guess) || matches!(current_search_context.search_type, SearchType::Unknown);
    let is_string = current_search_context.search_type == SearchType::String;
    let string_len = current_search_context.search_value_text.len();
    let string_len_chars = current_search_context.search_value_text.chars().count();

    // Limit displayed results for performance
    const MAX_DISPLAY_RESULTS: usize = 1000;
    let total_results = results.len();
    let displayed_results = results.iter().take(MAX_DISPLAY_RESULTS);
    let is_truncated = total_results > MAX_DISPLAY_RESULTS;

    let table_header = if is_string {
        row![
            container(text(fl!(crate::LANGUAGE_LOADER, "address-heading")).size(14)).width(Length::Fixed(120.0)),
            container(text(fl!(crate::LANGUAGE_LOADER, "value-heading")).size(14)).width(Length::Fixed(120.0)),
        ]
    } else {
        row![
            container(text(fl!(crate::LANGUAGE_LOADER, "address-heading")).size(14)).width(Length::Fixed(120.0)),
            container(text(fl!(crate::LANGUAGE_LOADER, "value-heading")).size(14)).width(Length::Fixed(120.0)),
            if show_search_types {
                container(text(fl!(crate::LANGUAGE_LOADER, "datatype-heading")).size(14)).width(Length::Fixed(120.0))
            } else {
                container(text("")).width(Length::Fixed(0.0))
            },
            container(text(fl!(crate::LANGUAGE_LOADER, "freezed-heading")).size(14)).width(Length::Fill)
        ]
    }
    .padding(2)
    .spacing(5)
    .align_y(alignment::Alignment::Center);

    let table_rows = displayed_results.enumerate().map(|(i, result)| -> iced::Element<'_, Message> {
        let value_text = if is_string {
            let utf16_hint = result.search_type == SearchType::StringUtf16;
            read_string_from_process(
                app.state.pid as process_memory::Pid,
                result.addr,
                utf16_hint,
                if utf16_hint { string_len_chars * 2 } else { string_len },
            )
            .unwrap_or_default()
        } else if let Ok(handle) = (app.state.pid as process_memory::Pid).try_into_process_handle() {
            if let Ok(buf) = copy_address(result.addr, result.search_type.get_byte_length(), &handle) {
                let val = SearchValue(result.search_type, buf);
                val.to_string()
            } else {
                String::new()
            }
        } else {
            String::new()
        };
        if is_string {
            row![
                container(text(format!("0x{:X}", result.addr)).size(14)).width(Length::Fixed(120.0)),
                container(text(value_text)).width(Length::Fill),
                button(text(fl!(crate::LANGUAGE_LOADER, "edit-button"))).on_press(Message::OpenEditor(i)),
                button(text(fl!(crate::LANGUAGE_LOADER, "remove-button"))).on_press(Message::RemoveResult(i))
            ]
        } else {
            row![
                container(text(format!("0x{:X}", result.addr)).size(14)).width(Length::Fixed(120.0)),
                text_input("", &value_text)
                    .on_input(move |v| Message::ResultValueChanged(i, v))
                    .width(Length::Fixed(120.0)),
                if show_search_types {
                    container(text(result.search_type.get_description_text()).size(14)).width(Length::Fixed(120.0))
                } else {
                    container(text("")).width(Length::Fixed(0.0))
                },
                checkbox("", current_search_context.freezed_addresses.contains(&result.addr))
                    .on_toggle(move |_| Message::ToggleFreeze(i))
                    .size(14)
                    .width(Length::Fill),
                button(text(fl!(crate::LANGUAGE_LOADER, "edit-button"))).on_press(Message::OpenEditor(i)),
                button(text(fl!(crate::LANGUAGE_LOADER, "remove-button"))).on_press(Message::RemoveResult(i))
            ]
        }
        .padding(2)
        .spacing(5)
        .align_y(alignment::Alignment::Center)
        .into()
    });

    let mut table_content = vec![
        container(table_header)
            .style(|theme: &iced::Theme| container::Style {
                background: Some(theme.extended_palette().background.weak.color.into()),
                ..Default::default()
            })
            .into(),
    ];

    // Add truncation warning if needed
    if is_truncated {
        table_content.push(
            container(
                text(
                    fl!(
                        crate::LANGUAGE_LOADER,
                        "truncated-results-warning",
                        shown = MAX_DISPLAY_RESULTS,
                        total = total_results
                    )
                    .chars()
                    .filter(|c| c.is_ascii())
                    .collect::<String>(),
                )
                .size(12)
                .style(|theme: &iced::Theme| iced::widget::text::Style {
                    color: Some(theme.extended_palette().danger.base.color),
                }),
            )
            .padding(5)
            .into(),
        );
    }

    table_content.push(
        scrollable(column(table_rows.collect::<Vec<Element<'_, Message>>>()).spacing(5).padding(5))
            .height(Length::FillPortion(1))
            .into(),
    );

    column(table_content).spacing(0).into()
}

pub fn show_search_in_process_view(app: &App) -> Element<'_, Message> {
    use iced::widget::{button, column, container, horizontal_rule, row, text};

    let searches = &app.state.searches;
    let current_search = app.state.current_search;

    let search_table = column![
        // Table header
        container(
            text(fl!(crate::LANGUAGE_LOADER, "searches-heading"))
                .size(14)
                .style(|theme: &iced::Theme| iced::widget::text::Style {
                    color: Some(theme.palette().primary),
                })
        )
        .padding(5)
        .width(140.0),
        container(horizontal_rule(1)).width(120.0),
        // Table body
        scrollable(
            column(
                searches
                    .iter()
                    .enumerate()
                    .map(|(i, search)| -> Element<'_, Message> {
                        let is_selected = i == current_search;
                        let is_renaming = app.renaming_search_index == Some(i);

                        if is_renaming {
                            container(
                                // Show text input for renaming
                                text_input("", &app.rename_search_text)
                                    .on_input(Message::RenameSearchTextChanged)
                                    .on_submit(Message::ConfirmRenameSearch)
                                    .width(Length::Fixed(120.0))
                                    .padding(5),
                            )
                            .style(move |_theme: &iced::Theme| container::Style::default())
                            .into()
                        } else {
                            let mut close_button = button(text("×")).padding(2).style(move |theme: &iced::Theme, _| button::Style {
                                background: Some(iced::Color::TRANSPARENT.into()),
                                text_color: if i == 0 {
                                    iced::Color::from_rgb(0.5, 0.5, 0.5)
                                } else {
                                    theme.palette().danger
                                },
                                ..Default::default()
                            });

                            if i != 0 {
                                close_button = close_button.on_press(Message::CloseSearch(i));
                            }

                            row![
                                // Show normal button
                                button(
                                    row![container(text(search.description.to_string()).size(14)).width(Length::Fixed(120.0)).padding(5)]
                                        as iced::widget::Row<'_, Message>,
                                )
                                .style(move |theme: &iced::Theme, status: iced::widget::button::Status| {
                                    use iced::widget::button::Status;
                                    match status {
                                        Status::Hovered => button::Style {
                                            background: Some(theme.palette().primary.into()),
                                            border: iced::Border::default(),
                                            text_color: theme.palette().text,
                                            ..Default::default()
                                        },
                                        _ => {
                                            if is_selected {
                                                button::Style {
                                                    background: Some(theme.palette().primary.into()),
                                                    border: iced::Border::default(),
                                                    text_color: theme.palette().text,
                                                    ..Default::default()
                                                }
                                            } else {
                                                button::Style {
                                                    background: Some(iced::Color::TRANSPARENT.into()),
                                                    border: iced::Border::default(),
                                                    text_color: theme.palette().text,
                                                    ..Default::default()
                                                }
                                            }
                                        }
                                    }
                                })
                                .on_press(Message::SwitchSearch(i))
                                .width(Length::Fixed(120.0))
                                .padding(0),
                                close_button
                            ]
                            .into()
                        }
                    })
                    .collect::<Vec<Element<'_, Message>>>()
            )
            .spacing(0)
        )
        .height(Length::FillPortion(1))
    ]
    .spacing(5);

    let add_button = button(text(fl!(crate::LANGUAGE_LOADER, "add-search-button")))
        .on_press(Message::NewSearch)
        .padding(5);
    let rename_button = button(text(fl!(crate::LANGUAGE_LOADER, "rename-button")))
        .on_press(Message::RenameSearch)
        .padding(5);

    // Process selection dialog
    container(column![
        // Filter input
        row![
            text(fl!(crate::LANGUAGE_LOADER, "process-label")),
            container(
                text(format!("{} ({})", app.state.process_name, app.state.pid)).style(|theme: &iced::Theme| iced::widget::text::Style {
                    color: Some(theme.palette().primary)
                })
            )
            .width(Length::Fill),
            button(text(fl!(crate::LANGUAGE_LOADER, "close-button"))).on_press(Message::MainMenu).padding(5)
        ]
        .spacing(10)
        .padding(crate::DIALOG_PADDING)
        .align_y(alignment::Alignment::Center),
        horizontal_rule(1),
        row![
            column![search_table, column![add_button, rename_button].spacing(5).padding(10)].height(Length::Fill),
            vertical_rule(1),
            search_ui(app)
        ]
        .spacing(5)
        .padding(crate::DIALOG_PADDING),
    ])
    .into()
}

fn read_string_from_process(pid: process_memory::Pid, addr: usize, utf16le: bool, max_bytes: usize) -> Option<String> {
    let handle = pid.try_into_process_handle().ok()?;

    if utf16le {
        // Read up to 256 UTF-16 code units (512 bytes)
        let buf = copy_address(addr, max_bytes, &handle).ok()?;
        if buf.len() < 2 {
            return None;
        }
        // Truncate to even length
        let even_len = buf.len() & !1usize;
        let mut units = Vec::with_capacity(even_len / 2);
        let mut iter: std::slice::ChunksExact<'_, u8> = buf[..even_len].chunks_exact(2);
        for ch in &mut iter {
            let u = u16::from_le_bytes([ch[0], ch[1]]);
            if u == 0 {
                break; // NUL-terminated
            }
            units.push(u);
        }
        if units.is_empty() { None } else { Some(String::from_utf16_lossy(&units)) }
    } else {
        // Read up to 256 bytes and stop at NUL
        let buf = copy_address(addr, max_bytes, &handle).ok()?;
        let sbytes = &buf[..];
        if sbytes.is_empty() {
            return None;
        }
        match std::str::from_utf8(sbytes) {
            Ok(s) => Some(s.to_string()),
            Err(e) => {
                let valid = e.valid_up_to();
                if valid > 0 {
                    std::str::from_utf8(&sbytes[..valid]).ok().map(|s| s.to_string())
                } else {
                    // Fallback: lossy
                    Some(String::from_utf8_lossy(sbytes).to_string())
                }
            }
        }
    }
}
