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

#[test]
fn unknown_compare_byte_boundaries() {
    // u8 wrap boundaries: 0 and 255 are extreme values commonly seen for
    // health/ammo-style fields.
    let zero = [0u8];
    let one = [1u8];
    let max = [255u8];

    // Equal values must never satisfy strict inequality comparisons.
    assert!(!compare_values(&zero, &zero, SearchType::Byte, UnknownComparison::Increased));
    assert!(!compare_values(&zero, &zero, SearchType::Byte, UnknownComparison::Decreased));
    assert!(compare_values(&zero, &zero, SearchType::Byte, UnknownComparison::Unchanged));
    assert!(!compare_values(&zero, &zero, SearchType::Byte, UnknownComparison::Changed));

    // 0 -> 1 increase, 255 -> 0 decrease (raw value comparison, no wrap).
    assert!(compare_values(&zero, &one, SearchType::Byte, UnknownComparison::Increased));
    assert!(compare_values(&max, &zero, SearchType::Byte, UnknownComparison::Decreased));
    assert!(compare_values(&max, &zero, SearchType::Byte, UnknownComparison::Changed));
}

#[test]
fn unknown_compare_short_signed_boundary() {
    // Crossing the i16 sign boundary (-1 -> 0) must register as an increase,
    // not a decrease, even though the unsigned representation rolls over.
    let neg_one = (-1i16).to_le_bytes();
    let zero = 0i16.to_le_bytes();
    let min = i16::MIN.to_le_bytes();
    let max = i16::MAX.to_le_bytes();

    assert!(compare_values(&neg_one, &zero, SearchType::Short, UnknownComparison::Increased));
    assert!(!compare_values(&neg_one, &zero, SearchType::Short, UnknownComparison::Decreased));

    assert!(compare_values(&min, &max, SearchType::Short, UnknownComparison::Increased));
    assert!(compare_values(&max, &min, SearchType::Short, UnknownComparison::Decreased));
}

#[test]
fn unknown_compare_int64_extremes() {
    let min = i64::MIN.to_le_bytes();
    let max = i64::MAX.to_le_bytes();
    let zero = 0i64.to_le_bytes();

    assert!(compare_values(&min, &zero, SearchType::Int64, UnknownComparison::Increased));
    assert!(compare_values(&max, &zero, SearchType::Int64, UnknownComparison::Decreased));
    assert!(compare_values(&max, &max, SearchType::Int64, UnknownComparison::Unchanged));
    assert!(!compare_values(&max, &max, SearchType::Int64, UnknownComparison::Changed));
}

#[test]
fn unknown_compare_float_subepsilon_change() {
    // For old=1000.0, eps = 1.0. A 0.999 nudge stays inside the band.
    let old = 1000.0f32;
    let nudged = 1000.0f32 + 0.999;
    let oldb = old.to_le_bytes();
    let nb = nudged.to_le_bytes();

    assert!(compare_values(&oldb, &nb, SearchType::Float, UnknownComparison::Unchanged));
    assert!(!compare_values(&oldb, &nb, SearchType::Float, UnknownComparison::Increased));
    assert!(!compare_values(&oldb, &nb, SearchType::Float, UnknownComparison::Decreased));
    assert!(!compare_values(&oldb, &nb, SearchType::Float, UnknownComparison::Changed));
}

#[test]
fn unknown_compare_float_zero_uses_floor_epsilon() {
    // old=0 collapses the relative term, so the absolute floor (1e-4) applies.
    let old = 0.0f32;
    let tiny = 5e-5f32; // below floor -> unchanged
    let larger = 1e-3f32; // above floor -> increased

    let oldb = old.to_le_bytes();
    let tb = tiny.to_le_bytes();
    let lb = larger.to_le_bytes();

    assert!(compare_values(&oldb, &tb, SearchType::Float, UnknownComparison::Unchanged));
    assert!(compare_values(&oldb, &lb, SearchType::Float, UnknownComparison::Increased));
    assert!(compare_values(&oldb, &lb, SearchType::Float, UnknownComparison::Changed));
}

#[test]
fn unknown_compare_float_non_finite() {
    // Non-finite values bypass the epsilon path and fall back to bitwise
    // equality, so only Changed/Unchanged are meaningful.
    let nan = f32::NAN.to_le_bytes();
    let inf = f32::INFINITY.to_le_bytes();
    let one = 1.0f32.to_le_bytes();

    // NaN != NaN under PartialEq: any NaN involvement is "changed".
    assert!(compare_values(&nan, &nan, SearchType::Float, UnknownComparison::Changed));
    assert!(!compare_values(&nan, &nan, SearchType::Float, UnknownComparison::Unchanged));

    // Inf vs finite registers as changed but neither increased nor decreased
    // (the function returns false for ordering on non-finite operands).
    assert!(compare_values(&inf, &one, SearchType::Float, UnknownComparison::Changed));
    assert!(!compare_values(&inf, &one, SearchType::Float, UnknownComparison::Increased));
    assert!(!compare_values(&inf, &one, SearchType::Float, UnknownComparison::Decreased));

    assert!(compare_values(&inf, &inf, SearchType::Float, UnknownComparison::Unchanged));
}

#[test]
fn unknown_compare_double_non_finite() {
    let nan = f64::NAN.to_le_bytes();
    let inf = f64::INFINITY.to_le_bytes();
    let neg_inf = f64::NEG_INFINITY.to_le_bytes();

    assert!(compare_values(&nan, &nan, SearchType::Double, UnknownComparison::Changed));
    assert!(compare_values(&inf, &neg_inf, SearchType::Double, UnknownComparison::Changed));
    assert!(!compare_values(&inf, &neg_inf, SearchType::Double, UnknownComparison::Increased));
    assert!(!compare_values(&inf, &neg_inf, SearchType::Double, UnknownComparison::Decreased));
}

#[test]
fn unknown_compare_unsupported_type_returns_false() {
    // Guess/Unknown/String have no fixed comparison semantics.
    let buf = [0u8; 8];
    for ty in [SearchType::Guess, SearchType::Unknown, SearchType::String, SearchType::StringUtf16] {
        for cmp in [
            UnknownComparison::Increased,
            UnknownComparison::Decreased,
            UnknownComparison::Changed,
            UnknownComparison::Unchanged,
        ] {
            assert!(!compare_values(&buf, &buf, ty, cmp), "{:?}/{:?} should be false", ty, cmp);
        }
    }
}
