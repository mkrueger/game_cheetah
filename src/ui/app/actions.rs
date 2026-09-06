//! User-initiated actions invoked from the view code.
//!
//! These methods translate UI events (clicks, key presses, edits) into
//! mutations on the engine state and side effects on the target process.
//! Grouped here so the top-level [`App`](super::App) module focuses on
//! lifecycle and rendering glue.

use process_memory::{PutAddress, TryIntoProcessHandle, copy_address};

use super::{App, AppState};
use crate::{AppError, FreezeMessage, GameCheetahEngine, MessageCommand, SearchContext, SearchResult, SearchType, SearchValue};

impl App {
    // ---- Process attach / detach ---------------------------------------

    pub fn attach_action(&mut self) {
        self.state.update_process_data();
        self.last_process_refresh = std::time::Instant::now();
        self.state.set_focus = true;
        self.process_selection = Default::default();
        self.app_state = AppState::ProcessSelection;
    }

    pub fn select_process(&mut self, process: &crate::ProcessInfo) {
        self.clear_result_interaction();
        self.state.select_process(process);
        self.app_state = AppState::InProcess;
        self.state.process_filter.clear();
    }

    pub fn back_to_main_menu(&mut self) {
        self.app_state = AppState::MainWindow;
        self.state = GameCheetahEngine::default();
        self.clear_change_tracker();
        self.cached_process_handle = None;
        self.clear_result_interaction();
        self.cheat_table_status.clear();
        self.cheat_table_status_at = None;
    }

    // ---- Search tabs ---------------------------------------------------

    pub fn new_search(&mut self) {
        self.clear_result_interaction();
        self.state.new_search();
        self.search_value_request_focus = true;
        self.clear_change_tracker();
    }

    pub fn close_search(&mut self, index: usize) {
        if index >= self.state.searches.len() {
            return;
        }
        self.clear_result_interaction();
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
        self.clear_result_interaction();
        self.clear_change_tracker();
    }

