use i18n_embed_fl::fl;

use crate::SearchValue;

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum SearchType {
    Guess,
    Byte,
    Short,
    Int,
    Int64,
    Float,
    Double,
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
        }
    }

    pub fn get_byte_length(&self) -> usize {
        match self {
            SearchType::Guess => panic!("guess has no length"),
            SearchType::Byte => 1,
            SearchType::Short => 2,
            SearchType::Int => 4,
            SearchType::Int64 => 4,
            SearchType::Float => 4,
            SearchType::Double => 8,
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
        }
    }

    pub fn from_string(&self, txt: &str) -> Result<SearchValue, String> {
        match self {
            SearchType::Byte => {
                let parsed = txt.parse::<u8>();
                match parsed {
                    Ok(f) => Ok(SearchValue(SearchType::Byte, vec![f])),
                    Err(_) => Err(fl!(crate::LANGUAGE_LOADER, "invalid-input-error")),
                }
            }
            SearchType::Short => {
                let parsed = txt.parse::<i16>();
                match parsed {
                    Ok(f) => Ok(SearchValue(SearchType::Short, i16::to_le_bytes(f).to_vec())),
                    Err(_) => Err(fl!(crate::LANGUAGE_LOADER, "invalid-input-error")),
                }
            }
            SearchType::Int => {
                let parsed = txt.parse::<i32>();
                match parsed {
                    Ok(f) => Ok(SearchValue(SearchType::Int, i32::to_le_bytes(f).to_vec())),
                    Err(_) => Err(fl!(crate::LANGUAGE_LOADER, "invalid-input-error")),
                }
            }
            SearchType::Int64 => {
                let parsed = txt.parse::<i64>();
                match parsed {
                    Ok(f) => Ok(SearchValue(SearchType::Int64, i64::to_le_bytes(f).to_vec())),
                    Err(_) => Err(fl!(crate::LANGUAGE_LOADER, "invalid-input-error")),
                }
            }
            SearchType::Float => {
                let parsed = txt.parse::<f32>();
                match parsed {
                    Ok(f) => Ok(SearchValue(SearchType::Float, f32::to_le_bytes(f).to_vec())),
                    Err(_) => Err(fl!(crate::LANGUAGE_LOADER, "invalid-input-error")),
                }
            }
            SearchType::Double => {
                let parsed = txt.parse::<f64>();
                match parsed {
                    Ok(f) => Ok(SearchValue(SearchType::Double, f64::to_le_bytes(f).to_vec())),
                    Err(_) => Err(fl!(crate::LANGUAGE_LOADER, "invalid-input-error")),
                }
            }
            SearchType::Guess => {
                let parsed = txt.as_bytes().to_vec();
                Ok(SearchValue(SearchType::Guess, parsed))
            }
        }
    }
}
