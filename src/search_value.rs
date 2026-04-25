use std::fmt::{Display, Formatter};

use crate::SearchType;

#[derive(Debug, PartialEq, Clone)]
pub struct SearchValue(pub SearchType, pub Vec<u8>);

impl SearchValue {
    fn fixed_bytes<const N: usize>(&self) -> Option<[u8; N]> {
        self.1.get(..N)?.try_into().ok()
    }
}

impl Display for SearchValue {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let s = match self.0 {
            SearchType::Byte => self.1.first().map(|b| b.to_string()),
            SearchType::Short => self.fixed_bytes::<2>().map(|arr| i16::from_le_bytes(arr).to_string()),
            SearchType::Int => self.fixed_bytes::<4>().map(|arr| i32::from_le_bytes(arr).to_string()),
            SearchType::Int64 => self.fixed_bytes::<8>().map(|arr| i64::from_le_bytes(arr).to_string()),
            SearchType::Float => self.fixed_bytes::<4>().map(|arr| f32::from_le_bytes(arr).to_string()),
            SearchType::Double => self.fixed_bytes::<8>().map(|arr| f64::from_le_bytes(arr).to_string()),
            SearchType::Guess => None,
            SearchType::Unknown => None,
            SearchType::String | SearchType::StringUtf16 => None,
        }
        .unwrap_or_else(|| "<invalid>".to_owned());
        f.write_str(&s)
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
