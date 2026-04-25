//! Microbenchmarks for the in-memory search hot paths.
//!
//! These benchmarks exercise the pure `&[u8]`-in / `Vec<SearchResult>`-out
//! functions that dominate CPU time during a scan. They do not touch the
//! target-process plumbing (rayon, channels, `process-memory`) so the numbers
//! are reproducible across machines and do not require a running game.
//!
//! Run the full suite with:
//!
//! ```sh
//! cargo bench --bench search_scan
//! ```
//!
//! Save a baseline before a perf change and compare after:
//!
//! ```sh
//! cargo bench --bench search_scan -- --save-baseline pre
//! # ... change code ...
//! cargo bench --bench search_scan -- --baseline pre
//! ```
use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use game_cheetah::{SearchResult, SearchType, UnknownComparison, compare_values, search_memory, search_string_in_memory};

/// Fold a result vector into a single value the optimiser cannot precompute.
///
/// `search_memory` returns `Vec<SearchResult>`. If the only observed output is
/// `r.len()`, LLVM is free to elide the per-match `Vec::push` and just count
/// hits, which – combined with inlining – can collapse a 16 MiB scan into a
/// few nanoseconds and produce nonsensical throughput numbers. Summing the
/// addresses (wrapping, to avoid overflow panics) forces every successful
/// match to be materialised before the loop finishes.
#[inline(never)]
fn consume(results: Vec<SearchResult>) -> usize {
    let mut acc: usize = results.len();
    for r in &results {
        acc = acc.wrapping_add(r.addr);
    }
    acc
}

/// 16 MiB is large enough to defeat L2 for most CPUs while still running each
/// iteration in well under a second.
const BUFFER_SIZE: usize = 16 * 1024 * 1024;

/// Deterministic pseudo-random fill using a small xorshift.
/// Keeps the benchmark independent of `rand` and reproducible across runs.
fn fill_deterministic(buf: &mut [u8], seed: u64) {
    let mut state = seed.wrapping_mul(0x9E3779B97F4A7C15).max(1);
    for chunk in buf.chunks_mut(8) {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let bytes = state.to_le_bytes();
        chunk.copy_from_slice(&bytes[..chunk.len()]);
    }
}

/// Build a buffer of `BUFFER_SIZE` bytes with `needle` embedded at regularly
/// spaced offsets so each benchmark has a stable, non-zero match count.
fn buffer_with_matches(needle: &[u8], stride: usize) -> Vec<u8> {
    let mut buf = vec![0u8; BUFFER_SIZE];
    fill_deterministic(&mut buf, 0xDEADBEEF);
    let mut off = 0;
    while off + needle.len() <= buf.len() {
        buf[off..off + needle.len()].copy_from_slice(needle);
        off += stride;
    }
    buf
}

fn bench_numeric_scans(c: &mut Criterion) {
    let mut group = c.benchmark_group("search_memory/numeric");
    group.throughput(Throughput::Bytes(BUFFER_SIZE as u64));

    // Short (u16)
    let needle = 0x1234u16.to_le_bytes();
    let buf = buffer_with_matches(&needle, 4096);
    group.bench_function("short_i16", |b| {
        b.iter(|| {
            let r = search_memory(black_box(&buf), black_box(&needle), SearchType::Short, 0);
            black_box(consume(r));
        });
    });

    // Int (i32)
    let needle = 0x1337C0DEu32.to_le_bytes();
    let buf = buffer_with_matches(&needle, 4096);
    group.bench_function("int_i32", |b| {
        b.iter(|| {
            let r = search_memory(black_box(&buf), black_box(&needle), SearchType::Int, 0);
            black_box(consume(r));
        });
    });

    // Int64 (i64)
    let needle = 0x0123_4567_89AB_CDEFu64.to_le_bytes();
    let buf = buffer_with_matches(&needle, 4096);
    group.bench_function("int64_i64", |b| {
        b.iter(|| {
            let r = search_memory(black_box(&buf), black_box(&needle), SearchType::Int64, 0);
            black_box(consume(r));
        });
    });

    // Float (f32)
    let needle = 1234.5f32.to_le_bytes();
    let buf = buffer_with_matches(&needle, 4096);
    group.bench_function("float_f32", |b| {
        b.iter(|| {
            let r = search_memory(black_box(&buf), black_box(&needle), SearchType::Float, 0);
            black_box(consume(r));
        });
    });

    // Double (f64)
    let needle = 1234.5f64.to_le_bytes();
    let buf = buffer_with_matches(&needle, 4096);
    group.bench_function("double_f64", |b| {
        b.iter(|| {
            let r = search_memory(black_box(&buf), black_box(&needle), SearchType::Double, 0);
            black_box(consume(r));
        });
    });

    group.finish();
}

