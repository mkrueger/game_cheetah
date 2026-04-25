//! Macrobenchmark: parallel multi-region scan.
//!
//! `benches/search_scan.rs` measures the inner loops in isolation. This file
//! mimics the orchestration in `state::spawn_parallel_search`:
//!
//!   * a list of memory "regions" (here: owned `Vec<u8>`s) is iterated with
//!     rayon's `par_iter`;
//!   * each worker calls `search_memory` over its region and forwards a
//!     `Vec<SearchResult>` to a **bounded** crossbeam channel
//!     (`bounded(100)`, matching `RESULTS_CHANNEL_CAPACITY`);
//!   * a collector thread drains the channel into a single `Vec`.
//!
//! Two scenarios are exercised:
//!
//!   * `int` - a single typed needle, the common case after the user has
//!     committed to a value type.
//!   * `guess` - the orchestration tries `Int`, `Float`, and `Double` per
//!     region. This is what 6bbb5f0 ("avoid repeated guess search decoding")
//!     targets, so any future regression in the precomputed-decode path
//!     should show up here.
//!
//! These benchmarks are intentionally CPU-bound: they neither read the target
//! process's memory nor serialise through the OS, so the numbers are
//! comparable across machines. Rayon's global thread pool is reused across
//! iterations.
//!
//! Run with:
//!
//! ```sh
//! cargo bench --bench search_regions
//! ```
use std::hint::black_box;
use std::sync::Arc;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use crossbeam_channel::bounded;
use game_cheetah::{SearchResult, SearchType, search_memory};
use rayon::prelude::*;

/// 64 regions × 1 MiB = 64 MiB of "scanned" memory per iteration. Roughly
/// matches the working set of a mid-sized game's heap on Linux/x86_64 and
/// is small enough that each bench iteration completes in well under a
/// second on a modern desktop.
const REGION_COUNT: usize = 64;
const REGION_SIZE: usize = 1024 * 1024;
const TOTAL_BYTES: u64 = (REGION_COUNT * REGION_SIZE) as u64;

/// Channel capacity used by `SearchContext` (see `RESULTS_CHANNEL_CAPACITY`).
/// Reproducing it here keeps the macrobench faithful to production behaviour.
const CHANNEL_CAPACITY: usize = 100;

/// Deterministic xorshift fill - keeps the suite reproducible without `rand`.
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

/// Build `REGION_COUNT` regions of `REGION_SIZE` bytes each, embedding `needle`
/// at a 4 KiB stride so every region produces a stable, non-zero match count.
fn build_regions(needle: &[u8]) -> Arc<Vec<Vec<u8>>> {
    let regions: Vec<Vec<u8>> = (0..REGION_COUNT)
        .map(|i| {
            let mut buf = vec![0u8; REGION_SIZE];
            fill_deterministic(&mut buf, 0xC0FFEE ^ (i as u64));
            let mut off = 0;
            while off + needle.len() <= buf.len() {
                buf[off..off + needle.len()].copy_from_slice(needle);
                off += 4096;
            }
            buf
        })
        .collect();
    Arc::new(regions)
}

/// Drives one `par_iter` pass exactly the way `state.rs` does: each worker
/// pushes its `Vec<SearchResult>` into a bounded channel, and a collector
/// thread folds them into a single `usize` so the optimiser cannot elide
/// the work. Returns that fold so the bench harness can `black_box` it.
fn run_pass<F>(regions: &Arc<Vec<Vec<u8>>>, scan_one: F) -> usize
where
    F: Fn(&[u8], usize) -> Vec<SearchResult> + Sync + Send,
{
    let (tx, rx) = bounded::<Vec<SearchResult>>(CHANNEL_CAPACITY);

    // Collector: fold incoming results into a single value the optimiser
    // cannot precompute. Mirrors the `results_receiver` loop in
    // `SearchContext`, minus the UI plumbing.
    let collector = std::thread::spawn(move || {
        let mut acc: usize = 0;
        for batch in rx.iter() {
            acc = acc.wrapping_add(batch.len());
            for r in &batch {
                acc = acc.wrapping_add(r.addr);
            }
        }
        acc
    });

    regions.par_iter().enumerate().for_each_with(tx, |tx, (idx, region)| {
        // The real code uses the absolute region base address as `start`;
        // a synthetic offset is sufficient to keep result addresses unique
        // and force the optimiser to consume every match.
        let start = idx * REGION_SIZE;
        let results = scan_one(region, start);
        if !results.is_empty() {
            // `send` will block once 100 batches are queued, exactly as in
            // production. We deliberately do *not* swallow the error: a
            // disconnected receiver in this context means the collector
            // thread panicked, and we want the bench harness to fail loudly.
            tx.send(results).expect("collector still alive");
        }
    });

    collector.join().expect("collector thread")
}

fn bench_regions_int(c: &mut Criterion) {
    let mut group = c.benchmark_group("regions/int");
    group.throughput(Throughput::Bytes(TOTAL_BYTES));

    let needle = 0x1337C0DEu32.to_le_bytes();
    let regions = build_regions(&needle);

    group.bench_function("64x1MiB", |b| {
        b.iter(|| {
            let acc = run_pass(&regions, |buf, start| search_memory(black_box(buf), black_box(&needle), SearchType::Int, start));
            black_box(acc);
        });
    });

    group.finish();
}

fn bench_regions_guess(c: &mut Criterion) {
    let mut group = c.benchmark_group("regions/guess");
    group.throughput(Throughput::Bytes(TOTAL_BYTES));

    // Embed a 4-byte pattern so every numeric interpretation finds something.
    // The exact value matters less than the fact that all three scans below
    // walk the full region.
    let needle = b"1234";
    let regions = build_regions(needle);

    // Precomputed parsed needles, matching the post-6bbb5f0 fast path: parse
    // once outside the per-region loop, then reuse the bytes per call.
    let int_bytes = 1234i32.to_le_bytes();
    let float_bytes = 1234.0f32.to_le_bytes();
    let double_bytes = 1234.0f64.to_le_bytes();

    group.bench_function("64x1MiB_int_float_double", |b| {
        b.iter(|| {
            let acc = run_pass(&regions, |buf, start| {
                let mut all = Vec::new();
                all.extend(search_memory(black_box(buf), black_box(&int_bytes), SearchType::Int, start));
                all.extend(search_memory(black_box(buf), black_box(&float_bytes), SearchType::Float, start));
                all.extend(search_memory(black_box(buf), black_box(&double_bytes), SearchType::Double, start));
                all
            });
            black_box(acc);
        });
    });

    group.finish();
}

criterion_group!(benches, bench_regions_int, bench_regions_guess);
criterion_main!(benches);
