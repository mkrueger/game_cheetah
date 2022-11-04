use core::panic;

use crate::SearchType;

#[derive(Debug, PartialEq, Clone)]
pub struct SearchValue(pub SearchType, pub Vec<u8>);

impl SearchValue {
    pub fn to_string(&self) -> String {
        match self.0 {
            SearchType::Short => i16::from_le_bytes(self.1.clone().try_into().unwrap()).to_string(),
            SearchType::Int => i32::from_le_bytes(self.1.clone().try_into().unwrap()).to_string(),
            SearchType::Int64 => i64::from_le_bytes(self.1.clone().try_into().unwrap()).to_string(),
            SearchType::Float => f32::from_le_bytes(self.1.clone().try_into().unwrap()).to_string(),
            SearchType::Double => f64::from_le_bytes(self.1.clone().try_into().unwrap()).to_string(),
            SearchType::Guess => panic!("invalid search value")
        }
    }
}

#[derive(Clone, Copy)]
pub struct SearchResult {
    pub addr: usize,
    pub search_type: SearchType,
    pub freezed: bool
}

impl SearchResult {
    pub fn new(addr: usize, search_type: SearchType) -> Self {
        Self {
            addr,
            search_type,
            freezed: false
        }
    }
}