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

#[derive(Clone, Copy, Debug)]
pub struct StoredBytes {
    bytes: [u8; 8],
    len: u8,
}

impl StoredBytes {
    pub fn new(search_type: SearchType, bytes: &[u8]) -> Option<Self> {
        let needed = search_type.fixed_byte_length()?;
        let len = needed.min(bytes.len()).min(8);
        if len == 0 {
            return None;
        }

        let mut stored = [0u8; 8];
        stored[..len].copy_from_slice(&bytes[..len]);
        Some(Self { bytes: stored, len: len as u8 })
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.bytes[..usize::from(self.len)]
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SearchResult {
    pub addr: usize,
    pub search_type: SearchType,
    pub stored: Option<StoredBytes>,
}

impl SearchResult {
    pub fn new(addr: usize, search_type: SearchType) -> Self {
        Self {
            addr,
            search_type,
            stored: None,
        }
    }

    pub fn new_with_bytes(addr: usize, search_type: SearchType, bytes: &[u8]) -> Self {
        Self {
            addr,
            search_type,
            stored: StoredBytes::new(search_type, bytes),
        }
    }

    // Returns a slice to the stored bytes without relying on SearchType length lookups.
    pub fn stored_bytes(&self) -> Option<&[u8]> {
        self.stored.as_ref().map(StoredBytes::as_slice)
    }
}
