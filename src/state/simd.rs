use crate::{SearchResult, SearchType};
use bytemuck::pod_read_unaligned;
use memchr::memmem;
use wide::{CmpLe, f32x8, f64x4};

// Float search tolerance.
//
// This is intentionally a coarse tolerance just under 1.0 (rather than a
// small relative epsilon). The tool is primarily used with games whose
// floating-point values drift slightly between frames or are stored in a
// scaled/quantized form that does not round-trip exactly to the user's
// typed value. The motivating example is "Warhammer 40,000: Dawn of War",
// where matching a typed integer like resource amounts against the in-game
// f32/f64 storage requires accepting differences well above machine
// epsilon. Tightening this would make those values impossible to find.
pub(super) fn get_epsilon_f32(_current: f32) -> f32 {
    1.0 - f32::EPSILON
}

pub(super) fn get_epsilon_f64(_current: f64) -> f64 {
    1.0 - f64::EPSILON
}

// Substring scan for fixed-width little-endian integer needles.
//
// Historically this function had two parallel implementations: a hand-rolled
// SSE2/AVX2 path on x86_64 that compared *aligned* lanes only, and a generic
// path that combined a chunked aligned scan with `memmem::find_iter` to
// recover unaligned matches. The two paths produced subtly different result
// sets - the x86_64 path missed unaligned matches on buffers larger than one
// SIMD chunk - and the hand-rolled SSE2 code for `Int64` ran roughly 10x
// slower than `memchr`'s portable implementation.
//
// `memchr 2.7` ships a runtime-dispatched substring search (Two-Way with a
// SIMD prefilter; uses AVX2/AVX-512 when available) that is correct for
// every byte alignment, faster than the hand-rolled SSE2 paths on every
// width we care about, and works uniformly across architectures. Using it
// here unifies the implementation, fixes the missing-unaligned-matches bug
// on x86_64, and shrinks the surface that has to be reasoned about for
// safety.
//
// `search_data.len()` is validated against the type so a malformed needle is
// rejected before the expensive scan starts.
pub(super) fn search_integers(memory_data: &[u8], search_data: &[u8], search_type: SearchType, start: usize) -> Vec<SearchResult> {
    let expected_len = match search_type {
        SearchType::Short => 2,
        SearchType::Int => 4,
        SearchType::Int64 => 8,
        _ => return Vec::new(),
    };
    if search_data.len() != expected_len {
        return Vec::new();
    }

    // `Finder` precomputes the prefilter / shift table for the needle. Since
    // each call here scans a fresh region with a fresh needle, building it
    // once per call is the correct trade-off; callers wanting to amortise it
    // across many regions should hoist the finder themselves.
    let finder = memmem::Finder::new(search_data);
    finder.find_iter(memory_data).map(|pos| SearchResult::new(start + pos, search_type)).collect()
}

