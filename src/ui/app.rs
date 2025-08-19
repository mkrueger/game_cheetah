use std::{
    sync::atomic::Ordering,
    thread::sleep,
    time::{Duration, SystemTime},
};

use i18n_embed_fl::fl;
use iced::{
    Element, Length, Task, Theme, alignment, keyboard,
    widget::{button, checkbox, column, container, horizontal_rule, pick_list, progress_bar, row, scrollable, text, text_input, vertical_rule},
    window,
};
use process_memory::{PutAddress, TryIntoProcessHandle, copy_address};

use crate::{DIALOG_PADDING, FreezeMessage, GameCheetahEngine, MessageCommand, SearchMode, SearchType, SearchValue, message::Message};
use crate::ui::process_selection::{ProcessSortColumn, SortDirection};

#[derive(Default, PartialEq, Debug, Clone, Copy)]
pub enum AppState {
    #[default]
    MainWindow,
    ProcessSelection,
    About,
    InProcess,
    MemoryEditor,
}

#[derive(Default)]
pub struct App {
    pub app_state: AppState,
    pub state: GameCheetahEngine,

    pub renaming_search_index: Option<usize>,
    rename_search_text: String,

    memory_editor: super::memory_editor::MemoryEditor,

    pub process_sort_column: ProcessSortColumn,
    pub process_sort_direction: SortDirection,
}

impl App {
    pub fn title(&self) -> String {
        format!("{} {}", crate::APP_NAME, crate::VERSION)
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
            Message::OpenGitHub => {
                if let Err(err) = webbrowser::open("https://github.com/mkrueger/game_cheetah") {
                    println!("Failed to open GitHub page: {}", err);
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
                        sleep(Duration::from_millis(2000));
                    },
                    |_| Message::TickProcess,
                )
            }
            Message::TickProcess => iced::Task::perform(
                async {
                    sleep(Duration::from_millis(2000));
                },
                |_| Message::TickProcess,
            ),
            Message::NewSearch => {
                self.state.new_search();
                Task::none()
            }
            Message::CloseSearch(index) => {
                if self.state.searches.is_empty() {
                    return Task::none();
                }
                self.state.remove_freezes(index);
                self.state.searches.remove(index);
                if self.state.current_search >= index {
                    self.state.current_search -= 1;
                }
                Task::none()
            }

            Message::RenameSearch => {
                if let Some(search) = self.state.searches.get(self.state.current_search) {
                    self.rename_search_text = search.description.clone();
                    self.renaming_search_index = Some(self.state.current_search);
                }
                Task::none()
            }
            Message::RenameSearchTextChanged(text) => {
                self.rename_search_text = text;
                Task::none()
            }
            Message::ConfirmRenameSearch => {
                if let Some(index) = self.renaming_search_index {
                    if let Some(search) = self.state.searches.get_mut(index) {
                        search.description = self.rename_search_text.clone();
                    }
                }
                self.renaming_search_index = None;
                self.rename_search_text.clear();
                Task::none()
            }

