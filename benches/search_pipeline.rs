//! Benchmarks of the production narrowing and incremental collection paths.
//! Narrowing reads only memory owned by this benchmark process (no game needed).
//! Save a pre-change baseline with `--save-baseline before`, then compare with
//! `--baseline before`. Setup is excluded from collection timings; draining,
//! sorting, merging, deduplication and snapshot preservation are included.
use std::{hint::black_box, time::Duration};

use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use game_cheetah::{SearchContext, SearchResult, SearchType, update_results};
use process_memory::TryIntoProcessHandle;

fn bench_narrow(c: &mut Criterion) {
    let pid = process_memory::Pid::try_from(std::process::id()).expect("own PID fits");
    let handle = pid.try_into_process_handle().expect("own process handle");
    let mut group = c.benchmark_group("narrow/own_process");
    for (name, count, stride, mixed, survivor_every) in [
        ("dense_int_all", 16_384, 8, false, 1),
        ("dense_int_half", 16_384, 8, false, 2),
        ("dense_int_none", 16_384, 8, false, usize::MAX),
        ("sparse_int_half", 4096, 4096, false, 2),
        ("dense_mixed_half", 16_384, 8, true, 2),
    ] {
        let mut memory = vec![0u8; count * stride];
        let mut results = Vec::with_capacity(count);
        let mut expected = Vec::new();
        for i in 0..count {
            let ty = if mixed {
                [SearchType::Int, SearchType::Float, SearchType::Double][i % 3]
            } else {
                SearchType::Int
            };
            let survives = survivor_every != usize::MAX && i % survivor_every == 0;
            let bytes = ty.from_string(if survives { "42" } else { "99" }).unwrap().1;
            memory[i * stride..i * stride + bytes.len()].copy_from_slice(&bytes);
            let result = SearchResult::new(memory.as_ptr() as usize + i * stride, ty);
            results.push(result);
            if survives {
                expected.push((result.addr, result.search_type));
            }
        }
        let actual = update_results(&results, "42", &handle);
        assert_eq!(actual.iter().map(|r| (r.addr, r.search_type)).collect::<Vec<_>>(), expected);
        group.throughput(Throughput::Elements(count as u64));
        group.bench_function(name, |b| {
            b.iter(|| black_box(update_results(black_box(&results), black_box("42"), black_box(&handle))));
        });
        black_box(&memory); // Keep target storage alive throughout every read.
    }
    group.finish();
}

fn bench_collect(c: &mut Criterion) {
    let mut group = c.benchmark_group("collect/streamed");
    for count in [16_384, 262_144] {
        for (name, interleaved, keep_snapshot) in [("ordered", false, false), ("interleaved", true, false), ("shared_snapshot", true, true)] {
            group.throughput(Throughput::Elements(count as u64));
            group.bench_with_input(BenchmarkId::new(name, count), &count, |b, &count| {
                b.iter_batched(
                    || {
                        let context = SearchContext::new("bench".into());
                        let batches = (0..32)
                            .map(|batch| {
                                let mut hits = (0..count / 32)
                                    .map(|i| {
                                        let index = if interleaved { i * 32 + batch } else { batch * (count / 32) + i };
                                        SearchResult::new(index * 8, SearchType::Int)
                                    })
                                    .collect::<Vec<_>>();
                                hits.push(hits[0]); // Exact duplicate, including the first batch.
                                hits.reverse(); // Arrival order is not guaranteed by workers.
                                hits
                            })
                            .collect::<Vec<_>>();
                        (context, batches)
                    },
                    |(context, batches)| {
                        let mut snapshot = None;
                        for (index, batch) in batches.into_iter().enumerate() {
                            context.results_sender.send(batch).unwrap();
                            let results = context.collect_results();
                            assert_eq!(results.len(), (index + 1) * (count / 32));
                            if keep_snapshot && index == 15 {
                                snapshot = Some(results.clone());
                            }
                            black_box(results);
                        }
                        if let Some(snapshot) = snapshot {
                            assert_eq!(snapshot.len(), count / 2);
                            black_box(snapshot);
                        }
                        black_box(context.collect_results());
                    },
                    BatchSize::LargeInput,
                );
            });
        }
    }
    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(20).warm_up_time(Duration::from_secs(1)).measurement_time(Duration::from_secs(2));
    targets = bench_narrow, bench_collect
}
criterion_main!(benches);
