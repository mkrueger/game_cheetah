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
    IntScaled10,
    IntScaled100,
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum UnknownComparison {
    Decreased,
    Increased,
    Changed,
    Unchanged,
}

impl SearchType {
    pub const NUMERIC_TYPES: [Self; 8] = [
        Self::Byte,
        Self::Short,
        Self::Int,
        Self::Int64,
        Self::Float,
        Self::Double,
        Self::IntScaled10,
        Self::IntScaled100,
    ];

    /// Prefer Int32 whenever the input fits; trying Int64 as well would
    /// duplicate hits when the adjacent four bytes happen to be zero.
    /// This is a heuristic, not detection of the game's variable width.
    pub fn guess_types(text: &str) -> &'static [Self] {
        if text.parse::<i32>().is_ok() {
            if parse_scaled_int(text, 2).is_some() {
                &[Self::Int, Self::Float, Self::Double, Self::IntScaled10, Self::IntScaled100]
            } else if parse_scaled_int(text, 1).is_some() {
                &[Self::Int, Self::Float, Self::Double, Self::IntScaled10]
            } else {
                &[Self::Int, Self::Float, Self::Double]
            }
        } else if text.parse::<i64>().is_ok() {
            &[Self::Int64, Self::Float, Self::Double]
        } else if parse_scaled_int(text, 1).is_some() {
            if parse_scaled_int(text, 2).is_some() {
                &[Self::Float, Self::Double, Self::IntScaled10, Self::IntScaled100]
            } else {
                &[Self::Float, Self::Double, Self::IntScaled10]
            }
        } else if parse_scaled_int(text, 2).is_some() {
            &[Self::Float, Self::Double, Self::IntScaled100]
        } else {
            &[Self::Float, Self::Double]
        }
    }

    pub fn integer_range(&self) -> Option<(&'static str, &'static str, &'static str)> {
        match self {
            Self::Byte => Some(("UInt8", "0", "255")),
            Self::Short => Some(("Int16", "-32768", "32767")),
            Self::Int => Some(("Int32", "-2147483648", "2147483647")),
            Self::Int64 => Some(("Int64", "-9223372036854775808", "9223372036854775807")),
            _ => None,
        }
    }

    pub fn scaling_hint(&self) -> Option<String> {
        match self {
            Self::IntScaled10 => Some(fl!(crate::LANGUAGE_LOADER, "int-scaled10-hint")),
            Self::IntScaled100 => Some(fl!(crate::LANGUAGE_LOADER, "int-scaled100-hint")),
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
            SearchType::IntScaled10 => fl!(crate::LANGUAGE_LOADER, "int-scaled10-value-item"),
            SearchType::IntScaled100 => fl!(crate::LANGUAGE_LOADER, "int-scaled100-value-item"),
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
            SearchType::Int | SearchType::IntScaled10 | SearchType::IntScaled100 => Some(4),
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
            SearchType::IntScaled10 => fl!(crate::LANGUAGE_LOADER, "int-scaled10-descr"),
            SearchType::IntScaled100 => fl!(crate::LANGUAGE_LOADER, "int-scaled100-descr"),
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
            SearchType::IntScaled10 => {
                let val = parse_scaled_int(txt, 1).ok_or_else(|| fl!(crate::LANGUAGE_LOADER, "int-scaled10-input-error"))?;
                Ok(SearchValue(*self, val.to_le_bytes().to_vec()))
            }
            SearchType::IntScaled100 => {
                let val = parse_scaled_int(txt, 2).ok_or_else(|| fl!(crate::LANGUAGE_LOADER, "int-scaled100-input-error"))?;
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
                // the parallel search selects candidates via guess_types.
                // Reject input that doesn't look like a number for
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
            SearchType::IntScaled10 => fl!(crate::LANGUAGE_LOADER, "int-scaled10-value-item"),
            SearchType::IntScaled100 => fl!(crate::LANGUAGE_LOADER, "int-scaled100-value-item"),
            SearchType::Int64 => fl!(crate::LANGUAGE_LOADER, "int64-value-item"),
            SearchType::Float => fl!(crate::LANGUAGE_LOADER, "float-value-item"),
            SearchType::Double => fl!(crate::LANGUAGE_LOADER, "double-value-item"),
            SearchType::Unknown => fl!(crate::LANGUAGE_LOADER, "unknown-value-item"),
            SearchType::String | SearchType::StringUtf16 => fl!(crate::LANGUAGE_LOADER, "string-value-item"),
        };
        write!(f, "{text}")
    }
}

// Parse fixed decimal places without rounding through floating point or overflowing.
fn parse_scaled_int(text: &str, decimal_places: u32) -> Option<i32> {
    let (whole, fraction) = text.split_once('.').unwrap_or((text, ""));
    if !fraction.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let fraction = fraction.trim_end_matches('0');
    if fraction.len() > decimal_places as usize {
        return None;
    }
    let fraction_value = fraction.bytes().fold(0_i64, |n, b| n * 10 + i64::from(b - b'0'));
    let fraction_value = fraction_value * 10_i64.pow(decimal_places - fraction.len() as u32);
    let whole = whole.parse::<i64>().ok()?;
    let raw = whole
        .checked_mul(10_i64.pow(decimal_places))?
        .checked_add(if text.starts_with('-') { -fraction_value } else { fraction_value })?;
    i32::try_from(raw).ok()
}
