use crate::{SearchResult, SearchType};
use memchr::memmem;

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

// SIMD-optimized f32 search for x86_64
#[cfg(target_arch = "x86_64")]
pub(super) fn search_f32_simd(memory_data: &[u8], target: f32, epsilon: f32, start: usize) -> Vec<SearchResult> {
    use std::arch::x86_64::*;

    let mut results = Vec::new();

    // Handle special cases separately
    if !target.is_finite() {
        // For NaN/Infinity, fall back to scalar
        if memory_data.len() >= 4 {
            for i in 0..=memory_data.len() - 4 {
                let value = f32::from_le_bytes([memory_data[i], memory_data[i + 1], memory_data[i + 2], memory_data[i + 3]]);
                if (value.is_nan() && target.is_nan()) || value == target {
                    results.push(SearchResult::new(start + i, SearchType::Float));
                }
            }
        }
        return results;
    }

    unsafe {
        // SAFETY: Each branch below is gated by `is_x86_feature_detected!`
        // for avx2 or sse. The unaligned float loads (`_mm256_loadu_ps` /
        // `_mm_loadu_ps`) read N readable bytes from `chunk`, which
        // `chunks_exact(N)` guarantees.
        // Try AVX2 first (processes 8 floats at a time)
        if is_x86_feature_detected!("avx2") {
            let target_vec = _mm256_set1_ps(target);
            let epsilon_vec = _mm256_set1_ps(epsilon);
            let neg_epsilon_vec = _mm256_set1_ps(-epsilon);

            // Process 32 bytes (8 f32s) at a time
            let chunks = memory_data.chunks_exact(32);
            let remainder = chunks.remainder();

            for (chunk_idx, chunk) in chunks.enumerate() {
                let data = _mm256_loadu_ps(chunk.as_ptr() as *const f32);

                let diff = _mm256_sub_ps(data, target_vec);
                let ge_neg_eps = _mm256_cmp_ps(diff, neg_epsilon_vec, _CMP_GE_OQ);
                let le_eps = _mm256_cmp_ps(diff, epsilon_vec, _CMP_LE_OQ);
                let in_range = _mm256_and_ps(ge_neg_eps, le_eps);

                let mask = _mm256_movemask_ps(in_range);

                if mask != 0 {
                    for i in 0..8 {
                        if (mask >> i) & 1 == 1 {
                            let value = f32::from_le_bytes([chunk[i * 4], chunk[i * 4 + 1], chunk[i * 4 + 2], chunk[i * 4 + 3]]);
                            if value.is_finite() && (value - target).abs() <= epsilon {
                                results.push(SearchResult::new(start + chunk_idx * 32 + i * 4, SearchType::Float));
                            }
                        }
                    }
                }
            }

            // Handle remainder with SSE or scalar
            if remainder.len() >= 4 {
                for i in 0..=(remainder.len() - 4) {
                    let value = f32::from_le_bytes([remainder[i], remainder[i + 1], remainder[i + 2], remainder[i + 3]]);
                    if value.is_finite() && (value - target).abs() <= epsilon {
                        let base_offset = memory_data.len() - remainder.len();
                        results.push(SearchResult::new(start + base_offset + i, SearchType::Float));
                    }
                }
            }
        } else if is_x86_feature_detected!("sse") {
            let target_vec = _mm_set1_ps(target);
            let epsilon_vec = _mm_set1_ps(epsilon);
            let neg_epsilon_vec = _mm_set1_ps(-epsilon);

            // Process 16 bytes (4 f32s) at a time
            let chunks = memory_data.chunks_exact(16);
            let remainder = chunks.remainder();

            for (chunk_idx, chunk) in chunks.enumerate() {
                let data = _mm_loadu_ps(chunk.as_ptr() as *const f32);

                // Calculate diff = data - target
                let diff = _mm_sub_ps(data, target_vec);

                // Check if -epsilon <= diff <= epsilon
                let ge_neg_eps = _mm_cmpge_ps(diff, neg_epsilon_vec);
                let le_eps = _mm_cmple_ps(diff, epsilon_vec);
                let in_range = _mm_and_ps(ge_neg_eps, le_eps);

                let mask = _mm_movemask_ps(in_range);

                if mask != 0 {
                    for i in 0..4 {
                        if (mask >> i) & 1 == 1 {
                            // Double-check to handle edge cases and ensure finite
                            let value = f32::from_le_bytes([chunk[i * 4], chunk[i * 4 + 1], chunk[i * 4 + 2], chunk[i * 4 + 3]]);
                            if value.is_finite() && (value - target).abs() <= epsilon {
                                results.push(SearchResult::new(start + chunk_idx * 16 + i * 4, SearchType::Float));
                            }
                        }
                    }
                }
            }

            // Handle remainder
            if remainder.len() >= 4 {
                for i in 0..=(remainder.len() - 4) {
                    let value = f32::from_le_bytes([remainder[i], remainder[i + 1], remainder[i + 2], remainder[i + 3]]);
                    if value.is_finite() && (value - target).abs() <= epsilon {
                        let base_offset = memory_data.len() - remainder.len();
                        results.push(SearchResult::new(start + base_offset + i, SearchType::Float));
                    }
                }
            }
        }
    }

    results
}

