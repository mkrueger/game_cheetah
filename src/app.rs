use std::{
    sync::{atomic::Ordering, Arc, Mutex},
    thread::sleep,
    time::{Duration, SystemTime},
};

use i18n_embed_fl::fl;
use iced::{
    alignment,
    widget::{button, checkbox, column, container, pick_list, progress_bar, row, scrollable, text, text_input, vertical_rule},
    window, Element, Length, Task, Theme,
};
use process_memory::{copy_address, PutAddress, TryIntoProcessHandle};

use crate::{FreezeMessage, GameCheetahEngine, MessageCommand, ProcessInfo, SearchMode, SearchType, SearchValue};

const APP_NAME: &str = "Game Cheetah";
const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone)]
pub enum Message {
    Attach,
    About,
    MainMenu,
    Discuss,
    ReportBug,
    Exit,
    FilterChanged(String),
    SelectProcess(ProcessInfo),
    TickProcess,
    SwitchSearch(usize),
    SearchValueChanged(String),
    AddSearch,
    RemoveSearch,

    SwitchSearchType(SearchType),
    Search,
    Tick,
    ClearResults,
    ToggleShowResult,
    Undo,
    ResultValueChanged(usize, String),
    ToggleFreeze(usize),
}

#[derive(Default)]
pub enum AppState {
    #[default]
    MainWindow,
    ProcessSelection,
    About,
    InProcess,
}

#[derive(Default)]
pub struct App {
    app_state: AppState,
    state: GameCheetahEngine,
}

const DIALOG_PADDING: u16 = 20;

