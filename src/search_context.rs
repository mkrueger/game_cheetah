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
}

#[derive(PartialEq, Clone, Copy, Debug)]
pub enum SearchMode {
    None,
    Percent,
    Memory,
}

pub struct SearchContext {
    pub description: String,

    pub search_value_text: String,
    pub search_type: SearchType,

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
        });
    }

    pub fn undo_last_search(&mut self) {
        let Some(old) = self.old_results.pop() else {
            return;
        };
        // Discard queued batches from the pass being undone as well as its
        // cached results so they cannot reappear on the next UI tick.
        let (tx, rx) = Self::result_channel();
        self.results_sender = tx;
        self.results_receiver = rx;
        self.set_cached_results((*old.results).clone());
        if let Ok(mut previous) = self.previous_unknown_values.write() {
            *previous = old.previous_unknown_values;
        }
        if let Ok(mut snapshot) = self.memory_snapshot.write() {
            *snapshot = old.memory_snapshot;
        }
        self.unknown_comparison = old.unknown_comparison;
        self.search_complete.store(old.search_complete, Ordering::Release);
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

        // Check if cache is valid - return Arc clone (cheap!) when there is no new data.
        if new_results.is_empty()
            && self.cache_valid.load(Ordering::Acquire)
            && let Ok(cache) = self.cached_results.read()
            && let Some(ref results) = *cache
        {
            return Arc::clone(results); // Only clones the Arc, not the Vec!
        }

        let mut all_results = Vec::new();

        // Get existing cached results in a separate scope to ensure lock is dropped
        {
            if let Ok(cache) = self.cached_results.read()
                && let Some(ref cached) = *cache
            {
                all_results.extend_from_slice(cached);
            }
        } // Read lock definitely dropped here

        all_results.extend(new_results);

        // Keep results sorted by address so the displayed list is stable as
        // parallel workers stream more hits in. Without this, the order
        // depends on worker completion order and rows visibly shuffle when
        // a refresh tick or filter pass merges fresh batches in. Sort by
        // (addr, search_type as u8) so Guess hits that produce multiple
        // typed entries at the same address keep a deterministic order.
        all_results.sort_by_key(|r| (r.addr, r.search_type as u8));
        all_results.dedup_by_key(|r| (r.addr, r.search_type as u8));

        // Wrap in Arc for cheap future clones
        let arc_results = Arc::new(all_results);

        // Update cache with the Arc
        if let Ok(mut cache) = self.cached_results.write() {
            *cache = Some(Arc::clone(&arc_results)); // Store an Arc clone
            self.cache_valid.store(true, Ordering::Release);
        }

        arc_results
    }

    pub fn invalidate_cache(&self) {
        self.cache_valid.store(false, Ordering::Release);
        if let Ok(mut cache) = self.cached_results.write() {
            *cache = None;
        }
    }

    pub fn update_search_mode(&mut self) {
        if self.search_complete.load(Ordering::Acquire) && !matches!(self.searching, SearchMode::None) {
            self.searching = SearchMode::None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
