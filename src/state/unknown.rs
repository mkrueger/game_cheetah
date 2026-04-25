use crate::{SearchType, UnknownComparison};

fn float_eps(old: f32) -> f32 {
    // 0.1% relative or 1e-4 absolute minimum
    (old.abs() * 1e-3).max(1e-4)
}

fn double_eps(old: f64) -> f64 {
    // 0.01% relative or 1e-6 absolute minimum
    (old.abs() * 1e-4).max(1e-6)
}

/// Helper function to compare values based on type and comparison.
///
/// Used by the unknown-search workflow to decide whether a candidate
/// address still matches a chosen relation (Increased / Decreased /
/// Changed / Unchanged) between two snapshots of the same memory.
pub fn compare_values(old_bytes: &[u8], new_bytes: &[u8], search_type: SearchType, comparison: UnknownComparison) -> bool {
    match search_type {
        SearchType::Byte => {
            let old = old_bytes[0];
            let new = new_bytes[0];
            match comparison {
                UnknownComparison::Decreased => new < old,
                UnknownComparison::Increased => new > old,
                UnknownComparison::Changed => new != old,
                UnknownComparison::Unchanged => new == old,
            }
        }
        SearchType::Short => {
            let old = i16::from_le_bytes([old_bytes[0], old_bytes[1]]);
            let new = i16::from_le_bytes([new_bytes[0], new_bytes[1]]);
            match comparison {
                UnknownComparison::Decreased => new < old,
                UnknownComparison::Increased => new > old,
                UnknownComparison::Changed => new != old,
                UnknownComparison::Unchanged => new == old,
            }
        }
        SearchType::Int => {
            let old = i32::from_le_bytes([old_bytes[0], old_bytes[1], old_bytes[2], old_bytes[3]]);
            let new = i32::from_le_bytes([new_bytes[0], new_bytes[1], new_bytes[2], new_bytes[3]]);
            match comparison {
                UnknownComparison::Decreased => new < old,
                UnknownComparison::Increased => new > old,
                UnknownComparison::Changed => new != old,
                UnknownComparison::Unchanged => new == old,
            }
        }
        SearchType::Int64 => {
            let old = i64::from_le_bytes([
                old_bytes[0],
                old_bytes[1],
                old_bytes[2],
                old_bytes[3],
                old_bytes[4],
                old_bytes[5],
                old_bytes[6],
                old_bytes[7],
            ]);
            let new = i64::from_le_bytes([
                new_bytes[0],
                new_bytes[1],
                new_bytes[2],
                new_bytes[3],
                new_bytes[4],
                new_bytes[5],
                new_bytes[6],
                new_bytes[7],
            ]);
            match comparison {
                UnknownComparison::Decreased => new < old,
                UnknownComparison::Increased => new > old,
                UnknownComparison::Changed => new != old,
                UnknownComparison::Unchanged => new == old,
            }
        }
        SearchType::Float => {
            let old = f32::from_le_bytes([old_bytes[0], old_bytes[1], old_bytes[2], old_bytes[3]]);
            let new = f32::from_le_bytes([new_bytes[0], new_bytes[1], new_bytes[2], new_bytes[3]]);
            if old.is_finite() && new.is_finite() {
                let eps = float_eps(old);
                match comparison {
                    UnknownComparison::Decreased => new < old - eps,
                    UnknownComparison::Increased => new > old + eps,
                    UnknownComparison::Changed => (new - old).abs() > eps,
                    UnknownComparison::Unchanged => (new - old).abs() <= eps,
                }
            } else {
                match comparison {
                    UnknownComparison::Changed => new != old,
                    UnknownComparison::Unchanged => new == old,
                    _ => false,
                }
            }
        }
        SearchType::Double => {
            let old = f64::from_le_bytes([
                old_bytes[0],
                old_bytes[1],
                old_bytes[2],
                old_bytes[3],
                old_bytes[4],
                old_bytes[5],
                old_bytes[6],
                old_bytes[7],
            ]);
            let new = f64::from_le_bytes([
                new_bytes[0],
                new_bytes[1],
                new_bytes[2],
                new_bytes[3],
                new_bytes[4],
                new_bytes[5],
                new_bytes[6],
                new_bytes[7],
            ]);
            if old.is_finite() && new.is_finite() {
                let eps = double_eps(old);
                match comparison {
                    UnknownComparison::Decreased => new < old - eps,
                    UnknownComparison::Increased => new > old + eps,
                    UnknownComparison::Changed => (new - old).abs() > eps,
                    UnknownComparison::Unchanged => (new - old).abs() <= eps,
                }
            } else {
                match comparison {
                    UnknownComparison::Changed => new != old,
                    UnknownComparison::Unchanged => new == old,
                    _ => false,
                }
            }
        }
        _ => false,
    }
}
