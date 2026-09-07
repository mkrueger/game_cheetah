use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

use crate::{SearchResult, SearchType, UnknownComparison};
use crossbeam_channel::{Receiver, Sender, bounded};

const RESULTS_CHANNEL_CAPACITY: usize = 128;

/// Type alias for memory snapshot storage to reduce type complexity
pub type MemorySnapshot = Arc<RwLock<Vec<(usize, Arc<[u8]>)>>>;

/// Per-(address, type) previous-value table used by unknown-search filtering.
///
/// Each entry stores up to 8 bytes that mirror the previous read of memory at
/// `addr` interpreted as `search_type`. Holding this map in the
/// [`SearchContext`] keeps the per-hit [`SearchResult`] footprint to two
/// `usize`-sized fields.
pub type UnknownPreviousValues = Arc<RwLock<HashMap<(usize, SearchType), [u8; 8]>>>;

/// Everything needed to retry a search from the same comparison baseline.
/// Snapshot pages and result lists share their immutable storage with the
/// running pass; the mutable previous-value map needs its own copy.
pub struct SearchHistoryEntry {
    results: Arc<Vec<SearchResult>>,
    previous_unknown_values: HashMap<(usize, SearchType), [u8; 8]>,
    memory_snapshot: Vec<(usize, Arc<[u8]>)>,
    unknown_comparison: Option<UnknownComparison>,
    search_complete: bool,
    total_bytes: usize,
    current_bytes: usize,
}

#[derive(PartialEq, Clone, Copy, Debug)]
pub enum SearchMode {
    None,
    Percent,
    Memory,
    Stability,
}

pub struct SearchContext {
    pub description: String,

    pub search_value_text: String,
    pub search_type: SearchType,

    /// Per-tab input state; predicates are applied explicitly, never live.
    pub show_numeric_filter: bool,
    pub numeric_filter_enabled: bool,
    pub numeric_comparison: crate::NumericComparison,
    pub numeric_filter_lower: String,
    pub numeric_filter_upper: String,
    pub type_filter_enabled: bool,
    pub filter_types: [bool; 6],
    pub stable_filter_enabled: bool,
    pub stable_filter_seconds: u32,
    /// Dropping a tab also drops the only cancellation sender for its workers.
    pub(crate) task: Option<crate::search_task::SearchTask>,
    pub(crate) pending_search_error: Option<crate::AppError>,

    pub searching: SearchMode,
    pub total_bytes: usize,
    pub current_bytes: Arc<AtomicUsize>,
    pub results_sender: Sender<Vec<SearchResult>>,
    pub results_receiver: Receiver<Vec<SearchResult>>,
    pub freezed_addresses: HashSet<usize>,

    pub old_results: Vec<SearchHistoryEntry>,
    pub search_complete: Arc<AtomicBool>,

    // Changed to Arc<Vec> for cheap cloning
    pub cached_results: Arc<RwLock<Option<Arc<Vec<SearchResult>>>>>,
    pub cache_valid: Arc<AtomicBool>,

    // For unknown searches - store memory snapshots
    pub memory_snapshot: MemorySnapshot,
    /// Previous-value table for unknown-search subsequent passes.
    pub previous_unknown_values: UnknownPreviousValues,
    pub unknown_comparison: Option<UnknownComparison>,
}

impl SearchContext {
    pub fn new(description: String) -> Self {
        let (tx, rx) = Self::result_channel();
        Self {
            description,
            search_value_text: "".to_owned(),
            show_numeric_filter: false,
            numeric_filter_enabled: true,
            numeric_comparison: crate::NumericComparison::default(),
            numeric_filter_lower: "0".to_owned(),
            numeric_filter_upper: String::new(),
            type_filter_enabled: false,
            filter_types: [true; 6],
            stable_filter_enabled: false,
            stable_filter_seconds: 3,
            task: None,
            pending_search_error: None,
            searching: SearchMode::None,
            results_sender: tx,
            results_receiver: rx,
            total_bytes: 0,
            current_bytes: Arc::new(AtomicUsize::new(0)),
            freezed_addresses: HashSet::new(),
            search_type: SearchType::Guess,
            old_results: Vec::new(),
            search_complete: Arc::new(AtomicBool::new(false)),

            cached_results: Arc::new(RwLock::new(None)),
            cache_valid: Arc::new(AtomicBool::new(false)),

            memory_snapshot: Arc::new(RwLock::new(Vec::new())),
            previous_unknown_values: Arc::new(RwLock::new(HashMap::new())),
            unknown_comparison: None,
        }
    }

