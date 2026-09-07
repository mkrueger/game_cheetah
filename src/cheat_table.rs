use std::{
    io::Write,
    path::{Path, PathBuf},
};

use i18n_embed_fl::fl;
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

use crate::{AddressSpec, GameCheetahEngine, ModuleCatalog, PendingAddress, SearchContext, SearchResult, SearchType};

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedEntry {
    pub address: AddressSpec,
    pub search_type: SearchType,
    // NOTE: We deliberately do NOT persist freeze state. Freeze is ephemeral
    // in-memory behavior; saving it across runs is unsafe (ASLR moves the
    // target address) and silently restoring it would write to unrelated
    // memory. Older files with `frozen` / `frozen_value` fields still parse
    // cleanly because serde ignores unknown TOML fields by default.
}

/// Returns the directory Game Cheetah uses for persisted user files.
///
/// Resolution is delegated to the `dirs` crate, which yields the
/// platform's standard config location:
/// - Linux: `$XDG_CONFIG_HOME` or `~/.config`
/// - macOS: `~/Library/Application Support`
/// - Windows: `%APPDATA%` (Roaming)
///
/// `game-cheetah` is appended. If `dirs::config_dir()` fails to resolve a
/// home/config location, the current working directory is used.
pub fn config_dir() -> PathBuf {
    dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join("game-cheetah")
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
    snapshot_cheat_table(engine)?.save(path)
}

/// Capture persistent address metadata without writing a file or resolving pointers.
pub fn snapshot_cheat_table(engine: &GameCheetahEngine) -> Result<CheatTable, String> {
    let modules = ModuleCatalog::for_process(engine.pid)?;
    let mut saved_searches = Vec::new();

    for ctx in &engine.searches {
        let results = ctx.collect_results();
        let mut entries = Vec::with_capacity(results.len());

        for result in results.iter() {
            entries.push(SavedEntry {
                address: ctx
                    .address_overrides
                    .get(&(result.addr, result.search_type))
                    .cloned()
                    .unwrap_or_else(|| modules.suggest(result)),
                search_type: result.search_type,
            });
        }
        entries.extend(ctx.unresolved_addresses.iter().map(|entry| SavedEntry {
            address: entry.address.clone(),
            search_type: entry.search_type,
        }));

        saved_searches.push(SavedSearch {
            description: ctx.description.clone(),
            entries,
        });
    }

    Ok(CheatTable {
        version: if saved_searches
            .iter()
            .any(|search| search.entries.iter().any(|entry| entry.address.is_pointer()))
        {
            3
        } else {
            2
        },
        process_name: engine.process_name.clone(),
        searches: saved_searches,
    })
}

impl CheatTable {
    /// Serialize before touching the filesystem, then atomically replace the target.
    /// Any failure before replacement leaves an existing target untouched.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let toml_str = toml::to_string_pretty(self).map_err(|e| format!("Serialization error: {e}"))?;
        let parent = path.parent().filter(|parent| !parent.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
        std::fs::create_dir_all(parent).map_err(|e| format!("Cannot create directory: {e}"))?;

        let mut file = NamedTempFile::new_in(parent).map_err(|e| format!("Cannot create temporary file for {}: {e}", path.display()))?;
        file.write_all(toml_str.as_bytes())
            .map_err(|e| format!("Cannot write {}: {e}", path.display()))?;
        file.as_file().sync_all().map_err(|e| format!("Cannot sync {}: {e}", path.display()))?;
        file.persist(path).map_err(|e| format!("Cannot replace {}: {e}", path.display()))?;
        Ok(())
    }
}

/// Load a cheat table from disk.
///
/// Safety notes:
/// - The table's `process_name` must match `expected_process_name`. Loading
///   addresses from a different process would write to unrelated memory.
/// - Freeze state is **not** persisted (see `SavedEntry`). After loading,
///   freezing is always off and the user must re-enable it explicitly.
pub fn load_cheat_table(path: &Path, expected_process_name: &str) -> Result<Vec<Box<SearchContext>>, String> {
    load_cheat_table_with_modules(path, expected_process_name, &ModuleCatalog::default())
}

pub fn load_cheat_table_with_modules(path: &Path, expected_process_name: &str, modules: &ModuleCatalog) -> Result<Vec<Box<SearchContext>>, String> {
    load_cheat_table_with_process(path, expected_process_name, modules, 0)
}

