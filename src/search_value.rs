use std::fmt::{Display, Formatter};

use crate::SearchType;

#[derive(Debug, PartialEq, Clone)]
pub struct SearchValue(pub SearchType, pub Vec<u8>);

impl Display for SearchValue {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        use SearchType::*;
        let to_str = |needed: usize, convert: &dyn Fn(&[u8]) -> String| -> Option<String> {
            if self.1.len() >= needed { Some(convert(&self.1[..needed])) } else { None }
        };
        let s = match self.0 {
            Byte => self.1.get(0).map(|b| b.to_string()),
            Short => to_str(2, &|b| {
                let arr: [u8; 2] = b.try_into().unwrap();
                i16::from_le_bytes(arr).to_string()
            }),
            Int => to_str(4, &|b| {
                let arr: [u8; 4] = b.try_into().unwrap();
                i32::from_le_bytes(arr).to_string()
            }),
            Int64 => to_str(8, &|b| {
                let arr: [u8; 8] = b.try_into().unwrap();
                i64::from_le_bytes(arr).to_string()
            }),
            Float => to_str(4, &|b| {
                let arr: [u8; 4] = b.try_into().unwrap();
                f32::from_le_bytes(arr).to_string()
            }),
            Double => to_str(8, &|b| {
                let arr: [u8; 8] = b.try_into().unwrap();
                f64::from_le_bytes(arr).to_string()
            }),
            Guess => None,
        }
        .ok_or(std::fmt::Error)?;
        f.write_str(&s)
    }
}

#[derive(Clone, Copy)]
pub struct SearchResult {
    pub addr: usize,
    pub search_type: SearchType,
}

impl SearchResult {
    pub fn new(addr: usize, search_type: SearchType) -> Self {
        Self { addr, search_type }
    }
}
