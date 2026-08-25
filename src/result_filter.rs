use crate::{SearchType, format_value, value_as_f64};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareOp {
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Equal,
    NotEqual,
}

impl CompareOp {
    fn eval(self, value: f64, target: f64) -> bool {
        match self {
            Self::Less => value < target,
            Self::LessEqual => value <= target,
            Self::Greater => value > target,
            Self::GreaterEqual => value >= target,
            Self::Equal => value == target,
            Self::NotEqual => value != target,
        }
    }
}

/// A predicate applied to the current value of every search result, used to
/// narrow a large result list down without starting a new scan.
#[derive(Debug, Clone, PartialEq)]
pub enum ResultFilter {
    /// Keep results whose displayed value contains this text.
    Contains(String),
    /// Keep results whose value compares as requested.
    Compare(CompareOp, f64),
}

impl ResultFilter {
    /// Parse a user-typed filter such as `>1000`, `<=-1`, `!=0` or a plain
    /// digit sequence to match as a substring. `Ok(None)` means "no filter".
    pub fn parse(text: &str) -> Result<Option<Self>, String> {
        let text = text.trim();
        if text.is_empty() {
            return Ok(None);
        }
        const OPERATORS: [(&str, CompareOp); 7] = [
            (">=", CompareOp::GreaterEqual),
            ("<=", CompareOp::LessEqual),
            ("!=", CompareOp::NotEqual),
            ("==", CompareOp::Equal),
            (">", CompareOp::Greater),
            ("<", CompareOp::Less),
            ("=", CompareOp::Equal),
        ];
        for (token, op) in OPERATORS {
            let Some(rest) = text.strip_prefix(token) else {
                continue;
            };
            let number = strip_group_separators(rest);
            if number.is_empty() {
                return Err(format!("Missing number after '{token}'"));
            }
            return match number.parse::<f64>() {
                Ok(target) => Ok(Some(Self::Compare(op, target))),
                Err(err) => Err(format!("Invalid number '{}': {err}", rest.trim())),
            };
        }
        Ok(Some(Self::Contains(text.to_owned())))
    }

    /// Test the raw bytes currently stored at a result's address.
    pub fn matches(&self, search_type: SearchType, bytes: &[u8]) -> bool {
        match self {
            Self::Contains(needle) => format_value(search_type, bytes).is_some_and(|value| value.contains(needle.as_str())),
            Self::Compare(op, target) => value_as_f64(search_type, bytes).is_some_and(|value| op.eval(value, *target)),
        }
    }
}

/// Drop the separators people paste in from a game's UI (`18,000`, `18 000`)
/// so the number still parses.
fn strip_group_separators(text: &str) -> String {
    text.chars().filter(|c| !matches!(c, ',' | '_' | ' ' | '\'')).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_filter_is_none() {
        assert_eq!(Ok(None), ResultFilter::parse("   "));
    }

    #[test]
    fn parses_comparisons() {
        assert_eq!(Ok(Some(ResultFilter::Compare(CompareOp::Greater, 1000.0))), ResultFilter::parse(">1000"));
        assert_eq!(Ok(Some(ResultFilter::Compare(CompareOp::GreaterEqual, 0.0))), ResultFilter::parse(">= 0"));
        assert_eq!(Ok(Some(ResultFilter::Compare(CompareOp::Less, -1.5))), ResultFilter::parse("<-1.5"));
        assert_eq!(Ok(Some(ResultFilter::Compare(CompareOp::NotEqual, 0.0))), ResultFilter::parse("!=0"));
        assert_eq!(Ok(Some(ResultFilter::Compare(CompareOp::Equal, 42.0))), ResultFilter::parse("=42"));
        assert_eq!(Ok(Some(ResultFilter::Compare(CompareOp::Equal, 42.0))), ResultFilter::parse("==42"));
    }

    #[test]
    fn parses_grouped_numbers() {
        assert_eq!(Ok(Some(ResultFilter::Compare(CompareOp::Greater, 18000.0))), ResultFilter::parse(">18,000"));
    }

    #[test]
    fn plain_text_is_a_substring_filter() {
        assert_eq!(Ok(Some(ResultFilter::Contains("1800".to_owned()))), ResultFilter::parse("1800"));
    }

    #[test]
    fn reports_unparsable_numbers() {
        assert!(ResultFilter::parse(">abc").is_err());
        assert!(ResultFilter::parse(">").is_err());
    }

    #[test]
    fn compares_against_typed_memory() {
        let filter = ResultFilter::Compare(CompareOp::GreaterEqual, 0.0);
        assert!(filter.matches(SearchType::Int, &1i32.to_le_bytes()));
        assert!(!filter.matches(SearchType::Int, &(-1i32).to_le_bytes()));
        assert!(filter.matches(SearchType::Double, &1.5f64.to_le_bytes()));
        assert!(!filter.matches(SearchType::Float, &(-0.5f32).to_le_bytes()));
    }

    #[test]
    fn substring_matches_the_displayed_value() {
        let filter = ResultFilter::Contains("800".to_owned());
        assert!(filter.matches(SearchType::Int, &1_800_000_000i32.to_le_bytes()));
        assert!(!filter.matches(SearchType::Int, &1234i32.to_le_bytes()));
    }

    #[test]
    fn types_without_a_numeric_value_never_match() {
        let filter = ResultFilter::Compare(CompareOp::Greater, 0.0);
        assert!(!filter.matches(SearchType::String, b"1234"));
        assert!(!filter.matches(SearchType::Int, &[1, 2]));
    }
}
