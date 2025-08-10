use std::{
    collections::HashSet,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

use crate::{FreezeMessage, GameCheetahEngine, SearchResult, SearchType};
use crossbeam_channel::{Receiver, Sender, unbounded};
use std::sync::RwLock;

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
    pub result_count: Arc<AtomicUsize>,
    pub freezed_addresses: HashSet<usize>,

    pub old_results: Vec<Vec<SearchResult>>,
    pub search_complete: Arc<AtomicBool>,

    pub cached_results: Arc<RwLock<Option<Vec<SearchResult>>>>,
    pub cache_valid: Arc<AtomicBool>,
}

impl SearchContext {
    pub fn new(description: String) -> Self {
        let (tx, rx) = unbounded();
        Self {
            description,
            search_value_text: "".to_owned(),
            searching: SearchMode::None,
            results_sender: tx,
            results_receiver: rx,
            result_count: Arc::new(AtomicUsize::new(0)),
            total_bytes: 0,
            current_bytes: Arc::new(AtomicUsize::new(0)),
            freezed_addresses: HashSet::new(),
            search_type: SearchType::Guess,
            old_results: Vec::new(),
            search_complete: Arc::new(AtomicBool::new(false)),

            cached_results: Arc::new(RwLock::new(None)),
            cache_valid: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn get_result_count(&self) -> usize {
        self.result_count.load(Ordering::Relaxed)
    }

    pub fn clear_results(&mut self, freeze_sender: &crossbeam_channel::Sender<FreezeMessage>) {
        GameCheetahEngine::remove_freezes_from(freeze_sender, &mut self.freezed_addresses);
        // Clear old results history
        self.old_results.clear();

        // Create new channel to clear all pending results
        let (tx, rx) = unbounded();
        self.results_sender = tx;
        self.results_receiver = rx;

        // Reset counters
        self.result_count.store(0, Ordering::SeqCst);
        self.current_bytes.store(0, Ordering::SeqCst);
        self.search_complete.store(false, Ordering::SeqCst);

        // Reset search mode
        self.searching = SearchMode::None;

        // Invalidate any cached results
        self.invalidate_cache();
    }

    pub fn set_cached_results(&self, results: Vec<SearchResult>) {
        // Update the cache with new results
        if let Ok(mut cache) = self.cached_results.write() {
            *cache = Some(results.clone());
            self.cache_valid.store(true, Ordering::Release);
            self.result_count.store(results.len(), Ordering::SeqCst);
        }
    }

    pub fn collect_results(&self) -> Vec<SearchResult> {
        // Check if cache is valid
        if self.cache_valid.load(Ordering::Acquire) {
            if let Ok(cache) = self.cached_results.read() {
                if let Some(ref results) = *cache {
                    return results.clone();
                }
            }
        }

        let mut all_results = Vec::new();

        // Get existing cached results in a separate scope to ensure lock is dropped
        {
            if let Ok(cache) = self.cached_results.read() {
                if let Some(ref cached) = *cache {
                    all_results.extend_from_slice(cached);
                }
            }
        } // Read lock definitely dropped here

        // Add any new results from the channel
        while let Ok(results) = self.results_receiver.try_recv() {
            all_results.extend(results);
        }

        // Update the result count
        self.result_count.store(all_results.len(), Ordering::SeqCst);

        // Update cache with the merged results
        if let Ok(mut cache) = self.cached_results.write() {
            *cache = Some(all_results.clone());
            self.cache_valid.store(true, Ordering::Release);
        }

        all_results
    }

    pub fn invalidate_cache(&self) {
        self.cache_valid.store(false, Ordering::Release);
        if let Ok(mut cache) = self.cached_results.write() {
            *cache = None;
        }
    }
}