// SIMD-optimized f64 search for x86_64
#[cfg(target_arch = "x86_64")]
pub(super) fn search_f64_simd(memory_data: &[u8], target: f64, epsilon: f64, start: usize) -> Vec<SearchResult> {
    use std::arch::x86_64::*;

    let mut results = Vec::new();

    // Handle special cases separately
    if !target.is_finite() {
        if memory_data.len() >= 8 {
            for i in 0..=memory_data.len() - 8 {
                let value = f64::from_le_bytes([
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

    unsafe {
        // SAFETY: Each branch below is gated by `is_x86_feature_detected!`
        // for avx2 or sse2. The unaligned double loads
        // (`_mm256_loadu_pd` / `_mm_loadu_pd`) read N readable bytes from
        // `chunk`, which `chunks_exact(N)` guarantees.
        // Try AVX2 first (processes 4 doubles at a time)
        if is_x86_feature_detected!("avx2") {
            let target_vec = _mm256_set1_pd(target);
            let epsilon_vec = _mm256_set1_pd(epsilon);
            let neg_epsilon_vec = _mm256_set1_pd(-epsilon);

            // Process 32 bytes (4 f64s) at a time
            let chunks = memory_data.chunks_exact(32);
            let remainder = chunks.remainder();

            for (chunk_idx, chunk) in chunks.enumerate() {
                let data = _mm256_loadu_pd(chunk.as_ptr() as *const f64);

                let diff = _mm256_sub_pd(data, target_vec);
                let ge_neg_eps = _mm256_cmp_pd(diff, neg_epsilon_vec, _CMP_GE_OQ);
                let le_eps = _mm256_cmp_pd(diff, epsilon_vec, _CMP_LE_OQ);
                let in_range = _mm256_and_pd(ge_neg_eps, le_eps);

                let mask = _mm256_movemask_pd(in_range);

                if mask != 0 {
                    for i in 0..4 {
                        if (mask >> i) & 1 == 1 {
                            let offset = i * 8;
                            let value = f64::from_le_bytes([
                                chunk[offset],
                                chunk[offset + 1],
                                chunk[offset + 2],
                                chunk[offset + 3],
                                chunk[offset + 4],
                                chunk[offset + 5],
                                chunk[offset + 6],
                                chunk[offset + 7],
                            ]);
                            if value.is_finite() && (value - target).abs() <= epsilon {
                                results.push(SearchResult::new(start + chunk_idx * 32 + offset, SearchType::Double));
                            }
                        }
                    }
                }
            }

            // Handle remainder
            if remainder.len() >= 8 {
                for i in 0..=(remainder.len() - 8) {
                    let value = f64::from_le_bytes([
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
                        let base_offset = memory_data.len() - remainder.len();
                        results.push(SearchResult::new(start + base_offset + i, SearchType::Double));
                    }
                }
            }
        } else if is_x86_feature_detected!("sse2") {
            let target_vec = _mm_set1_pd(target);
            let epsilon_vec = _mm_set1_pd(epsilon);
            let neg_epsilon_vec = _mm_set1_pd(-epsilon);

            // Process 16 bytes (2 f64s) at a time
            let chunks = memory_data.chunks_exact(16);
            let remainder = chunks.remainder();

            for (chunk_idx, chunk) in chunks.enumerate() {
                let data = _mm_loadu_pd(chunk.as_ptr() as *const f64);

                let diff = _mm_sub_pd(data, target_vec);
                let ge_neg_eps = _mm_cmpge_pd(diff, neg_epsilon_vec);
                let le_eps = _mm_cmple_pd(diff, epsilon_vec);
                let in_range = _mm_and_pd(ge_neg_eps, le_eps);

                let mask = _mm_movemask_pd(in_range);

                if mask != 0 {
                    for i in 0..2 {
                        if (mask >> i) & 1 == 1 {
                            let offset = i * 8;
                            let value = f64::from_le_bytes([
                                chunk[offset],
                                chunk[offset + 1],
                                chunk[offset + 2],
                                chunk[offset + 3],
                                chunk[offset + 4],
                                chunk[offset + 5],
                                chunk[offset + 6],
                                chunk[offset + 7],
                            ]);
                            if value.is_finite() && (value - target).abs() <= epsilon {
                                results.push(SearchResult::new(start + chunk_idx * 16 + offset, SearchType::Double));
                            }
                        }
                    }
                }
            }

            // Handle remainder
            if remainder.len() >= 8 {
                for i in 0..=(remainder.len() - 8) {
                    let value = f64::from_le_bytes([
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
                        let base_offset = memory_data.len() - remainder.len();
                        results.push(SearchResult::new(start + base_offset + i, SearchType::Double));
                    }
                }
            }
        }
    }

    results
}
