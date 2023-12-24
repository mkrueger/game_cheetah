use core::panic;
use std::fmt::Display;

use crate::SearchType;

#[derive(Debug, PartialEq, Clone)]
pub struct SearchValue(pub SearchType, pub Vec<u8>);

impl Display for SearchValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self.0 {
            SearchType::Byte => self.1[0].to_string(),
            SearchType::Short => i16::from_le_bytes(self.1.clone().try_into().unwrap()).to_string(),
            SearchType::Int => i32::from_le_bytes(self.1.clone().try_into().unwrap()).to_string(),
            SearchType::Int64 => i64::from_le_bytes(self.1.clone().try_into().unwrap()).to_string(),
            SearchType::Float => f32::from_le_bytes(self.1.clone().try_into().unwrap()).to_string(),
            SearchType::Double => f64::from_le_bytes(self.1.clone().try_into().unwrap()).to_string(),
            SearchType::Guess => panic!("invalid search value"),
        };
        write!(f, "{s}")
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
