use std::{
    sync::atomic::Ordering,
    thread::sleep,
    time::{Duration, SystemTime},
};

use i18n_embed_fl::fl;
use iced::{
    Element, Length, Task, Theme, alignment,
    border::Radius,
    keyboard,
    widget::{button, checkbox, column, container, horizontal_rule, pick_list, progress_bar, row, scrollable, text, text_input, vertical_rule},
    window,
};
use process_memory::{PutAddress, TryIntoProcessHandle, copy_address};

use crate::{FreezeMessage, GameCheetahEngine, MessageCommand, ProcessInfo, SearchMode, SearchType, SearchValue};
use crossbeam_channel;

const APP_NAME: &str = "Game Cheetah";
const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone)]
pub enum Message {
    Attach,
    About,
    MainMenu,
    Discuss,
    ReportBug,
    OpenGitHub, // Add this
    Exit,
    FilterChanged(String),
    SelectProcess(ProcessInfo),
    TickProcess,
    SwitchSearch(usize),
    SearchValueChanged(String),
    AddSearch,
    RemoveSearch,
    RenameSearch,
    RenameSearchTextChanged(String),
    ConfirmRenameSearch,
    CancelRenameSearch,

    SwitchSearchType(SearchType),
    Search,
    Tick,
    ClearResults,
    ToggleShowResult,
    Undo,
    ResultValueChanged(usize, String),
    ToggleFreeze(usize),
    OpenEditor(usize),
    RemoveResult(usize),
    CloseMemoryEditor,
    MemoryEditorAddressChanged(String),
    MemoryEditorJumpToAddress,
    MemoryEditorCellChanged(usize, String), // offset, new hex value
    MemoryEditorScroll(i32),                // scroll by n rows (positive = down, negative = up)
    MemoryEditorPageUp,
    MemoryEditorPageDown,
    MemoryEditorMoveCursor(i32, i32), // (row_delta, col_delta)
    MemoryEditorSetCursor(usize, usize),
    MemoryEditorEditHex(u8), // hex digit input
    MemoryEditorBeginEdit,
    MemoryEditorEndEdit,
}

