use std::path::{Path, PathBuf};

use i18n_embed_fl::fl;
use serde::{Deserialize, Serialize};

use crate::{GameCheetahEngine, SearchContext, SearchResult, SearchType};

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
    // NOTE: We deliberately do NOT persist freeze state. Freeze is ephemeral
    // in-memory behavior; saving it across runs is unsafe (ASLR moves the
    // target address) and silently restoring it would write to unrelated
    // memory. Older files with `frozen` / `frozen_value` fields still parse
    // cleanly because serde ignores unknown TOML fields by default.
}

/// Returns the directory Game Cheetah uses for persisted user files.
///
/// The directory is derived without an extra platform-directory dependency:
/// Windows uses `APPDATA`, other platforms use `HOME`, and if the relevant
/// environment variable is unavailable the current working directory is used.
/// `.game-cheetah` is appended in all cases.
pub fn config_dir() -> PathBuf {
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

    base.join(".game-cheetah")
}

/// Returns the default TOML cheat-table path inside [`config_dir`].
pub fn default_cheat_table_path(process_name: &str) -> PathBuf {
    let safe_name: String = process_name
        .chars()
        .map(|c| if c.is_alphanumeric() || matches!(c, '-' | '_' | '.') { c } else { '_' })
        .collect();
    let safe_name = if safe_name.is_empty() { "unnamed".to_owned() } else { safe_name };

    config_dir().join(format!("{safe_name}.toml"))
}

pub fn save_cheat_table(engine: &GameCheetahEngine, path: &Path) -> Result<(), String> {
    let mut saved_searches = Vec::new();

    for ctx in &engine.searches {
        let results = ctx.collect_results();
        let mut entries = Vec::with_capacity(results.len());

        for result in results.iter() {
            entries.push(SavedEntry {
                address: format!("0x{:X}", result.addr),
                search_type: result.search_type,
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

/// Load a cheat table from disk.
///
/// Safety notes:
/// - The table's `process_name` must match `expected_process_name`. Loading
///   addresses from a different process would write to unrelated memory.
/// - Freeze state is **not** persisted (see `SavedEntry`). After loading,
///   freezing is always off and the user must re-enable it explicitly.
pub fn load_cheat_table(path: &Path, expected_process_name: &str) -> Result<Vec<Box<SearchContext>>, String> {
    let toml_str = std::fs::read_to_string(path).map_err(|e| format!("Cannot read {}: {e}", path.display()))?;

    let table: CheatTable = toml::from_str(&toml_str).map_err(|e| format!("Parse error: {e}"))?;

    if table.version != 1 {
        return Err(format!("Unsupported cheat table version {} (expected 1)", table.version));
    }

    if !expected_process_name.is_empty() && table.process_name != expected_process_name {
        return Err(format!(
            "Cheat table was saved for process '{}', but the attached process is '{}'. Refusing to load.",
            table.process_name, expected_process_name
        ));
    }

    let mut searches: Vec<Box<SearchContext>> = Vec::new();

    for saved in table.searches {
        let ctx = Box::new(SearchContext::new(saved.description));
        let mut results = Vec::with_capacity(saved.entries.len());

        for entry in &saved.entries {
            let addr = parse_hex_address(&entry.address)?;
            results.push(SearchResult::new(addr, entry.search_type));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_cheat_table_path_uses_config_dir_and_sanitizes_name() {
        let path = default_cheat_table_path("Game: Test/1");

        assert!(path.starts_with(config_dir()));
        assert_eq!(path.file_name().and_then(|name| name.to_str()), Some("Game__Test_1.toml"));
    }
}
