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
pub struct SearchResult {
    pub addr: usize,
    pub search_type: SearchType,
    // Inline 0..=8 bytes. Actual logical length is determined by search_type.get_byte_length().
    pub stored: Option<[u8; 8]>,
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
        let mut buf = [0u8; 8];
        let needed = search_type.get_byte_length().min(8);
        let copy_len = needed.min(bytes.len());
        if copy_len > 0 {
            buf[..copy_len].copy_from_slice(&bytes[..copy_len]);
            Self {
                addr,
                search_type,
                stored: Some(buf),
            }
        } else {
            // For types without a fixed byte length (e.g., Guess/Unknown), don't store bytes.
            Self {
                addr,
                search_type,
                stored: None,
            }
        }
    }

    // Returns a slice to the stored bytes matching the SearchType's byte length.
    pub fn stored_bytes(&self) -> Option<&[u8]> {
        let len = self.search_type.get_byte_length().min(8);
        if len == 0 {
            return None;
        }
        self.stored.as_ref().map(|arr| &arr[..len])
    }
}
