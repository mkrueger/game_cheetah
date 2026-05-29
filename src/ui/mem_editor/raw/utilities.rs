//! Vendored from egui_memory_editor v0.2.14 (MIT/Apache-2.0).
use super::option_data::{DataFormatType, Endianness};

/// Placeholder shown when `bytes` is too short for the requested format.
const TRUNCATED: &str = "??";

/// Take the first `N` bytes as a fixed-size array, or `None` if the slice is
/// shorter than `N`. Used to avoid panics when the available memory window is
/// smaller than the selected data type (e.g. reading a `u64` at the very end
/// of an address range).
fn first<const N: usize>(bytes: &[u8]) -> Option<[u8; N]> {
    bytes.get(..N).and_then(|s| s.try_into().ok())
}

pub fn slice_to_decimal_string(data_preview: super::option_data::DataPreviewOptions, bytes: &[u8]) -> String {
    let big = matches!(data_preview.selected_endianness, Endianness::Big);
    let value = match data_preview.selected_data_format {
        DataFormatType::U8 => first(bytes).map(|b| if big { u8::from_be_bytes(b) } else { u8::from_le_bytes(b) }.to_string()),
        DataFormatType::U16 => first(bytes).map(|b| if big { u16::from_be_bytes(b) } else { u16::from_le_bytes(b) }.to_string()),
        DataFormatType::U32 => first(bytes).map(|b| if big { u32::from_be_bytes(b) } else { u32::from_le_bytes(b) }.to_string()),
        DataFormatType::U64 => first(bytes).map(|b| if big { u64::from_be_bytes(b) } else { u64::from_le_bytes(b) }.to_string()),
        DataFormatType::I8 => first(bytes).map(|b| if big { i8::from_be_bytes(b) } else { i8::from_le_bytes(b) }.to_string()),
        DataFormatType::I16 => first(bytes).map(|b| if big { i16::from_be_bytes(b) } else { i16::from_le_bytes(b) }.to_string()),
        DataFormatType::I32 => first(bytes).map(|b| if big { i32::from_be_bytes(b) } else { i32::from_le_bytes(b) }.to_string()),
        DataFormatType::I64 => first(bytes).map(|b| if big { i64::from_be_bytes(b) } else { i64::from_le_bytes(b) }.to_string()),
        DataFormatType::F32 => first(bytes).map(|b| if big { f32::from_be_bytes(b) } else { f32::from_le_bytes(b) }.to_string()),
        DataFormatType::F64 => first(bytes).map(|b| if big { f64::from_be_bytes(b) } else { f64::from_le_bytes(b) }.to_string()),
    };
    value.unwrap_or_else(|| TRUNCATED.to_string())
}