            Message::CancelRenameSearch => {
                self.renaming_search_index = None;
                self.rename_search_text.clear();
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
                            // Check the actual result count, not just search_results
                            let has_results = current_search.result_count.load(Ordering::Relaxed) > 0 || current_search.get_result_count() > 0;

                            if !has_results {
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

                if current_search_context.search_complete.load(Ordering::SeqCst) {
                    // Collect any final results before marking as complete
                    let final_results = current_search_context.collect_results();

                    // If we have results but they're not in the channel anymore, put them back
                    if current_search_context.get_result_count() > 0 && final_results.is_empty() {
                        // The results were already collected, so we need to ensure they stay available
                        current_search_context.invalidate_cache();
                    }

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
                        search_context.set_cached_results(old);
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
                        // Collect all results
                        let results = current_search.collect_results();

                        if index < results.len() {
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
                    // Collect all results
                    let results = search_context.collect_results();

                    if index < results.len() {
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
                }
                Task::none()
            }
            Message::RemoveResult(index) => {
                if let Some(search_context) = self.state.searches.get_mut(self.state.current_search) {
                    // Collect all results - now returns Arc<Vec<SearchResult>>
                    let results = search_context.collect_results();

                    if index < results.len() {
                        // Remove any freeze for this address before removing it
                        let result = &results[index];
                        if search_context.freezed_addresses.contains(&result.addr) {
                            search_context.freezed_addresses.remove(&result.addr);
                            self.state
                                .freeze_sender
                                .send(FreezeMessage::from_addr(MessageCommand::Unfreeze, result.addr))
                                .unwrap_or_default();
                        }

                        // Save current results to old_results for undo functionality
                        // Need to clone the underlying Vec here since we're modifying it
                        search_context.old_results.push((*results).clone());

                        // Create a new vector without the removed item
                        let mut new_results = (*results).clone();
                        new_results.remove(index);

                        // Update the cached results
                        search_context.set_cached_results(new_results);
                    }
                }
                Task::none()
            }
            Message::OpenEditor(index) => {
                if let Some(search_context) = self.state.searches.get_mut(self.state.current_search) {
                    // Collect all results
                    let results = search_context.collect_results();

                    if index < results.len() {
                        let result = &results[index];
                        self.state.edit_address = result.addr;
                        self.memory_editor.initalize(result.addr, result.search_type);
                        self.app_state = AppState::MemoryEditor;
                    }
                }
                Task::none()
            }
            Message::CloseMemoryEditor => {
                self.app_state = AppState::InProcess;
                Task::none()
            }
            Message::MemoryEditorAddressChanged(text) => {
                self.memory_editor.address_text = text;
                Task::none()
            }

            Message::MemoryEditorJumpToAddress => {
                // Parse the address from the text input
                let text = self.memory_editor.address_text.trim();
                let text = text.strip_prefix("0x").unwrap_or(text);

                if let Ok(new_address) = u64::from_str_radix(text, 16) {
                    self.state.edit_address = new_address as usize;
                }
                Task::none()
            }

            Message::MemoryEditorCellChanged(offset, value) => {
                // Validate and update the byte at the given offset
                if value.len() <= 2 {
                    if let Ok(byte_value) = u8::from_str_radix(&value, 16) {
                        if let Ok(handle) = (self.state.pid as process_memory::Pid).try_into_process_handle() {
                            let address = self.state.edit_address + offset;
                            handle.put_address(address, &[byte_value]).unwrap_or_default();
                        }
                    }
                }
                Task::none()
            }

            Message::MemoryEditorScroll(rows) => {
                const BYTES_PER_ROW: usize = 16;
                let offset = (rows * BYTES_PER_ROW as i32) as isize;
                let new_address = (self.state.edit_address as isize + offset).max(0) as usize;
                self.state.edit_address = new_address;
                self.memory_editor.address_text = format!("{:X}", new_address);
                Task::none()
            }

            Message::MemoryEditorPageUp => {
                const BYTES_PER_ROW: usize = 16;
                const ROWS_PER_PAGE: usize = 16;
                let page_size = BYTES_PER_ROW * ROWS_PER_PAGE;
                let new_address = self.state.edit_address.saturating_sub(page_size);
                self.state.edit_address = new_address;
                self.memory_editor.address_text = format!("{:X}", new_address);
                Task::none()
            }

            Message::MemoryEditorPageDown => {
                const BYTES_PER_ROW: usize = 16;
                const ROWS_PER_PAGE: usize = 16;
                let page_size = BYTES_PER_ROW * ROWS_PER_PAGE;
                let new_address = self.state.edit_address.saturating_add(page_size);
                self.state.edit_address = new_address;
                self.memory_editor.address_text = format!("{:X}", new_address);
                Task::none()
            }

            Message::MemoryEditorMoveCursor(row_delta, col_delta) => {
                self.state.edit_address = self.memory_editor.move_cursor(self.state.edit_address, row_delta, col_delta);
                Task::none()
            }

            Message::MemoryEditorSetCursor(row, col) => {
                self.memory_editor.set_cursor(row, col);
                Task::none()
            }
            Message::MemoryEditorBeginEdit => Task::none(),
            Message::MemoryEditorEndEdit => Task::none(),
            Message::MemoryEditorEditHex(hex_digit) => {
                self.memory_editor.edit_hex(self.state.edit_address, self.state.pid, hex_digit);

                Task::none()
            }
            Message::SortProcesses(column) => {
                if self.process_sort_column == column {
                    // Toggle direction if clicking same column
                    self.process_sort_direction = match self.process_sort_direction {
                        SortDirection::Ascending => SortDirection::Descending,
                        SortDirection::Descending => SortDirection::Ascending,
                    };
                } else {
                    // New column, default to ascending
                    self.process_sort_column = column;
                    self.process_sort_direction = SortDirection::Ascending;
                }
                iced::Task::none()
            }
        }
    }

    pub fn theme(&self) -> Theme {
        Theme::Dracula.clone()
    }

    pub fn view(&self) -> Element<'_, Message> {
        match self.app_state {
            AppState::MainWindow => crate::main_window::view_main_window(self),
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
                .padding(crate::DIALOG_PADDING),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
            AppState::ProcessSelection => crate::process_selection::view_process_selection(self),
            AppState::InProcess => self.show_search_in_process_view(),
            AppState::MemoryEditor => self.memory_editor.show_memory_editor(self),
        }
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
        let search_results = current_search_context.get_result_count();
        let is_search_complete = current_search_context.search_complete.load(Ordering::SeqCst);
        let show_error = !current_search_context.search_value_text.is_empty()
            && current_search_context
                .search_type
                .from_string(&current_search_context.search_value_text)
                .is_err();
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
            } else {
                if !is_search_complete {
                    row![{
                        let b = button(text(fl!(crate::LANGUAGE_LOADER, "initial-search-button")));
                        if show_error { b } else { b.on_press(Message::Search) }
                    }]
                    .spacing(10.0)
                } else {
                    row![
                        {
                            let b = button(text(fl!(crate::LANGUAGE_LOADER, "update-button")));
                            if show_error { b } else { b.on_press(Message::Search) }
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
        // Don't show results if search is still in progress
        if !matches!(current_search_context.searching, SearchMode::None) {
            return column![].into();
        }

        // Collect all results for display
        let results = current_search_context.collect_results();
        let show_search_types = matches!(current_search_context.search_type, SearchType::Guess);

        let table_header = row![
            container(text(fl!(crate::LANGUAGE_LOADER, "address-heading")).size(14)).width(Length::Fixed(120.0)),
            container(text(fl!(crate::LANGUAGE_LOADER, "value-heading")).size(14)).width(Length::Fixed(120.0)),
            if show_search_types {
                container(text(fl!(crate::LANGUAGE_LOADER, "datatype-heading")).size(14)).width(Length::Fixed(120.0))
            } else {
                container(text("")).width(Length::Fixed(0.0))
            },
            container(text(fl!(crate::LANGUAGE_LOADER, "freezed-heading")).size(14)).width(Length::Fill)
        ]
        .padding(2)
        .spacing(5)
        .align_y(alignment::Alignment::Center);

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
            .padding(2)
            .spacing(5)
            .align_y(alignment::Alignment::Center)
            .into()
        });

        column![
            container(table_header).style(|theme: &iced::Theme| {
                container::Style {
                    background: Some(theme.extended_palette().background.weak.color.into()),
                    ..Default::default()
                }
            }),
            scrollable(column(table_rows.collect::<Vec<Element<'_, Message>>>()).spacing(5).padding(5)).height(Length::FillPortion(1))
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
                            let is_renaming = self.renaming_search_index == Some(i);

                            if is_renaming {
                                container(
                                    // Show text input for renaming
                                    text_input("", &self.rename_search_text)
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
                    text(format!("{} ({})", self.state.process_name, self.state.pid)).style(|theme: &iced::Theme| iced::widget::text::Style {
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
                self.search_ui()
            ]
            .spacing(5)
            .padding(DIALOG_PADDING),
        ])
        .into()
    }

    pub fn subscription(&self) -> iced::Subscription<Message> {
        // Only subscribe to keyboard events when in memory editor mode
        if matches!(self.app_state, AppState::MemoryEditor) {
            keyboard::on_key_press(|key, _modifiers| match key {
                keyboard::Key::Named(keyboard::key::Named::ArrowUp) => Some(Message::MemoryEditorMoveCursor(-1, 0)),
                keyboard::Key::Named(keyboard::key::Named::ArrowDown) => Some(Message::MemoryEditorMoveCursor(1, 0)),
                keyboard::Key::Named(keyboard::key::Named::ArrowLeft) => Some(Message::MemoryEditorMoveCursor(0, -1)),
                keyboard::Key::Named(keyboard::key::Named::ArrowRight) => Some(Message::MemoryEditorMoveCursor(0, 1)),
                keyboard::Key::Named(keyboard::key::Named::PageUp) => Some(Message::MemoryEditorPageUp),
                keyboard::Key::Named(keyboard::key::Named::PageDown) => Some(Message::MemoryEditorPageDown),
                keyboard::Key::Named(keyboard::key::Named::Tab) => Some(Message::MemoryEditorMoveCursor(0, 1)),
                keyboard::Key::Named(keyboard::key::Named::Enter) => Some(Message::MemoryEditorBeginEdit),
                keyboard::Key::Named(keyboard::key::Named::Escape) => Some(Message::MemoryEditorEndEdit),
                keyboard::Key::Character(c) => match c.as_str() {
                    "0" => Some(Message::MemoryEditorEditHex(0)),
                    "1" => Some(Message::MemoryEditorEditHex(1)),
                    "2" => Some(Message::MemoryEditorEditHex(2)),
                    "3" => Some(Message::MemoryEditorEditHex(3)),
                    "4" => Some(Message::MemoryEditorEditHex(4)),
                    "5" => Some(Message::MemoryEditorEditHex(5)),
                    "6" => Some(Message::MemoryEditorEditHex(6)),
                    "7" => Some(Message::MemoryEditorEditHex(7)),
                    "8" => Some(Message::MemoryEditorEditHex(8)),
                    "9" => Some(Message::MemoryEditorEditHex(9)),
                    "a" | "A" => Some(Message::MemoryEditorEditHex(10)),
                    "b" | "B" => Some(Message::MemoryEditorEditHex(11)),
                    "c" | "C" => Some(Message::MemoryEditorEditHex(12)),
                    "d" | "D" => Some(Message::MemoryEditorEditHex(13)),
                    "e" | "E" => Some(Message::MemoryEditorEditHex(14)),
                    "f" | "F" => Some(Message::MemoryEditorEditHex(15)),
                    _ => None,
                },
                _ => None,
            })
        } else if self.renaming_search_index.is_some() {
            // Only subscribe to ESC when renaming
            keyboard::on_key_press(|key, _modifiers| match key {
                keyboard::Key::Named(keyboard::key::Named::Escape) => Some(Message::CancelRenameSearch),
                _ => None,
            })
        } else {
            iced::Subscription::none()
        }
    }
}
