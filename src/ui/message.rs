use crate::{ProcessInfo, SearchType};

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
