# Search performance checks

## Suites

- `search_scan`: in-memory numeric/string scans and unknown-value comparisons.
- `search_regions`: parallel scans of synthetic regions, without process-memory
  I/O or the production sorted result cache.
- `search_pipeline`: the production narrowing and incremental collection paths.
  Narrowing reads the benchmark process's own allocated memory, so no game,
  elevated permissions or external target is needed.

## Before/after workflow

Run the same benchmark harness on both versions, on the same idle machine:

```sh
cargo bench --locked --bench search_pipeline -- --save-baseline before
# Apply the implementation changes, leaving benchmark inputs unchanged.
cargo bench --locked --bench search_pipeline -- --baseline before
```

Criterion stores the local baseline and reports under `target/criterion`.
They are generated artifacts, not committed source. Do not overwrite the
`before` baseline with an optimized run if it is still needed for comparison.
For longer measurements, pass e.g. `--sample-size 50 --measurement-time 5` to
Criterion on both versions.

The harness defaults to 20 samples, 1 s warmup and 2 s measurement per case.
Criterion can extend measurement for slow cases. Narrowing timings include
preparation, reads, comparisons and result allocation, but exclude target and
handle setup. Production prepares comparisons once for all Rayon chunks; the
benchmark calls the same narrowing implementation for one candidate list.
It does not measure thread scheduling or the complete interactive application.

Collection uses 32 batches. Target generation and context setup are excluded;
channel sends, draining, sorting/merging, deduplication and result checks are
included. Every batch contains a duplicate and arrives internally reversed.
`ordered` batches cover increasing address ranges; `interleaved` batches
interleave addresses. `shared_snapshot` retains the halfway result until the
end, checking that copy-on-write does not mutate it. The throughput count is
the number of unique final results, not the total number of merge operations.

## Local measurements, 2026-09-06

Linux, AMD Ryzen 9 9950X3D, Rust 1.96.0, Cargo's optimized bench profile.
Baseline: commit `629db69` plus this benchmark and a visibility-only change to
the original `update_results`. Final measurements include exact-length Linux
reads, which reject partial `process_vm_readv` results.

Central time estimates from the baseline and final comparison:

| Case | Before | After | Approx. speedup |
| --- | ---: | ---: | ---: |
| Narrow 16,384 dense Int hits, all survive | 2.670 ms | 62.96 µs | 42× |
| Narrow 16,384 dense Int hits, half survive | 2.712 ms | 60.34 µs | 45× |
| Narrow 16,384 dense Int hits, none survive | 2.608 ms | 57.57 µs | 45× |
| Narrow 4,096 sparse Int hits, half survive | 696.2 µs | 673.1 µs | 1.03× |
| Narrow 16,384 dense mixed hits, half survive | 2.807 ms | 53.29 µs | 53× |
| Collect 16,384 ordered results | 342.3 µs | 112.7 µs | 3.0× |
| Collect 16,384 interleaved results | 969.0 µs | 374.5 µs | 2.6× |
| Collect 16,384 results with shared snapshot | 975.2 µs | 372.3 µs | 2.6× |
| Collect 262,144 ordered results | 6.703 ms | 2.516 ms | 2.7× |
| Collect 262,144 interleaved results | 16.694 ms | 8.365 ms | 2.0× |
| Collect 262,144 results with shared snapshot | 19.175 ms | 7.548 ms | 2.5× |

Dense candidates are 8 bytes apart; sparse candidates are 4 KiB apart. Mixed
candidates cycle through Int, Float and Double. Targets remain constant during
measurement. Large interleaved collection measurements varied between runs
(about 6.2–8.4 ms); these are local workload results, not universal speedups.
Criterion's reported percentage change uses its own statistical estimate and
may differ slightly from dividing the central values above.

## Correctness checks

`cargo test --all-features --locked` covers:

- All numeric types, duplicate/unsorted/unaligned candidates, epsilon comparison,
  NaN, infinities, signed zero, invalid input and overflowing addresses.
- Bounded dense reads and minimal sparse reads; individual fallback when a
  grouped read fails, including destinations partially modified before failure.
- On Linux, an actual anonymous mapping with a protected page verifies that
  short successful syscalls cannot cause false matches from unread bytes.
- Incremental merge equivalence to full sort/dedup, distinct types at the same
  address, immutable shared/Undo snapshots, cache invalidation and allocation
  reuse when results are exclusively owned.
