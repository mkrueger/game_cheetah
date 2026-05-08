use i18n_embed_fl::fl;
use std::fmt;

use crate::SearchValue;

#[repr(u8)]
#[derive(Debug, PartialEq, Clone, Copy, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum SearchType {
    Guess,
    Byte,
    Short,
    Int,
    Int64,
    Float,
    Double,
    Unknown,
    String,
    StringUtf16,
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum UnknownComparison {
    Decreased,
    Increased,
    Changed,
    Unchanged,
}

impl SearchType {
    pub fn get_description_text(&self) -> String {
        match self {
            SearchType::Guess => fl!(crate::LANGUAGE_LOADER, "guess-value-item"),
            SearchType::Byte => fl!(crate::LANGUAGE_LOADER, "byte-value-item"),
            SearchType::Short => fl!(crate::LANGUAGE_LOADER, "short-value-item"),
            SearchType::Int => fl!(crate::LANGUAGE_LOADER, "int-value-item"),
            SearchType::Int64 => fl!(crate::LANGUAGE_LOADER, "int64-value-item"),
            SearchType::Float => fl!(crate::LANGUAGE_LOADER, "float-value-item"),
            SearchType::Double => fl!(crate::LANGUAGE_LOADER, "double-value-item"),
            SearchType::Unknown => fl!(crate::LANGUAGE_LOADER, "unknown-value-item"),
            SearchType::String | SearchType::StringUtf16 => fl!(crate::LANGUAGE_LOADER, "string-value-item"),
        }
    }

    pub fn fixed_byte_length(&self) -> Option<usize> {
        match self {
            SearchType::Guess | SearchType::Unknown | SearchType::String | SearchType::StringUtf16 => None,
            SearchType::Byte => Some(1),
            SearchType::Short => Some(2),
            SearchType::Int => Some(4),
            SearchType::Int64 => Some(8),
            SearchType::Float => Some(4),
            SearchType::Double => Some(8),
        }
    }

    pub fn get_short_description_text(&self) -> String {
        match self {
            SearchType::Guess => fl!(crate::LANGUAGE_LOADER, "guess-descr"),
            SearchType::Byte => fl!(crate::LANGUAGE_LOADER, "byte-descr"),
            SearchType::Short => fl!(crate::LANGUAGE_LOADER, "short-descr"),
            SearchType::Int => fl!(crate::LANGUAGE_LOADER, "int-descr"),
            SearchType::Int64 => fl!(crate::LANGUAGE_LOADER, "int64-descr"),
            SearchType::Float => fl!(crate::LANGUAGE_LOADER, "float-descr"),
            SearchType::Double => fl!(crate::LANGUAGE_LOADER, "double-descr"),
            SearchType::Unknown => fl!(crate::LANGUAGE_LOADER, "unknown-descr"),
            SearchType::String | SearchType::StringUtf16 => fl!(crate::LANGUAGE_LOADER, "string-descr"),
        }
    }

    pub fn from_string(&self, txt: &str) -> Result<SearchValue, String> {
        match self {
            SearchType::Byte => {
                let val = txt.parse::<u8>().map_err(|_| format!("Invalid byte value: {txt}"))?;
                Ok(SearchValue(*self, vec![val]))
            }
            SearchType::Short => {
                let val = txt.parse::<i16>().map_err(|_| format!("Invalid short value: {txt}"))?;
                Ok(SearchValue(*self, val.to_le_bytes().to_vec()))
            }
            SearchType::Int => {
                let val = txt.parse::<i32>().map_err(|_| format!("Invalid int value: {txt}"))?;
                Ok(SearchValue(*self, val.to_le_bytes().to_vec()))
            }
            SearchType::Int64 => {
                let val = txt.parse::<i64>().map_err(|_| format!("Invalid int64 value: {txt}"))?;
                Ok(SearchValue(*self, val.to_le_bytes().to_vec()))
            }
            SearchType::Float => {
                let val = txt.parse::<f32>().map_err(|_| format!("Invalid float value: {txt}"))?;
                Ok(SearchValue(*self, val.to_le_bytes().to_vec()))
            }
            SearchType::Double => {
                let val = txt.parse::<f64>().map_err(|_| format!("Invalid double value: {txt}"))?;
                Ok(SearchValue(*self, val.to_le_bytes().to_vec()))
            }
            SearchType::Guess => {
                // For Guess, we don't parse here - it's handled in spawn_parallel_search
                Ok(SearchValue(*self, txt.as_bytes().to_vec()))
            }
            SearchType::Unknown => {
                // Unknown doesn't use text input, return empty
                Ok(SearchValue(*self, vec![]))
            }
            SearchType::String | SearchType::StringUtf16 => {
                let val = txt.as_bytes().to_vec();
                Ok(SearchValue(*self, val))
            }
        }
    }
}

impl fmt::Display for SearchType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            SearchType::Guess => fl!(crate::LANGUAGE_LOADER, "guess-value-item"),
            SearchType::Byte => fl!(crate::LANGUAGE_LOADER, "byte-value-item"),
            SearchType::Short => fl!(crate::LANGUAGE_LOADER, "short-value-item"),
            SearchType::Int => fl!(crate::LANGUAGE_LOADER, "int-value-item"),
            SearchType::Int64 => fl!(crate::LANGUAGE_LOADER, "int64-value-item"),
            SearchType::Float => fl!(crate::LANGUAGE_LOADER, "float-value-item"),
            SearchType::Double => fl!(crate::LANGUAGE_LOADER, "double-value-item"),
            SearchType::Unknown => fl!(crate::LANGUAGE_LOADER, "unknown-value-item"),
            SearchType::String | SearchType::StringUtf16 => fl!(crate::LANGUAGE_LOADER, "string-value-item"),
        };
        write!(f, "{text}")
    }
}
