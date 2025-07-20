use std::{
    collections::HashSet,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    },
};

use crate::{FreezeMessage, GameCheetahEngine, SearchResult, SearchType};
use crossbeam_channel::{Receiver, Sender, unbounded};
use std::sync::RwLock;

#[derive(PartialEq, Clone, Copy)]
pub enum SearchMode {
    None,
    Percent,
    Memory,
}

pub struct SearchContext {
    pub description: String,
    pub rename_mode: bool,

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

    cached_results: Arc<RwLock<Option<Vec<SearchResult>>>>,
    pub cache_valid: Arc<AtomicBool>,
}

impl SearchContext {
    pub fn new(description: String) -> Self {
        let (tx, rx) = unbounded();
        Self {
            description,
            rename_mode: false,
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

    pub fn clear_results(&mut self, freeze_sender: &mpsc::Sender<FreezeMessage>) {
        GameCheetahEngine::remove_freezes_from(freeze_sender, &mut self.freezed_addresses);
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

        // Collect from channel
        let mut all_results = Vec::new();
        while let Ok(results) = self.results_receiver.try_recv() {
            all_results.extend(results);
        }

        // Update cache but DON'T put results back in channel
        if let Ok(mut cache) = self.cached_results.write() {
            *cache = Some(all_results.clone());
            self.cache_valid.store(true, Ordering::Release);
        }

        all_results
    }

    pub fn invalidate_cache(&self) {
        self.cache_valid.store(false, Ordering::Release);
    }
}