impl App {
    pub fn title(&self) -> String {
        format!("{APP_NAME} {VERSION}")
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        if SystemTime::now().duration_since(self.state.last_process_update).unwrap().as_millis() > 500 {
            self.state.update_process_data();
        }

        match message {
            Message::Attach => {
                self.app_state = AppState::ProcessSelection;
                Task::none()
            }
            Message::MainMenu => {
                self.app_state = AppState::MainWindow;
                Task::none()
            }
            Message::About => {
                self.app_state = AppState::About;
                Task::none()
            }
            Message::Discuss => {
                if let Err(err) = webbrowser::open("https://github.com/mkrueger/game_cheetah/discussions") {
                    println!("Failed to open discussion page: {}", err);
                }
                Task::none()
            }
            Message::ReportBug => {
                if let Err(err) = webbrowser::open("https://github.com/mkrueger/game_cheetah/issues/new") {
                    println!("Failed to open bug report page: {}", err);
                }
                Task::none()
            }
            Message::Exit => window::get_latest().and_then(window::close),
            Message::FilterChanged(filter) => {
                self.state.process_filter = filter;
                Task::none()
            }
            Message::SelectProcess(process) => {
                self.state.select_process(&process);
                self.app_state = AppState::InProcess;
                self.state.process_filter.clear();
                iced::Task::perform(
                    async {
                        sleep(Duration::from_millis(500));
                    },
                    |_| Message::TickProcess,
                )
            }
            Message::TickProcess => iced::Task::perform(
                async {
                    sleep(Duration::from_millis(500));
                },
                |_| Message::TickProcess,
            ),
            Message::AddSearch => {
                self.state.new_search();
                Task::none()
            }
            Message::RemoveSearch => {
                if self.state.searches.is_empty() {
                    return Task::none();
                }
                self.state.remove_freezes(self.state.current_search);
                self.state.searches.remove(self.state.current_search);
                if self.state.current_search > 0 {
                    self.state.current_search -= 1;
                }
                Task::none()
            }
            Message::SwitchSearch(index) => {
                if index < self.state.searches.len() {
                    self.state.current_search = index;
                }
                Task::none()
            }
            Message::SearchValueChanged(value) => {
                if let Some(current_search) = self.state.searches.get_mut(self.state.current_search) {
                    current_search.search_value_text = value;
                }
                Task::none()
            }

            Message::SwitchSearchType(search_type) => {
                if let Some(current_search) = self.state.searches.get_mut(self.state.current_search) {
                    current_search.search_type = search_type;
                }
                Task::none()
            }
            Message::Search => {
                let search_index = self.state.current_search;
                if let Some(current_search) = self.state.searches.get_mut(search_index) {
                    if current_search.search_value_text.is_empty() {
                        return Task::none();
                    }
                    let search_type = current_search.search_type;
                    match search_type.from_string(&current_search.search_value_text) {
                        Ok(_search_value) => {
                            let len = self.state.searches.get(search_index).unwrap().results.lock().unwrap().len();
                            if len == 0 {
                                self.state.initial_search(search_index);
                            } else {
                                self.state.filter_searches(search_index);
                            }
                        }
                        Err(err) => {
                            println!("Error parsing search value: {}", err);
                        }
                    }
                }
                Task::done(Message::Tick)
            }
            Message::Tick => {
                // If searching, keep scheduling ticks
                let current_search_context = &mut self.state.searches[self.state.current_search];
                let current_bytes = current_search_context.current_bytes.load(Ordering::Acquire);
                if current_bytes >= current_search_context.total_bytes {
                    current_search_context.search_results = current_search_context.results.lock().unwrap().len() as i64;
                    current_search_context.searching = SearchMode::None;
                }

                if !matches!(current_search_context.searching, SearchMode::None) {
                    sleep(Duration::from_millis(100));
                    return Task::done(Message::Tick);
                }
                Task::none()
            }
            Message::Undo => {
                if let Some(search_context) = self.state.searches.get_mut(self.state.current_search) {
                    if let Some(old) = search_context.old_results.pop() {
                        search_context.search_results = old.len() as i64;
                        search_context.results = Arc::new(Mutex::new(old));
                    }
                }
                Task::none()
            }
            Message::ClearResults => {
                if let Some(search_context) = self.state.searches.get_mut(self.state.current_search) {
                    search_context.clear_results(&self.state.freeze_sender);
                }
                Task::none()
            }
            Message::ToggleShowResult => {
                self.state.show_results = !self.state.show_results;
                Task::none()
            }
            Message::ResultValueChanged(index, value_text) => {
                if let Ok(handle) = (self.state.pid as process_memory::Pid).try_into_process_handle() {
                    if let Some(current_search) = self.state.searches.get_mut(self.state.current_search) {
                        let results: std::sync::MutexGuard<'_, Vec<crate::SearchResult>> = current_search.results.lock().unwrap();
                        let result = results[index];
                        if let Ok(value) = result.search_type.from_string(&value_text) {
                            handle.put_address(result.addr, &value.1).unwrap_or_default();
                            if current_search.freezed_addresses.contains(&result.addr) {
                                self.state
                                    .freeze_sender
                                    .send(FreezeMessage {
                                        msg: crate::MessageCommand::Freeze,
                                        addr: result.addr,
                                        value,
                                    })
                                    .unwrap_or_default();
                            }
                        } else {
                            println!("Invalid value for result at index {}: {}", index, value_text);
                        }
                    }
                }
                Task::none()
            }
            Message::ToggleFreeze(index) => {
                if let Some(search_context) = self.state.searches.get_mut(self.state.current_search) {
                    let results: std::sync::MutexGuard<'_, Vec<crate::SearchResult>> = search_context.results.lock().unwrap();
                    let result = &results[index];
                    let b = !search_context.freezed_addresses.contains(&result.addr);
                    if b {
                        search_context.freezed_addresses.insert(result.addr);
                        if let Ok(handle) = (self.state.pid as process_memory::Pid).try_into_process_handle() {
                            if let Ok(buf) = copy_address(result.addr, result.search_type.get_byte_length(), &handle) {
                                self.state
                                    .freeze_sender
                                    .send(FreezeMessage {
                                        msg: MessageCommand::Freeze,
                                        addr: result.addr,
                                        value: SearchValue(result.search_type, buf),
                                    })
                                    .unwrap_or_default();
                            }
                        }
                    } else {
                        search_context.freezed_addresses.remove(&(result.addr));
                        self.state
                            .freeze_sender
                            .send(FreezeMessage::from_addr(MessageCommand::Unfreeze, result.addr))
                            .unwrap_or_default();
                    }
                }
                Task::none()
            }
        }
    }

    pub fn theme(&self) -> Theme {
        Theme::Dracula.clone()
    }

    pub fn view(&self) -> Element<'_, Message> {
        match self.app_state {
            AppState::MainWindow => self.view_main_window(),
            AppState::About => container(
                column![
                    container(text(fl!(crate::LANGUAGE_LOADER, "about-dialog-heading")).size(24))
                        .width(Length::Fill)
                        .align_x(alignment::Alignment::Center),
                    text(fl!(crate::LANGUAGE_LOADER, "about-dialog-description")).size(16),
                    container(
                        button(text(fl!(crate::LANGUAGE_LOADER, "close-button")))
                            .on_press(Message::MainMenu)
                            .padding(10)
                    )
                    .width(Length::Fill)
                    .align_x(alignment::Alignment::Center)
                ]
                .spacing(20)
                .padding(DIALOG_PADDING),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
            AppState::ProcessSelection => self.view_process_selection(),
            AppState::InProcess => self.show_search_in_process_view(),
        }
    }