pub fn load_cheat_table_with_process(
    path: &Path,
    expected_process_name: &str,
    modules: &ModuleCatalog,
    pid: process_memory::Pid,
) -> Result<Vec<Box<SearchContext>>, String> {
    let toml_str = std::fs::read_to_string(path).map_err(|e| format!("Cannot read {}: {e}", path.display()))?;

    let table: CheatTable = toml::from_str(&toml_str).map_err(|e| format!("Parse error: {e}"))?;

    if !matches!(table.version, 1..=3) {
        return Err(format!("Unsupported cheat table version {} (expected 1, 2 or 3)", table.version));
    }

    if !expected_process_name.is_empty() && table.process_name != expected_process_name {
        return Err(format!(
            "Cheat table was saved for process '{}', but the attached process is '{}'. Refusing to load.",
            table.process_name, expected_process_name
        ));
    }

    let mut searches: Vec<Box<SearchContext>> = Vec::new();

    for saved in table.searches {
        let mut ctx = Box::new(SearchContext::new(saved.description));
        let mut results = Vec::with_capacity(saved.entries.len());

        for entry in &saved.entries {
            entry.address.validate()?;
            if table.version < 3 && entry.address.is_pointer() {
                return Err("Pointer addresses require cheat table version 3".to_owned());
            }
            if table.version == 1 && entry.address.is_relative() {
                return Err("Module addresses require cheat table version 2".to_owned());
            }
            match modules.resolve_for_process(&entry.address, entry.search_type.fixed_byte_length().unwrap_or(1), pid) {
                Ok(addr) => {
                    let key = (addr, entry.search_type);
                    if ctx.address_overrides.get(&key).is_some_and(|old| old != &entry.address) {
                        return Err(fl!(crate::LANGUAGE_LOADER, "address-duplicate"));
                    }
                    ctx.address_overrides.insert(key, entry.address.clone());
                    results.push(SearchResult::new(addr, entry.search_type));
                }
                Err(reason) => ctx.unresolved_addresses.push(PendingAddress {
                    address: entry.address.clone(),
                    search_type: entry.search_type,
                    reason,
                }),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn table_fixture() -> CheatTable {
        CheatTable {
            version: 2,
            process_name: "test-game".into(),
            searches: vec![SavedSearch {
                description: "Health".into(),
                entries: vec![SavedEntry {
                    address: AddressSpec::absolute(0x1234),
                    search_type: SearchType::Int,
                }],
            }],
        }
    }

    #[test]
    fn snapshot_preserves_pending_metadata_and_version_two_compatibility() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("table.toml");
        let mut engine = GameCheetahEngine::default();
        engine.process_name = "test-game".into();
        let mut ctx = Box::new(SearchContext::new("Health".into()));
        ctx.set_cached_results(vec![SearchResult::new(0x1234, SearchType::Int)]);
        let pending = AddressSpec::Module {
            module: "missing-game-module".into(),
            offset: "0x20".into(),
        };
        ctx.unresolved_addresses.push(PendingAddress {
            address: pending.clone(),
            search_type: SearchType::Double,
            reason: "not loaded".into(),
        });
        engine.searches = vec![ctx];

        let snapshot = snapshot_cheat_table(&engine).unwrap();
        assert_eq!(snapshot.version, 2);
        assert_eq!(snapshot.process_name, "test-game");
        assert_eq!(snapshot.searches[0].description, "Health");
        assert_eq!(snapshot.searches[0].entries.len(), 2);
        assert_eq!(snapshot.searches[0].entries[0].address, AddressSpec::absolute(0x1234));
        assert_eq!(snapshot.searches[0].entries[1].address, pending);
        assert_eq!(snapshot.searches[0].entries[1].search_type, SearchType::Double);
        assert_eq!(engine.searches[0].unresolved_addresses[0].reason, "not loaded");

        // The snapshot owns its metadata and can be saved after the engine changes.
        engine.searches.clear();
        engine.process_name.clear();
        snapshot.save(&path).unwrap();
        let loaded = load_cheat_table(&path, "test-game").unwrap();
        assert_eq!(loaded[0].description, "Health");
        assert_eq!(loaded[0].collect_results()[0].addr, 0x1234);
        assert_eq!(loaded[0].unresolved_addresses[0].address, pending);
        assert_eq!(loaded[0].unresolved_addresses[0].search_type, SearchType::Double);
    }

    #[test]
    fn snapshot_preserves_explicit_and_pending_pointers_without_resolving_them() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pointers.toml");
        for width in [crate::PointerWidth::Bits32, crate::PointerWidth::Bits64] {
            for pending_only in [false, true] {
                let mut engine = GameCheetahEngine::default();
                engine.process_name = "test-game".into();
                let mut ctx = Box::new(SearchContext::new("Pointers".into()));
                let pointer = AddressSpec::Pointer {
                    module: "missing-game-module".into(),
                    offset: "0x20".into(),
                    offsets: vec!["0x10".into(), "-0x8".into()],
                    pointer_width: width,
                };
                if !pending_only {
                    ctx.set_cached_results(vec![SearchResult::new(0x1234, SearchType::Int)]);
                    ctx.address_overrides.insert((0x1234, SearchType::Int), pointer.clone());
                }
                ctx.unresolved_addresses.push(PendingAddress {
                    address: pointer.clone(),
                    search_type: SearchType::Float,
                    reason: "not loaded".into(),
                });
                engine.searches = vec![ctx];

                let snapshot = snapshot_cheat_table(&engine).unwrap();
                assert_eq!(snapshot.version, 3);
                assert_eq!(snapshot.searches[0].entries.len(), if pending_only { 1 } else { 2 });
                for entry in &snapshot.searches[0].entries {
                    assert_eq!(entry.address, pointer);
                }
                if !pending_only {
                    assert_eq!(snapshot.searches[0].entries[0].search_type, SearchType::Int);
                }
                assert_eq!(snapshot.searches[0].entries.last().unwrap().search_type, SearchType::Float);

                save_cheat_table(&engine, &path).unwrap();
                assert_eq!(std::fs::read_to_string(&path).unwrap(), toml::to_string_pretty(&snapshot).unwrap());
                let loaded = load_cheat_table(&path, "test-game").unwrap();
                assert!(loaded[0].collect_results().is_empty());
                assert_eq!(loaded[0].unresolved_addresses.len(), snapshot.searches[0].entries.len());
                for (pending, saved) in loaded[0].unresolved_addresses.iter().zip(&snapshot.searches[0].entries) {
                    assert_eq!(pending.address, saved.address);
                    assert_eq!(pending.search_type, saved.search_type);
                }
            }
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn snapshot_retains_module_suggestions() {
        let mut engine = GameCheetahEngine::default();
        engine.pid = std::process::id() as _;
        let result = SearchResult::new(snapshot_retains_module_suggestions as *const () as usize, SearchType::Byte);
        let expected = ModuleCatalog::for_process(engine.pid).unwrap().suggest(&result);
        assert!(matches!(expected, AddressSpec::Module { .. }));
        let ctx = Box::new(SearchContext::new("Code".into()));
        ctx.set_cached_results(vec![result]);
        engine.searches = vec![ctx];

        let snapshot = snapshot_cheat_table(&engine).unwrap();
        assert_eq!(snapshot.version, 2);
        assert_eq!(snapshot.searches[0].entries[0].address, expected);
    }

    #[test]
    fn save_atomically_replaces_existing_file_and_cleans_up_temporary_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("table.toml");
        std::fs::write(&path, "old table").unwrap();
        // A hard link distinguishes atomic replacement from truncating the old inode.
        #[cfg(unix)]
        let old_link = dir.path().join("old-table.toml");
        #[cfg(unix)]
        std::fs::hard_link(&path, &old_link).unwrap();
        let entries_before = std::fs::read_dir(dir.path()).unwrap().count();
        let table = table_fixture();

        table.save(&path).unwrap();

        assert_eq!(std::fs::read_to_string(&path).unwrap(), toml::to_string_pretty(&table).unwrap());
        #[cfg(unix)]
        assert_eq!(std::fs::read_to_string(old_link).unwrap(), "old table");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), entries_before);
    }

    #[test]
    fn save_creates_missing_parent_directories() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/tables/table.toml");
        let table = table_fixture();

        table.save(&path).unwrap();

        assert_eq!(std::fs::read_to_string(&path).unwrap(), toml::to_string_pretty(&table).unwrap());
        assert_eq!(std::fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
    }

    #[test]
    fn save_filesystem_failures_preserve_existing_files_and_clean_up() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target");
        std::fs::create_dir(&target).unwrap();
        let old_path = target.join("old.toml");
        std::fs::write(&old_path, "old table").unwrap();
        let table = table_fixture();

        // A regular file cannot be the destination's parent, even when run as root.
        let error = table.save(&old_path.join("table.toml")).unwrap_err();
        assert!(error.starts_with("Cannot create directory:"));
        assert_eq!(std::fs::read_to_string(&old_path).unwrap(), "old table");

        // Replacing a nonempty directory fails after the temporary file was synced.
        let error = table.save(&target).unwrap_err();
        assert!(error.starts_with("Cannot replace "));
        assert_eq!(std::fs::read_to_string(&old_path).unwrap(), "old table");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        assert_eq!(std::fs::read_dir(&target).unwrap().count(), 1);
    }

    #[test]
    fn default_cheat_table_path_uses_config_dir_and_sanitizes_name() {
        let path = default_cheat_table_path("Game: Test/1");

        assert!(path.starts_with(config_dir()));
        assert_eq!(path.file_name().and_then(|name| name.to_str()), Some("Game__Test_1.toml"));
    }
}