    pub fn switch_search(&mut self, index: usize) {
        if index < self.state.searches.len() {
            self.state.current_search = index;
            self.clear_result_interaction();
            self.search_value_request_focus = self.state.searches[index].get_result_count() == 0;
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

    // ---- Search execution ----------------------------------------------

    pub fn start_search(&mut self) {
        let search_index = self.state.current_search;
        let Some(current_search) = self.state.searches.get_mut(search_index) else {
            return;
        };
        if current_search.searching != crate::SearchMode::None {
            return;
        }
        let search_type = current_search.search_type;
        if search_type == SearchType::Unknown {
            self.clear_result_interaction();
            self.state.take_memory_snapshot(search_index);
            return;
        }
        if current_search.search_value_text.is_empty() {
            return;
        }
        match search_type.from_string(&current_search.search_value_text) {
            Ok(_) => {
                let has_results = current_search.get_result_count() > 0;
                self.clear_result_interaction();
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
        self.clear_result_interaction();
        self.state.unknown_search_compare(self.state.current_search, comparison);
    }

    pub fn undo_search(&mut self) {
        self.clear_result_interaction();
        if let Some(search_context) = self.state.searches.get_mut(self.state.current_search) {
            search_context.undo_last_search();
        }
        self.clear_change_tracker();
    }

    pub fn clear_results(&mut self) {
        self.state.remove_freezes(self.state.current_search);
        if let Some(search_context) = self.state.searches.get_mut(self.state.current_search) {
            search_context.clear_results();
        }
        self.clear_result_interaction();
        self.search_value_request_focus = true;
        self.state.show_results = false;
        self.clear_change_tracker();
    }

    // ---- Freeze / unfreeze ---------------------------------------------

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
            self.state.remove_freeze(self.state.current_search, result.addr);
        }
    }

    pub fn toggle_freeze_all(&mut self) {
        let freeze_sender = self.state.freeze_sender.clone();
        let pid = self.state.pid;
        let mut send_error = None;
        let mut unfreeze_addresses = Vec::new();
        if let Some(search_context) = self.state.searches.get_mut(self.state.current_search) {
            let results = search_context.collect_results();
            if results.is_empty() {
                return;
            }
            let all_frozen = results.iter().all(|r| search_context.freezed_addresses.contains(&r.addr));
            if all_frozen {
                for result in results.iter() {
                    unfreeze_addresses.push(result.addr);
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
        for addr in unfreeze_addresses {
            self.state.remove_freeze(self.state.current_search, addr);
        }
        if let Some(error) = send_error {
            self.state.push_error(error);
        }
    }

    // ---- Result row mutations ------------------------------------------

    pub fn remove_result(&mut self, index: usize) {
        // Row indices may shift, but selection follows the result identity.
        self.editing_result = None;
        self.result_edit_request_focus = false;
        let mut removed_address = None;
        if let Some(search_context) = self.state.searches.get_mut(self.state.current_search) {
            let results = search_context.collect_results();
            if index < results.len() {
                let result = results[index];
                search_context.push_undo_state(std::sync::Arc::clone(&results));
                let mut new_results = (*results).clone();
                new_results.remove(index);
                if !new_results.iter().any(|other| other.addr == result.addr) {
                    removed_address = Some(result.addr);
                }
                if self
                    .selected_result
                    .is_some_and(|selected| selected.addr == result.addr && selected.search_type == result.search_type)
                {
                    self.selected_result = new_results.get(index).or_else(|| new_results.last()).copied();
                    self.result_selection_request_scroll = true;
                }
                self.search_value_request_focus = new_results.is_empty();
                search_context.set_cached_results(new_results);
            }
        }
        if let Some(addr) = removed_address {
            self.state.remove_freeze(self.state.current_search, addr);
        }
        self.clear_change_tracker();
    }

    /// Write the typed value back to the target process. Returns whether the
    /// write succeeded.
    pub fn commit_result_value(&mut self, index: usize, value_text: &str) -> bool {
        self.write_result_value(index, value_text, true)
    }

    /// Same as [`Self::commit_result_value`] but suppresses the "invalid
    /// value" toast when parsing fails. Used for live, per-keystroke writes
    /// so transient half-typed numbers don't spam the error queue.
    pub fn try_write_result_value(&mut self, index: usize, value_text: &str) -> bool {
        self.write_result_value(index, value_text, false)
    }

    fn write_result_value(&mut self, index: usize, value_text: &str, report_parse_errors: bool) -> bool {
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
                if report_parse_errors {
                    self.state.push_error(AppError::InvalidValue {
                        value: value_text.to_owned(),
                        source: err,
                    });
                }
                false
            }
        }
    }

    // ---- Memory editor -------------------------------------------------

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
        let replacement = SearchResult::new(old.addr, new_type);
        new_results[index] = replacement;
        search_context.set_cached_results(new_results);
        if self
            .selected_result
            .is_some_and(|selected| selected.addr == old.addr && selected.search_type == old.search_type)
        {
            self.selected_result = Some(replacement);
        }
        self.editing_result = None;
        self.result_edit_request_focus = false;

        // Look up the new index (address is preserved; type may have
        // shifted the sort position).
        let refreshed = search_context.collect_results();
        let new_index = refreshed.iter().position(|r| r.addr == old.addr && r.search_type == new_type);
        self.memory_editor_result_index = new_index;

        let byte_length = new_type.fixed_byte_length().unwrap_or(1);
        self.memory_editor.update_result_type(new_type, byte_length);
    }

    // ---- Cheat tables --------------------------------------------------

    pub fn save_cheat_table(&mut self) {
        let path = crate::default_cheat_table_path(&self.state.process_name);
        self.cheat_table_status = match crate::save_cheat_table(&self.state, &path) {
            Ok(()) => format!("Saved: {}", path.display()),
            Err(e) => format!("Save error: {e}"),
        };
        self.cheat_table_status_at = Some(std::time::Instant::now());
    }

    pub fn load_cheat_table(&mut self) {
        let path = crate::default_cheat_table_path(&self.state.process_name);
        self.load_cheat_table_from_path(&path);
    }

    fn load_cheat_table_from_path(&mut self, path: &std::path::Path) {
        match crate::load_cheat_table(path, &self.state.process_name) {
            Ok(searches) => {
                // Only release old freezes after loading succeeds. Shared
                // addresses are released when their last owning tab is cleared.
                for index in 0..self.state.searches.len() {
                    self.state.remove_freezes(index);
                }
                self.state.searches = searches;
                self.state.current_search = 0;
                self.clear_result_interaction();
                self.clear_change_tracker();
                self.cheat_table_status = format!("Loaded: {}", path.display());
            }
            Err(e) => self.cheat_table_status = format!("Load error: {e}"),
        }
        self.cheat_table_status_at = Some(std::time::Instant::now());
    }
}

/// Compute the byte length of a result for the memory-editor's
/// result-range highlight. Fixed-width numeric types map straight to
/// their `SearchType::fixed_byte_length()`; variable-length string
/// searches use the originally-entered text. Unknown/Guess fall back
/// to one byte so the highlight is at least visible.
fn result_byte_length(result: &SearchResult, ctx: &SearchContext) -> usize {
    result.search_type.byte_length_for_text(&ctx.search_value_text).unwrap_or(1).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ADDRESS: usize = 0x1234;

    fn shared_freeze_app() -> (App, crossbeam_channel::Receiver<FreezeMessage>) {
        let mut app = App::default();
        let (tx, rx) = crossbeam_channel::unbounded();
        app.state.freeze_sender = tx;
        app.state.process_name = "freeze-regression".to_owned();
        app.new_search();
        for search in &mut app.state.searches {
            search.set_cached_results(vec![SearchResult::new(ADDRESS, SearchType::Int)]);
            search.freezed_addresses.insert(ADDRESS);
        }
        (app, rx)
    }

    #[test]
    fn releasing_one_tab_preserves_another_tabs_freeze() {
        for action in ["close", "close_others", "clear", "remove", "toggle", "toggle_all"] {
            let (mut app, rx) = shared_freeze_app();
            match action {
                "close" => app.close_search(1),
                "close_others" => app.close_other_searches(0),
                "clear" => app.clear_results(),
                "remove" => app.remove_result(0),
                "toggle" => app.toggle_freeze(0),
                "toggle_all" => app.toggle_freeze_all(),
                _ => unreachable!(),
            }
            assert!(app.state.searches[0].freezed_addresses.contains(&ADDRESS), "{action}");
            assert!(rx.try_recv().is_err(), "{action} stopped another tab's freeze");

            app.state.remove_freezes(0);
            let message = rx.try_recv().expect("last owner must stop the freeze");
            assert!(matches!(message.msg, MessageCommand::Unfreeze));
            assert_eq!(message.addr, ADDRESS);
            assert!(rx.try_recv().is_err());
        }
    }

    #[test]
    fn removing_one_typed_result_preserves_same_address_freeze() {
        let (mut app, rx) = shared_freeze_app();
        app.state.searches[1].set_cached_results(vec![SearchResult::new(ADDRESS, SearchType::Int), SearchResult::new(ADDRESS, SearchType::Float)]);
        app.remove_result(0);
        assert!(app.state.searches[1].freezed_addresses.contains(&ADDRESS));
        assert!(rx.try_recv().is_err());
    }

    struct TableFixture(std::path::PathBuf);

    impl TableFixture {
        fn new(app: &App) -> Self {
            static NEXT_ID: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!("game-cheetah-table-test-{}-{id}.toml", std::process::id()));
            std::fs::OpenOptions::new().write(true).create_new(true).open(&path).unwrap();
            let fixture = Self(path);
            crate::save_cheat_table(&app.state, &fixture.0).unwrap();
            fixture
        }
    }

    impl Drop for TableFixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[test]
    fn loading_table_releases_old_freezes_once() {
        let (mut app, rx) = shared_freeze_app();
        let fixture = TableFixture::new(&app);
        app.load_cheat_table_from_path(&fixture.0);
        assert!(app.cheat_table_status.starts_with("Loaded:"));
        assert!(app.state.searches.iter().all(|search| search.freezed_addresses.is_empty()));
        assert_eq!(app.state.searches[0].get_result_count(), 1);
        let message = rx.try_recv().expect("loading must stop the old freeze");
        assert!(matches!(message.msg, MessageCommand::Unfreeze));
        assert_eq!(message.addr, ADDRESS);
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn failed_table_load_preserves_existing_freezes() {
        let (mut app, rx) = shared_freeze_app();
        let fixture = TableFixture::new(&app);
        app.state.process_name = "different-process".to_owned();
        app.load_cheat_table_from_path(&fixture.0);
        assert!(app.cheat_table_status.starts_with("Load error:"));
        assert!(app.state.searches.iter().all(|search| search.freezed_addresses.contains(&ADDRESS)));
        assert_eq!(app.state.current_search, 1);
        assert!(rx.try_recv().is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn editing_utf16_result_writes_utf16_without_touching_following_bytes() {
        let mut app = App::default();
        app.state.pid = std::process::id() as _;
        let mut bytes = Box::new([0xCCu8; 16]);
        let result = SearchResult::new(bytes.as_mut_ptr() as usize, SearchType::StringUtf16);
        app.state.searches[0].search_value_text = "A😀B".to_owned();
        app.state.searches[0].set_cached_results(vec![result]);
        assert_eq!(result_byte_length(&result, &app.state.searches[0]), 8);
        assert!(app.commit_result_value(0, "A😀B"));
        assert_eq!(&bytes[..8], &[65, 0, 0x3D, 0xD8, 0, 0xDE, 66, 0]);
        assert!(bytes[8..].iter().all(|byte| *byte == 0xCC));
    }
}
