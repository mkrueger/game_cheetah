//! Vendored from egui_memory_editor v0.2.14 (MIT/Apache-2.0).
use super::Address;
use egui::{Color32, TextStyle};
use std::ops::Range;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Endianness {
    Big,
    Little,
}

impl Endianness {
    pub fn iter() -> impl Iterator<Item = Endianness> {
        vec![Endianness::Big, Endianness::Little].into_iter()
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum DataFormatType {
    U8,
    U16,
    U32,
    U64,
    I8,
    I16,
    I32,
    I64,
    F32,
    F64,
}

impl DataFormatType {
    pub fn iter() -> impl Iterator<Item = DataFormatType> {
        use DataFormatType::*;
        [U8, U16, U32, U64, I8, I16, I32, I64, F32, F64].into_iter()
    }

    pub const fn bytes_to_read(&self) -> usize {
        use DataFormatType::*;
        match *self {
            U8 | I8 => 1,
            U16 | I16 => 2,
            U32 | I32 | F32 => 4,
            U64 | I64 | F64 => 8,
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub struct DataPreviewOptions {
    pub selected_endianness: Endianness,
    pub selected_data_format: DataFormatType,
}

impl Default for DataPreviewOptions {
    fn default() -> Self {
        DataPreviewOptions {
            selected_endianness: Endianness::Little,
            selected_data_format: DataFormatType::U32,
        }
    }
}

#[derive(Clone, Debug)]
pub struct MemoryEditorOptions {
    pub show_ascii: bool,
    pub show_zero_colour: bool,
    pub none_display_value: String,
    pub data_preview: DataPreviewOptions,
    pub column_count: usize,
    pub is_resizable_column: bool,
    pub zero_colour: Color32,
    pub address_text_colour: Color32,
    pub highlight_text_colour: Color32,
    pub memory_editor_text_style: TextStyle,
    pub memory_editor_address_text_style: TextStyle,
    pub memory_editor_ascii_text_style: TextStyle,
    pub(crate) selected_address_range: String,
}

impl Default for MemoryEditorOptions {
    fn default() -> Self {
        MemoryEditorOptions {
            data_preview: Default::default(),
            show_ascii: true,
            show_zero_colour: true,
            none_display_value: "--".to_string(),
            zero_colour: Color32::from_gray(80),
            is_resizable_column: true,
            column_count: 16,
            address_text_colour: Color32::from_rgb(125, 0, 125),
            highlight_text_colour: Color32::from_rgb(0, 140, 140),
            memory_editor_text_style: TextStyle::Monospace,
            memory_editor_address_text_style: TextStyle::Monospace,
            memory_editor_ascii_text_style: TextStyle::Monospace,
            selected_address_range: "".to_string(),
        }
    }
}

#[derive(Debug, Default, Clone)]
pub(crate) struct BetweenFrameData {
    pub previous_frame_editor_width: f32,
    pub selected_edit_address: Option<Address>,

    pub selected_highlight_address: Option<Address>,
    pub show_additional_highlights: bool,

    pub goto_address_line: Option<usize>,

    /// Programmatic jump kept as an address until the next frame has resolved
    /// adaptive columns and can center the correct row in the viewport.
    pub center_address_on_next_frame: Option<Address>,

    /// Address whose row should be re-centered from its real rectangle once it
    /// is rendered, correcting the estimate used for the initial jump.
    pub center_refine_address: Option<Address>,

    /// A keyboard move requested an exact minimal scroll once its target
    /// cell is rendered.
    pub scroll_caret_into_view: bool,

    /// Address range that corresponds to the cheat / search result currently being edited.
    /// All bytes in this range are highlighted with a translucent accent so the user knows
    /// where the value actually lives in memory.
    pub result_highlight_range: Option<Range<Address>>,

    /// Which nibble of the byte at `selected_edit_address` the caret is on.
    /// `false` = high nibble (left hex digit), `true` = low nibble (right
    /// hex digit). Typing a hex digit replaces this nibble in memory and
    /// advances the caret by one nibble.
    pub selected_low_nibble: bool,
}

impl BetweenFrameData {
    pub fn set_highlight_address(&mut self, new_address: Address) {
        self.selected_highlight_address = if matches!(self.selected_highlight_address, Some(current) if current == new_address) {
            None
        } else {
            Some(new_address)
        };
    }

    pub fn set_selected_edit_address(&mut self, new_address: Option<Address>, address_space: &Range<Address>) {
        self.selected_low_nibble = false;
        if matches!(new_address, Some(address) if address_space.contains(&address)) {
            self.set_highlight_address(new_address.unwrap());
            self.selected_edit_address = new_address;
        } else {
            self.selected_edit_address = None;
        }
    }

    #[inline]
    pub fn should_highlight(&self, address: Address) -> bool {
        (self.selected_highlight_address == Some(address)) || (self.selected_edit_address == Some(address))
    }

    pub fn should_subtle_highlight(&self, address: Address, data_format: DataFormatType) -> bool {
        self.show_additional_highlights
            && self
                .selected_highlight_address
                .is_some_and(|addr| (addr..addr + data_format.bytes_to_read()).contains(&address))
    }

    #[inline]
    pub fn is_in_result_range(&self, address: Address) -> bool {
        self.result_highlight_range.as_ref().is_some_and(|range| range.contains(&address))
    }
}