// SIMD-optimised f32 search using the `wide` crate.
//
// Replaces a hand-rolled x86_64-only AVX2/SSE implementation with a portable
// 8-wide vectorised scan that compiles to AVX2 on x86_64 (when the target
// supports it) and to NEON on aarch64. The scan visits every 4-byte-aligned
// position - the same set of positions the previous SIMD path examined - so
// observable results are unchanged on x86_64. Targets that are not finite
// (NaN / infinity) take a scalar fallback that walks every byte offset, so
// those rare searches still find unaligned matches.
pub(super) fn search_f32_simd(memory_data: &[u8], target: f32, epsilon: f32, start: usize) -> Vec<SearchResult> {
    let mut results = Vec::new();

    if !target.is_finite() {
        // For NaN/Infinity, fall back to scalar
        if memory_data.len() >= 4 {
            for i in 0..=memory_data.len() - 4 {
                let value = f32::from_ne_bytes([memory_data[i], memory_data[i + 1], memory_data[i + 2], memory_data[i + 3]]);
                if (value.is_nan() && target.is_nan()) || value == target {
                    results.push(SearchResult::new(start + i, SearchType::Float));
                }
            }
        }
        return results;
    }

    let target_vec = f32x8::splat(target);
    let epsilon_vec = f32x8::splat(epsilon);

    // Process 32 bytes (8 f32s) at a time
    let chunks = memory_data.chunks_exact(32);
    let remainder = chunks.remainder();

    for (chunk_idx, chunk) in chunks.enumerate() {
        // Unaligned read into [f32; 8] - compiles to a single vector load.
        let arr: [f32; 8] = pod_read_unaligned(chunk);
        let data = f32x8::from(arr);

        let diff = (data - target_vec).abs();
        let in_range = diff.cmp_le(epsilon_vec) & data.is_finite();

        let mask = in_range.move_mask();
        if mask != 0 {
            for i in 0..8 {
                if (mask >> i) & 1 == 1 {
                    results.push(SearchResult::new(start + chunk_idx * 32 + i * 4, SearchType::Float));
                }
            }
        }
    }

    // Handle remainder
    if remainder.len() >= 4 {
        let base_offset = memory_data.len() - remainder.len();
        for i in 0..=(remainder.len() - 4) {
            let value = f32::from_ne_bytes([remainder[i], remainder[i + 1], remainder[i + 2], remainder[i + 3]]);
            if value.is_finite() && (value - target).abs() <= epsilon {
                results.push(SearchResult::new(start + base_offset + i, SearchType::Float));
            }
        }
    }

    results
}

// SIMD-optimised f64 search using the `wide` crate.
//
// 4-wide portable vector scan, replacing the x86_64-only AVX2/SSE2 path.
// Same alignment semantics as the prior SIMD code: every 8-byte-aligned
// position is checked, with a per-byte scalar fallback only when the target
// is not finite.
pub(super) fn search_f64_simd(memory_data: &[u8], target: f64, epsilon: f64, start: usize) -> Vec<SearchResult> {
    let mut results = Vec::new();

    if !target.is_finite() {
        if memory_data.len() >= 8 {
            for i in 0..=memory_data.len() - 8 {
                let value = f64::from_ne_bytes([
                    memory_data[i],
                    memory_data[i + 1],
                    memory_data[i + 2],
                    memory_data[i + 3],
                    memory_data[i + 4],
                    memory_data[i + 5],
                    memory_data[i + 6],
                    memory_data[i + 7],
                ]);
                if (value.is_nan() && target.is_nan()) || value == target {
                    results.push(SearchResult::new(start + i, SearchType::Double));
                }
            }
        }
        return results;
    }

    let target_vec = f64x4::splat(target);
    let epsilon_vec = f64x4::splat(epsilon);

    // Process 32 bytes (4 f64s) at a time
    let chunks = memory_data.chunks_exact(32);
    let remainder = chunks.remainder();

    for (chunk_idx, chunk) in chunks.enumerate() {
        let arr: [f64; 4] = pod_read_unaligned(chunk);
        let data = f64x4::from(arr);

        let diff = (data - target_vec).abs();
        let in_range = diff.cmp_le(epsilon_vec) & data.is_finite();

        let mask = in_range.move_mask();
        if mask != 0 {
            for i in 0..4 {
                if (mask >> i) & 1 == 1 {
                    results.push(SearchResult::new(start + chunk_idx * 32 + i * 8, SearchType::Double));
                }
            }
        }
    }

    // Handle remainder
    if remainder.len() >= 8 {
        let base_offset = memory_data.len() - remainder.len();
        for i in 0..=(remainder.len() - 8) {
            let value = f64::from_ne_bytes([
                remainder[i],
                remainder[i + 1],
                remainder[i + 2],
                remainder[i + 3],
                remainder[i + 4],
                remainder[i + 5],
                remainder[i + 6],
                remainder[i + 7],
            ]);
            if value.is_finite() && (value - target).abs() <= epsilon {
                results.push(SearchResult::new(start + base_offset + i, SearchType::Double));
            }
        }
    }

    results
}
