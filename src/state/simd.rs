use crate::{SearchResult, SearchType};
#[cfg(not(target_arch = "x86_64"))]
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

// Optimized search for aligned integers using SIMD
pub(super) fn search_aligned_integers(memory_data: &[u8], search_data: &[u8], search_type: SearchType, start: usize) -> Vec<SearchResult> {
    let mut results = Vec::new();

    match search_type {
        SearchType::Short => {
            if search_data.len() != 2 {
                return results;
            }
            let search_value = u16::from_le_bytes([search_data[0], search_data[1]]);

            #[cfg(target_arch = "x86_64")]
            {
                results.extend(search_u16_simd(memory_data, search_value, start));
            }

            #[cfg(not(target_arch = "x86_64"))]
            {
                // Search aligned positions first (much faster)
                let aligned_data = &memory_data[..memory_data.len() & !1];
                for (i, chunk) in aligned_data.chunks_exact(2).enumerate() {
                    let value = u16::from_le_bytes([chunk[0], chunk[1]]);
                    if value == search_value {
                        results.push(SearchResult::new(start + i * 2, SearchType::Short));
                    }
                }

                // Check unaligned positions (slower, but necessary for completeness)
                if memory_data.len() > 2 {
                    for i in 1..memory_data.len() - 1 {
                        if memory_data[i] == search_data[0] && memory_data[i + 1] == search_data[1] {
                            results.push(SearchResult::new(start + i, SearchType::Short));
                        }
                    }
                }
            }
        }
        SearchType::Int => {
            if search_data.len() != 4 {
                return results;
            }
            let search_value = u32::from_le_bytes([search_data[0], search_data[1], search_data[2], search_data[3]]);

            // Use SIMD on x86_64 if available
            #[cfg(target_arch = "x86_64")]
            {
                results.extend(search_u32_simd(memory_data, search_value, start));
            }

            // Fallback for non-x86_64 or if SIMD didn't find everything
            #[cfg(not(target_arch = "x86_64"))]
            {
                // Search aligned positions first
                let aligned_data = &memory_data[..memory_data.len() & !3];
                for (i, chunk) in aligned_data.chunks_exact(4).enumerate() {
                    let value = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                    if value == search_value {
                        results.push(SearchResult::new(start + i * 4, SearchType::Int));
                    }
                }

                // For unaligned search, use memmem which is SIMD optimized
                let finder = memmem::Finder::new(search_data);
                for pos in finder.find_iter(memory_data) {
                    // Skip aligned positions we already found
                    if pos % 4 != 0 {
                        results.push(SearchResult::new(start + pos, SearchType::Int));
                    }
                }
            }
        }
        SearchType::Int64 => {
            if search_data.len() != 8 {
                return results;
            }
            let search_value = u64::from_le_bytes([
                search_data[0],
                search_data[1],
                search_data[2],
                search_data[3],
                search_data[4],
                search_data[5],
                search_data[6],
                search_data[7],
            ]);

            // Use SIMD on x86_64 if available
            #[cfg(target_arch = "x86_64")]
            {
                results.extend(search_u64_simd(memory_data, search_value, start));
            }

            // Fallback for non-x86_64
            #[cfg(not(target_arch = "x86_64"))]
            {
                // Search aligned positions first
                let aligned_data = &memory_data[..memory_data.len() & !7];
                for (i, chunk) in aligned_data.chunks_exact(8).enumerate() {
                    let value = u64::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5], chunk[6], chunk[7]]);
                    if value == search_value {
                        results.push(SearchResult::new(start + i * 8, SearchType::Int64));
                    }
                }

                // For unaligned search, use memmem
                let finder = memmem::Finder::new(search_data);
                for pos in finder.find_iter(memory_data) {
                    if pos % 8 != 0 {
                        results.push(SearchResult::new(start + pos, SearchType::Int64));
                    }
                }
            }
        }
        _ => {}
    }

    results
}

