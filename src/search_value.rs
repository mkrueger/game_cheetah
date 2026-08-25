use std::fmt::{Display, Formatter};

use crate::SearchType;

#[derive(Debug, PartialEq, Clone)]
pub struct SearchValue(pub SearchType, pub Vec<u8>);

impl SearchValue {
    fn fixed_bytes<const N: usize>(&self) -> Option<[u8; N]> {
        self.1.get(..N)?.try_into().ok()
    }

    pub fn to_hex_string(&self) -> String {
        match self.0 {
            SearchType::Byte => self.1.first().map(|b| format!("0x{b:02X}")),
            SearchType::Short => self.fixed_bytes::<2>().map(|arr| format!("0x{:04X}", u16::from_le_bytes(arr))),
            SearchType::Int => self.fixed_bytes::<4>().map(|arr| format!("0x{:08X}", u32::from_le_bytes(arr))),
            SearchType::Int64 => self.fixed_bytes::<8>().map(|arr| format!("0x{:016X}", u64::from_le_bytes(arr))),
            SearchType::Float => self.fixed_bytes::<4>().map(|arr| format!("0x{:08X}", u32::from_le_bytes(arr))),
            SearchType::Double => self.fixed_bytes::<8>().map(|arr| format!("0x{:016X}", u64::from_le_bytes(arr))),
            _ => None,
        }
        .unwrap_or_else(|| self.to_string())
    }
}

impl Display for SearchValue {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let s = format_value(self.0, &self.1).unwrap_or_else(|| "<invalid>".to_owned());
        f.write_str(&s)
    }
}

fn fixed_bytes<const N: usize>(bytes: &[u8]) -> Option<[u8; N]> {
    bytes.get(..N)?.try_into().ok()
}

/// Render raw memory bytes the way the result table displays them.
/// `None` for types without a numeric representation (string/guess/unknown)
/// or when `bytes` is too short.
pub fn format_value(search_type: SearchType, bytes: &[u8]) -> Option<String> {
    match search_type {
        SearchType::Byte => bytes.first().map(|b| b.to_string()),
        SearchType::Short => fixed_bytes::<2>(bytes).map(|arr| i16::from_le_bytes(arr).to_string()),
        SearchType::Int => fixed_bytes::<4>(bytes).map(|arr| i32::from_le_bytes(arr).to_string()),
        SearchType::Int64 => fixed_bytes::<8>(bytes).map(|arr| i64::from_le_bytes(arr).to_string()),
        SearchType::Float => fixed_bytes::<4>(bytes).map(|arr| f32::from_le_bytes(arr).to_string()),
        SearchType::Double => fixed_bytes::<8>(bytes).map(|arr| f64::from_le_bytes(arr).to_string()),
        SearchType::Guess | SearchType::Unknown | SearchType::String | SearchType::StringUtf16 => None,
    }
}

/// A single match produced by a search pass.
///
/// For most search types `search_type` mirrors [`SearchContext::search_type`],
/// but `Guess` and `Unknown` searches can yield several typed hits at the same
/// address, so the type is carried per-result. Previous-value bookkeeping for
/// the unknown-search filter lives in
/// [`SearchContext::previous_unknown_values`] rather than in this struct, which
/// keeps the per-hit footprint to two `usize`-sized fields.
#[derive(Clone, Copy, Debug)]
pub struct SearchResult {
    pub addr: usize,
    pub search_type: SearchType,
}

impl SearchResult {
    pub fn new(addr: usize, search_type: SearchType) -> Self {
        Self { addr, search_type }
    }
}
