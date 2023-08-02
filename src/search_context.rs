use std::{sync::{atomic::AtomicUsize, mpsc, Arc, Mutex}, collections::HashSet};

use crate::{GameCheetahEngine, Message, SearchResult, SearchType};

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
    pub results: Arc<Mutex<Vec<SearchResult>>>,
    pub freezed_addresses: HashSet<usize>,

    pub old_results: Vec<Vec<SearchResult>>,
    pub search_results: i64,
}

impl SearchContext {
    pub fn new(description: String) -> Self {
        Self {
            description,
            search_value_text: "".to_owned(),
            searching: SearchMode::None,
            results: Arc::new(Mutex::new(Vec::new())),
            total_bytes: 0,
            current_bytes: Arc::new(AtomicUsize::new(0)),
            freezed_addresses: HashSet::new(),
            search_results: -1,
            search_type: SearchType::Guess,
            old_results: Vec::new(),
        }
    }

    pub fn clear_results(&mut self, freeze_sender: &mpsc::Sender<Message>) {
        GameCheetahEngine::remove_freezes_from(freeze_sender, &mut self.freezed_addresses);
        self.results.lock().unwrap().clear();
        self.search_results = -1;
    }
}