// For even better performance with explicit SIMD, you can use the `packed_simd` or `std::simd` features
#[cfg(target_arch = "x86_64")]
fn search_u32_simd(memory_data: &[u8], search_value: u32, start: usize) -> Vec<SearchResult> {
    use std::arch::x86_64::*;

    let mut results = Vec::new();

    // SAFETY: All x86_64 SSE2 intrinsics below are gated by
    // `is_x86_feature_detected!("sse2")`. `_mm_loadu_si128` performs an
    // unaligned 16-byte load, and `chunks_exact(16)` guarantees each `chunk`
    // is exactly 16 readable bytes from `memory_data`. No pointers are
    // retained beyond the loop body.
    unsafe {
        // Ensure we have SSE2 support
        if is_x86_feature_detected!("sse2") {
            let search_vec = _mm_set1_epi32(search_value as i32);

            // Process 16 bytes (4 u32s) at a time
            let chunks = memory_data.chunks_exact(16);
            let remainder = chunks.remainder();

            for (chunk_idx, chunk) in chunks.enumerate() {
                let data = _mm_loadu_si128(chunk.as_ptr() as *const __m128i);
                let cmp = _mm_cmpeq_epi32(data, search_vec);
                let mask = _mm_movemask_epi8(cmp);

                if mask != 0 {
                    // Check each u32 in the chunk
                    for i in 0..4 {
                        if (mask >> (i * 4)) & 0xF == 0xF {
                            results.push(SearchResult::new(start + chunk_idx * 16 + i * 4, SearchType::Int));
                        }
                    }
                }
            }

            // Handle remainder with regular search
            if remainder.len() >= 4 {
                for i in 0..=(remainder.len() - 4) {
                    let value = u32::from_le_bytes([remainder[i], remainder[i + 1], remainder[i + 2], remainder[i + 3]]);
                    if value == search_value {
                        let base_offset = memory_data.len() - remainder.len();
                        results.push(SearchResult::new(start + base_offset + i, SearchType::Int));
                    }
                }
            }
        }
    }

    results
}

#[cfg(target_arch = "x86_64")]
fn search_u64_simd(memory_data: &[u8], search_value: u64, start: usize) -> Vec<SearchResult> {
    use std::arch::x86_64::*;

    let mut results = Vec::new();

    // SAFETY: Each branch is gated by the appropriate `is_x86_feature_detected!`
    // check (avx2 / sse2). `chunks_exact(N)` guarantees N readable bytes per
    // chunk for the unaligned loads (`_mm256_loadu_si256`/`_mm_loadu_si128`).
    unsafe {
        // For u64, we can use different strategies depending on available features
        if is_x86_feature_detected!("avx2") {
            // AVX2 path - process 32 bytes (4 u64s) at a time
            let search_vec = _mm256_set1_epi64x(search_value as i64);

            let chunks = memory_data.chunks_exact(32);
            let remainder = chunks.remainder();

            for (chunk_idx, chunk) in chunks.enumerate() {
                let data = _mm256_loadu_si256(chunk.as_ptr() as *const __m256i);
                let cmp = _mm256_cmpeq_epi64(data, search_vec);
                let mask = _mm256_movemask_epi8(cmp);

                if mask != 0 {
                    // Check each u64 in the chunk
                    for i in 0..4 {
                        if (mask >> (i * 8)) & 0xFF == 0xFF {
                            results.push(SearchResult::new(start + chunk_idx * 32 + i * 8, SearchType::Int64));
                        }
                    }
                }
            }

            // Handle remainder
            if remainder.len() >= 8 {
                for i in 0..=(remainder.len() - 8) {
                    let value = u64::from_le_bytes([
                        remainder[i],
                        remainder[i + 1],
                        remainder[i + 2],
                        remainder[i + 3],
                        remainder[i + 4],
                        remainder[i + 5],
                        remainder[i + 6],
                        remainder[i + 7],
                    ]);
                    if value == search_value {
                        let base_offset = memory_data.len() - remainder.len();
                        results.push(SearchResult::new(start + base_offset + i, SearchType::Int64));
                    }
                }
            }
        } else if is_x86_feature_detected!("sse2") {
            // SSE2 path - process 16 bytes (2 u64s) at a time
            let search_low = _mm_set1_epi32((search_value & 0xFFFFFFFF) as i32);
            let search_high = _mm_set1_epi32((search_value >> 32) as i32);

            let chunks = memory_data.chunks_exact(16);
            let remainder = chunks.remainder();

            for (chunk_idx, chunk) in chunks.enumerate() {
                let data = _mm_loadu_si128(chunk.as_ptr() as *const __m128i);

                // Compare both u64 values in the chunk
                // First u64 (bytes 0-7)
                let data_low = _mm_shuffle_epi32(data, 0b01000100); // Get low 32 bits of both u64s
                let data_high = _mm_shuffle_epi32(data, 0b11101110); // Get high 32 bits of both u64s

                let cmp_low = _mm_cmpeq_epi32(data_low, search_low);
                let cmp_high = _mm_cmpeq_epi32(data_high, search_high);
                let cmp_combined = _mm_and_si128(cmp_low, cmp_high);

                let mask = _mm_movemask_epi8(cmp_combined);

                // Check first u64
                if (mask & 0x00FF) == 0x00FF {
                    let value = u64::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5], chunk[6], chunk[7]]);
                    if value == search_value {
                        results.push(SearchResult::new(start + chunk_idx * 16, SearchType::Int64));
                    }
                }

                // Check second u64
                if (mask & 0xFF00) == 0xFF00 {
                    let value = u64::from_le_bytes([chunk[8], chunk[9], chunk[10], chunk[11], chunk[12], chunk[13], chunk[14], chunk[15]]);
                    if value == search_value {
                        results.push(SearchResult::new(start + chunk_idx * 16 + 8, SearchType::Int64));
                    }
                }
            }

            // Handle remainder
            if remainder.len() >= 8 {
                for i in 0..=(remainder.len() - 8) {
                    let value = u64::from_le_bytes([
                        remainder[i],
                        remainder[i + 1],
                        remainder[i + 2],
                        remainder[i + 3],
                        remainder[i + 4],
                        remainder[i + 5],
                        remainder[i + 6],
                        remainder[i + 7],
                    ]);
                    if value == search_value {
                        let base_offset = memory_data.len() - remainder.len();
                        results.push(SearchResult::new(start + base_offset + i, SearchType::Int64));
                    }
                }
            }
        } else {
            // Fallback to non-SIMD implementation
            for i in 0..=(memory_data.len().saturating_sub(8)) {
                let value = u64::from_le_bytes([
                    memory_data[i],
                    memory_data[i + 1],
                    memory_data[i + 2],
                    memory_data[i + 3],
                    memory_data[i + 4],
                    memory_data[i + 5],
                    memory_data[i + 6],
                    memory_data[i + 7],
                ]);
                if value == search_value {
                    results.push(SearchResult::new(start + i, SearchType::Int64));
                }
            }
        }
    }

    results
}

