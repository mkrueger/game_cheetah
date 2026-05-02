use std::{sync::atomic::Ordering, thread::sleep, time::{Duration, Instant}};

use i18n_embed_fl::fl;
use icy_ui::{
    Element, Length, Task, Theme, alignment, keyboard,
    widget::{
        button, column, container,
        operation::{focus_next, focus_previous},
        text,
    },
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

    /// In-progress edit of a result row's value field: `(row_index, typed_text)`.
    /// Buffering keystrokes here keeps the `text_input` from being re-bound to
    /// the freshly-read memory value on every render, which would otherwise
    /// look like the field is losing focus mid-edit.
    pub editing_result: Option<(usize, String)>,

    /// Counter bumped on every periodic Tick while the in-process view is
    /// shown. Folded into the result table's cache key so the virtualized row
    /// list rebuilds even when only the live-read values changed.
    pub refresh_counter: u64,

    memory_editor: super::memory_editor::MemoryEditor,

    pub process_sort_column: ProcessSortColumn,
    pub process_sort_direction: SortDirection,

    last_tab_click: Option<(usize, Instant)>,

    /// Brief status shown next to the Save/Load buttons (e.g. path on success, error on failure).
    pub cheat_table_status: String,

    /// When true, result values are displayed in hexadecimal instead of decimal.
    pub hex_display: bool,

    /// When true, automatically reattach to a process with the same name when
    /// the current process exits.
    pub auto_reattach: bool,
}

