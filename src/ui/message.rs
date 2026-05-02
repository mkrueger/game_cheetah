use crate::{ProcessInfo, SearchType, memory_editor::InspectorValueKind, process_selection::ProcessSortColumn};

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
    NewSearch,
    CloseSearch(usize),
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
    ResultEditingBegin(usize, String),
    /// Buffered keystroke in a result-row value field. Holding it in state
    /// instead of writing on every keystroke prevents the input from being
    /// re-driven from memory mid-edit (which looks like a focus loss).
    ResultEditingChanged(usize, String),
    ResultEditingCommit(usize),
    ResultEditingCancel,
    ToggleFreeze(usize),
    ToggleFreezeAll,
    OpenEditor(usize),
    RemoveResult(usize),
    CloseMemoryEditor,
    MemoryEditorAddressChanged(String),
    MemoryEditorJumpToAddress,
    MemoryEditorCellChanged(usize, String), // offset, new hex value
    MemoryEditorScroll(i32),                // scroll by n rows (positive = down, negative = up)
    MemoryEditorScrolled(icy_ui::widget::scrollable::Viewport),
    MemoryEditorPageUp,
    MemoryEditorPageDown,
    MemoryEditorMoveCursor(i32, i32), // (row_delta, col_delta)
    MemoryEditorSetCursor(usize, usize),
    MemoryEditorEditHex(u8), // hex digit input
    MemoryEditorBeginEdit,
    MemoryEditorEndEdit,
    MemoryEditorInspectorValueChanged(InspectorValueKind, String),
    MemoryEditorInspectorValueSubmit(InspectorValueKind),
    MemoryEditorTick,
    MemoryEditorUndo,
    MemoryEditorRedo,
    SortProcesses(ProcessSortColumn),

    UnknownSearchDecrease,
    UnknownSearchIncrease,
    UnknownSearchChanged,
    UnknownSearchUnchanged,

    FocusNext,
    FocusPrevious,

    SaveCheatTable,
    LoadCheatTable,
    ToggleHexDisplay,
}