    fn view_main_window(&self) -> Element<'_, Message> {
        container(
            column![
                button(text(fl!(crate::LANGUAGE_LOADER, "attach-button")).size(24)).on_press(Message::Attach),
                button(text(fl!(crate::LANGUAGE_LOADER, "discuss-button"))).on_press(Message::Discuss),
                button(text(fl!(crate::LANGUAGE_LOADER, "bug-button"))).on_press(Message::ReportBug),
                button(text(fl!(crate::LANGUAGE_LOADER, "about-button"))).on_press(Message::About),
                button(text(fl!(crate::LANGUAGE_LOADER, "quit-button"))).on_press(Message::Exit)
            ]
            .spacing(10)
            .align_x(alignment::Alignment::Center),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(alignment::Alignment::Center)
        .align_y(alignment::Alignment::Center)
        .into()
    }

    fn view_process_selection(&self) -> Element<'_, Message> {
        let filter = self.state.process_filter.to_ascii_uppercase();

        // Process selection dialog
        container(
            column![
                // Filter input row with Clear and Close buttons
                row![
                    text(fl!(crate::LANGUAGE_LOADER, "process-label")),
                    text_input(&fl!(crate::LANGUAGE_LOADER, "filter-processes-hint"), &self.state.process_filter)
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
                        self.state
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
            .padding(DIALOG_PADDING),
        )
        .into()
    }

    fn search_ui(&self) -> Element<'_, Message> {
        let search_types = vec![
            SearchType::Guess,
            SearchType::Short,
            SearchType::Int,
            SearchType::Int64,
            SearchType::Float,
            SearchType::Double,
        ];
        let current_search_context = &self.state.searches[self.state.current_search];
        let selected_type = current_search_context.search_type;
        let value_text = &current_search_context.search_value_text;
        let search_results = current_search_context.search_results;
        let show_error = !current_search_context.search_value_text.is_empty()
            && current_search_context
                .search_type
                .from_string(&current_search_context.search_value_text)
                .is_err();
        let auto_show_treshold = 20;

        column![
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
            .align_y(alignment::Alignment::Center),
            if !matches!(current_search_context.searching, SearchMode::None) {
                let current_bytes = current_search_context.current_bytes.load(Ordering::Acquire);
                //let percentage = current_bytes as f32 / current_search_context.total_bytes as f32;
                row![
                    progress_bar(0.0..=current_search_context.total_bytes as f32, current_bytes as f32).width(Length::Fixed(280.0)),
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
            } else {
                if search_results < 0 {
                    row![{
                        let b = button(text(fl!(crate::LANGUAGE_LOADER, "initial-search-button")));
                        if show_error {
                            b
                        } else {
                            b.on_press(Message::Search)
                        }
                    }]
                    .spacing(10.0)
                } else {
                    row![
                        {
                            let b = button(text(fl!(crate::LANGUAGE_LOADER, "update-button")));
                            if show_error {
                                b
                            } else {
                                b.on_press(Message::Search)
                            }
                        },
                        button(text(fl!(crate::LANGUAGE_LOADER, "clear-button"))).on_press(Message::ClearResults),
                        button(text(fl!(crate::LANGUAGE_LOADER, "undo-button"))).on_press(Message::Undo),
                        if search_results >= auto_show_treshold {
                            if self.state.show_results {
                                button(text(fl!(crate::LANGUAGE_LOADER, "hide-results-button"))).on_press(Message::ToggleShowResult)
                            } else {
                                button(text(fl!(crate::LANGUAGE_LOADER, "show-results-button"))).on_press(Message::ToggleShowResult)
                            }
                        } else {
                            // Return an invisible button or a placeholder to match the type
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
                }
            },
            if search_results > 0 && search_results < auto_show_treshold || self.state.show_results {
                self.render_result_table()
            } else {
                // Return an empty element as a placeholder
                column![].into()
            }
        ]
        .spacing(10)
        .padding(10)
        .into()
    }

    fn render_result_table(&self) -> Element<'_, Message> {
        let current_search_context = &self.state.searches[self.state.current_search];
        let results = current_search_context.results.lock().unwrap();
        let show_search_types = matches!(current_search_context.search_type, SearchType::Guess);

        let table_header = row![
            container(text(fl!(crate::LANGUAGE_LOADER, "address-heading")).size(14))
                .width(Length::Fixed(120.0))
                .padding(5),
            container(text(fl!(crate::LANGUAGE_LOADER, "value-heading")).size(14))
                .width(Length::Fixed(120.0))
                .padding(5),
            if show_search_types {
                container(text(fl!(crate::LANGUAGE_LOADER, "datatype-heading")).size(14))
                    .width(Length::Fixed(120.0))
                    .padding(5)
            } else {
                container(text("")).width(Length::Fixed(0.0))
            },
            container(text(fl!(crate::LANGUAGE_LOADER, "freezed-heading")).size(14))
                .width(Length::Fill)
                .padding(5),
        ];

        let table_rows = results.iter().enumerate().map(|(i, result)| -> iced::Element<'_, Message> {
            let value_text = if let Ok(handle) = (self.state.pid as process_memory::Pid).try_into_process_handle() {
                if let Ok(buf) = copy_address(result.addr, result.search_type.get_byte_length(), &handle) {
                    let val = SearchValue(result.search_type, buf);
                    val.to_string()
                } else {
                    String::new()
                }
            } else {
                String::new()
            };
            row![
                container(text(format!("0x{:X}", result.addr)).size(14)).width(Length::Fixed(120.0)).padding(5),
                text_input("", &value_text)
                    .on_input(move |v| Message::ResultValueChanged(i, v))
                    .width(Length::Fixed(120.0))
                    .padding(5),
                if show_search_types {
                    container(text(result.search_type.get_description_text()).size(14))
                        .width(Length::Fixed(120.0))
                        .padding(5)
                } else {
                    container(text("")).width(Length::Fixed(0.0))
                },
                checkbox("", current_search_context.freezed_addresses.contains(&result.addr))
                    .on_toggle(move |_| Message::ToggleFreeze(i))
                    .size(14)
                    .width(Length::Fill)
            ]
            .into()
        });

        column![
            container(table_header).style(|theme: &iced::Theme| {
                container::Style {
                    background: Some(theme.extended_palette().background.weak.color.into()),
                    ..Default::default()
                }
            }),
            scrollable(column(table_rows.collect::<Vec<Element<'_, Message>>>())).height(Length::FillPortion(1))
        ]
        .spacing(0)
        .into()
    }

    fn show_search_in_process_view(&self) -> Element<'_, Message> {
        use iced::widget::{button, column, container, horizontal_rule, row, text};

        let searches = &self.state.searches;
        let current_search = self.state.current_search;

        let search_table = column![
            // Table header
            container(row![container(text(fl!(crate::LANGUAGE_LOADER, "searches-heading")).size(14))
                .width(Length::Fixed(120.0))
                .padding(5),])
            .style(|theme: &iced::Theme| {
                container::Style {
                    background: Some(theme.extended_palette().background.weak.color.into()),
                    ..Default::default()
                }
            }),
            // Table body
            scrollable(
                column(
                    searches
                        .iter()
                        .enumerate()
                        .map(|(i, search)| {
                            let is_selected = i == current_search;
                            container(
                                button(row![container(text(search.description.to_string()).size(14))
                                    .width(Length::Fixed(120.0))
                                    .padding(5),])
                                .style(move |theme: &iced::Theme, status: iced::widget::button::Status| {
                                    use iced::widget::button::Status;
                                    match status {
                                        Status::Hovered => button::Style {
                                            background: Some(theme.palette().primary.into()), // highlight color
                                            border: iced::Border::default(),
                                            text_color: theme.palette().text,
                                            ..Default::default()
                                        },
                                        _ => {
                                            if is_selected {
                                                button::Style {
                                                    background: Some(theme.palette().primary.into()), // selected color
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
                            )
                            .style(move |_theme: &iced::Theme| container::Style::default())
                            .into()
                        })
                        .collect::<Vec<Element<'_, Message>>>()
                )
                .spacing(0)
            )
            .height(Length::FillPortion(1))
        ]
        .spacing(5);

        let add_button = button("+").on_press(Message::AddSearch).padding(5);

        let remove_button: button::Button<'_, Message> = button("-").on_press(Message::RemoveSearch).padding(5);

        // Process selection dialog
        container(column![
            // Filter input
            row![
                text(fl!(crate::LANGUAGE_LOADER, "process-label")),
                container(
                    text(format!("{} ({})", self.state.process_name, self.state.pid)).style(|theme: &iced::Theme| iced::widget::text::Style {
                        color: Some(theme.palette().primary)
                    })
                )
                .width(Length::Fill),
                button(text(fl!(crate::LANGUAGE_LOADER, "close-button"))).on_press(Message::MainMenu).padding(5)
            ]
            .spacing(10)
            .padding(DIALOG_PADDING)
            .align_y(alignment::Alignment::Center),
            horizontal_rule(1),
            row![
                column![search_table, row![add_button, remove_button].spacing(5).padding(10)].height(Length::Fill),
                vertical_rule(1),
                self.search_ui()
            ]
            .spacing(5)
            .padding(DIALOG_PADDING),
        ])
        .into()
    }
}
