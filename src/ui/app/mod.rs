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
    collections::{HashMap, HashSet},
    time::{Duration, Instant},
};

use process_memory::ProcessHandle;

use crate::{
    AppError, GameCheetahEngine,
    ui::{
        in_process_view, main_window,
        mem_editor::MemoryEditor,
        process_selection,
        process_selection::{ProcessSortColumn, SortDirection},
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
    pub app_state: AppState,
    pub state: GameCheetahEngine,

    pub renaming_search_index: Option<usize>,
    pub rename_search_text: String,
    /// One-shot flag: when `true`, the rename text field should grab
    /// keyboard focus on the next frame and then clear the flag.
    pub rename_request_focus: bool,

    /// One-shot request to return keyboard focus to the search value field.
    pub search_value_request_focus: bool,

    /// `(row_index, typed_buffer)` while a result row's value is being
    /// edited. We don't read the live value into this buffer per frame —
    /// once the user clicks/focuses the field, it owns the keystrokes
    /// until they commit (Enter) or cancel (Escape / focus loss).
    pub editing_result: Option<(usize, String)>,

    pub process_sort_column: ProcessSortColumn,
    pub process_sort_direction: SortDirection,
    /// Representative pids of process groups the user expanded in the
    /// process selection table.
    pub expanded_process_groups: HashSet<process_memory::Pid>,

    /// Result row under the pointer last frame; drives the hover-only row
    /// action icons.
    pub hovered_result_row: Option<usize>,

    /// Brief Save/Load status shown as a transient toast.
    pub cheat_table_status: String,
    pub cheat_table_status_at: Option<Instant>,

    /// Reattach to a process with the same name after the attached one exits.
    pub auto_reconnect: bool,

    /// Check the GitHub releases API once per launch.
    pub check_for_updates: bool,

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
            app_state: AppState::default(),
            state: GameCheetahEngine::default(),
            renaming_search_index: None,
            rename_search_text: String::new(),
            rename_request_focus: false,
            search_value_request_focus: false,
            editing_result: None,
            process_sort_column: ProcessSortColumn::default(),
            process_sort_direction: SortDirection::default(),
            expanded_process_groups: HashSet::new(),
            hovered_result_row: None,
            cheat_table_status: String::new(),
            cheat_table_status_at: None,
            auto_reconnect: false,
            check_for_updates: false,
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
