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
    pub const NUMERIC_TYPES: [Self; 6] = [Self::Byte, Self::Short, Self::Int, Self::Int64, Self::Float, Self::Double];

    /// Guess finds possible interpretations, not the target's variable type.
    pub const GUESS_TYPES: [Self; 4] = [Self::Int, Self::Int64, Self::Float, Self::Double];

    pub fn integer_range(&self) -> Option<(&'static str, &'static str, &'static str)> {
        match self {
            Self::Byte => Some(("UInt8", "0", "255")),
            Self::Short => Some(("Int16", "-32768", "32767")),
            Self::Int => Some(("Int32", "-2147483648", "2147483647")),
            Self::Int64 => Some(("Int64", "-9223372036854775808", "9223372036854775807")),
            _ => None,
        }
    }

    fn integer_parse_error(&self, text: &str, error: std::num::ParseIntError) -> String {
        if matches!(error.kind(), std::num::IntErrorKind::PosOverflow | std::num::IntErrorKind::NegOverflow)
            && let Some((kind, min, max)) = self.integer_range()
        {
            fl!(crate::LANGUAGE_LOADER, "integer-range-error", kind = kind, min = min, max = max)
        } else {
            fl!(crate::LANGUAGE_LOADER, "integer-input-error", value = text)
        }
    }

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

    /// Byte width of a result, including variable-width string encodings.
    pub fn byte_length_for_text(&self, text: &str) -> Option<usize> {
        match self {
            Self::String => Some(text.len()),
            Self::StringUtf16 => Some(text.encode_utf16().count().saturating_mul(2)),
            _ => self.fixed_byte_length(),
        }
    }

    pub fn from_string(&self, txt: &str) -> Result<SearchValue, String> {
        match self {
            SearchType::Byte => {
                let val = txt.parse::<u8>().map_err(|err| self.integer_parse_error(txt, err))?;
                Ok(SearchValue(*self, vec![val]))
            }
            SearchType::Short => {
                let val = txt.parse::<i16>().map_err(|err| self.integer_parse_error(txt, err))?;
                Ok(SearchValue(*self, val.to_le_bytes().to_vec()))
            }
            SearchType::Int => {
                let val = txt.parse::<i32>().map_err(|err| self.integer_parse_error(txt, err))?;
                Ok(SearchValue(*self, val.to_le_bytes().to_vec()))
            }
            SearchType::Int64 => {
                let val = txt.parse::<i64>().map_err(|err| self.integer_parse_error(txt, err))?;
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
                // For Guess, we don't decide the concrete numeric type here —
                // the parallel search uses every type that successfully parses
                // the text. Reject input that doesn't look like a number for
                // any of the supported numeric types so the UI can flag it.
                let parses_as_number = txt.parse::<i64>().is_ok() || txt.parse::<u64>().is_ok() || txt.parse::<f64>().is_ok();
                if !parses_as_number {
                    return Err(format!("Invalid number value: {txt}"));
                }
                Ok(SearchValue(*self, txt.as_bytes().to_vec()))
            }
            SearchType::Unknown => {
                // Unknown doesn't use text input, return empty
                Ok(SearchValue(*self, vec![]))
            }
            SearchType::String => {
                let val = txt.as_bytes().to_vec();
                Ok(SearchValue(*self, val))
            }
            SearchType::StringUtf16 => {
                let val = txt.encode_utf16().flat_map(u16::to_le_bytes).collect();
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
