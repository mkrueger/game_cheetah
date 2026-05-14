use std::{
    collections::HashMap,
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

use process_memory::{PutAddress, TryIntoProcessHandle, copy_address};

use crate::{
    AppError, FreezeMessage, GameCheetahEngine, MessageCommand, SearchContext, SearchMode, SearchResult, SearchType, SearchValue,
    ui::{
        in_process_view, main_window,
        memory_editor::MemoryEditor,
        process_selection,
        process_selection::{ProcessSortColumn, SortDirection},
    },
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

/// Visible duration of a "value changed" row highlight.
pub const CHANGE_HIGHLIGHT: Duration = Duration::from_millis(1500);

/// Channel used by the background update-check thread to report results.
type UpdateCheckRx = crossbeam_channel::Receiver<Option<String>>;

pub struct App {
    pub app_state: AppState,
    pub state: GameCheetahEngine,

    pub renaming_search_index: Option<usize>,
    pub rename_search_text: String,
    /// One-shot flag: when `true`, the rename text field should grab
    /// keyboard focus on the next frame and then clear the flag.
    pub rename_request_focus: bool,

    /// `(row_index, typed_buffer)` while a result row's value is being
    /// edited. We don't read the live value into this buffer per frame —
    /// once the user clicks/focuses the field, it owns the keystrokes
    /// until they commit (Enter) or cancel (Escape / focus loss).
    pub editing_result: Option<(usize, String)>,

    pub process_sort_column: ProcessSortColumn,
    pub process_sort_direction: SortDirection,

    /// Brief status next to the Save/Load buttons.
    pub cheat_table_status: String,

    /// Reattach to a process with the same name after the attached one exits.
    pub auto_reconnect: bool,

    /// Check the GitHub releases API once per launch.
    pub check_for_updates: bool,

    update_check_rx: Option<UpdateCheckRx>,
    /// Tag of the latest release if it is newer than [`crate::VERSION`].
    pub latest_version: Option<String>,

    /// Last-read value string per address for the active search.
    pub value_change_tracker: HashMap<usize, String>,
    /// Address → time when the value last changed. A row is highlighted
    /// while `Instant::now() - stored < CHANGE_HIGHLIGHT`.
    pub changed_addresses: HashMap<usize, Instant>,

    /// Throttle process-list refresh in [`AppState::ProcessSelection`].
    last_process_refresh: Instant,

    /// Last attempt to find the previously-attached process by name.
    last_reattach_attempt: Instant,

    pub memory_editor: MemoryEditor,
    pub memory_editor_result_index: Option<usize>,

    /// Tracks if a search just completed last frame, so the next frame can
    /// repopulate the value cache before the row callback runs.
    pub last_searching_was_active: bool,
}

impl Default for App {
    /// Returns an `App` with all settings at their built-in defaults.
    ///
    /// Note: does **not** read persisted user settings — that's what
    /// [`App::new`] is for. Keeping `Default` settings-free makes
    /// unit tests deterministic regardless of the developer's local
    /// config dir.
    fn default() -> Self {
        Self {
            app_state: AppState::default(),
            state: GameCheetahEngine::default(),
            renaming_search_index: None,
            rename_search_text: String::new(),
            rename_request_focus: false,
            editing_result: None,
            process_sort_column: ProcessSortColumn::default(),
            process_sort_direction: SortDirection::default(),
            cheat_table_status: String::new(),
            auto_reconnect: false,
            check_for_updates: false,
            update_check_rx: None,
            latest_version: None,
            value_change_tracker: HashMap::new(),
            changed_addresses: HashMap::new(),
            last_process_refresh: Instant::now() - Duration::from_secs(10),
            last_reattach_attempt: Instant::now() - Duration::from_secs(10),
            memory_editor: MemoryEditor::default(),
            memory_editor_result_index: None,
            last_searching_was_active: false,
        }
    }
}

impl App {
    pub fn new() -> Self {
        let settings = crate::UserSettings::load();
        Self {
            auto_reconnect: settings.auto_reconnect,
            check_for_updates: settings.check_for_updates,
            ..Self::default()
        }
    }

    pub fn title(&self) -> String {
        format!("{} {}", crate::APP_NAME, crate::VERSION)
    }

    /// Persist user-settings to disk; logs an error into the in-app error
    /// stack on failure.
    pub(crate) fn persist_settings(&mut self) {
        let settings = crate::UserSettings {
            auto_reconnect: self.auto_reconnect,
            check_for_updates: self.check_for_updates,
        };
        if let Err(e) = settings.save() {
            self.state.push_error(AppError::Generic { message: e });
        }
    }

    /// Kick off the once-per-launch update check on a worker thread.
    fn start_update_check_if_needed(&mut self) {
        if !self.check_for_updates || self.update_check_rx.is_some() || self.latest_version.is_some() {
            return;
        }
        let (tx, rx) = crossbeam_channel::bounded::<Option<String>>(1);
        std::thread::spawn(move || {
            let result = crate::update_check::fetch_latest_version();
            let _ = tx.send(result);
        });
        self.update_check_rx = Some(rx);
    }

    fn poll_update_check(&mut self) {
        if let Some(rx) = &self.update_check_rx
            && let Ok(latest) = rx.try_recv()
        {
            self.update_check_rx = None;
            if let Some(tag) = latest
                && crate::update_check::is_newer(&tag, crate::VERSION)
            {
                self.latest_version = Some(tag);
            }
        }
    }

    /// Periodic per-frame housekeeping run before the view code.
    fn tick(&mut self, ctx: &egui::Context) {
        // Defensive: make sure none of egui's debug paint overlays
        // (red/blue widget outlines from `style.debug.show_interactive_widgets`
        // or `show_widget_hits` etc.) sneak back on between frames — they
        // can persist via egui's built-in style editor and become visible
        // mid-session, e.g. as red rectangles appearing during scrolling.
        if ctx.global_style().debug != egui::style::DebugOptions::default() {
            ctx.global_style_mut(|style| style.debug = egui::style::DebugOptions::default());
        }

        // Search context state machine: cycle each context's search-mode
        // flag back to `None` once `search_complete` flips.
        for search_context in &mut self.state.searches {
            search_context.update_search_mode();
        }

        match self.app_state {
            AppState::ProcessSelection => {
                if self.last_process_refresh.elapsed() >= Duration::from_millis(1000) {
                    self.state.update_process_data();
                    self.last_process_refresh = Instant::now();
                }
                // Schedule the next repaint to coincide with the next
                // cache refresh. The list is cached, so painting more
                // often just wastes CPU.
                ctx.request_repaint_after(Duration::from_millis(1000));
            }
            AppState::InProcess => {
                self.state.detach_if_gone();
                if self.auto_reconnect
                    && self.state.pid == 0
                    && !self.state.process_name.is_empty()
                    && self.last_reattach_attempt.elapsed() >= Duration::from_secs(1)
                {
                    self.last_reattach_attempt = Instant::now();
                    self.state.update_process_data();
                    let target = self.state.process_name.clone();
                    if let Some(process) = self.state.processes.iter().find(|p| p.name == target).cloned() {
                        self.state.select_process(&process);
                    }
                }
                // Finalize in-flight searches so the tracker refresh below
                // sees the final result set.
                {
                    let ctx_search = &mut self.state.searches[self.state.current_search];
                    if !matches!(ctx_search.searching, SearchMode::None) {
                        let _ = ctx_search.collect_results();
                    }
                    if ctx_search.search_complete.load(Ordering::SeqCst) {
                        // Drain trailing batches so addresses don't keep
                        // shifting after the search ends.
                        loop {
                            let before = ctx_search.get_result_count();
                            let _ = ctx_search.collect_results();
                            let after = ctx_search.get_result_count();
                            if before == after {
                                break;
                            }
                        }
                        ctx_search.searching = SearchMode::None;
                    }
                }

                let search_running = self.state.searches.iter().any(|s| !matches!(s.searching, SearchMode::None));
                if !search_running && self.state.is_process_running() {
                    self.update_change_tracker();
                }

                // 30 Hz repaint for fluid live values.
                ctx.request_repaint_after(Duration::from_millis(33));
            }
            AppState::MemoryEditor => {
                self.memory_editor.tick(self.state.pid as process_memory::Pid);
                ctx.request_repaint_after(Duration::from_millis(33));
            }
            _ => {}
        }

        // Prune expired change highlights so rows return to their default
        // appearance at the next frame (and the tracker doesn't grow
        // unboundedly while scanning long sessions).
        self.changed_addresses.retain(|_, t| t.elapsed() < CHANGE_HIGHLIGHT);
    }

    /// Read current values for all results in the active search and record
    /// which addresses changed since the last call.
    fn update_change_tracker(&mut self) {
        /// Upper bound on the number of results we'll re-read per tick.
        /// At ~1 syscall per address, going much beyond this stalls the UI.
        const MAX_TRACKED_RESULTS: usize = 4096;

        let search_index = self.state.current_search;
        let results = self.state.searches[search_index].collect_results();
        if results.len() > MAX_TRACKED_RESULTS {
            self.value_change_tracker.clear();
            self.changed_addresses.clear();
            return;
        }
        let pid = self.state.pid;
        let now = Instant::now();

        let search_value_text = self.state.searches[search_index].search_value_text.clone();
        let string_byte_len = search_value_text.len();
        let string_char_count = search_value_text.chars().count();

        let Ok(handle) = (pid as process_memory::Pid).try_into_process_handle() else {
            return;
        };

        for result in results.iter() {
            let value_str = if let Some(byte_len) = result.search_type.fixed_byte_length() {
                let Ok(buf) = copy_address(result.addr, byte_len, &handle) else { continue };
                SearchValue(result.search_type, buf).to_string()
            } else if matches!(result.search_type, SearchType::String | SearchType::StringUtf16) {
                let utf16 = result.search_type == SearchType::StringUtf16;
                let max_bytes = if utf16 { string_char_count * 2 } else { string_byte_len };
                let Some(s) = in_process_view::read_string_from_process(pid as process_memory::Pid, result.addr, utf16, max_bytes) else {
                    continue;
                };
                s
            } else {
                continue;
            };

            let prev = self.value_change_tracker.insert(result.addr, value_str.clone());
            if let Some(prev_val) = prev
                && prev_val != value_str
            {
                self.changed_addresses.insert(result.addr, now);
            }
        }

        // Prune addresses no longer in the result set.
        let live: std::collections::HashSet<usize> = results.iter().map(|r| r.addr).collect();
        self.value_change_tracker.retain(|addr, _| live.contains(addr));
        self.changed_addresses.retain(|addr, _| live.contains(addr));
    }

    pub fn clear_change_tracker(&mut self) {
        self.value_change_tracker.clear();
        self.changed_addresses.clear();
    }

    // ---- Actions invoked from the views --------------------------------

    pub fn attach_action(&mut self) {
        self.state.update_process_data();
        self.app_state = AppState::ProcessSelection;
    }

    pub fn select_process(&mut self, process: &crate::ProcessInfo) {
        self.state.select_process(process);
        self.app_state = AppState::InProcess;
        self.state.process_filter.clear();
    }

    pub fn back_to_main_menu(&mut self) {
        self.app_state = AppState::MainWindow;
        self.state = GameCheetahEngine::default();
        self.clear_change_tracker();
        self.editing_result = None;
    }

    pub fn new_search(&mut self) {
        self.state.new_search();
        self.clear_change_tracker();
    }

    pub fn close_search(&mut self, index: usize) {
        if index >= self.state.searches.len() {
            return;
        }
        self.state.remove_freezes(index);
        self.state.searches.remove(index);
        if self.state.searches.is_empty() {
            self.state.current_search = 0;
            self.state.new_search();
        } else if self.state.current_search > index {
            self.state.current_search -= 1;
        } else if self.state.current_search >= self.state.searches.len() {
            self.state.current_search = self.state.searches.len() - 1;
        }
        self.clear_change_tracker();
    }

    /// Close every search except `keep_index`. The kept search becomes the
    /// active one.
    pub fn close_other_searches(&mut self, keep_index: usize) {
        if keep_index >= self.state.searches.len() {
            return;
        }
        // Walk from the end so indices stay valid as we remove.
        for i in (0..self.state.searches.len()).rev() {
            if i != keep_index {
                self.state.remove_freezes(i);
                self.state.searches.remove(i);
            }
        }
        self.state.current_search = 0;
        self.editing_result = None;
        self.clear_change_tracker();
    }

    pub fn switch_search(&mut self, index: usize) {
        if index < self.state.searches.len() {
            self.state.current_search = index;
            self.editing_result = None;
            self.clear_change_tracker();
        }
    }

    pub fn begin_rename_search(&mut self, index: usize) {
        if let Some(search) = self.state.searches.get(index) {
            self.rename_search_text = search.description.clone();
            self.renaming_search_index = Some(index);
            self.rename_request_focus = true;
        }
    }

    pub fn commit_rename_search(&mut self) {
        if let Some(index) = self.renaming_search_index
            && let Some(search) = self.state.searches.get_mut(index)
        {
            search.description = self.rename_search_text.clone();
        }
        self.renaming_search_index = None;
        self.rename_search_text.clear();
        self.rename_request_focus = false;
    }

    pub fn cancel_rename_search(&mut self) {
        self.renaming_search_index = None;
        self.rename_search_text.clear();
        self.rename_request_focus = false;
    }

    pub fn start_search(&mut self) {
        let search_index = self.state.current_search;
        let Some(current_search) = self.state.searches.get_mut(search_index) else {
            return;
        };
        let search_type = current_search.search_type;
        if search_type == SearchType::Unknown {
            self.state.take_memory_snapshot(search_index);
            return;
        }
        if current_search.search_value_text.is_empty() {
            return;
        }
        match search_type.from_string(&current_search.search_value_text) {
            Ok(_) => {
                let has_results = current_search.get_result_count() > 0;
                if !has_results || search_type == SearchType::String {
                    self.state.initial_search(search_index);
                } else {
                    self.state.filter_searches(search_index);
                }
            }
            Err(err) => {
                self.state.push_error(AppError::SearchValueParse { source: err });
            }
        }
    }

    pub fn unknown_search(&mut self, comparison: crate::UnknownComparison) {
        if let Some(ctx) = self.state.searches.get_mut(self.state.current_search) {
            ctx.unknown_comparison = Some(comparison);
        }
        self.state.unknown_search_compare(self.state.current_search, comparison);
    }

    pub fn undo_search(&mut self) {
        if let Some(search_context) = self.state.searches.get_mut(self.state.current_search)
            && let Some(old) = search_context.old_results.pop()
        {
            search_context.set_cached_results(old);
        }
        self.clear_change_tracker();
    }

    pub fn clear_results(&mut self) {
        if let Some(search_context) = self.state.searches.get_mut(self.state.current_search) {
            search_context.clear_results(&self.state.freeze_sender);
        }
        self.editing_result = None;
        self.clear_change_tracker();
    }

    pub fn toggle_freeze(&mut self, index: usize) {
        let Some(search_context) = self.state.searches.get_mut(self.state.current_search) else {
            return;
        };
        let results = search_context.collect_results();
        let Some(result) = results.get(index).copied() else {
            return;
        };
        let now_freeze = !search_context.freezed_addresses.contains(&result.addr);
        if now_freeze {
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
            search_context.freezed_addresses.remove(&result.addr);
            if let Err(e) = self.state.freeze_sender.send(FreezeMessage::from_addr(MessageCommand::Unfreeze, result.addr)) {
                self.state.push_error(AppError::FreezeChannelClosed { source: e.to_string() });
            }
        }
    }

    pub fn toggle_freeze_all(&mut self) {
        let freeze_sender = self.state.freeze_sender.clone();
        let pid = self.state.pid;
        let mut send_error = None;
        if let Some(search_context) = self.state.searches.get_mut(self.state.current_search) {
            let results = search_context.collect_results();
            if results.is_empty() {
                return;
            }
            let all_frozen = results.iter().all(|r| search_context.freezed_addresses.contains(&r.addr));
            if all_frozen {
                for result in results.iter() {
                    if search_context.freezed_addresses.remove(&result.addr)
                        && let Err(e) = freeze_sender.send(FreezeMessage::from_addr(MessageCommand::Unfreeze, result.addr))
                    {
                        send_error.get_or_insert_with(|| AppError::FreezeChannelClosed { source: e.to_string() });
                    }
                }
            } else if let Ok(handle) = (pid as process_memory::Pid).try_into_process_handle() {
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
        if let Some(error) = send_error {
            self.state.push_error(error);
        }
    }

    pub fn remove_result(&mut self, index: usize) {
        let freeze_sender = self.state.freeze_sender.clone();
        let mut send_error = None;
        if let Some(search_context) = self.state.searches.get_mut(self.state.current_search) {
            let results = search_context.collect_results();
            if index < results.len() {
                let result = results[index];
                if search_context.freezed_addresses.remove(&result.addr)
                    && let Err(e) = freeze_sender.send(FreezeMessage::from_addr(MessageCommand::Unfreeze, result.addr))
                {
                    send_error = Some(AppError::FreezeChannelClosed { source: e.to_string() });
                }
                search_context.old_results.push((*results).clone());
                let mut new_results = (*results).clone();
                new_results.remove(index);
                search_context.set_cached_results(new_results);
            }
        }
        if let Some(error) = send_error {
            self.state.push_error(error);
        }
        self.clear_change_tracker();
    }

    /// Write the typed value back to the target process. Returns whether the
    /// write succeeded.
    pub fn commit_result_value(&mut self, index: usize, value_text: &str) -> bool {
        let Ok(handle) = (self.state.pid as process_memory::Pid).try_into_process_handle() else {
            return false;
        };
        let Some(current_search) = self.state.searches.get_mut(self.state.current_search) else {
            return false;
        };
        let results = current_search.collect_results();
        let Some(result) = results.get(index).copied() else {
            return false;
        };
        match result.search_type.from_string(value_text) {
            Ok(value) => {
                if let Err(err) = handle.put_address(result.addr, &value.1) {
                    self.state.push_error(AppError::MemoryWrite {
                        addr: result.addr,
                        source: err.to_string(),
                    });
                    return false;
                }
                if current_search.freezed_addresses.contains(&result.addr)
                    && let Err(err) = self.state.freeze_sender.send(FreezeMessage {
                        msg: MessageCommand::Freeze,
                        addr: result.addr,
                        value,
                    })
                {
                    self.state.push_error(AppError::FreezeChannelClosed { source: err.to_string() });
                }
                true
            }
            Err(err) => {
                self.state.push_error(AppError::InvalidValue {
                    value: value_text.to_owned(),
                    source: err,
                });
                false
            }
        }
    }

    pub fn open_memory_editor(&mut self, index: usize) {
        let Some(search_context) = self.state.searches.get(self.state.current_search) else {
            return;
        };
        let results = search_context.collect_results();
        let Some(result) = results.get(index).copied() else {
            return;
        };
        let byte_length = result_byte_length(&result, search_context);
        match self.memory_editor.initialize(self.state.pid, result.addr, result.search_type, byte_length) {
            Ok(()) => {
                self.app_state = AppState::MemoryEditor;
                self.memory_editor_result_index = Some(index);
            }
            Err(err) => self.state.push_error(AppError::memory_editor(err)),
        }
    }

    pub fn close_memory_editor(&mut self) {
        self.app_state = AppState::InProcess;
        self.memory_editor_result_index = None;
        self.memory_editor.reset_change_tracker();
    }

    /// Reinterpret the currently-edited cheat result as a different
    /// numeric `SearchType`. Updates the cached result vector in place,
    /// re-resolves the row index (since `set_cached_results` re-sorts
    /// by `(addr, search_type)`), and refreshes the memory editor's
    /// result-range highlight so it reflects the new byte width.
    pub fn change_result_type(&mut self, new_type: SearchType) {
        let Some(index) = self.memory_editor_result_index else {
            return;
        };
        let Some(search_context) = self.state.searches.get(self.state.current_search) else {
            return;
        };
        let results = search_context.collect_results();
        let Some(old) = results.get(index).copied() else {
            return;
        };
        if old.search_type == new_type {
            return;
        }

        // Build a new Vec with the entry replaced. set_cached_results
        // sorts and dedupes, so we need to re-locate the entry afterwards.
        let mut new_results: Vec<SearchResult> = (*results).clone();
        new_results[index] = SearchResult::new(old.addr, new_type);
        search_context.set_cached_results(new_results);

        // Look up the new index (address is preserved; type may have
        // shifted the sort position).
        let refreshed = search_context.collect_results();
        let new_index = refreshed.iter().position(|r| r.addr == old.addr && r.search_type == new_type);
        self.memory_editor_result_index = new_index;

        let byte_length = new_type.fixed_byte_length().unwrap_or(1);
        self.memory_editor.update_result_type(new_type, byte_length);
    }

    pub fn save_cheat_table(&mut self) {
        let path = crate::default_cheat_table_path(&self.state.process_name);
        match crate::save_cheat_table(&self.state, &path) {
            Ok(()) => self.cheat_table_status = format!("Saved: {}", path.display()),
            Err(e) => self.cheat_table_status = format!("Save error: {e}"),
        }
    }

    pub fn load_cheat_table(&mut self) {
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
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.start_update_check_if_needed();
        self.poll_update_check();
        self.tick(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // Render based on current screen.
        match self.app_state {
            AppState::MainWindow => main_window::view_main_window(self, ui),
            AppState::Settings => main_window::view_settings(self, ui),
            AppState::About => main_window::view_about(self, ui),
            AppState::ProcessSelection => process_selection::view_process_selection(self, ui),
            AppState::InProcess => in_process_view::view_in_process(self, ui),
            AppState::MemoryEditor => crate::ui::memory_editor::view_memory_editor(self, ui),
        }

        // Global escape handling for dismissable dialogs.
        if ui.ctx().input(|i| i.key_pressed(egui::Key::Escape)) {
            match self.app_state {
                AppState::ProcessSelection | AppState::About | AppState::Settings => {
                    self.app_state = AppState::MainWindow;
                }
                AppState::MemoryEditor => self.close_memory_editor(),
                _ => {}
            }
        }
    }
}

/// Compute the byte length of a result for the memory-editor's
/// result-range highlight. Fixed-width numeric types map straight to
/// their `SearchType::fixed_byte_length()`; variable-length string
/// searches use the originally-entered text. Unknown/Guess fall back
/// to one byte so the highlight is at least visible.
fn result_byte_length(result: &SearchResult, ctx: &SearchContext) -> usize {
    if let Some(len) = result.search_type.fixed_byte_length() {
        return len;
    }
    match result.search_type {
        SearchType::String => ctx.search_value_text.len().max(1),
        SearchType::StringUtf16 => (ctx.search_value_text.encode_utf16().count() * 2).max(1),
        _ => 1,
    }
}
