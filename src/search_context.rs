use std::sync::{Arc, atomic::AtomicUsize, Mutex, mpsc};

use crate::{SearchType, GameCheetahEngine, SearchResult, Message};

pub struct SearchContext {
    pub description: String,

    pub search_value_text: String,
    pub search_type: SearchType,

    pub searching: bool,
    pub total_bytes: usize,
    pub current_bytes: Arc<AtomicUsize>,
    pub results: Arc<Mutex<Vec<SearchResult>>>,

    pub old_results: Vec<Vec<SearchResult>>,
    pub search_results: i64,
}

impl SearchContext {
    pub fn new(description: String) -> Self {
        Self {
            description,
            search_value_text: "".to_owned(),
            searching: false,
            results: Arc::new(Mutex::new(Vec::new())),
            total_bytes: 0,
            current_bytes: Arc::new(AtomicUsize::new(0)),
            search_results: -1,
            search_type: SearchType::Guess,
            old_results: Vec::new()
        }
    }

    pub fn clear_results(&mut self, freeze_sender: &mpsc::Sender<Message>) {
        match &mut self.results.lock() {
            Ok(r) => {
                GameCheetahEngine::remove_freezes_from(freeze_sender, &r.clone());
                r.clear();
            }
            Err(err) => {
                eprintln!("Error while clearing {}", err);
            }
        }
        self.search_results = -1;
    }
}
