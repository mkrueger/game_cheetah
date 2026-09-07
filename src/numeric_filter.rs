//! Absolute numeric predicates. Integer comparisons never round through f64.
use std::cmp::Ordering;

use i18n_embed_fl::fl;

use crate::SearchType;

/// Independent, combinable filters for existing numeric result interpretations.
#[derive(Debug, Clone)]
pub struct ResultFilter {
    pub numeric: Option<NumericFilter>,
    pub types: Vec<SearchType>,
    pub stable_for: Option<std::time::Duration>,
}

impl ResultFilter {
    pub fn validate(&self) -> Result<(), String> {
        if self.types.is_empty() || self.types.iter().any(|ty| !SearchType::NUMERIC_TYPES.contains(ty)) {
            return Err(fl!(crate::LANGUAGE_LOADER, "result-filter-no-types"));
        }
        if self
            .stable_for
            .is_some_and(|duration| !(std::time::Duration::from_secs(1)..=std::time::Duration::from_secs(30)).contains(&duration))
        {
            return Err(fl!(crate::LANGUAGE_LOADER, "result-filter-duration-error"));
        }
        Ok(())
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum NumericComparison {
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    #[default]
    GreaterEqual,
    Between,
}

impl NumericComparison {
    pub const ALL: [Self; 7] = [
        Self::Equal,
        Self::NotEqual,
        Self::Less,
        Self::LessEqual,
        Self::Greater,
        Self::GreaterEqual,
        Self::Between,
    ];

    pub fn label(self) -> String {
        match self {
            Self::Equal => "=".to_owned(),
            Self::NotEqual => "≠".to_owned(),
            Self::Less => "<".to_owned(),
            Self::LessEqual => "≤".to_owned(),
            Self::Greater => ">".to_owned(),
            Self::GreaterEqual => "≥".to_owned(),
            Self::Between => fl!(crate::LANGUAGE_LOADER, "numeric-filter-between"),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum Number {
    Integer(i64),
    Float(f64),
}

impl Number {
    fn parse(text: &str) -> Result<Self, String> {
        let text = text.trim();
        let number = if text.contains(['.', 'e', 'E']) {
            text.parse::<f64>().ok().filter(|n| n.is_finite()).map(Self::Float)
        } else {
            text.parse::<i64>().ok().map(Self::Integer)
        };
        number.ok_or_else(|| fl!(crate::LANGUAGE_LOADER, "numeric-filter-invalid"))
    }

    fn compare(self, other: Self) -> Option<Ordering> {
        match (self, other) {
            (Self::Integer(a), Self::Integer(b)) => Some(a.cmp(&b)),
            (Self::Float(a), Self::Float(b)) => a.partial_cmp(&b),
            (Self::Integer(a), Self::Float(b)) => compare_integer_float(a, b),
            (Self::Float(a), Self::Integer(b)) => compare_integer_float(b, a).map(Ordering::reverse),
        }
    }

    fn as_float32(self) -> Self {
        let value = match self {
            Self::Integer(n) => n as f32,
            Self::Float(n) => n as f32,
        };
        Self::Float(f64::from(value))
    }
}

fn compare_integer_float(integer: i64, float: f64) -> Option<Ordering> {
    if float.is_nan() {
        return None;
    }
    // i64::MAX rounds UP to 2^63 in f64. Handle that boundary before casting.
    if float >= 9223372036854775808.0 {
        return Some(Ordering::Less);
    }
    if float < -9223372036854775808.0 {
        return Some(Ordering::Greater);
    }
    match integer.cmp(&(float as i64)) {
        Ordering::Equal => 0.0_f64.partial_cmp(&float.fract()),
        order => Some(order),
    }
}

#[derive(Debug, Clone, Copy)]
pub struct NumericFilter {
    comparison: NumericComparison,
    lower: Number,
    upper: Number,
}

impl NumericFilter {
    pub fn parse(comparison: NumericComparison, lower: &str, upper: &str) -> Result<Self, String> {
        let lower = Number::parse(lower)?;
        let upper = if comparison == NumericComparison::Between {
            Number::parse(upper)?
        } else {
            lower
        };
        if lower.compare(upper) == Some(Ordering::Greater) {
            return Err(fl!(crate::LANGUAGE_LOADER, "numeric-filter-reversed"));
        }
        Ok(Self { comparison, lower, upper })
    }

    /// Exact comparisons, with Float32 bounds rounded to that storage format.
    /// Non-finite memory values and non-numeric results are excluded for every operator.
    pub fn matches(self, ty: SearchType, bytes: &[u8]) -> bool {
        let value = match ty {
            SearchType::Byte if bytes.len() == 1 => Number::Integer(i64::from(bytes[0])),
            SearchType::Short if bytes.len() == 2 => Number::Integer(i64::from(i16::from_le_bytes(bytes.try_into().unwrap()))),
            SearchType::Int if bytes.len() == 4 => Number::Integer(i64::from(i32::from_le_bytes(bytes.try_into().unwrap()))),
            SearchType::Int64 if bytes.len() == 8 => Number::Integer(i64::from_le_bytes(bytes.try_into().unwrap())),
            SearchType::Float if bytes.len() == 4 => Number::Float(f64::from(f32::from_le_bytes(bytes.try_into().unwrap()))),
            SearchType::Double if bytes.len() == 8 => Number::Float(f64::from_le_bytes(bytes.try_into().unwrap())),
            _ => return false,
        };
        if matches!(value, Number::Float(n) if !n.is_finite()) {
            return false;
        }
        let (lower, upper) = if ty == SearchType::Float {
            (self.lower.as_float32(), self.upper.as_float32())
        } else {
            (self.lower, self.upper)
        };
        let order = value.compare(lower);
        match self.comparison {
            NumericComparison::Equal => order == Some(Ordering::Equal),
            NumericComparison::NotEqual => order.is_some_and(|order| order != Ordering::Equal),
            NumericComparison::Less => order == Some(Ordering::Less),
            NumericComparison::LessEqual => matches!(order, Some(Ordering::Less | Ordering::Equal)),
            NumericComparison::Greater => order == Some(Ordering::Greater),
            NumericComparison::GreaterEqual => matches!(order, Some(Ordering::Greater | Ordering::Equal)),
            NumericComparison::Between => {
                matches!(order, Some(Ordering::Greater | Ordering::Equal)) && matches!(value.compare(upper), Some(Ordering::Less | Ordering::Equal))
            }
        }
    }
}
