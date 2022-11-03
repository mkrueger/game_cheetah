use crate::SearchValue;

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum SearchType {
    Guess,
    Short,
    Int,
    Int64,
    Float,
    Double
}

impl SearchType {
    pub fn get_description_text(&self) -> &str {
        match self {
            SearchType::Guess => "guess value (2-8 bytes)",
            SearchType::Short => "short (2 bytes)",
            SearchType::Int => "int (4 bytes)",
            SearchType::Int64 => "int64 (4 bytes)",
            SearchType::Float => "float (4 bytes)",
            SearchType::Double => "double (8 bytes)"
        }
    }

    pub fn get_byte_length(&self) -> usize {
        match self {
            SearchType::Guess => panic!("guess has no length"),
            SearchType::Short => 2,
            SearchType::Int => 4,
            SearchType::Int64 => 4,
            SearchType::Float => 4,
            SearchType::Double => 8
        }
    }
    
    pub fn get_short_description_text(&self) -> &str {
        match self {
            SearchType::Guess => "Guess",
            SearchType::Short => "short",
            SearchType::Int => "int",
            SearchType::Int64 => "int64",
            SearchType::Float => "float",
            SearchType::Double => "double"
        }
    }

    pub fn from_string(&self, txt: &str) -> Result<SearchValue, &str> {
        match self {
            SearchType::Short => {
                let parsed = txt.parse::<i16>();
                match parsed {
                    Ok(f) => Ok(SearchValue(SearchType::Short, i16::to_le_bytes(f).to_vec())),
                    Err(_) => Err("Invalid input")
                }
            }
            SearchType::Int =>  {
                let parsed = txt.parse::<i32>();
                match parsed {
                    Ok(f) => Ok(SearchValue(SearchType::Short, i32::to_le_bytes(f).to_vec())),
                    Err(_) => Err("Invalid input")
                }
            }
            SearchType::Int64 =>  {
                let parsed = txt.parse::<i64>();
                match parsed {
                    Ok(f) => Ok(SearchValue(SearchType::Short, i64::to_le_bytes(f).to_vec())),
                    Err(_) => Err("Invalid input")
                }
            }
            SearchType::Float => {
                let parsed = txt.parse::<f32>();
                match parsed {
                    Ok(f) => Ok(SearchValue(SearchType::Short, f32::to_le_bytes(f).to_vec())),
                    Err(_) => Err("Invalid input")
                }
            }
            SearchType::Double => {
                let parsed = txt.parse::<f64>();
                match parsed {
                    Ok(f) => Ok(SearchValue(SearchType::Short, f64::to_le_bytes(f).to_vec())),
                    Err(_) => Err("Invalid input")
                }
            }
            SearchType::Guess => {
                let parsed = txt.as_bytes().to_vec();
                Ok(SearchValue(SearchType::Guess, parsed))
            }
        }
    }
}