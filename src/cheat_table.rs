use std::path::{Path, PathBuf};

use i18n_embed_fl::fl;
use process_memory::{TryIntoProcessHandle, copy_address};
use serde::{Deserialize, Serialize};

use crate::{FreezeMessage, GameCheetahEngine, MessageCommand, SearchContext, SearchResult, SearchType, SearchValue};

#[derive(Serialize, Deserialize)]
pub struct CheatTable {
    pub version: u32,
    pub process_name: String,
    pub searches: Vec<SavedSearch>,
}

#[derive(Serialize, Deserialize)]
pub struct SavedSearch {
    pub description: String,
    pub entries: Vec<SavedEntry>,
}

#[derive(Serialize, Deserialize)]
pub struct SavedEntry {
    /// Hex address string, e.g. "0x7FFF12345678"
    pub address: String,
    pub search_type: SearchType,
    pub frozen: bool,
    #[serde(default)]
    pub frozen_value: Vec<u8>,
}

/// Returns `~/.game-cheetah/<process_name>.toml` (Linux/macOS) or
/// `%APPDATA%\game-cheetah\<process_name>.toml` (Windows).
pub fn default_cheat_table_path(process_name: &str) -> PathBuf {
    let safe_name: String = process_name
        .chars()
        .map(|c| if c.is_alphanumeric() || matches!(c, '-' | '_' | '.') { c } else { '_' })
        .collect();
    let safe_name = if safe_name.is_empty() { "unnamed".to_owned() } else { safe_name };

    let base = {
        #[cfg(windows)]
        {
            std::env::var("APPDATA").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("."))
        }
        #[cfg(not(windows))]
        {
            std::env::var("HOME").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("."))
        }
    };

    base.join(".game-cheetah").join(format!("{safe_name}.toml"))
}

pub fn save_cheat_table(engine: &GameCheetahEngine, path: &Path) -> Result<(), String> {
    let handle = (engine.pid as process_memory::Pid)
        .try_into_process_handle()
        .map_err(|e| format!("Cannot open process: {e}"))?;

    let mut saved_searches = Vec::new();

    for ctx in &engine.searches {
        let results = ctx.collect_results();
        let mut entries = Vec::with_capacity(results.len());

        for result in results.iter() {
            let frozen = ctx.freezed_addresses.contains(&result.addr);
            let frozen_value = if frozen {
                result
                    .search_type
                    .fixed_byte_length()
                    .and_then(|len| copy_address(result.addr, len, &handle).ok())
                    .unwrap_or_default()
            } else {
                Vec::new()
            };

            entries.push(SavedEntry {
                address: format!("0x{:X}", result.addr),
                search_type: result.search_type,
                frozen,
                frozen_value,
            });
        }

        saved_searches.push(SavedSearch {
            description: ctx.description.clone(),
            entries,
        });
    }

    let table = CheatTable {
        version: 1,
        process_name: engine.process_name.clone(),
        searches: saved_searches,
    };

    let toml_str = toml::to_string_pretty(&table).map_err(|e| format!("Serialization error: {e}"))?;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("Cannot create directory: {e}"))?;
    }

    std::fs::write(path, toml_str).map_err(|e| format!("Cannot write {}: {e}", path.display()))?;

    Ok(())
}

pub fn load_cheat_table(path: &Path, freeze_sender: &crossbeam_channel::Sender<FreezeMessage>) -> Result<Vec<Box<SearchContext>>, String> {
    let toml_str = std::fs::read_to_string(path).map_err(|e| format!("Cannot read {}: {e}", path.display()))?;

    let table: CheatTable = toml::from_str(&toml_str).map_err(|e| format!("Parse error: {e}"))?;

    let mut searches: Vec<Box<SearchContext>> = Vec::new();

    for saved in table.searches {
        let mut ctx = Box::new(SearchContext::new(saved.description));
        let mut results = Vec::new();

        for entry in &saved.entries {
            let addr = parse_hex_address(&entry.address)?;
            results.push(SearchResult::new(addr, entry.search_type));

            if entry.frozen && !entry.frozen_value.is_empty() {
                ctx.freezed_addresses.insert(addr);
                let _ = freeze_sender.send(FreezeMessage {
                    msg: MessageCommand::Freeze,
                    addr,
                    value: SearchValue(entry.search_type, entry.frozen_value.clone()),
                });
            }
        }

        ctx.set_cached_results(results);
        searches.push(ctx);
    }

    if searches.is_empty() {
        searches.push(Box::new(SearchContext::new(fl!(crate::LANGUAGE_LOADER, "first-search-label"))));
    }

    Ok(searches)
}

fn parse_hex_address(s: &str) -> Result<usize, String> {
    let hex = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")).unwrap_or(s);
    usize::from_str_radix(hex, 16).map_err(|e| format!("Invalid address '{s}': {e}"))
}
