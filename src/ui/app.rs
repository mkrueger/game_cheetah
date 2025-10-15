use std::{
    sync::atomic::Ordering,
    thread::sleep,
    time::{Duration, SystemTime},
};

use i18n_embed_fl::fl;
use iced::{
    Element, Length, Task, Theme, alignment, keyboard,
    widget::{button, column, container, text},
    window,
};
use process_memory::{PutAddress, TryIntoProcessHandle, copy_address};

use crate::{FreezeMessage, GameCheetahEngine, MessageCommand, SearchMode, SearchValue, message::Message};
use crate::{
    SearchType, UnknownComparison,
    ui::process_selection::{ProcessSortColumn, SortDirection},
};

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
    pub rename_search_text: String,

    memory_editor: super::memory_editor::MemoryEditor,

    pub process_sort_column: ProcessSortColumn,
    pub process_sort_direction: SortDirection,
}

impl App {
    pub fn title(&self) -> String {
        format!("{} {}", crate::APP_NAME, crate::VERSION)
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        if self.app_state == AppState::ProcessSelection && SystemTime::now().duration_since(self.state.last_process_update).unwrap().as_millis() > 500 {
            self.state.update_process_data();
        }
        // Check and update search modes for all searches
        for search_context in &mut self.state.searches {
            search_context.update_search_mode();
        }

        match message {
            Message::Attach => {
                self.state.update_process_data();
                self.app_state = AppState::ProcessSelection;
                Task::none()
            }
            Message::MainMenu => {
                self.app_state = AppState::MainWindow;
                self.state = GameCheetahEngine::default();
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
            Message::Exit => window::latest().and_then(window::close),
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
                self.state.remove_freezes(self.state.current_search);
                if let Some(current_search) = self.state.searches.get_mut(self.state.current_search) {
                    current_search.search_type = search_type;
                    current_search.clear_results(&self.state.freeze_sender);
                }
                Task::none()
            }
            Message::Search => {
                let search_index = self.state.current_search;
                if let Some(current_search) = self.state.searches.get_mut(search_index) {
                    let search_type = current_search.search_type;
                    if current_search.search_type == SearchType::Unknown {
                        self.state.take_memory_snapshot(self.state.current_search);
                        return Task::none();
                    }
                    if current_search.search_value_text.is_empty() {
                        return Task::none();
                    }
                    match search_type.from_string(&current_search.search_value_text) {
                        Ok(_search_value) => {
                            // Check the actual result count, not just search_results
                            let has_results = current_search.get_result_count() > 0;

                            if !has_results || current_search.search_type == SearchType::String {
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
                            let result = &results[index];
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

            Message::UnknownSearchDecrease => {
                if let Some(ctx) = self.state.searches.get_mut(self.state.current_search) {
                    ctx.unknown_comparison = Some(UnknownComparison::Decreased);
                }
                self.state.unknown_search_compare(self.state.current_search, UnknownComparison::Decreased);
                Task::none()
            }

            Message::UnknownSearchIncrease => {
                if let Some(ctx) = self.state.searches.get_mut(self.state.current_search) {
                    ctx.unknown_comparison = Some(UnknownComparison::Increased);
                }
                self.state.unknown_search_compare(self.state.current_search, UnknownComparison::Increased);
                Task::none()
            }
            Message::UnknownSearchChanged => {
                if let Some(ctx) = self.state.searches.get_mut(self.state.current_search) {
                    ctx.unknown_comparison = Some(UnknownComparison::Changed);
                }
                self.state.unknown_search_compare(self.state.current_search, UnknownComparison::Changed);
                Task::none()
            }
            Message::UnknownSearchUnchanged => {
                if let Some(ctx) = self.state.searches.get_mut(self.state.current_search) {
                    ctx.unknown_comparison = Some(UnknownComparison::Unchanged);
                }
                self.state.unknown_search_compare(self.state.current_search, UnknownComparison::Unchanged);
                Task::none()
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
            AppState::InProcess => crate::in_process_view::show_search_in_process_view(self),
            AppState::MemoryEditor => self.memory_editor.show_memory_editor(self),
        }
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