    pub fn result_channel() -> (Sender<Vec<SearchResult>>, Receiver<Vec<SearchResult>>) {
        bounded(RESULTS_CHANNEL_CAPACITY)
    }

    pub fn result_filter(&self) -> Result<crate::ResultFilter, String> {
        if !self.numeric_filter_enabled && !self.type_filter_enabled && !self.stable_filter_enabled {
            return Err(i18n_embed_fl::fl!(crate::LANGUAGE_LOADER, "result-filter-no-criteria"));
        }
        let numeric = self
            .numeric_filter_enabled
            .then(|| crate::NumericFilter::parse(self.numeric_comparison, &self.numeric_filter_lower, &self.numeric_filter_upper))
            .transpose()?;
        let types = SearchType::NUMERIC_TYPES
            .into_iter()
            .zip(self.filter_types)
            .filter_map(|(ty, selected)| (!self.type_filter_enabled || selected).then_some(ty))
            .collect();
        let filter = crate::ResultFilter {
            numeric,
            types,
            stable_for: self
                .stable_filter_enabled
                .then(|| std::time::Duration::from_secs(u64::from(self.stable_filter_seconds))),
        };
        filter.validate()?;
        Ok(filter)
    }

    /// Snapshot before mutation, then allocate private state for this generation.
    pub(crate) fn begin_search(&mut self, mode: SearchMode, pid: process_memory::Pid, start_time: u64, name: String) -> crate::search_task::SearchWorker {
        self.task.take();
        self.push_undo_state(self.collect_results());
        self.search_complete = Arc::new(AtomicBool::new(false));
        self.current_bytes = Arc::new(AtomicUsize::new(0));
        self.cache_valid = Arc::new(AtomicBool::new(false));
        self.cached_results = Arc::new(RwLock::new(None));
        // Workers must never mutate a previous generation's snapshot/map.
        let snapshot = self.memory_snapshot.read().map(|pages| pages.clone()).unwrap_or_default();
        self.memory_snapshot = Arc::new(RwLock::new(snapshot));
        let previous = self.previous_unknown_values.read().map(|map| map.clone()).unwrap_or_default();
        self.previous_unknown_values = Arc::new(RwLock::new(previous));
        let (tx, rx) = Self::result_channel();
        self.results_sender = tx;
        self.results_receiver = rx;
        self.total_bytes = 0;
        self.searching = mode;
        self.pending_search_error = None;
        let task = crate::search_task::SearchTask::new(pid, start_time, name, self.search_complete.clone());
        let worker = task.worker.clone();
        self.task = Some(task);
        worker
    }

    pub(crate) fn search_worker(&mut self, mode: SearchMode, pid: process_memory::Pid, start_time: u64, name: String) -> crate::search_task::SearchWorker {
        if let Some(task) = &self.task {
            task.worker.clone()
        } else {
            self.begin_search(mode, pid, start_time, name)
        }
    }

    pub fn cancel_search(&mut self) {
        if self.task.is_some() {
            self.undo_last_search();
        }
    }

    pub fn get_result_count(&self) -> usize {
        self.collect_results().len()
    }

    pub fn store_memory_snapshot(&self, address: usize, data: Vec<u8>) {
        if let Ok(mut snapshot) = self.memory_snapshot.write() {
            snapshot.push((address, Arc::<[u8]>::from(data.into_boxed_slice())));
        }
    }

    pub fn clear_memory_snapshot(&self) {
        if let Ok(mut snapshot) = self.memory_snapshot.write() {
            snapshot.clear();
            snapshot.shrink_to_fit();
        }
    }

    /// Discard any previous-value bookkeeping accumulated by unknown searches.
    pub fn clear_previous_unknown_values(&self) {
        if let Ok(mut map) = self.previous_unknown_values.write() {
            map.clear();
            map.shrink_to_fit();
        }
    }

