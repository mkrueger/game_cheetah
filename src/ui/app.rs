use std::{
    collections::HashMap,
    sync::atomic::Ordering,
    thread::sleep,
    time::{Duration, Instant},
};

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

use crate::{AppError, FreezeMessage, GameCheetahEngine, MessageCommand, SearchMode, SearchValue, message::Message};
use crate::{
    SearchType, UnknownComparison,
    ui::process_selection::{ProcessSortColumn, SortDirection},
};

#[derive(Default, PartialEq, Eq, Hash, Debug, Clone, Copy)]
pub enum AppState {
    #[default]
    MainWindow,
    ProcessSelection,
    Settings,
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

    /// In-progress edit of a result row's value field:
    /// `(row_index, typed_buffer, last_synced_live_value)`.
    ///
    /// Buffering keystrokes here keeps the `text_input` from being re-bound to
    /// the freshly-read memory value on every render, which would otherwise
    /// look like the field is losing focus mid-edit. The third element stores
    /// the live value the buffer was last synchronized with: as long as the
    /// user hasn't typed since (`buffer == last_synced`), the periodic Tick
    /// re-syncs both fields to the latest memory read so the editor keeps
    /// reflecting external changes. Once the user types, the buffer diverges
    /// from the snapshot and is left untouched until commit/cancel.
    pub editing_result: Option<(usize, String, String)>,

    /// Counter bumped on every periodic Tick while the in-process view is
    /// shown. Folded into the result table's cache key so the virtualized row
    /// list rebuilds even when only the live-read values changed.
    pub refresh_counter: u64,

    memory_editor: super::memory_editor::MemoryEditor,
    memory_editor_result_index: Option<usize>,

    pub process_sort_column: ProcessSortColumn,
    pub process_sort_direction: SortDirection,

    last_tab_click: Option<(usize, Instant)>,

    /// Brief status shown next to the Save/Load buttons (e.g. path on success, error on failure).
    pub cheat_table_status: String,

    /// When true, result values are displayed in hexadecimal instead of decimal.
    pub hex_display: bool,

    /// Optional advanced setting: after the attached process exits, keep
    /// watching for a process with the same name and attach again automatically.
    /// This is off by default and only configurable from the main menu settings.
    pub auto_reconnect: bool,

    /// When true, the app contacts the GitHub releases API once per launch
    /// to check for a newer version. Defaults to true; can be disabled in
    /// Settings.
    pub check_for_updates: bool,

    /// Set to true the first time the update check runs so we never fire
    /// it twice within a single launch.
    update_check_started: bool,

    /// Tag of the latest release if it is newer than [`crate::VERSION`].
    /// `None` until the check completes (or if no newer version exists).
    pub latest_version: Option<String>,

    /// Last-read value string per address for the active search, used to
    /// detect value changes between refresh ticks.
    pub value_change_tracker: HashMap<usize, String>,
    /// Maps address → `refresh_counter` when its value last changed.
    /// A row is highlighted while `refresh_counter - stored` < CHANGE_HIGHLIGHT_TICKS.
    pub changed_addresses: HashMap<usize, u64>,
}

impl App {
    /// Construct a fresh `App` and load persisted user preferences from the
    /// config directory. Used as the icy_ui state factory.
    pub fn new() -> Self {
        let settings = crate::UserSettings::load();
        Self {
            auto_reconnect: settings.auto_reconnect,
            hex_display: settings.hex_display,
            check_for_updates: settings.check_for_updates,
            ..Self::default()
        }
    }

    fn persist_settings(&mut self) {
        let settings = crate::UserSettings {
            auto_reconnect: self.auto_reconnect,
            hex_display: self.hex_display,
            check_for_updates: self.check_for_updates,
        };
        if let Err(e) = settings.save() {
            self.state.push_error(AppError::Generic { message: e });
        }
    }