impl App {
    pub fn title(&self) -> String {
        format!("{} {}", crate::APP_NAME, crate::VERSION)
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        let should_update_processes = self.state.last_process_update.elapsed().map_or(true, |elapsed| elapsed.as_millis() > 500);
        let watching = self.auto_reattach && self.state.pid == 0 && !self.state.process_name.is_empty();
        if (self.app_state == AppState::ProcessSelection || watching) && should_update_processes {
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
                    println!("Failed to open discussion page: {err}");
                }
                Task::none()
            }
            Message::ReportBug => {
                if let Err(err) = webbrowser::open("https://github.com/mkrueger/game_cheetah/issues/new") {
                    println!("Failed to open bug report page: {err}");
                }
                Task::none()
            }
            Message::OpenGitHub => {
                if let Err(err) = webbrowser::open("https://github.com/mkrueger/game_cheetah") {
                    println!("Failed to open GitHub page: {err}");
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
                icy_ui::Task::perform(
                    async {
                        sleep(Duration::from_millis(2000));
                    },
                    |_| Message::TickProcess,
                )
            }
            Message::TickProcess => {
                self.state.detach_if_gone();
                // Auto-reattach: scan for the process by name and reattach if found.
                if self.auto_reattach && self.state.pid == 0 && !self.state.process_name.is_empty() {
                    let target = self.state.process_name.clone();
                    if let Some(process) = self.state.processes.iter().find(|p| p.name == target).cloned() {
                        self.state.select_process(&process);
                    }
                }
                icy_ui::Task::perform(
                    async {
                        sleep(Duration::from_millis(2000));
                    },
                    |_| Message::TickProcess,
                )
            }
            Message::NewSearch => {
                self.state.new_search();
                Task::none()
            }
            Message::CloseSearch(index) => {
                if index >= self.state.searches.len() {
                    return Task::none();
                }
                self.state.remove_freezes(index);
                self.state.searches.remove(index);
                if self.state.searches.is_empty() {
                    self.state.current_search = 0;
                } else if self.state.current_search > index {
                    self.state.current_search -= 1;
                } else if self.state.current_search >= self.state.searches.len() {
                    self.state.current_search = self.state.searches.len() - 1;
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
                if let Some(index) = self.renaming_search_index
                    && let Some(search) = self.state.searches.get_mut(index)
                {
                    search.description = self.rename_search_text.clone();
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
                    let now = Instant::now();
                    let is_double_click = self
                        .last_tab_click
                        .is_some_and(|(i, t)| i == index && now.duration_since(t) < Duration::from_millis(300));

                    if is_double_click {
                        self.last_tab_click = None;
                        if let Some(search) = self.state.searches.get(index) {
                            self.rename_search_text = search.description.clone();
                            self.renaming_search_index = Some(index);
                        }
                    } else {
                        self.last_tab_click = Some((index, now));
                        self.state.current_search = index;
                        self.editing_result = None;
                    }
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
                            println!("Error parsing search value: {err}");
                        }
                    }
                }
                Task::done(Message::Tick)
            }
            Message::Tick => {
                if matches!(self.app_state, AppState::InProcess) {
                    self.refresh_counter = self.refresh_counter.wrapping_add(1);
                }
                // If searching, keep scheduling ticks
                let current_search_context = &mut self.state.searches[self.state.current_search];

                if !matches!(current_search_context.searching, SearchMode::None) {
                    current_search_context.collect_results();
                }

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
                if let Some(search_context) = self.state.searches.get_mut(self.state.current_search)
                    && let Some(old) = search_context.old_results.pop()
                {
                    search_context.set_cached_results(old);
                }
                Task::none()
            }
            Message::ClearResults => {
                if let Some(search_context) = self.state.searches.get_mut(self.state.current_search) {
                    search_context.clear_results(&self.state.freeze_sender);
                }
                self.editing_result = None;
                Task::none()
            }
            Message::ToggleShowResult => {
                self.state.show_results = !self.state.show_results;
                Task::none()
            }
            Message::ResultValueChanged(index, value_text) => {
                if let Ok(handle) = (self.state.pid as process_memory::Pid).try_into_process_handle()
                    && let Some(current_search) = self.state.searches.get_mut(self.state.current_search)
                {
                    // Collect all results
                    let results = current_search.collect_results();

                    if index < results.len() {
                        let result = &results[index];
                        match result.search_type.from_string(&value_text) {
                            Ok(value) => {
                                if let Err(err) = handle.put_address(result.addr, &value.1) {
                                    self.state.error_text = format!("Failed to write 0x{:X}: {}", result.addr, err);
                                } else if current_search.freezed_addresses.contains(&result.addr)
                                    && let Err(err) = self.state.freeze_sender.send(FreezeMessage {
                                        msg: crate::MessageCommand::Freeze,
                                        addr: result.addr,
                                        value,
                                    })
                                {
                                    self.state.error_text = format!("Freeze channel closed: {err}");
                                }
                            }
                            Err(err) => {
                                self.state.error_text = format!("Invalid value '{value_text}': {err}");
                            }
                        }
                    } else {
                        self.state.error_text = format!("Invalid result index {index}");
                    }
                }
                self.editing_result = None;
                Task::none()
            }
            Message::ResultEditingBegin(index, text) => {
                self.editing_result = Some((index, text));
                icy_ui::widget::operation::focus(icy_ui::widget::Id::from(format!("result-value-{index}")))
            }
            Message::ResultEditingChanged(index, text) => {
                // Best-effort live write: if the buffered text parses cleanly,
                // commit it to memory immediately so the user sees the value
                // change in the running game while they're typing. Parse
                // failures are silent here — the user is mid-edit.
                if let Ok(handle) = (self.state.pid as process_memory::Pid).try_into_process_handle()
                    && let Some(current_search) = self.state.searches.get_mut(self.state.current_search)
                {
                    let results = current_search.collect_results();
                    if let Some(result) = results.get(index)
                        && let Ok(value) = result.search_type.from_string(&text)
                    {
                        let _ = handle.put_address(result.addr, &value.1);
                        if current_search.freezed_addresses.contains(&result.addr) {
                            let _ = self.state.freeze_sender.send(FreezeMessage {
                                msg: crate::MessageCommand::Freeze,
                                addr: result.addr,
                                value,
                            });
                        }
                    }
                }
                self.editing_result = Some((index, text));
                Task::none()
            }
            Message::ResultEditingCommit(index) => {
                if let Some((i, text)) = self.editing_result.take()
                    && i == index
                {
                    return self.update(Message::ResultValueChanged(index, text));
                }
                Task::none()
            }
            Message::ResultEditingCancel => {
                self.editing_result = None;
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
                            if let Some(byte_len) = result.search_type.fixed_byte_length()
                                && let Ok(handle) = (self.state.pid as process_memory::Pid).try_into_process_handle()
                                && let Ok(buf) = copy_address(result.addr, byte_len, &handle)
                                && let Err(e) = self.state.freeze_sender.send(FreezeMessage {
                                    msg: MessageCommand::Freeze,
                                    addr: result.addr,
                                    value: SearchValue(result.search_type, buf),
                                })
                            {
                                self.state.error_text = format!("Freeze channel closed: {e}");
                            }
                        } else {
                            search_context.freezed_addresses.remove(&(result.addr));
                            if let Err(e) = self.state.freeze_sender.send(FreezeMessage::from_addr(MessageCommand::Unfreeze, result.addr)) {
                                self.state.error_text = format!("Freeze channel closed: {e}");
                            }
                        }
                    }
                }
                Task::none()
            }
            Message::ToggleFreezeAll => {
                if let Some(search_context) = self.state.searches.get_mut(self.state.current_search) {
                    let results = search_context.collect_results();
                    if results.is_empty() {
                        return Task::none();
                    }

                    // Check if all are frozen - if so, unfreeze all; otherwise freeze all
                    let all_frozen = results.iter().all(|r| search_context.freezed_addresses.contains(&r.addr));

                    if all_frozen {
                        // Unfreeze all
                        for result in results.iter() {
                            if search_context.freezed_addresses.remove(&result.addr)
                                && let Err(e) = self.state.freeze_sender.send(FreezeMessage::from_addr(MessageCommand::Unfreeze, result.addr))
                            {
                                self.state.error_text = format!("Freeze channel closed: {e}");
                            }
                        }
                    } else {
                        // Freeze all
                        if let Ok(handle) = (self.state.pid as process_memory::Pid).try_into_process_handle() {
                            for result in results.iter() {
                                if !search_context.freezed_addresses.contains(&result.addr) {
                                    search_context.freezed_addresses.insert(result.addr);
                                    let Some(byte_len) = result.search_type.fixed_byte_length() else {
                                        continue;
                                    };
                                    if let Ok(buf) = copy_address(result.addr, byte_len, &handle)
                                        && let Err(e) = self.state.freeze_sender.send(FreezeMessage {
                                            msg: MessageCommand::Freeze,
                                            addr: result.addr,
                                            value: SearchValue(result.search_type, buf),
                                        })
                                    {
                                        self.state.error_text = format!("Freeze channel closed: {e}");
                                    }
                                }
                            }
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
                            if let Err(e) = self.state.freeze_sender.send(FreezeMessage::from_addr(MessageCommand::Unfreeze, result.addr)) {
                                self.state.error_text = format!("Freeze channel closed: {e}");
                            }
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
                        match self.memory_editor.initialize(self.state.pid, result.addr, result.search_type) {
                            Ok(()) => {
                                self.app_state = AppState::MemoryEditor;
                                return Task::batch([self.memory_editor.snap_to_cursor(), Task::done(Message::MemoryEditorTick)]);
                            }
                            Err(err) => self.state.error_text = err,
                        }
                    }
                }
                Task::none()
            }
            Message::CloseMemoryEditor => {
                self.app_state = AppState::InProcess;
                self.memory_editor.reset_change_tracker();
                Task::none()
            }
            Message::MemoryEditorAddressChanged(text) => {
                self.memory_editor.address_text = text;
                Task::none()
            }

            Message::MemoryEditorJumpToAddress => {
                // Parse the address from the text input
                let raw = self.memory_editor.address_text.trim();
                let stripped = raw.strip_prefix("0x").or_else(|| raw.strip_prefix("0X")).unwrap_or(raw);
                match u64::from_str_radix(stripped, 16) {
                    Ok(new_address) => {
                        match self
                            .memory_editor
                            .refresh_regions(self.state.pid)
                            .and_then(|()| self.memory_editor.focus_on(new_address as usize))
                        {
                            Ok(()) => {
                                self.state.error_text.clear();
                                return self.memory_editor.snap_to_cursor();
                            }
                            Err(err) => self.state.error_text = err,
                        }
                    }
                    Err(err) => {
                        self.state.error_text = format!("Invalid address '{raw}': {err}");
                    }
                }
                Task::none()
            }

            Message::MemoryEditorCellChanged(offset, value) => {
                // Validate and update the byte at the given offset
                if value.len() <= 2
                    && let Ok(byte_value) = u8::from_str_radix(&value, 16)
                    && let Ok(handle) = (self.state.pid as process_memory::Pid).try_into_process_handle()
                    && let Some(address) = self.memory_editor.address_for_offset(offset)
                    && let Err(err) = handle.put_address(address, &[byte_value])
                {
                    self.state.error_text = format!("Failed to write 0x{address:X}: {err}");
                }
                Task::none()
            }

            Message::MemoryEditorScroll(rows) => {
                let offset = icy_ui::widget::operation::AbsoluteOffset {
                    x: 0.0,
                    y: rows as f32 * super::memory_editor::ROW_HEIGHT,
                };
                icy_ui::widget::operation::scroll_by(icy_ui::widget::Id::new("memory-editor-scroll"), offset)
            }

            Message::MemoryEditorPageUp => {
                self.memory_editor.move_cursor(-(super::memory_editor::PAGE_ROWS as i32), 0);
                self.memory_editor.ensure_cursor_visible()
            }

            Message::MemoryEditorPageDown => {
                self.memory_editor.move_cursor(super::memory_editor::PAGE_ROWS as i32, 0);
                self.memory_editor.ensure_cursor_visible()
            }

            Message::MemoryEditorMoveCursor(row_delta, col_delta) => {
                let row_changed = self.memory_editor.move_cursor(row_delta, col_delta);
                if row_changed {
                    self.memory_editor.ensure_cursor_visible()
                } else {
                    Task::none()
                }
            }

            Message::MemoryEditorSetCursor(row, col) => {
                self.memory_editor.set_cursor(row, col);
                self.memory_editor.ensure_cursor_visible()
            }
            Message::MemoryEditorBeginEdit => Task::none(),
            Message::MemoryEditorEndEdit => Task::none(),
            Message::MemoryEditorEditHex(hex_digit) => {
                let cursor_row_before = self.memory_editor.cursor_row();
                if let Err(err) = self.memory_editor.edit_hex(self.state.pid, hex_digit) {
                    self.state.error_text = err;
                }
                if self.memory_editor.cursor_row() != cursor_row_before {
                    self.memory_editor.ensure_cursor_visible()
                } else {
                    Task::none()
                }
            }
            Message::MemoryEditorInspectorValueChanged(kind, value) => {
                self.memory_editor.set_inspector_value_text(kind, value);
                Task::none()
            }
            Message::MemoryEditorInspectorValueSubmit(kind) => {
                match self.memory_editor.submit_inspector_value(self.state.pid, kind) {
                    Ok(()) => self.state.error_text.clear(),
                    Err(err) => self.state.error_text = err,
                }
                Task::none()
            }
            Message::MemoryEditorScrolled(viewport) => {
                self.memory_editor.set_viewport(viewport);
                Task::none()
            }
            Message::MemoryEditorTick => {
                if matches!(self.app_state, AppState::MemoryEditor) {
                    icy_ui::Task::perform(
                        async {
                            sleep(super::memory_editor::TICK_INTERVAL);
                        },
                        |_| Message::MemoryEditorTick,
                    )
                } else {
                    Task::none()
                }
            }
            Message::MemoryEditorUndo => {
                match self.memory_editor.undo(self.state.pid) {
                    Ok(Some(address)) => {
                        self.state.error_text.clear();
                        if self.memory_editor.focus_on(address).is_ok() {
                            return self.memory_editor.ensure_cursor_visible();
                        }
                    }
                    Ok(None) => {}
                    Err(err) => self.state.error_text = err,
                }
                Task::none()
            }
            Message::MemoryEditorRedo => {
                match self.memory_editor.redo(self.state.pid) {
                    Ok(Some(address)) => {
                        self.state.error_text.clear();
                        if self.memory_editor.focus_on(address).is_ok() {
                            return self.memory_editor.ensure_cursor_visible();
                        }
                    }
                    Ok(None) => {}
                    Err(err) => self.state.error_text = err,
                }
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
                icy_ui::Task::none()
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
            Message::FocusNext => focus_next(),
            Message::FocusPrevious => focus_previous(),

            Message::SaveCheatTable => {
                let path = crate::default_cheat_table_path(&self.state.process_name);
                match crate::save_cheat_table(&self.state, &path) {
                    Ok(()) => self.cheat_table_status = format!("Saved: {}", path.display()),
                    Err(e) => self.cheat_table_status = format!("Save error: {e}"),
                }
                Task::none()
            }

            Message::LoadCheatTable => {
                let path = crate::default_cheat_table_path(&self.state.process_name);
                match crate::load_cheat_table(&path, &self.state.freeze_sender) {
                    Ok(searches) => {
                        self.state.searches = searches;
                        self.state.current_search = 0;
                        self.editing_result = None;
                        self.cheat_table_status = format!("Loaded: {}", path.display());
                    }
                    Err(e) => self.cheat_table_status = format!("Load error: {e}"),
                }
                Task::none()
            }
            Message::ToggleHexDisplay => {
                self.hex_display = !self.hex_display;
                Task::none()
            }
            Message::ToggleAutoReattach => {
                self.auto_reattach = !self.auto_reattach;
                Task::none()
            }
        }
    }

    pub fn theme(&self) -> Theme {
        Theme::dark().clone()
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

    pub fn subscription(&self) -> icy_ui::Subscription<Message> {
        // Periodic refresh while showing live result rows so the values
        // re-read memory and update on screen.
        let live_results_tick = if matches!(self.app_state, AppState::InProcess) {
            icy_ui::time::every(Duration::from_millis(250)).map(|_| Message::Tick)
        } else {
            icy_ui::Subscription::none()
        };

        let keyboard_sub: icy_ui::Subscription<Message> = if matches!(self.app_state, AppState::MemoryEditor) {
            keyboard::listen().filter_map(|event| {
                let keyboard::Event::KeyPressed { key, modifiers, .. } = event else {
                    return None;
                };
                // Ctrl/Cmd + Z / Shift+Ctrl/Cmd + Z drive undo / redo. Match
                // these before plain character handling so a `Z` keystroke
                // with the modifier doesn't fall through to the hex editor.
                if modifiers.command()
                    && let keyboard::Key::Character(c) = &key
                    && matches!(c.as_str(), "z" | "Z")
                {
                    return if modifiers.shift() {
                        Some(Message::MemoryEditorRedo)
                    } else {
                        Some(Message::MemoryEditorUndo)
                    };
                }
                match key {
                    keyboard::Key::Named(keyboard::key::Named::ArrowUp) => Some(Message::MemoryEditorMoveCursor(-1, 0)),
                    keyboard::Key::Named(keyboard::key::Named::ArrowDown) => Some(Message::MemoryEditorMoveCursor(1, 0)),
                    keyboard::Key::Named(keyboard::key::Named::ArrowLeft) => Some(Message::MemoryEditorMoveCursor(0, -1)),
                    keyboard::Key::Named(keyboard::key::Named::ArrowRight) => Some(Message::MemoryEditorMoveCursor(0, 1)),
                    keyboard::Key::Named(keyboard::key::Named::PageUp) => Some(Message::MemoryEditorPageUp),
                    keyboard::Key::Named(keyboard::key::Named::PageDown) => Some(Message::MemoryEditorPageDown),
                    keyboard::Key::Named(keyboard::key::Named::Tab) => Some(Message::MemoryEditorMoveCursor(0, 1)),
                    keyboard::Key::Named(keyboard::key::Named::Enter) => Some(Message::MemoryEditorBeginEdit),
                    keyboard::Key::Named(keyboard::key::Named::Escape) => Some(Message::CloseMemoryEditor),
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
                }
            })
        } else if self.renaming_search_index.is_some() {
            // Only subscribe to ESC when renaming
            keyboard::listen().filter_map(|event| {
                let keyboard::Event::KeyPressed { key, .. } = event else {
                    return None;
                };
                match key {
                    keyboard::Key::Named(keyboard::key::Named::Escape) => Some(Message::CancelRenameSearch),
                    _ => None,
                }
            })
        } else {
            // Tab/Shift+Tab for focus navigation in normal mode
            keyboard::listen().filter_map(|event| {
                let keyboard::Event::KeyPressed { key, modifiers, .. } = event else {
                    return None;
                };
                match key {
                    keyboard::Key::Named(keyboard::key::Named::Tab) => {
                        if modifiers.shift() {
                            Some(Message::FocusPrevious)
                        } else {
                            Some(Message::FocusNext)
                        }
                    }
                    _ => None,
                }
            })
        };
        icy_ui::Subscription::batch([live_results_tick, keyboard_sub])
    }
}
