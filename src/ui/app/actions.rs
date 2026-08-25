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
        self.cached_process_handle = None;
        self.editing_result = None;
        self.cheat_table_status.clear();
        self.cheat_table_status_at = None;
    }

    // ---- Search tabs ---------------------------------------------------

    pub fn new_search(&mut self) {
        self.state.new_search();
        self.search_value_request_focus = true;
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

    // ---- Result row mutations ------------------------------------------

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
                self.search_value_request_focus = new_results.is_empty();
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
        self.cheat_table_status_at = Some(std::time::Instant::now());
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
