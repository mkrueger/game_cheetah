use game_cheetah::state::compare_values;
use game_cheetah::{SearchType, UnknownComparison};

#[test]
fn unknown_compare_int_variants() {
    let old = 100i32.to_le_bytes();
    let new_inc = 105i32.to_le_bytes();
    let new_dec = 90i32.to_le_bytes();
    let new_same = 100i32.to_le_bytes();

    assert!(compare_values(&old, &new_inc, SearchType::Int, UnknownComparison::Increased));
    assert!(!compare_values(&old, &new_inc, SearchType::Int, UnknownComparison::Decreased));
    assert!(compare_values(&old, &new_inc, SearchType::Int, UnknownComparison::Changed));
    assert!(!compare_values(&old, &new_inc, SearchType::Int, UnknownComparison::Unchanged));

    assert!(compare_values(&old, &new_dec, SearchType::Int, UnknownComparison::Decreased));
    assert!(!compare_values(&old, &new_dec, SearchType::Int, UnknownComparison::Increased));
    assert!(compare_values(&old, &new_dec, SearchType::Int, UnknownComparison::Changed));
    assert!(!compare_values(&old, &new_dec, SearchType::Int, UnknownComparison::Unchanged));

    assert!(compare_values(&old, &new_same, SearchType::Int, UnknownComparison::Unchanged));
    assert!(!compare_values(&old, &new_same, SearchType::Int, UnknownComparison::Changed));
}

#[test]
fn unknown_compare_float_with_epsilon() {
    // For old=1000.0, eps = max(1e-4, 0.1% of 1000) = 1.0
    let old = 1000.0f32;
    let within = old + 0.5; // <= eps -> unchanged
    let beyond = old + 2.0; // > eps -> changed/increased
    let below = old - 2.0; // < old - eps -> decreased

    let oldb = old.to_le_bytes();
    let withinb = within.to_le_bytes();
    let beyondb = beyond.to_le_bytes();
    let belowb = below.to_le_bytes();

    // Unchanged within epsilon
    assert!(compare_values(&oldb, &withinb, SearchType::Float, UnknownComparison::Unchanged));
    assert!(!compare_values(&oldb, &withinb, SearchType::Float, UnknownComparison::Changed));

    // Increased beyond epsilon
    assert!(compare_values(&oldb, &beyondb, SearchType::Float, UnknownComparison::Increased));
    assert!(compare_values(&oldb, &beyondb, SearchType::Float, UnknownComparison::Changed));
    assert!(!compare_values(&oldb, &beyondb, SearchType::Float, UnknownComparison::Decreased));

    // Decreased beyond epsilon
    assert!(compare_values(&oldb, &belowb, SearchType::Float, UnknownComparison::Decreased));
    assert!(compare_values(&oldb, &belowb, SearchType::Float, UnknownComparison::Changed));
    assert!(!compare_values(&oldb, &belowb, SearchType::Float, UnknownComparison::Increased));
}

#[test]
fn unknown_compare_double_with_epsilon() {
    // For old=10_000.0, eps = max(1e-6, 0.01% of 10000) = 1.0
    let old = 10_000.0f64;
    let within = old + 0.5; // unchanged
    let beyond = old + 2.0; // increased
    let below = old - 2.0; // decreased

    let oldb = old.to_le_bytes();
    let withinb = within.to_le_bytes();
    let beyondb = beyond.to_le_bytes();
    let belowb = below.to_le_bytes();

    assert!(compare_values(&oldb, &withinb, SearchType::Double, UnknownComparison::Unchanged));
    assert!(!compare_values(&oldb, &withinb, SearchType::Double, UnknownComparison::Changed));

    assert!(compare_values(&oldb, &beyondb, SearchType::Double, UnknownComparison::Increased));
    assert!(compare_values(&oldb, &beyondb, SearchType::Double, UnknownComparison::Changed));
    assert!(!compare_values(&oldb, &beyondb, SearchType::Double, UnknownComparison::Decreased));

    assert!(compare_values(&oldb, &belowb, SearchType::Double, UnknownComparison::Decreased));
    assert!(compare_values(&oldb, &belowb, SearchType::Double, UnknownComparison::Changed));
    assert!(!compare_values(&oldb, &belowb, SearchType::Double, UnknownComparison::Increased));
}
