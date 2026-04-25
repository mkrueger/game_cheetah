use std::fmt::{Display, Formatter};

use crate::SearchType;

#[derive(Debug, PartialEq, Clone)]
pub struct SearchValue(pub SearchType, pub Vec<u8>);

impl Display for SearchValue {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let to_str = |needed: usize, convert: &dyn Fn(&[u8]) -> String| -> Option<String> {
            if self.1.len() >= needed { Some(convert(&self.1[..needed])) } else { None }
        };
        let s = match self.0 {
            SearchType::Byte => self.1.first().map(|b| b.to_string()),
            SearchType::Short => to_str(2, &|b| {
                let arr: [u8; 2] = b.try_into().unwrap();
                i16::from_le_bytes(arr).to_string()
            }),
            SearchType::Int => to_str(4, &|b| {
                let arr: [u8; 4] = b.try_into().unwrap();
                i32::from_le_bytes(arr).to_string()
            }),
            SearchType::Int64 => to_str(8, &|b| {
                let arr: [u8; 8] = b.try_into().unwrap();
                i64::from_le_bytes(arr).to_string()
            }),
            SearchType::Float => to_str(4, &|b| {
                let arr: [u8; 4] = b.try_into().unwrap();
                f32::from_le_bytes(arr).to_string()
            }),
            SearchType::Double => to_str(8, &|b| {
                let arr: [u8; 8] = b.try_into().unwrap();
                f64::from_le_bytes(arr).to_string()
            }),
            SearchType::Guess => None,
            SearchType::Unknown => None,
            SearchType::String | SearchType::StringUtf16 => None,
        }
        .ok_or(std::fmt::Error)?;
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
