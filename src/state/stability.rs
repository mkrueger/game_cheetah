//! Stability is sampled after Apply, not inferred from the visible-row cache.
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use crossbeam_channel::{Receiver, RecvTimeoutError};
use process_memory::{CopyAddress, TryIntoProcessHandle};

use crate::{GameCheetahEngine, SearchMode, SearchResult};

use super::{memory_reader::ExactProcessReader, narrowing::PreparedSearch};

const SAMPLE_INTERVAL: Duration = Duration::from_millis(200);

struct StableValues {
    results: Vec<SearchResult>,
    bytes: Vec<[u8; 8]>,
}

impl StableValues {
    fn capture<T: CopyAddress>(old: &[SearchResult], prepared: &PreparedSearch, reader: &T) -> Self {
        let mut bytes = Vec::new();
        let results = prepared.read_matching(old, reader, |_, current| {
            let mut packed = [0; 8];
            packed[..current.len()].copy_from_slice(current);
            bytes.push(packed);
            true
        });
        Self { results, bytes }
    }

    fn sample<T: CopyAddress>(&mut self, prepared: &PreparedSearch, reader: &T) {
        let mut bytes = Vec::new();
        let results = prepared.read_matching(&self.results, reader, |index, current| {
            if current == &self.bytes[index][..current.len()] {
                bytes.push(self.bytes[index]);
                true
            } else {
                false
            }
        });
        // Removed candidates never return, even if they later revert to baseline.
        self.results = results;
        self.bytes = bytes;
    }
}

fn observe<T: CopyAddress>(
    old: &[SearchResult],
    prepared: &PreparedSearch,
    reader: &T,
    duration: Duration,
    cancel: &Receiver<()>,
    progress: &AtomicUsize,
) -> Option<Vec<SearchResult>> {
    let mut stable = StableValues::capture(old, prepared, reader);
    // Start timing after every candidate has a baseline, including large scans.
    let start = Instant::now();
    while !stable.results.is_empty() {
        let remaining = duration.saturating_sub(start.elapsed());
        match cancel.recv_timeout(SAMPLE_INTERVAL.min(remaining)) {
            Ok(()) | Err(RecvTimeoutError::Disconnected) => return None,
            Err(RecvTimeoutError::Timeout) => {}
        }
        stable.sample(prepared, reader);
        progress.store(start.elapsed().min(duration).as_millis() as usize, Ordering::Release);
        if start.elapsed() >= duration {
            break;
        }
    }
    Some(stable.results)
}

