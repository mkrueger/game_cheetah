//! Top-level [`App`] type wiring engine state to the egui front end.
//!
//! Submodules:
//! - [`tick`] — per-frame housekeeping and the in-process change tracker.
//! - [`update_check`] — once-per-launch GitHub release check.
//! - [`actions`] — user-initiated mutations driven by the view code.

mod actions;
mod tick;
mod update_check;

use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use process_memory::ProcessHandle;

use crate::{
    AppError, GameCheetahEngine,
    ui::{
        in_process_view, main_window,
        mem_editor::MemoryEditor,
        process_selection,
        process_selection::{ProcessSelectionState, ProcessSortColumn, SortDirection},
        value_cache::ValueCache,
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

/// Maximum number of `(addr -> last value)` entries kept by the in-process
/// view's change tracker. Sized to comfortably cover any plausible visible
/// row count plus headroom; values scrolled out of view age out via the LRU.
const VALUE_CACHE_CAPACITY: usize = 8192;

/// Channel used by the background update-check thread to report results.
type UpdateCheckRx = crossbeam_channel::Receiver<Option<String>>;

pub struct App {
    pub address_editor: Option<crate::ui::address_editor::AddressEditor>,
    pub pointer_scanner: crate::ui::pointer_scanner::PointerScanner,
    pub(crate) automatic_save: Option<crate::ui::auto_save::AutomaticSave>,
    pub(crate) automatic_save_notice: Option<crate::ui::notice::Notice>,
    last_address_refresh: Instant,
    pub app_state: AppState,
    pub state: GameCheetahEngine,

    pub renaming_search_index: Option<usize>,
    pub rename_search_text: String,
    /// One-shot flag: when `true`, the rename text field should grab
    /// keyboard focus on the next frame and then clear the flag.
    pub rename_request_focus: bool,

    /// One-shot request to return keyboard focus to the search value field.
    pub search_value_request_focus: bool,
    pub(crate) search_value_select_all: bool,

    /// Selected result identity (address and type), independent of row order.
    pub selected_result: Option<crate::SearchResult>,
    /// One-shot request to scroll the selected result into view.
    pub result_selection_request_scroll: bool,
    /// One-shot request to focus the result value editor.
    pub result_edit_request_focus: bool,

    /// `(row_index, typed_buffer)` while a result row's value is being
    /// edited. We don't read the live value into this buffer per frame —
    /// once the user clicks/focuses the field, it owns the keystrokes
    /// until they commit (Enter) or cancel (Escape / focus loss).
    pub editing_result: Option<(usize, String)>,

    pub process_sort_column: ProcessSortColumn,
    pub process_sort_direction: SortDirection,
    /// Selection and expansion are keyed by process identity, never row index.
    pub process_selection: ProcessSelectionState,

    /// Result row under the pointer last frame; drives the hover-only row
    /// action icons.
    pub hovered_result_row: Option<usize>,

    /// Brief Save/Load status shown as a transient toast.
    pub cheat_table_status: String,
    /// Technical information, collapsed by default; never parsed from summary.
    pub cheat_table_status_details: String,
    pub cheat_table_status_at: Option<Instant>,

    /// Reattach to a process with the same name after the attached one exits.
    pub auto_reconnect: bool,

    /// Check the GitHub releases API once per launch.
    pub check_for_updates: bool,

    /// Opt-in: result edits are buffered until Enter instead of written live.
    pub confirm_value_writes: bool,

    /// Experimental address persistence; hidden unless explicitly enabled.
    pub enable_persistence: bool,

    update_check_rx: Option<UpdateCheckRx>,
    /// Tag of the latest release if it is newer than [`crate::VERSION`].
    pub latest_version: Option<String>,

    /// Last-read value bytes per address for the active search. Capped via
    /// an LRU so a million-row search doesn't grow this map without bound —
    /// the visible rows constantly refresh their entries and stay live, while
    /// rows scrolled out of view age out and get evicted.
    pub value_change_tracker: ValueCache<Vec<u8>>,
    /// Address → time when the value last changed. A row is highlighted
    /// while `Instant::now() - stored < CHANGE_HIGHLIGHT`.
    pub changed_addresses: HashMap<usize, Instant>,
    /// Round-robin cursor into the result vector for the bulk change tracker.
    /// Wraps around when it reaches the end of the result list. This lets
    /// the tracker cover arbitrarily-large result sets in `TRACKER_WINDOW`
    /// slices per tick without ever bailing out.
    change_tracker_cursor: usize,
    /// Last time the bulk change tracker actually ran. Together with
    /// `TRACKER_INTERVAL` this throttles its rate independent of the
    /// UI repaint rate.
    last_change_tracker_run: Instant,
    /// Cached `(pid, ProcessHandle)` reused across bulk-tracker invocations.
    /// On Linux this is essentially free; on Windows it avoids re-opening
    /// the process handle every tick.
    cached_process_handle: Option<(process_memory::Pid, ProcessHandle)>,

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
            address_editor: None,
            pointer_scanner: Default::default(),
            automatic_save: None,
            automatic_save_notice: None,
            last_address_refresh: Instant::now(),
            app_state: AppState::default(),
            state: GameCheetahEngine::default(),
            renaming_search_index: None,
            rename_search_text: String::new(),
            rename_request_focus: false,
            search_value_request_focus: false,
            search_value_select_all: false,
            selected_result: None,
            result_selection_request_scroll: false,
            result_edit_request_focus: false,
            editing_result: None,
            process_sort_column: ProcessSortColumn::default(),
            process_sort_direction: SortDirection::default(),
            process_selection: ProcessSelectionState::default(),
            hovered_result_row: None,
            cheat_table_status: String::new(),
            cheat_table_status_details: String::new(),
            cheat_table_status_at: None,
            auto_reconnect: false,
            check_for_updates: false,
            confirm_value_writes: false,
            enable_persistence: false,
            update_check_rx: None,
            latest_version: None,
            value_change_tracker: ValueCache::new(VALUE_CACHE_CAPACITY),
            changed_addresses: HashMap::new(),
            change_tracker_cursor: 0,
            last_change_tracker_run: tick::idle_last_run(),
            cached_process_handle: None,
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
            confirm_value_writes: settings.confirm_value_writes,
            enable_persistence: settings.enable_persistence,
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
            confirm_value_writes: self.confirm_value_writes,
            enable_persistence: self.enable_persistence,
        };
        if let Err(e) = settings.save() {
            self.state.push_error(AppError::Generic { message: e });
        }
    }

    /// Disable experimental tools without leaving hidden pointer-backed rows.
    pub fn set_persistence_enabled(&mut self, enabled: bool) {
        self.enable_persistence = enabled;
        if enabled {
            return;
        }
        self.cancel_cheat_table_save();
        self.automatic_save_notice = None;
        self.cheat_table_status.clear();
        self.cheat_table_status_details.clear();
        self.cheat_table_status_at = None;
        self.pointer_scanner = Default::default();
        self.address_editor = None;
        // Do not turn previously loaded pointers into unguarded absolute rows.
        // Clear only contexts with persistence metadata, including their Undo.
        for index in 0..self.state.searches.len() {
            let search = &self.state.searches[index];
            if search.has_persistent_address_state() {
                self.state.remove_freezes(index);
                self.state.searches[index].clear_results();
            }
        }
        self.clear_result_interaction();
        self.clear_change_tracker();
        if self.app_state == AppState::MemoryEditor {
            self.close_memory_editor();
        }
    }

    /// Discard result interaction when the active result set is replaced.
    pub fn clear_result_interaction(&mut self) {
        if let Some(search) = self.state.searches.get_mut(self.state.current_search) {
            search.selected_result = None;
        }
        self.address_editor = None;
        self.selected_result = None;
        self.editing_result = None;
        self.result_selection_request_scroll = false;
        self.result_edit_request_focus = false;
        self.hovered_result_row = None;
    }

    /// Tab changes end editing, but never discard browsing state or write values.
    pub(crate) fn remember_search_view(&mut self) {
        let selected = self.selected_result;
        self.clear_result_interaction();
        if let Some(search) = self.state.searches.get_mut(self.state.current_search) {
            search.selected_result = selected;
        }
        self.search_value_request_focus = false;
        self.search_value_select_all = false;
    }

    pub(crate) fn restore_search_view(&mut self) {
        self.selected_result = self.state.searches[self.state.current_search].selected_result;
        // Scroll is held by egui under the context's stable view_id, not its index.
        self.result_selection_request_scroll = false;
        self.clear_change_tracker();
    }

    pub(crate) fn prepare_refinement(&mut self) {
        self.remember_search_view();
        self.restore_search_view();
        self.state.searches[self.state.current_search].refinement_pending = true;
    }

    /// Runs after engine polling and also on view-only/inline completion paths.
    pub(crate) fn finish_refinement_ui(&mut self, ctx: &egui::Context) {
        for (index, search) in self.state.searches.iter_mut().enumerate() {
            if !search.refinement_pending || search.searching != crate::SearchMode::None {
                continue;
            }
            search.refinement_pending = false;
            let selected = if index == self.state.current_search {
                self.selected_result
            } else {
                search.selected_result
            };
            let results = search.collect_results();
            search.selected_result = selected.filter(|selected| {
                results
                    .binary_search_by_key(&(selected.addr, selected.search_type as u8), |r| (r.addr, r.search_type as u8))
                    .is_ok()
            });
            if index == self.state.current_search {
                self.selected_result = search.selected_result;
                // Keep a surviving selection visible after rows before it vanish.
                self.result_selection_request_scroll = self.selected_result.is_some();
                if search.search_complete.load(std::sync::atomic::Ordering::Acquire)
                    && self.app_state == AppState::InProcess
                    && ctx.input(|input| input.focused)
                    && self.editing_result.is_none()
                    && self.address_editor.is_none()
                    && !self.pointer_scanner.open
                    && !egui::Popup::is_any_open(ctx)
                    && ctx.memory(|memory| {
                        let focused = memory.focused();
                        focused.is_none() || focused == search.search_input_id || focused == Some(egui::Id::new("result_table_focus"))
                    })
                {
                    self.search_value_request_focus = true;
                    self.search_value_select_all = true;
                }
            }
        }
    }

    pub fn clear_change_tracker(&mut self) {
        self.value_change_tracker.clear();
        self.changed_addresses.clear();
        self.change_tracker_cursor = 0;
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
            AppState::MemoryEditor => crate::ui::mem_editor::view_memory_editor(self, ui),
        }

        // Global escape handling for dismissable dialogs. The memory editor is
        // excluded: there Escape cancels an inspector edit or the grid
        // selection, and it is closed through its own button.
        if ui.ctx().input(|i| i.key_pressed(egui::Key::Escape)) {
            match self.app_state {
                AppState::ProcessSelection | AppState::About | AppState::Settings => {
                    self.app_state = AppState::MainWindow;
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod search_view_tests {
    use super::*;
    use crate::{SearchResult, SearchType};
    use std::sync::atomic::Ordering;

    #[test]
    fn completing_background_tab_keeps_active_focus_and_selection() {
        let mut app = App {
            app_state: AppState::InProcess,
            ..Default::default()
        };
        let selected = SearchResult::new(0x1000, SearchType::Int);
        app.state.searches[0].set_cached_results(vec![selected]);
        app.selected_result = Some(selected);
        app.prepare_refinement();
        app.new_search();
        let active = SearchResult::new(0x2000, SearchType::Int);
        app.state.searches[1].set_cached_results(vec![active]);
        app.selected_result = Some(active);
        app.search_value_request_focus = false;
        app.state.searches[0].search_complete.store(true, Ordering::Release);
        let ctx = egui::Context::default();
        let editing = egui::Id::new("other_input");
        ctx.memory_mut(|memory| memory.request_focus(editing));
        app.finish_refinement_ui(&ctx);
        assert_eq!(app.selected_result.unwrap().addr, active.addr);
        assert_eq!(ctx.memory(|memory| memory.focused()), Some(editing));
        assert!(!app.search_value_request_focus);
        assert!(!app.search_value_select_all);
        app.switch_search(0);
        assert_eq!(app.selected_result.unwrap().addr, selected.addr);
        assert!(!app.search_value_request_focus);
    }

    #[test]
    fn refinement_never_steals_focus_from_another_input() {
        let mut app = App {
            app_state: AppState::InProcess,
            ..Default::default()
        };
        app.state.searches[0].search_complete.store(true, Ordering::Release);
        app.state.searches[0].refinement_pending = true;
        let ctx = egui::Context::default();
        let editing = egui::Id::new("filter_input");
        ctx.memory_mut(|memory| memory.request_focus(editing));
        app.finish_refinement_ui(&ctx);
        assert_eq!(ctx.memory(|memory| memory.focused()), Some(editing));
        assert!(!app.search_value_request_focus);
        assert!(!app.search_value_select_all);
    }
}
