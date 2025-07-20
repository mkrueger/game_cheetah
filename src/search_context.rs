use std::{
    collections::HashSet,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize},
        mpsc,
    },
};

use crate::{FreezeMessage, GameCheetahEngine, SearchResult, SearchType};
use crossbeam_channel::{Receiver, Sender, unbounded};

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
    pub search_results: i64,
    pub search_complete: Arc<AtomicBool>,
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
            search_results: -1,
            search_type: SearchType::Guess,
            old_results: Vec::new(),
            search_complete: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn clear_results(&mut self, freeze_sender: &mpsc::Sender<FreezeMessage>) {
        GameCheetahEngine::remove_freezes_from(freeze_sender, &mut self.freezed_addresses);
        self.search_results = -1;
    }

    pub fn collect_results(&self) -> Vec<SearchResult> {
        let mut all_results = Vec::new();
        while let Ok(results) = self.results_receiver.try_recv() {
            all_results.extend(results);
        }
        all_results
    }
}