    /// Reset a search after the engine has released this tab's freezes.
    pub(crate) fn clear_results(&mut self) {
        self.task.take();
        self.search_complete = Arc::new(AtomicBool::new(false));
        self.current_bytes = Arc::new(AtomicUsize::new(0));
        self.memory_snapshot = Arc::new(RwLock::new(Vec::new()));
        self.previous_unknown_values = Arc::new(RwLock::new(HashMap::new()));
        self.cached_results = Arc::new(RwLock::new(None));
        self.cache_valid = Arc::new(AtomicBool::new(false));
        self.pending_search_error = None;
        debug_assert!(self.freezed_addresses.is_empty());
        // Clear old results history
        self.old_results.clear();
        self.clear_previous_unknown_values();
        self.clear_memory_snapshot();
        self.unknown_comparison = None;

        // Create new channel to clear all pending results
        let (tx, rx) = Self::result_channel();
        self.results_sender = tx;
        self.results_receiver = rx;

        // Reset counters
        self.current_bytes.store(0, Ordering::SeqCst);
        self.search_complete.store(false, Ordering::SeqCst);

        // Reset search mode
        self.searching = SearchMode::None;

        // Invalidate any cached results
        self.invalidate_cache();
    }

    pub fn push_undo_state(&mut self, results: Arc<Vec<SearchResult>>) {
        self.old_results.push(SearchHistoryEntry {
            results,
            previous_unknown_values: self.previous_unknown_values.read().map(|map| map.clone()).unwrap_or_default(),
            memory_snapshot: self.memory_snapshot.read().map(|pages| pages.clone()).unwrap_or_default(),
            unknown_comparison: self.unknown_comparison,
            search_complete: self.search_complete.load(Ordering::Acquire),
            total_bytes: self.total_bytes,
            current_bytes: self.current_bytes.load(Ordering::Acquire),
        });
    }

    pub fn undo_last_search(&mut self) {
        let Some(old) = self.old_results.pop() else {
            return;
        };
        self.task.take();
        // Discard queued batches from the pass being undone as well as its
        // cached results so they cannot reappear on the next UI tick.
        let (tx, rx) = Self::result_channel();
        self.results_sender = tx;
        self.results_receiver = rx;
        self.cached_results = Arc::new(RwLock::new(Some(old.results)));
        self.cache_valid = Arc::new(AtomicBool::new(true));
        self.previous_unknown_values = Arc::new(RwLock::new(old.previous_unknown_values));
        self.memory_snapshot = Arc::new(RwLock::new(old.memory_snapshot));
        self.unknown_comparison = old.unknown_comparison;
        self.search_complete = Arc::new(AtomicBool::new(old.search_complete));
        self.current_bytes = Arc::new(AtomicUsize::new(old.current_bytes));
        self.total_bytes = old.total_bytes;
        self.searching = SearchMode::None;
    }

    pub fn set_cached_results(&self, mut results: Vec<SearchResult>) {
        // Keep displayed results sorted by address (then by type for Guess
        // hits that produce multiple typed entries at the same address) so
        // the visible list doesn't shuffle when parallel workers stream
        // batches in. Dedup exact duplicates that any double-emission in the
        // streaming path could have produced.
        results.sort_by_key(|r| (r.addr, r.search_type as u8));
        results.dedup_by_key(|r| (r.addr, r.search_type as u8));
        // Wrap in Arc for cheap future clones
        let arc_results = Arc::new(results);

        if let Ok(mut cache) = self.cached_results.write() {
            *cache = Some(arc_results);
            self.cache_valid.store(true, Ordering::Release);
        }
    }

    pub fn collect_results(&self) -> Arc<Vec<SearchResult>> {
        let mut new_results = Vec::new();

        // Always drain pending channel results before returning cached data. This keeps bounded
        // result channels from filling up while a search is still running.
        while let Ok(results) = self.results_receiver.try_recv() {
            new_results.extend(results);
        }

        // Completion can invalidate the flag without changing any results.
        // No incoming batch means the sorted cache can still be reused.
        if new_results.is_empty()
            && let Ok(cache) = self.cached_results.read()
            && let Some(ref results) = *cache
        {
            self.cache_valid.store(true, Ordering::Release);
            return Arc::clone(results); // Only clones the Arc, not the Vec!
        }

        new_results.sort_unstable_by_key(result_key);
        new_results.dedup_by_key(|r| result_key(r));

        let Ok(mut cache) = self.cached_results.write() else {
            return Arc::new(new_results);
        };
        let cached = cache.get_or_insert_with(|| Arc::new(Vec::new()));
        if !new_results.is_empty() {
            // Copy-on-write preserves snapshots held by the UI or Undo. When
            // nobody else owns the cache, reuse its allocation directly.
            let results = Arc::make_mut(cached);
            merge_results(results, &new_results);
        }
        self.cache_valid.store(true, Ordering::Release);
        Arc::clone(cached)
    }