#[derive(Default)]
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
    app_state: AppState,
    state: GameCheetahEngine,

    renaming_search_index: Option<usize>,
    rename_search_text: String,

    memory_editor_address_text: String,
    memory_cursor_row: usize,
    memory_cursor_col: usize,
    memory_cursor_nibble: usize,          // 0 = high nibble, 1 = low nibble
    memory_editor_initial_address: usize, // Add this to track the initial address
    memory_editor_initial_size: usize,    // Add this to track the size of the initial value
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
                    // Collect all results
                    let mut results = search_context.collect_results();

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
                        search_context.old_results.push(results.clone());

                        // Remove the item
                        results.remove(index);

                        // Clear the channel and send updated results
                        search_context.set_cached_results(results);
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
                        self.memory_editor_address_text = format!("{:X}", result.addr);
                        self.memory_editor_initial_address = result.addr;
                        self.memory_editor_initial_size = result.search_type.get_byte_length();
                        // Reset cursor to highlight the first byte
                        self.memory_cursor_row = 0;
                        self.memory_cursor_col = 0;
                        self.memory_cursor_nibble = 0;
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
                self.memory_editor_address_text = text;
                Task::none()
            }

            Message::MemoryEditorJumpToAddress => {
                // Parse the address from the text input
                let text = self.memory_editor_address_text.trim();
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
                self.memory_editor_address_text = format!("{:X}", new_address);
                Task::none()
            }

            Message::MemoryEditorPageUp => {
                const BYTES_PER_ROW: usize = 16;
                const ROWS_PER_PAGE: usize = 16;
                let page_size = BYTES_PER_ROW * ROWS_PER_PAGE;
                let new_address = self.state.edit_address.saturating_sub(page_size);
                self.state.edit_address = new_address;
                self.memory_editor_address_text = format!("{:X}", new_address);
                Task::none()
            }

            Message::MemoryEditorPageDown => {
                const BYTES_PER_ROW: usize = 16;
                const ROWS_PER_PAGE: usize = 16;
                let page_size = BYTES_PER_ROW * ROWS_PER_PAGE;
                let new_address = self.state.edit_address.saturating_add(page_size);
                self.state.edit_address = new_address;
                self.memory_editor_address_text = format!("{:X}", new_address);
                Task::none()
            }

            Message::MemoryEditorMoveCursor(row_delta, col_delta) => {
                const BYTES_PER_ROW: usize = 16;
                const MAX_VISIBLE_ROWS: usize = 24; // This should match MAX_ROWS in show_memory_editor

                if col_delta != 0 {
                    // Handle horizontal movement (nibble by nibble)
                    let total_nibbles = BYTES_PER_ROW * 2;
                    let current_nibble_pos = self.memory_cursor_col * 2 + self.memory_cursor_nibble;
                    let new_nibble_pos = (current_nibble_pos as i32 + col_delta).clamp(0, total_nibbles as i32 - 1) as usize;

                    self.memory_cursor_col = new_nibble_pos / 2;
                    self.memory_cursor_nibble = new_nibble_pos % 2;
                }

                if row_delta != 0 {
                    // Handle vertical movement with scrolling
                    let new_row = self.memory_cursor_row as i32 + row_delta;

                    if new_row < 0 {
                        // Cursor at top, scroll up
                        let new_address = self.state.edit_address.saturating_sub(BYTES_PER_ROW);
                        self.state.edit_address = new_address;
                        self.memory_editor_address_text = format!("{:X}", new_address);
                        self.memory_cursor_row = 0;
                    } else if new_row >= MAX_VISIBLE_ROWS as i32 {
                        // Cursor would go beyond visible area, scroll down
                        let new_address = self.state.edit_address.saturating_add(BYTES_PER_ROW);
                        self.state.edit_address = new_address;
                        self.memory_editor_address_text = format!("{:X}", new_address);
                        // Keep cursor at last visible row
                        self.memory_cursor_row = MAX_VISIBLE_ROWS - 1;
                    } else {
                        // Normal cursor movement within visible area
                        self.memory_cursor_row = new_row as usize;
                    }
                }

                Task::none()
            }

            Message::MemoryEditorSetCursor(row, col) => {
                self.memory_cursor_row = row;
                self.memory_cursor_col = col;
                self.memory_cursor_nibble = 0; // Always start at high nibble when clicking
                Task::none()
            }
            Message::MemoryEditorBeginEdit => Task::none(),
            Message::MemoryEditorEndEdit => Task::none(),
            Message::MemoryEditorEditHex(hex_digit) => {
                let offset = self.memory_cursor_row * 16 + self.memory_cursor_col;
                if let Ok(handle) = (self.state.pid as process_memory::Pid).try_into_process_handle() {
                    let address = self.state.edit_address + offset;
                    // Read current byte
                    if let Ok(buf) = copy_address(address, 1, &handle) {
                        let current_byte = buf[0];
                        let new_byte = if self.memory_cursor_nibble == 0 {
                            // Editing high nibble
                            (hex_digit << 4) | (current_byte & 0x0F)
                        } else {
                            // Editing low nibble
                            (current_byte & 0xF0) | hex_digit
                        };
                        handle.put_address(address, &[new_byte]).unwrap_or_default();
                    }
                }

                // Advance to next nibble
                if self.memory_cursor_nibble == 0 {
                    self.memory_cursor_nibble = 1;
                } else {
                    self.memory_cursor_nibble = 0;
                    self.memory_cursor_col += 1;
                    if self.memory_cursor_col >= 16 {
                        self.memory_cursor_col = 0;
                        self.memory_cursor_row += 1;
                        if self.memory_cursor_row >= 16 {
                            self.memory_cursor_row = 0;
                        }
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
            AppState::MemoryEditor => self.show_memory_editor(),
        }
    }

    fn view_main_window(&self) -> Element<'_, Message> {
        container(
            column![
                // Add title and version at the top
                container(
                    column![
                        text(APP_NAME).size(32),
                        text(format!("v{} © Mike Krüger 2023-2025", VERSION)).size(16).style(|theme: &iced::Theme| {
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
            container(row![
                container(text(fl!(crate::LANGUAGE_LOADER, "searches-heading")).size(14))
                    .width(Length::Fixed(120.0))
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
                                container(
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
                                )
                                .style(move |_theme: &iced::Theme| container::Style::default())
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
            .on_press(Message::AddSearch)
            .padding(5);
        let remove_button = button(text(fl!(crate::LANGUAGE_LOADER, "remove-search-button")))
            .on_press(Message::RemoveSearch)
            .padding(5);
        let rename_button = button(text(fl!(crate::LANGUAGE_LOADER, "rename-search-button")))
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
            .padding(DIALOG_PADDING)
            .align_y(alignment::Alignment::Center),
            horizontal_rule(1),
            row![
                column![search_table, column![add_button, remove_button, rename_button].spacing(5).padding(10)].height(Length::Fill),
                vertical_rule(1),
                self.search_ui()
            ]
            .spacing(5)
            .padding(DIALOG_PADDING),
        ])
        .into()
    }

    // Update the show_memory_editor function to highlight the initial bytes:
    fn show_memory_editor(&self) -> Element<'_, Message> {
        use iced::widget::{button, column, container, mouse_area, row, scrollable, text};

        const BYTES_PER_ROW: usize = 16;
        const MAX_ROWS: usize = 24;

        let address = self.state.edit_address;
        let total_bytes = BYTES_PER_ROW * MAX_ROWS;
        let mut memory = vec![0u8; total_bytes];

        if let Ok(handle) = (self.state.pid as process_memory::Pid).try_into_process_handle() {
            if let Ok(buf) = copy_address(address, total_bytes, &handle) {
                memory[..buf.len().min(total_bytes)].copy_from_slice(&buf[..buf.len().min(total_bytes)]);
            }
        }

        // Calculate which bytes to highlight
        let highlight_start = self.memory_editor_initial_address;
        let highlight_end = highlight_start + self.memory_editor_initial_size;

        // Build header row (unchanged)
        let header = row![
            container(text("Address").size(14).font(iced::Font::MONOSPACE))
                .width(Length::Fixed(95.0))
                .padding(0),
            container({
                let mut hex_headers = row![];
                for i in 0..BYTES_PER_ROW {
                    hex_headers = hex_headers.push(
                        container(text(format!("{:02X}", i)).size(14).font(iced::Font::MONOSPACE))
                            .width(Length::Fixed(30.0))
                            .align_x(alignment::Alignment::Center),
                    );
                }
                hex_headers
            })
            .width(Length::Fixed(480.0))
            .padding(0),
            container(text("ASCII").size(14).font(iced::Font::MONOSPACE))
                .width(Length::Fixed(140.0))
                .padding(0),
        ]
        .spacing(0);

        // Build hex rows
        let mut rows: Vec<iced::widget::Row<'_, Message>> = Vec::new();
        let actual_rows = memory.len() / BYTES_PER_ROW;

        for row_idx in 0..actual_rows {
            let offset = row_idx * BYTES_PER_ROW;
            let row_bytes = &memory[offset..offset + BYTES_PER_ROW];

            // Hex cells with nibble highlighting
            let mut hex_cells = row![];
            for (col_idx, byte) in row_bytes.iter().enumerate() {
                let is_selected_byte = self.memory_cursor_row == row_idx && self.memory_cursor_col == col_idx;
                let current_address = address + offset + col_idx;
                let is_initial_location = current_address >= highlight_start && current_address < highlight_end;

                let high_nibble = (byte >> 4) & 0x0F;
                let low_nibble = byte & 0x0F;

                let hex_display = if is_selected_byte {
                    row![
                        text(format!("{:X}", high_nibble))
                            .size(14)
                            .font(iced::Font::MONOSPACE)
                            .style(move |theme: &iced::Theme| {
                                if self.memory_cursor_nibble == 0 {
                                    iced::widget::text::Style {
                                        color: Some(theme.palette().primary),
                                    }
                                } else {
                                    iced::widget::text::Style {
                                        color: Some(theme.palette().text),
                                    }
                                }
                            }),
                        text(format!("{:X}", low_nibble))
                            .size(14)
                            .font(iced::Font::MONOSPACE)
                            .style(move |theme: &iced::Theme| {
                                if self.memory_cursor_nibble == 1 {
                                    iced::widget::text::Style {
                                        color: Some(theme.palette().primary),
                                    }
                                } else {
                                    iced::widget::text::Style {
                                        color: Some(theme.palette().text),
                                    }
                                }
                            }),
                    ]
                    .spacing(0)
                } else {
                    row![
                        text(format!("{:X}", high_nibble)).size(14).font(iced::Font::MONOSPACE),
                        text(format!("{:X}", low_nibble)).size(14).font(iced::Font::MONOSPACE),
                    ]
                    .spacing(0)
                };

                hex_cells = hex_cells.push(
                    mouse_area(container(hex_display).width(Length::Fixed(30.0)).padding(2).style(move |theme: &iced::Theme| {
                        if is_selected_byte {
                            container::Style {
                                background: Some(theme.palette().background.into()),
                                text_color: Some(theme.palette().text),
                                border: iced::Border {
                                    color: theme.palette().primary,
                                    width: 2.0,
                                    radius: Radius::new(4.0),
                                },
                                ..Default::default()
                            }
                        } else if is_initial_location {
                            // Highlight initial location with a different background
                            container::Style {
                                background: Some(theme.extended_palette().primary.weak.color.into()),
                                text_color: Some(theme.palette().text),
                                border: iced::Border {
                                    color: theme.extended_palette().primary.weak.color,
                                    width: 1.0,
                                    radius: Radius::new(0.0),
                                },
                                ..Default::default()
                            }
                        } else {
                            container::Style::default()
                        }
                    }))
                    .on_press(Message::MemoryEditorSetCursor(row_idx, col_idx)),
                );
            }

            // ASCII representation
            let ascii = row_bytes
                .iter()
                .enumerate()
                .map(|(i, b)| {
                    let c = *b as char;
                    let display_char = if c.is_ascii_graphic() || c == ' ' { c.to_string() } else { ".".to_string() };
                    let is_selected = self.memory_cursor_row == row_idx && self.memory_cursor_col == i;
                    let current_address = address + offset + i;
                    let is_initial_location = current_address >= highlight_start && current_address < highlight_end;

                    container(text(display_char).size(14).font(iced::Font::MONOSPACE))
                        .width(Length::Fixed(8.0))
                        .align_x(alignment::Alignment::Center)
                        .style(move |theme: &iced::Theme| {
                            if is_selected {
                                container::Style {
                                    background: Some(theme.palette().primary.into()),
                                    text_color: Some(theme.palette().background),
                                    ..Default::default()
                                }
                            } else if is_initial_location {
                                container::Style {
                                    background: Some(theme.extended_palette().success.weak.color.into()),
                                    text_color: Some(theme.palette().text),
                                    ..Default::default()
                                }
                            } else {
                                container::Style::default()
                            }
                        })
                })
                .fold(row![], |row, elem| row.push(elem));

            rows.push(
                row![
                    container(text(format!("{:08X}", address + offset as usize)).size(14).font(iced::Font::MONOSPACE))
                        .width(Length::Fixed(100.0))
                        .padding(0),
                    container(hex_cells).width(Length::Fixed(480.0)).padding(0),
                    container(ascii).width(Length::Fixed(140.0)).padding(0),
                ]
                .spacing(0),
            );
        }

        // Build info area showing values at cursor position
        let info_area = {
            let cursor_offset = self.memory_cursor_row * BYTES_PER_ROW + self.memory_cursor_col;
            let cursor_address = address + cursor_offset;

            // Get enough bytes for the largest type (8 bytes for u64/double)
            let mut value_bytes = vec![0u8; 8];
            let bytes_available = memory.len().saturating_sub(cursor_offset).min(8);
            if bytes_available > 0 {
                value_bytes[..bytes_available].copy_from_slice(&memory[cursor_offset..cursor_offset + bytes_available]);
            }

            // Calculate values in different formats
            let byte_val = value_bytes[0];
            let u16_val = if bytes_available >= 2 {
                u16::from_le_bytes([value_bytes[0], value_bytes[1]])
            } else {
                0
            };
            let u32_val = if bytes_available >= 4 {
                u32::from_le_bytes([value_bytes[0], value_bytes[1], value_bytes[2], value_bytes[3]])
            } else {
                0
            };
            let u64_val = if bytes_available >= 8 {
                u64::from_le_bytes([
                    value_bytes[0],
                    value_bytes[1],
                    value_bytes[2],
                    value_bytes[3],
                    value_bytes[4],
                    value_bytes[5],
                    value_bytes[6],
                    value_bytes[7],
                ])
            } else {
                0
            };

            container(
                column![
                    row![text(format!("Cursor: 0x{:08X}", cursor_address)).size(14).font(iced::Font::MONOSPACE),].spacing(20),
                    row![
                        column![
                            text("Byte:").size(14).font(iced::Font::MONOSPACE),
                            text("U16:").size(14).font(iced::Font::MONOSPACE),
                            text("U32:").size(14).font(iced::Font::MONOSPACE),
                        ]
                        .width(Length::Fixed(60.0))
                        .spacing(5),
                        column![
                            text(format!("{}", byte_val)).size(14).font(iced::Font::MONOSPACE),
                            text(format!("{}", u16_val)).size(14).font(iced::Font::MONOSPACE),
                            text(format!("{}", u32_val)).size(14).font(iced::Font::MONOSPACE),
                        ]
                        .width(Length::Fixed(200.0))
                        .spacing(5),
                        column![text("U64:").size(14).font(iced::Font::MONOSPACE),]
                            .width(Length::Fixed(60.0))
                            .spacing(5),
                        column![text(format!("{}", u64_val)).size(14).font(iced::Font::MONOSPACE),].spacing(5),
                    ]
                    .spacing(20)
                ]
                .spacing(10)
                .padding(10),
            )
            .width(Length::Fill)
            .style(|theme: &iced::Theme| container::Style {
                background: Some(theme.extended_palette().background.weak.color.into()),
                border: iced::Border {
                    color: theme.extended_palette().background.strong.color,
                    width: 1.0,
                    radius: Radius::new(4.0),
                },
                ..Default::default()
            })
        };

        // Rest of the UI remains the same...
        container(
            column![
                // Title and navigation bar
                row![
                    text(format!("Memory Editor - PID: {}", self.state.pid)).size(20),
                    container(
                        row![
                            text("Address:"),
                            text_input("0x", &self.memory_editor_address_text)
                                .on_input(Message::MemoryEditorAddressChanged)
                                .on_submit(Message::MemoryEditorJumpToAddress)
                                .width(Length::Fixed(120.0)),
                            button(text("Go")).on_press(Message::MemoryEditorJumpToAddress).padding(5),
                        ]
                        .spacing(10)
                        .align_y(alignment::Alignment::Center)
                    )
                    .width(Length::Fill),
                    button(text("Close")).on_press(Message::CloseMemoryEditor).padding(10)
                ]
                .spacing(20)
                .align_y(alignment::Alignment::Center),
                horizontal_rule(1),
                // Header row with column labels
                container(header).style(|theme: &iced::Theme| {
                    container::Style {
                        background: Some(theme.extended_palette().background.weak.color.into()),
                        ..Default::default()
                    }
                }),
                // Memory view
                scrollable(column(rows.into_iter().map(|r| r.into()).collect::<Vec<_>>()).spacing(0)).height(Length::FillPortion(3)),
                // Info area
                info_area,
            ]
            .spacing(0)
            .padding(DIALOG_PADDING),
        )
        .width(Length::Fill)
        .height(Length::Fill)
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