fn bench_string_scan(c: &mut Criterion) {
    let mut group = c.benchmark_group("search_string_in_memory");
    group.throughput(Throughput::Bytes(BUFFER_SIZE as u64));

    for needle_str in ["hp", "gold", "resource_count", "GameCheetahSearchTargetToken"] {
        let needle = needle_str.as_bytes();
        let stride = 8192usize.max(needle.len() * 4);
        // Embed the UTF-8 form of the needle at a regular stride. The function
        // also scans for the UTF-16LE form in the same pass, so both paths are
        // exercised by a single benchmark per needle length.
        let buf = buffer_with_matches(needle, stride);

        group.bench_with_input(BenchmarkId::new("len", needle_str.len()), &buf, |b, buf| {
            b.iter(|| {
                let r = search_string_in_memory(black_box(buf), black_box(needle_str), 0);
                black_box(consume(r));
            });
        });
    }

    group.finish();
}

fn bench_guess_scan(c: &mut Criterion) {
    let mut group = c.benchmark_group("search_memory/guess");
    group.throughput(Throughput::Bytes(BUFFER_SIZE as u64));

    // Guess uses the raw input bytes to scan for exact matches across all
    // numeric widths. Simulate a typical "user typed 1234" query (4 bytes).
    let needle = b"1234";
    let buf = buffer_with_matches(needle, 4096);
    group.bench_function("digits_4", |b| {
        b.iter(|| {
            let r = search_memory(black_box(&buf), black_box(needle), SearchType::Guess, 0);
            black_box(consume(r));
        });
    });

    group.finish();
}

fn bench_compare_values(c: &mut Criterion) {
    let mut group = c.benchmark_group("compare_values");

    let old_i32 = 1000i32.to_le_bytes();
    let new_i32 = 1010i32.to_le_bytes();
    group.bench_function("int_increased", |b| {
        b.iter(|| {
            black_box(compare_values(
                black_box(&old_i32),
                black_box(&new_i32),
                SearchType::Int,
                UnknownComparison::Increased,
            ))
        });
    });

    let old_f32 = 1000.0f32.to_le_bytes();
    let new_f32 = 1000.25f32.to_le_bytes();
    group.bench_function("float_unchanged_eps", |b| {
        b.iter(|| {
            black_box(compare_values(
                black_box(&old_f32),
                black_box(&new_f32),
                SearchType::Float,
                UnknownComparison::Unchanged,
            ))
        });
    });

    let old_f64 = 10_000.0f64.to_le_bytes();
    let new_f64 = 10_002.0f64.to_le_bytes();
    group.bench_function("double_increased_eps", |b| {
        b.iter(|| {
            black_box(compare_values(
                black_box(&old_f64),
                black_box(&new_f64),
                SearchType::Double,
                UnknownComparison::Increased,
            ))
        });
    });

    // Bulk loop simulating one region-pass of the unknown-search filter:
    // ~1M (addr, ty) comparisons over 4-byte aligned data.
    const N: usize = 1_000_000;
    let mut old_bytes = vec![0u8; N * 4];
    let mut new_bytes = vec![0u8; N * 4];
    fill_deterministic(&mut old_bytes, 0xABCDEF);
    fill_deterministic(&mut new_bytes, 0xABCDEF ^ 0xFF);

    group.throughput(Throughput::Elements(N as u64));
    group.bench_function("bulk_int_changed_1M", |b| {
        b.iter(|| {
            let mut hits = 0usize;
            for i in 0..N {
                let a = &old_bytes[i * 4..(i + 1) * 4];
                let c = &new_bytes[i * 4..(i + 1) * 4];
                if compare_values(a, c, SearchType::Int, UnknownComparison::Changed) {
                    hits += 1;
                }
            }
            black_box(hits);
        });
    });

    group.finish();
}

criterion_group!(benches, bench_numeric_scans, bench_string_scan, bench_guess_scan, bench_compare_values);
criterion_main!(benches);