// SIMD-optimized u16 search for x86_64
#[cfg(target_arch = "x86_64")]
fn search_u16_simd(memory_data: &[u8], search_value: u16, start: usize) -> Vec<SearchResult> {
    use std::arch::x86_64::*;

    let mut results = Vec::new();

    // SAFETY: Gated by `is_x86_feature_detected!("sse2")`. Each `chunk` from
    // `chunks_exact(16)` provides 16 readable bytes for the unaligned
    // `_mm_loadu_si128` load.
    unsafe {
        if is_x86_feature_detected!("sse2") {
            let search_vec = _mm_set1_epi16(search_value as i16);

            // Process 16 bytes (8 u16s) at a time
            let chunks = memory_data.chunks_exact(16);
            let remainder = chunks.remainder();

            for (chunk_idx, chunk) in chunks.enumerate() {
                let data = _mm_loadu_si128(chunk.as_ptr() as *const __m128i);
                let cmp = _mm_cmpeq_epi16(data, search_vec);
                let mask = _mm_movemask_epi8(cmp);

                if mask != 0 {
                    // Check each u16 in the chunk
                    for i in 0..8 {
                        if (mask >> (i * 2)) & 0x3 == 0x3 {
                            results.push(SearchResult::new(start + chunk_idx * 16 + i * 2, SearchType::Short));
                        }
                    }
                }
            }

            // Handle remainder with regular search
            if remainder.len() >= 2 {
                for i in 0..=(remainder.len() - 2) {
                    let value = u16::from_le_bytes([remainder[i], remainder[i + 1]]);
                    if value == search_value {
                        let base_offset = memory_data.len() - remainder.len();
                        results.push(SearchResult::new(start + base_offset + i, SearchType::Short));
                    }
                }
            }
        }
    }

    results
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