    pub fn invalidate_cache(&self) {
        self.cache_valid.store(false, Ordering::Release);
        if let Ok(mut cache) = self.cached_results.write() {
            *cache = None;
        }
    }

    pub fn update_search_mode(&mut self) {
        if self.search_complete.load(Ordering::Acquire) && !matches!(self.searching, SearchMode::None) {
            if let Some(error) = self.task.as_ref().and_then(|task| task.worker.error()) {
                self.cancel_search();
                self.pending_search_error = Some(error);
                return;
            }
            self.collect_results();
            self.task.take();
            self.searching = SearchMode::None;
        }
    }
}

fn result_key(result: &SearchResult) -> (usize, u8) {
    (result.addr, result.search_type as u8)
}

/// Merge sorted, deduplicated batches backwards into the existing allocation.
/// Appending non-overlapping batches requires neither a full scan nor sorting.
fn merge_results(results: &mut Vec<SearchResult>, incoming: &[SearchResult]) {
    let old_len = results.len();
    let append_only = results.last().is_none_or(|last| result_key(last) < result_key(&incoming[0]));
    results.extend_from_slice(incoming);
    if append_only {
        return;
    }
    let (mut left, mut right) = (old_len, incoming.len());
    while right > 0 {
        let dest = left + right - 1;
        if left > 0 && result_key(&results[left - 1]) > result_key(&incoming[right - 1]) {
            left -= 1;
            results[dest] = results[left];
        } else {
            right -= 1;
            results[dest] = incoming[right];
        }
    }
    results.dedup_by_key(|r| result_key(r));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_restores_history_and_isolates_all_worker_owned_state() {
        let mut search = SearchContext::new("cancel".into());
        let original = SearchResult::new(0x1000, SearchType::Int);
        search.set_cached_results(vec![original]);
        search.search_complete.store(true, Ordering::Release);
        search.total_bytes = 17;
        search.current_bytes.store(17, Ordering::Release);
        search.unknown_comparison = Some(UnknownComparison::Changed);
        search.store_memory_snapshot(0x1000, vec![1; 8]);
        search.previous_unknown_values.write().unwrap().insert((0x1000, SearchType::Int), [2; 8]);
        search.push_undo_state(search.collect_results());
        let worker = search.begin_search(SearchMode::Memory, 0, 0, "test".into());
        let old_snapshot = search.memory_snapshot.clone();
        let old_previous = search.previous_unknown_values.clone();
        let old_complete = search.search_complete.clone();
        let old_progress = search.current_bytes.clone();
        let sender = search.results_sender.clone();
        sender.send(vec![SearchResult::new(0x2000, SearchType::Double)]).unwrap();
        search.cancel_search();
        assert!(worker.cancelled());
        assert_eq!(keys(&search.collect_results()), vec![(0x1000, SearchType::Int as u8)]);
        assert_eq!(search.old_results.len(), 1);
        assert_eq!(search.total_bytes, 17);
        assert_eq!(search.current_bytes.load(Ordering::Acquire), 17);
        assert_eq!(search.unknown_comparison, Some(UnknownComparison::Changed));
        // Simulate late writes from an in-flight OS read/comparison.
        old_snapshot.write().unwrap().clear();
        old_previous.write().unwrap().clear();
        old_complete.store(true, Ordering::Release);
        old_progress.store(999, Ordering::Release);
        assert_eq!(search.memory_snapshot.read().unwrap().len(), 1);
        assert_eq!(search.previous_unknown_values.read().unwrap().len(), 1);
        let next = search.begin_search(SearchMode::Percent, 0, 0, "next".into());
        worker.finish();
        assert!(!next.cancelled());
        assert!(!search.search_complete.load(Ordering::Acquire));
        assert!(sender.send(vec![original]).is_err());
        search.cancel_search();
    }

    #[test]
    fn closing_a_tab_broadcasts_cancellation_to_all_workers() {
        let mut search = SearchContext::new("drop".into());
        let worker = search.begin_search(SearchMode::Memory, 0, 0, "test".into());
        let other_worker = worker.clone();
        drop(search);
        assert!(worker.cancelled());
        assert!(other_worker.cancelled());
    }

    fn keys(results: &[SearchResult]) -> Vec<(usize, u8)> {
        results.iter().map(result_key).collect()
    }

    #[test]
    fn streamed_merge_matches_full_sort_and_dedup() {
        let search = SearchContext::new("merge".into());
        let mut expected = Vec::new();
        let mut seed = 0x1234_5678u64;
        for _ in 0..80 {
            let mut batch = Vec::new();
            for _ in 0..250 {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                let ty = [SearchType::Int, SearchType::Float, SearchType::Double][seed as usize % 3];
                batch.push(SearchResult::new((seed as usize % 1024) * 8, ty));
            }
            expected.extend_from_slice(&batch);
            expected.sort_by_key(result_key);
            expected.dedup_by_key(|r| result_key(r));
            search.results_sender.send(batch).unwrap();
            assert_eq!(keys(&search.collect_results()), keys(&expected));
        }
    }

    #[test]
    fn merge_preserves_shared_snapshots_and_types() {
        let mut search = SearchContext::new("snapshots".into());
        search.set_cached_results(vec![SearchResult::new(20, SearchType::Int)]);
        let snapshot = search.collect_results();
        search.push_undo_state(snapshot.clone());
        search
            .results_sender
            .send(vec![
                SearchResult::new(20, SearchType::Float),
                SearchResult::new(10, SearchType::Int),
                SearchResult::new(20, SearchType::Int),
                SearchResult::new(30, SearchType::Int),
            ])
            .unwrap();
        assert_eq!(keys(&search.collect_results()), vec![(10, 3), (20, 3), (20, 5), (30, 3)]);
        assert_eq!(keys(&snapshot), vec![(20, 3)]);
        search.undo_last_search();
        assert_eq!(keys(&search.collect_results()), keys(&snapshot));
    }

    #[test]
    fn no_new_results_reuses_cache_even_after_completion() {
        let search = SearchContext::new("cache".into());
        search.set_cached_results(vec![SearchResult::new(20, SearchType::Int)]);
        let snapshot = search.collect_results();
        search.cache_valid.store(false, Ordering::Release);
        search.results_sender.send(Vec::new()).unwrap();
        assert!(Arc::ptr_eq(&snapshot, &search.collect_results()));
        search.invalidate_cache();
        assert!(search.collect_results().is_empty());
    }

    #[test]
    fn exclusive_cache_reuses_vector_allocation() {
        let search = SearchContext::new("reuse".into());
        let mut initial = Vec::with_capacity(64);
        initial.push(SearchResult::new(20, SearchType::Int));
        let pointer = initial.as_ptr();
        search.set_cached_results(initial);
        for address in [30, 10, 20, 40] {
            search.results_sender.send(vec![SearchResult::new(address, SearchType::Int)]).unwrap();
            let result = search.collect_results();
            assert_eq!(result.as_ptr(), pointer);
        }
        assert_eq!(keys(&search.collect_results()), vec![(10, 3), (20, 3), (30, 3), (40, 3)]);
    }

    #[test]
    fn undo_restores_complete_state_and_discards_pending_results() {
        let mut search = SearchContext::new("undo".to_owned());
        let result = SearchResult::new(0x1234, SearchType::Int);
        search.set_cached_results(vec![result]);
        search
            .previous_unknown_values
            .write()
            .unwrap()
            .insert((result.addr, result.search_type), [1; 8]);
        search.store_memory_snapshot(result.addr, vec![2; 8]);
        search.unknown_comparison = Some(UnknownComparison::Increased);
        search.search_complete.store(true, Ordering::Release);
        search.push_undo_state(search.collect_results());

        search.set_cached_results(Vec::new());
        search.clear_previous_unknown_values();
        search.clear_memory_snapshot();
        search.unknown_comparison = Some(UnknownComparison::Changed);
        search.results_sender.send(vec![SearchResult::new(0xFFFF, SearchType::Int)]).unwrap();
        search.undo_last_search();

        assert_eq!(search.collect_results().len(), 1);
        assert_eq!(search.collect_results()[0].addr, result.addr);
        assert_eq!(search.previous_unknown_values.read().unwrap()[&(result.addr, result.search_type)], [1; 8]);
        assert_eq!(&*search.memory_snapshot.read().unwrap()[0].1, &[2; 8]);
        assert_eq!(search.unknown_comparison, Some(UnknownComparison::Increased));
        assert!(search.search_complete.load(Ordering::Acquire));
        assert!(search.old_results.is_empty());

        search.clear_results();
        assert_eq!(search.get_result_count(), 0);
        assert!(search.memory_snapshot.read().unwrap().is_empty());
        assert!(search.previous_unknown_values.read().unwrap().is_empty());
        assert_eq!(search.unknown_comparison, None);
    }
}