    pub fn title(&self) -> String {
        format!("{} {}", crate::APP_NAME, crate::VERSION)
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        let should_update_processes = self.state.last_process_update.elapsed().map_or(true, |elapsed| elapsed.as_millis() > 500);
        let watching_for_reconnect = self.auto_reconnect && self.state.pid == 0 && !self.state.process_name.is_empty();
        if (self.app_state == AppState::ProcessSelection || watching_for_reconnect) && should_update_processes {
            self.state.update_process_data();
        }
        // Check and update search modes for all searches
        for search_context in &mut self.state.searches {
            search_context.update_search_mode();
        }

        // Kick off the update check exactly once per launch (and only if the
        // user has not opted out). Runs on a background thread so it never
        // blocks the UI; failures are silent.
        let update_check_task = if self.check_for_updates && !self.update_check_started {
            self.update_check_started = true;
            icy_ui::Task::perform(
                async { smol::unblock(crate::update_check::fetch_latest_version).await },
                Message::UpdateCheckCompleted,
            )
        } else {
            Task::none()
        };

        let message_task = match message {
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
            Message::DismissDialog => {
                if matches!(self.app_state, AppState::ProcessSelection | AppState::About | AppState::Settings) {
                    self.app_state = AppState::MainWindow;
                }
                Task::none()
            }
            Message::About => {
                self.app_state = AppState::About;
                Task::none()
            }
            Message::Settings => {
                self.app_state = AppState::Settings;
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
                if self.auto_reconnect && self.state.pid == 0 && !self.state.process_name.is_empty() {
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
                self.clear_change_tracker();
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
                        self.clear_change_tracker();
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
                // The picker is only visible while no search has been started
                // yet (see `show_type_picker` in `in_process_view`), so this
                // never has to worry about preserving an existing result set.
                if let Some(current_search) = self.state.searches.get_mut(self.state.current_search) {
                    current_search.search_type = search_type;
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
                    // Only track per-row value changes when no search is in progress.
                    // During a scan, `collect_results()` can return millions of intermediate
                    // hits and one `copy_address` syscall per result would freeze the UI.
                    let search_running = self.state.searches.iter().any(|s| !matches!(s.searching, SearchMode::None));
                    if !search_running && self.state.is_process_running() {
                        self.update_change_tracker();
                    } else if search_running {
                        // Keep stale highlights from confusing the user once the search
                        // finishes and the result set changes.
                        self.clear_change_tracker();
                    }
                    // Pull live memory into the in-progress edit buffer
                    // independently of `update_change_tracker` — the tracker
                    // bails out for huge result sets and during searches, but
                    // an open editor only needs a single address re-read.
                    if !search_running {
                        self.sync_editing_buffer();
                    }
                }
                // If searching, keep scheduling ticks
                let current_search_context = &mut self.state.searches[self.state.current_search];

                if !matches!(current_search_context.searching, SearchMode::None) {
                    current_search_context.collect_results();
                }

                if current_search_context.search_complete.load(Ordering::SeqCst) {
                    // Drain the channel until it's empty so we don't keep
                    // re-sorting the cache (and visibly shifting addresses)
                    // for the next few ticks while the last few worker
                    // batches trickle in. Workers may have queued up to
                    // RESULTS_CHANNEL_CAPACITY batches that were not yet
                    // consumed when `search_complete` flipped.
                    loop {
                        let before = current_search_context.get_result_count();
                        let _ = current_search_context.collect_results();
                        let after = current_search_context.get_result_count();
                        if before == after {
                            break;
                        }
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
                self.clear_change_tracker();
                Task::none()
            }
            Message::ClearResults => {
                if let Some(search_context) = self.state.searches.get_mut(self.state.current_search) {
                    search_context.clear_results(&self.state.freeze_sender);
                }
                self.editing_result = None;
                self.clear_change_tracker();
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
                                    self.state.push_error(AppError::MemoryWrite {
                                        addr: result.addr,
                                        source: err.to_string(),
                                    });
                                } else if current_search.freezed_addresses.contains(&result.addr)
                                    && let Err(err) = self.state.freeze_sender.send(FreezeMessage {
                                        msg: crate::MessageCommand::Freeze,
                                        addr: result.addr,
                                        value,
                                    })
                                {
                                    self.state.push_error(AppError::FreezeChannelClosed { source: err.to_string() });
                                }
                            }
                            Err(err) => {
                                self.state.push_error(AppError::InvalidValue {
                                    value: value_text,
                                    source: err.to_string(),
                                });
                            }
                        }
                    } else {
                        self.state.push_error(AppError::InvalidResultIndex { index });
                    }
                }
                self.editing_result = None;
                Task::none()
            }
            Message::ResultEditingBegin(index, text) => {
                self.editing_result = Some((index, text.clone(), text));
                icy_ui::widget::operation::focus(icy_ui::widget::Id::from(format!("result-value-{}-{index}", self.state.current_search)))
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
                // User typed — break the link to the live snapshot so the
                // periodic Tick stops overwriting the buffer until commit/cancel.
                let snapshot = self.editing_result.as_ref().map(|(_, _, s)| s.clone()).unwrap_or_default();
                self.editing_result = Some((index, text, snapshot));
                Task::none()
            }
            Message::ResultEditingCommit(index) => {
                if let Some((i, text, _)) = self.editing_result.take()
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
                                self.state.push_error(AppError::FreezeChannelClosed { source: e.to_string() });
                            }
                        } else {
                            search_context.freezed_addresses.remove(&(result.addr));
                            if let Err(e) = self.state.freeze_sender.send(FreezeMessage::from_addr(MessageCommand::Unfreeze, result.addr)) {
                                self.state.push_error(AppError::FreezeChannelClosed { source: e.to_string() });
                            }
                        }
                    }
                }
                Task::none()
            }
            Message::ToggleFreezeAll => {
                let freeze_sender = self.state.freeze_sender.clone();
                let pid = self.state.pid;
                let mut send_error = None;
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
                                && let Err(e) = freeze_sender.send(FreezeMessage::from_addr(MessageCommand::Unfreeze, result.addr))
                            {
                                send_error.get_or_insert_with(|| AppError::FreezeChannelClosed { source: e.to_string() });
                            }
                        }
                    } else {
                        // Freeze all
                        if let Ok(handle) = (pid as process_memory::Pid).try_into_process_handle() {
                            for result in results.iter() {
                                if !search_context.freezed_addresses.contains(&result.addr) {
                                    search_context.freezed_addresses.insert(result.addr);
                                    let Some(byte_len) = result.search_type.fixed_byte_length() else {
                                        continue;
                                    };
                                    if let Ok(buf) = copy_address(result.addr, byte_len, &handle)
                                        && let Err(e) = freeze_sender.send(FreezeMessage {
                                            msg: MessageCommand::Freeze,
                                            addr: result.addr,
                                            value: SearchValue(result.search_type, buf),
                                        })
                                    {
                                        send_error.get_or_insert_with(|| AppError::FreezeChannelClosed { source: e.to_string() });
                                    }
                                }
                            }
                        }
                    }
                }
                if let Some(error) = send_error {
                    self.state.push_error(error);
                }
                Task::none()
            }
            Message::RemoveResult(index) => {
                let freeze_sender = self.state.freeze_sender.clone();
                let mut send_error = None;
                if let Some(search_context) = self.state.searches.get_mut(self.state.current_search) {
                    // Collect all results - now returns Arc<Vec<SearchResult>>
                    let results = search_context.collect_results();

                    if index < results.len() {
                        // Remove any freeze for this address before removing it
                        let result = &results[index];
                        if search_context.freezed_addresses.contains(&result.addr) {
                            search_context.freezed_addresses.remove(&result.addr);
                            if let Err(e) = freeze_sender.send(FreezeMessage::from_addr(MessageCommand::Unfreeze, result.addr)) {
                                send_error = Some(AppError::FreezeChannelClosed { source: e.to_string() });
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
                if let Some(error) = send_error {
                    self.state.push_error(error);
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
                                self.memory_editor_result_index = Some(index);
                                return Task::batch([
                                    self.memory_editor.snap_to_cursor(),
                                    self.memory_editor.focus_grid(),
                                    Task::done(Message::MemoryEditorTick),
                                ]);
                            }
                            Err(err) => self.state.push_error(AppError::memory_editor(err)),
                        }
                    }
                }
                Task::none()
            }
            Message::CloseMemoryEditor => {
                self.app_state = AppState::InProcess;
                self.memory_editor_result_index = None;
                self.memory_editor.reset_change_tracker();
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
                    self.state.push_error(AppError::MemoryWrite {
                        addr: address,
                        source: err.to_string(),
                    });
                }
                Task::none()
            }

            Message::MemoryEditorScroll(rows) => {
                if !self.memory_editor.is_grid_focused() {
                    return Task::none();
                }
                let offset = icy_ui::widget::operation::AbsoluteOffset {
                    x: 0.0,
                    y: rows as f32 * super::memory_editor::ROW_HEIGHT,
                };
                icy_ui::widget::operation::scroll_by(icy_ui::widget::Id::new("memory-editor-scroll"), offset)
            }

            Message::MemoryEditorPageUp => {
                if !self.memory_editor.is_grid_focused() {
                    return Task::none();
                }
                self.memory_editor.move_cursor(-(super::memory_editor::PAGE_ROWS as i32), 0);
                self.memory_editor.ensure_cursor_visible()
            }

            Message::MemoryEditorPageDown => {
                if !self.memory_editor.is_grid_focused() {
                    return Task::none();
                }
                self.memory_editor.move_cursor(super::memory_editor::PAGE_ROWS as i32, 0);
                self.memory_editor.ensure_cursor_visible()
            }

            Message::MemoryEditorMoveCursor(row_delta, col_delta) => {
                if !self.memory_editor.is_grid_focused() {
                    return Task::none();
                }
                let row_changed = self.memory_editor.move_cursor(row_delta, col_delta);
                if row_changed {
                    self.memory_editor.ensure_cursor_visible()
                } else {
                    Task::none()
                }
            }

            Message::MemoryEditorSetCursor(row, col) => {
                self.memory_editor.set_cursor(row, col);
                Task::batch([self.memory_editor.ensure_cursor_visible(), self.memory_editor.focus_grid()])
            }
            Message::MemoryEditorBeginEdit => self.memory_editor.focus_grid(),
            Message::MemoryEditorEndEdit => {
                self.memory_editor.set_grid_focused(false);
                Task::none()
            }
            Message::MemoryEditorKeyPressed(key, modifiers) => {
                if modifiers.command()
                    && let keyboard::Key::Character(c) = &key
                    && matches!(c.as_str(), "z" | "Z")
                {
                    return if modifiers.shift() {
                        Task::done(Message::MemoryEditorRedo)
                    } else {
                        Task::done(Message::MemoryEditorUndo)
                    };
                }

                match key {
                    keyboard::Key::Named(keyboard::key::Named::Escape) => {
                        self.app_state = AppState::InProcess;
                        self.memory_editor_result_index = None;
                        self.memory_editor.reset_change_tracker();
                        Task::none()
                    }
                    keyboard::Key::Named(keyboard::key::Named::Enter) => self.memory_editor.focus_grid(),
                    keyboard::Key::Named(keyboard::key::Named::ArrowUp) => {
                        let row_changed = self.memory_editor.move_cursor(-1, 0);
                        if row_changed {
                            Task::batch([self.memory_editor.ensure_cursor_visible(), self.memory_editor.focus_grid()])
                        } else {
                            self.memory_editor.focus_grid()
                        }
                    }
                    keyboard::Key::Named(keyboard::key::Named::ArrowDown) => {
                        let row_changed = self.memory_editor.move_cursor(1, 0);
                        if row_changed {
                            Task::batch([self.memory_editor.ensure_cursor_visible(), self.memory_editor.focus_grid()])
                        } else {
                            self.memory_editor.focus_grid()
                        }
                    }
                    keyboard::Key::Named(keyboard::key::Named::ArrowLeft) => {
                        self.memory_editor.move_cursor(0, -1);
                        self.memory_editor.focus_grid()
                    }
                    keyboard::Key::Named(keyboard::key::Named::ArrowRight) | keyboard::Key::Named(keyboard::key::Named::Tab) => {
                        self.memory_editor.move_cursor(0, 1);
                        self.memory_editor.focus_grid()
                    }
                    keyboard::Key::Named(keyboard::key::Named::PageUp) => {
                        self.memory_editor.move_cursor(-(super::memory_editor::PAGE_ROWS as i32), 0);
                        Task::batch([self.memory_editor.ensure_cursor_visible(), self.memory_editor.focus_grid()])
                    }
                    keyboard::Key::Named(keyboard::key::Named::PageDown) => {
                        self.memory_editor.move_cursor(super::memory_editor::PAGE_ROWS as i32, 0);
                        Task::batch([self.memory_editor.ensure_cursor_visible(), self.memory_editor.focus_grid()])
                    }
                    keyboard::Key::Character(c) => {
                        let hex_digit = match c.as_str() {
                            "0" => Some(0),
                            "1" => Some(1),
                            "2" => Some(2),
                            "3" => Some(3),
                            "4" => Some(4),
                            "5" => Some(5),
                            "6" => Some(6),
                            "7" => Some(7),
                            "8" => Some(8),
                            "9" => Some(9),
                            "a" | "A" => Some(10),
                            "b" | "B" => Some(11),
                            "c" | "C" => Some(12),
                            "d" | "D" => Some(13),
                            "e" | "E" => Some(14),
                            "f" | "F" => Some(15),
                            _ => None,
                        };
                        if let Some(hex_digit) = hex_digit {
                            let cursor_row_before = self.memory_editor.cursor_row();
                            if let Err(err) = self.memory_editor.edit_hex(self.state.pid, hex_digit) {
                                self.state.push_error(AppError::memory_editor(err));
                            }
                            if self.memory_editor.cursor_row() != cursor_row_before {
                                Task::batch([self.memory_editor.ensure_cursor_visible(), self.memory_editor.focus_grid()])
                            } else {
                                self.memory_editor.focus_grid()
                            }
                        } else {
                            Task::none()
                        }
                    }
                    _ => Task::none(),
                }
            }
            Message::MemoryEditorEditHex(hex_digit) => {
                if !self.memory_editor.is_grid_focused() {
                    return Task::none();
                }
                let cursor_row_before = self.memory_editor.cursor_row();
                if let Err(err) = self.memory_editor.edit_hex(self.state.pid, hex_digit) {
                    self.state.push_error(AppError::memory_editor(err));
                }
                if self.memory_editor.cursor_row() != cursor_row_before {
                    self.memory_editor.ensure_cursor_visible()
                } else {
                    Task::none()
                }
            }
            Message::MemoryEditorInspectorValueChanged(kind, value) => {
                self.memory_editor.set_inspector_value_text(kind, value);
                self.memory_editor.set_grid_focused(false);
                Task::none()
            }
            Message::MemoryEditorInspectorValueSubmit(kind) => {
                match self.memory_editor.submit_inspector_value(self.state.pid, kind) {
                    Ok(()) => {
                        self.state.clear_errors();
                        return self.memory_editor.focus_grid();
                    }
                    Err(err) => self.state.push_error(AppError::memory_editor(err)),
                }
                Task::none()
            }
            Message::MemoryEditorDataTypeChanged(search_type) => {
                self.memory_editor.set_data_type(search_type);

                if let Some(index) = self.memory_editor_result_index
                    && let Some(current_search) = self.state.searches.get_mut(self.state.current_search)
                {
                    let results = current_search.collect_results();
                    let mut updated_results = (*results).clone();
                    if let Some(result) = updated_results.get_mut(index) {
                        result.search_type = search_type;
                        current_search.set_cached_results(updated_results);
                        self.refresh_counter = self.refresh_counter.wrapping_add(1);
                    }
                }

                self.memory_editor.focus_grid()
            }
            Message::MemoryEditorScrolled(viewport) => {
                self.memory_editor.set_viewport_and_refresh(viewport, self.state.pid as process_memory::Pid);
                self.memory_editor.focus_grid()
            }
            Message::MemoryEditorTick => {
                if matches!(self.app_state, AppState::MemoryEditor) {
                    self.memory_editor.tick(self.state.pid as process_memory::Pid);
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
            Message::MemoryEditorFadeTick => {
                if matches!(self.app_state, AppState::MemoryEditor) {
                    self.memory_editor.tick_fade_animation();
                }
                Task::none()
            }
            Message::MemoryEditorUndo => {
                if !self.memory_editor.is_grid_focused() {
                    return Task::none();
                }
                match self.memory_editor.undo(self.state.pid) {
                    Ok(Some(address)) => {
                        self.state.clear_errors();
                        if self.memory_editor.focus_on(address).is_ok() {
                            return Task::batch([
                                self.memory_editor.ensure_cursor_visible(),
                                self.memory_editor.focus_grid(),
                                Task::done(Message::MemoryEditorFadeTick),
                            ]);
                        }
                        return Task::done(Message::MemoryEditorFadeTick);
                    }
                    Ok(None) => {}
                    Err(err) => self.state.push_error(AppError::memory_editor(err)),
                }
                Task::none()
            }
            Message::MemoryEditorRedo => {
                if !self.memory_editor.is_grid_focused() {
                    return Task::none();
                }
                match self.memory_editor.redo(self.state.pid) {
                    Ok(Some(address)) => {
                        self.state.clear_errors();
                        if self.memory_editor.focus_on(address).is_ok() {
                            return Task::batch([
                                self.memory_editor.ensure_cursor_visible(),
                                self.memory_editor.focus_grid(),
                                Task::done(Message::MemoryEditorFadeTick),
                            ]);
                        }
                        return Task::done(Message::MemoryEditorFadeTick);
                    }
                    Ok(None) => {}
                    Err(err) => self.state.push_error(AppError::memory_editor(err)),
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
                match crate::load_cheat_table(&path, &self.state.process_name) {
                    Ok(searches) => {
                        self.state.searches = searches;
                        self.state.current_search = 0;
                        self.editing_result = None;
                        self.clear_change_tracker();
                        self.cheat_table_status = format!("Loaded: {}", path.display());
                    }
                    Err(e) => self.cheat_table_status = format!("Load error: {e}"),
                }
                Task::none()
            }
            Message::ToggleHexDisplay => {
                self.hex_display = !self.hex_display;
                self.persist_settings();
                Task::none()
            }
            Message::ToggleAutoReconnect => {
                self.auto_reconnect = !self.auto_reconnect;
                self.persist_settings();
                Task::none()
            }
            Message::ToggleCheckForUpdates => {
                self.check_for_updates = !self.check_for_updates;
                self.persist_settings();
                Task::none()
            }
            Message::UpdateCheckCompleted(latest) => {
                if let Some(tag) = latest
                    && crate::update_check::is_newer(&tag, crate::VERSION)
                {
                    self.latest_version = Some(tag);
                }
                Task::none()
            }
            Message::OpenLatestRelease => {
                let _ = webbrowser::open("https://github.com/mkrueger/game_cheetah/releases/latest");
                Task::none()
            }
            Message::OpenConfigDir => {
                let path = crate::config_dir();
                if let Err(e) = std::fs::create_dir_all(&path) {
                    self.state.push_error(AppError::Generic {
                        message: format!("Cannot create {}: {e}", path.display()),
                    });
                } else if let Err(e) = opener::open(&path) {
                    self.state.push_error(AppError::Generic {
                        message: format!("Cannot open {}: {e}", path.display()),
                    });
                }
                Task::none()
            }
            Message::CopyConfigDir => icy_ui::clipboard::STANDARD.write_text(crate::config_dir().display().to_string()),
            Message::DismissError => {
                self.state.dismiss_error();
                Task::none()
            }
        };
        Task::batch([update_check_task, message_task])
    }

    fn clear_change_tracker(&mut self) {
        self.value_change_tracker.clear();
        self.changed_addresses.clear();
    }

    /// Read current values for all results in the active search and record
    /// which addresses changed since the last call.
    ///
    /// Skipped entirely if the result set exceeds `MAX_TRACKED_RESULTS` to keep
    /// the UI responsive. Callers must also avoid invoking this while a search
    /// is in progress (intermediate result sets can be enormous).
    fn update_change_tracker(&mut self) {
        /// Upper bound on the number of results we'll re-read per tick.
        /// At ~1 syscall per address, going much beyond this stalls the UI thread.
        const MAX_TRACKED_RESULTS: usize = 4096;

        let results = self.state.searches[self.state.current_search].collect_results();
        if results.len() > MAX_TRACKED_RESULTS {
            // Too many candidates to poll every tick — user needs to filter further.
            self.clear_change_tracker();
            return;
        }
        let pid = self.state.pid;
        let hex_display = self.hex_display;
        let counter = self.refresh_counter;

        // Open the process handle once for the whole pass instead of per result.
        let Ok(handle) = (pid as process_memory::Pid).try_into_process_handle() else {
            return;
        };

        for result in results.iter() {
            let Some(byte_len) = result.search_type.fixed_byte_length() else { continue };
            let Ok(buf) = copy_address(result.addr, byte_len, &handle) else { continue };
            let val = SearchValue(result.search_type, buf);
            let value_str = if hex_display { val.to_hex_string() } else { val.to_string() };

            let prev = self.value_change_tracker.insert(result.addr, value_str.clone());
            if let Some(prev_val) = prev
                && prev_val != value_str
            {
                self.changed_addresses.insert(result.addr, counter);
            }
        }

        // Prune addresses no longer in the result set (O(n) via a single pass over results).
        let live: std::collections::HashSet<usize> = results.iter().map(|r| r.addr).collect();
        self.value_change_tracker.retain(|addr, _| live.contains(addr));
        self.changed_addresses.retain(|addr, _| live.contains(addr));
    }

    /// While a row is being edited, refresh the buffered text from the live
    /// memory value as long as the user hasn't typed since the last sync.
    /// Without this the input keeps showing the value that was current when
    /// editing began, even after the game has changed memory many times.
    ///
    /// Reads memory directly instead of going through `value_change_tracker`
    /// so the editor stays current even when the tracker is skipped (large
    /// result sets, in-flight searches, briefly stale `is_process_running`
    /// throttle window).
    fn sync_editing_buffer(&mut self) {
        let Some((idx, buffer, snapshot)) = self.editing_result.as_mut() else {
            return;
        };
        let Some(search_context) = self.state.searches.get(self.state.current_search) else {
            return;
        };
        let results = search_context.collect_results();
        let Some(result) = results.get(*idx) else { return };
        let Some(byte_len) = result.search_type.fixed_byte_length() else {
            return;
        };
        let Ok(handle) = (self.state.pid as process_memory::Pid).try_into_process_handle() else {
            return;
        };
        let Ok(buf) = copy_address(result.addr, byte_len, &handle) else {
            return;
        };
        let val = SearchValue(result.search_type, buf);
        let live = if self.hex_display { val.to_hex_string() } else { val.to_string() };

        if buffer == snapshot {
            // User has not typed since the last sync — adopt the new live
            // value (even if equal to the snapshot, the assignment is a
            // no-op so the early-out is just an optimization).
            if *buffer != live {
                *buffer = live.clone();
            }
            *snapshot = live;
        } else {
            // User typed; just keep the snapshot up to date so the next
            // time the typed buffer happens to coincide with live again,
            // sync resumes (e.g., after they erase their edit).
            *snapshot = live;
        }
    }

    pub fn theme(&self) -> Theme {
        Theme::dark().clone()
    }

    pub fn view(&self) -> Element<'_, Message> {
        match self.app_state {
            AppState::MainWindow => crate::main_window::view_main_window(self),
            AppState::Settings => crate::main_window::view_settings(self),
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
        // re-read memory and update on screen. ~30 Hz keeps fast-changing
        // game values (position, velocity, ammo) visually fluid without
        // overwhelming the memory-read path.
        let live_results_tick = if matches!(self.app_state, AppState::InProcess) {
            icy_ui::time::every(Duration::from_millis(33)).map(|_| Message::Tick)
        } else {
            icy_ui::Subscription::none()
        };

        // Keep the process list fresh while the user is picking a process so
        // newly launched games appear and exited ones disappear without a
        // manual refresh. The actual scan is throttled inside `update()` to
        // at most once every 500 ms.
        let process_list_tick = if matches!(self.app_state, AppState::ProcessSelection) {
            icy_ui::time::every(Duration::from_millis(1000)).map(|_| Message::TickProcess)
        } else {
            icy_ui::Subscription::none()
        };

        // Fade highlights are time-based (`Instant::now()` in the view), so
        // they need redraws even when the memory bytes themselves are not
        // changing. This drives only the animation; it does not re-read target
        // process memory.
        let memory_editor_fade_tick = if matches!(self.app_state, AppState::MemoryEditor) && self.memory_editor.has_active_fades() {
            icy_ui::time::every(Duration::from_millis(16)).map(|_| Message::MemoryEditorFadeTick)
        } else {
            icy_ui::Subscription::none()
        };

        let keyboard_sub: icy_ui::Subscription<Message> = if matches!(self.app_state, AppState::MemoryEditor) {
            icy_ui::Subscription::none()
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
            // Tab/Shift+Tab for focus navigation in normal mode. Also map
            // Escape to "back to main menu" while showing the secondary
            // dialogs (process picker, About, Settings) so the user can
            // dismiss them without reaching for the close button.
            //
            // The closure must be non-capturing per icy_ui's Subscription
            // contract, so the current AppState is plumbed through with
            // `Subscription::with` and matched inside the closure.
            keyboard::listen().with(self.app_state).filter_map(|(state, event)| {
                let keyboard::Event::KeyPressed { key, modifiers, .. } = event else {
                    return None;
                };
                let dismissable = matches!(state, AppState::ProcessSelection | AppState::About | AppState::Settings);
                match key {
                    keyboard::Key::Named(keyboard::key::Named::Tab) => {
                        if modifiers.shift() {
                            Some(Message::FocusPrevious)
                        } else {
                            Some(Message::FocusNext)
                        }
                    }
                    keyboard::Key::Named(keyboard::key::Named::Escape) if dismissable => Some(Message::MainMenu),
                    _ => None,
                }
            })
        };
        // Escape on dismissable dialogs (process picker, About, Settings)
        // needs to fire regardless of whether some focused widget already
        // marked the event as captured, so use `event::listen_with` instead
        // of `keyboard::listen()` (which only sees Status::Ignored events).
        // The handler must be a non-capturing `fn`, so the AppState filter
        // happens in `update` (`Message::DismissDialog`).
        let dismiss_sub = if matches!(self.app_state, AppState::ProcessSelection | AppState::About | AppState::Settings) {
            icy_ui::event::listen_with(|event, _status, _window| match event {
                icy_ui::Event::Keyboard(keyboard::Event::KeyPressed {
                    key: keyboard::Key::Named(keyboard::key::Named::Escape),
                    ..
                }) => Some(Message::DismissDialog),
                _ => None,
            })
        } else {
            icy_ui::Subscription::none()
        };
        icy_ui::Subscription::batch([live_results_tick, process_list_tick, memory_editor_fade_tick, dismiss_sub, keyboard_sub])
    }
}