impl GameCheetahEngine {
    pub(super) fn spawn_stability_filter(&mut self, index: usize, old: Arc<Vec<SearchResult>>, prepared: PreparedSearch, duration: Duration) {
        let search = &mut self.searches[index];
        let worker = search.search_worker(SearchMode::Stability, self.pid, self.attached_start_time, self.process_name.clone());
        let prepared = prepared.with_worker(worker.clone());
        search.searching = SearchMode::Stability;
        search.total_bytes = duration.as_millis() as usize;
        let progress = search.current_bytes.clone();
        let sender = search.results_sender.clone();
        let pid = self.pid;
        std::thread::spawn(move || {
            let results = match pid.try_into_process_handle() {
                Ok(handle) => observe(
                    &old,
                    &prepared,
                    &worker.reader(ExactProcessReader(&handle)),
                    duration,
                    &worker.cancel,
                    &progress,
                ),
                Err(err) => {
                    worker.fail(crate::AppError::Generic {
                        message: format!("Failed to open process {pid}: {err}"),
                    });
                    Some(Vec::new())
                }
            };
            if let Some(results) = results {
                worker.send(&sender, results);
                progress.store(duration.as_millis() as usize, Ordering::Release);
                worker.finish();
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use std::{
        cell::{Cell, RefCell},
        io,
    };

    use crate::{NumericComparison, NumericFilter, SearchType};

    use super::*;

    const BASE: usize = 0x1000;

    struct Memory {
        bytes: RefCell<[u8; 64]>,
        unreadable: Cell<Option<usize>>,
        reads: Cell<usize>,
    }

    impl Memory {
        fn new() -> Self {
            Self {
                bytes: RefCell::new([0; 64]),
                unreadable: Cell::new(None),
                reads: Cell::new(0),
            }
        }

        fn put(&self, offset: usize, value: i64) {
            self.bytes.borrow_mut()[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        }
    }

    impl CopyAddress for Memory {
        fn copy_address(&self, address: usize, bytes: &mut [u8]) -> io::Result<()> {
            self.reads.set(self.reads.get() + 1);
            if self.unreadable.get().is_some_and(|bad| address <= bad && bad < address + bytes.len()) {
                bytes.fill(0); // Failed/partial reads must never qualify as stable.
                return Err(io::Error::other("unreadable"));
            }
            let offset = address - BASE;
            bytes.copy_from_slice(&self.bytes.borrow()[offset..offset + bytes.len()]);
            Ok(())
        }

        fn get_pointer_width(&self) -> process_memory::Architecture {
            process_memory::Architecture::Arch64Bit
        }
    }

    #[test]
    fn sampled_changes_are_permanent_and_compare_the_entire_type_width() {
        let memory = Memory::new();
        memory.put(0, 42);
        memory.put(16, 100);
        let old = [
            SearchResult::new(BASE, SearchType::Int),
            SearchResult::new(BASE, SearchType::Int64),
            SearchResult::new(BASE + 16, SearchType::Int64),
        ];
        let prepared = PreparedSearch::filtered(None, &SearchType::NUMERIC_TYPES);
        let mut stable = StableValues::capture(&old, &prepared, &memory);
        assert_eq!(stable.results.len(), 3);
        assert_eq!(memory.reads.get(), 1); // Nearby interpretations share a read.
        memory.put(0, (1_i64 << 32) + 42); // Only the high Int64 bytes change.
        stable.sample(&prepared, &memory);
        assert_eq!(stable.results.len(), 2);
        assert_eq!(stable.results[0].search_type, SearchType::Int);
        memory.put(0, 42);
        memory.put(16, 101);
        stable.sample(&prepared, &memory);
        memory.put(16, 100);
        stable.sample(&prepared, &memory);
        assert_eq!(stable.results.len(), 1);
        assert_eq!(stable.results[0].addr, BASE);
        assert_eq!(stable.results[0].search_type, SearchType::Int);
    }

    #[test]
    fn failed_reads_are_removed_but_readable_neighbors_survive() {
        let memory = Memory::new();
        let old = [SearchResult::new(BASE, SearchType::Int64), SearchResult::new(BASE + 16, SearchType::Int64)];
        let prepared = PreparedSearch::filtered(None, &SearchType::NUMERIC_TYPES);
        let mut stable = StableValues::capture(&old, &prepared, &memory);
        memory.unreadable.set(Some(BASE + 16));
        stable.sample(&prepared, &memory);
        assert_eq!(stable.results.len(), 1);
        assert_eq!(stable.results[0].addr, BASE);
        memory.unreadable.set(None);
        stable.sample(&prepared, &memory);
        assert_eq!(stable.results.len(), 1);
        memory.unreadable.set(Some(BASE));
        assert!(StableValues::capture(&old[..1], &prepared, &memory).results.is_empty());
    }

    #[test]
    fn stability_combines_value_and_type_filters_without_reinterpretation() {
        let memory = Memory::new();
        memory.put(0, -1);
        memory.put(16, 42);
        let old = [
            SearchResult::new(BASE, SearchType::Int64),
            SearchResult::new(BASE + 16, SearchType::Int),
            SearchResult::new(BASE + 16, SearchType::Int64),
        ];
        let filter = NumericFilter::parse(NumericComparison::GreaterEqual, "0", "").unwrap();
        let prepared = PreparedSearch::filtered(Some(filter), &[SearchType::Int64]);
        let mut stable = StableValues::capture(&old, &prepared, &memory);
        stable.sample(&prepared, &memory);
        assert_eq!(stable.results.len(), 1);
        assert_eq!(stable.results[0].addr, BASE + 16);
        assert_eq!(stable.results[0].search_type, SearchType::Int64);
        assert_eq!(i64::from_le_bytes(memory.bytes.borrow()[..8].try_into().unwrap()), -1);
    }

    #[test]
    fn dropping_cancel_sender_stops_observation_without_waiting_for_duration() {
        let memory = Memory::new();
        let old = [SearchResult::new(BASE, SearchType::Int64)];
        let prepared = PreparedSearch::filtered(None, &SearchType::NUMERIC_TYPES);
        let (sender, receiver) = crossbeam_channel::bounded(1);
        drop(sender);
        assert!(observe(&old, &prepared, &memory, Duration::from_secs(30), &receiver, &AtomicUsize::new(0)).is_none());
        assert_eq!(memory.reads.get(), 1);
    }
}
