//! Persistent definitions are tested against synthetic ASLR layouts. Live
//! access tests use only this test process and its own allocations.
use std::{
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

use game_cheetah::{AddressSpec, App, CheatTable, GameCheetahEngine, LoadedModule, ModuleCatalog, PendingAddress, SearchResult, SearchType};

fn catalog(base: usize) -> ModuleCatalog {
    ModuleCatalog {
        modules: vec![LoadedModule {
            path: "/games/game.exe".into(),
            base,
            ranges: vec![base..base + 0x1000, base + 0x2000..base + 0x3000],
            ambiguous: false,
        }],
    }
}

fn relative(module: &str, offset: usize) -> AddressSpec {
    AddressSpec::Module {
        module: module.into(),
        offset: format!("0x{offset:X}"),
    }
}

fn result(address: usize) -> SearchResult {
    SearchResult::new(address, SearchType::Int)
}

struct TableFile(PathBuf);

impl TableFile {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "game-cheetah-address-{}-{}.toml",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::OpenOptions::new().write(true).create_new(true).open(&path).unwrap();
        Self(path)
    }

    fn write(&self, text: &str) {
        std::fs::write(&self.0, text).unwrap();
    }
}

impl Drop for TableFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[test]
fn module_offset_tracks_relocated_image_not_writable_segment() {
    let first = catalog(0x100000);
    let second = catalog(0x500000);
    let spec = first.suggest(&result(0x102080));
    assert_eq!(spec, relative("/games/game.exe", 0x2080));
    assert_eq!(second.resolve(&spec, 4).unwrap(), 0x502080);
}

#[test]
fn heap_gaps_cross_boundary_values_and_overflows_are_rejected() {
    let modules = catalog(0x10000);
    assert!(matches!(modules.suggest(&result(0x14000)), AddressSpec::Absolute(_)));
    assert!(matches!(modules.suggest(&result(0x11fff)), AddressSpec::Absolute(_)));
    for offset in [0x1000, 0xffe, 0x3000, usize::MAX] {
        assert!(modules.resolve(&relative("game.exe", offset), 4).is_err());
    }
    for invalid in ["", "0x", "-1", "+10", "xyz", "0xFFFFFFFFFFFFFFFFFFFFFFFF"] {
        assert!(game_cheetah::address::parse_hex(invalid).is_err());
    }
    assert_eq!(game_cheetah::address::parse_hex(" 0Xff ").unwrap(), 255);
}

#[test]
fn missing_and_ambiguous_modules_never_fall_back_to_an_old_address() {
    let mut modules = catalog(0x10000);
    assert!(modules.resolve(&relative("missing.dll", 0x20), 4).is_err());
    assert_eq!(modules.resolve(&relative("game.exe", 0x20), 4).unwrap(), 0x10020);
    let mut other = catalog(0x50000).modules.remove(0);
    other.path = "/other/game.exe".into();
    modules.modules.push(other);
    assert!(modules.resolve(&relative("game.exe", 0x20), 4).is_err());
    assert_eq!(modules.resolve(&relative("/games/game.exe", 0x20), 4).unwrap(), 0x10020);
    assert!(modules.resolve(&relative("/old/game.exe", 0x20), 4).is_err());
    modules.modules[0].ambiguous = true;
    assert!(modules.resolve(&relative("/games/game.exe", 0x20), 4).is_err());
}

#[test]
fn version_two_save_load_rebases_and_preserves_unresolved_entries() {
    let file = TableFile::new();
    let mut engine = GameCheetahEngine::default();
    engine.process_name = "game".into();
    engine.searches[0].set_cached_results(vec![result(0x102080), result(0x999)]);
    engine.searches[0]
        .address_overrides
        .insert((0x102080, SearchType::Int), relative("game.exe", 0x2080));
    engine.searches[0].unresolved_addresses.push(PendingAddress {
        address: relative("later.dll", 0x30),
        search_type: SearchType::Int64,
        reason: "not loaded".into(),
    });
    game_cheetah::save_cheat_table(&engine, &file.0).unwrap();
    let text = std::fs::read_to_string(&file.0).unwrap();
    let saved: CheatTable = toml::from_str(&text).unwrap();
    assert_eq!(saved.version, 2);
    let loaded = game_cheetah::load_cheat_table_with_modules(&file.0, "game", &catalog(0x500000)).unwrap();
    assert_eq!(loaded[0].collect_results().iter().map(|r| r.addr).collect::<Vec<_>>(), vec![0x999, 0x502080]);
    assert_eq!(loaded[0].unresolved_addresses.len(), 1);
    assert!(loaded[0].freezed_addresses.is_empty());
    engine.searches = loaded;
    game_cheetah::save_cheat_table(&engine, &file.0).unwrap();
    let unavailable = game_cheetah::load_cheat_table(&file.0, "game").unwrap();
    assert_eq!(unavailable[0].get_result_count(), 1);
    assert_eq!(unavailable[0].unresolved_addresses.len(), 2);
    assert!(game_cheetah::load_cheat_table(&file.0, "other game").is_err());
}

#[test]
fn legacy_tables_load_but_invalid_or_future_definitions_fail_closed() {
    let file = TableFile::new();
    let legacy = "version = 1\nprocess_name = 'game'\n[[searches]]\ndescription = 'Gold'\n[[searches.entries]]\naddress = '0x1234'\nsearch_type = 'Int'\nfrozen = true\n";
    file.write(legacy);
    let loaded = game_cheetah::load_cheat_table(&file.0, "game").unwrap();
    assert_eq!(loaded[0].collect_results()[0].addr, 0x1234);
    assert!(loaded[0].freezed_addresses.is_empty());
    for invalid in [
        legacy.replace("version = 1", "version = 4"),
        legacy.replace("'0x1234'", "'not hex'"),
        legacy.replace("'0x1234'", "{ module = 'game', offset = '0x20' }"),
        legacy
            .replace("version = 1", "version = 2")
            .replace("'0x1234'", "{ module = 'game', offset = '0x20', pointers = [32] }"),
    ] {
        file.write(&invalid);
        assert!(game_cheetah::load_cheat_table(&file.0, "game").is_err());
    }
}

#[test]
fn reconnect_rebases_modules_but_quarantines_absolute_results_and_history() {
    let mut engine = GameCheetahEngine::default();
    let search = &mut engine.searches[0];
    search.set_cached_results(vec![result(0x102080), result(0x999)]);
    search.address_overrides.insert((0x102080, SearchType::Int), relative("game.exe", 0x2080));
    search.push_undo_state(search.collect_results());
    search.freezed_addresses.insert(0x102080);
    search.store_memory_snapshot(0x100, vec![1; 8]);
    assert!(engine.resolve_table_addresses(&catalog(0x500000), true));
    let search = &engine.searches[0];
    assert_eq!(search.collect_results()[0].addr, 0x502080);
    assert_eq!(search.get_result_count(), 1);
    assert_eq!(search.unresolved_addresses.len(), 1);
    assert!(matches!(search.unresolved_addresses[0].address, AddressSpec::Absolute(_)));
    assert!(search.old_results.is_empty());
    assert!(search.memory_snapshot.read().unwrap().is_empty());
    assert!(search.freezed_addresses.is_empty());
    assert!(!engine.resolve_table_addresses(&catalog(0x500000), false));
    assert_eq!(engine.searches[0].unresolved_addresses.len(), 1);
}

#[test]
fn unloaded_module_is_retained_and_reloaded_without_refreezing() {
    let mut engine = GameCheetahEngine::default();
    engine.searches[0].set_cached_results(vec![result(0x10020)]);
    engine.searches[0]
        .address_overrides
        .insert((0x10020, SearchType::Int), relative("game.exe", 0x20));
    engine.searches[0].freezed_addresses.insert(0x10020);
    assert!(engine.resolve_table_addresses(&ModuleCatalog::default(), false));
    assert_eq!(engine.searches[0].get_result_count(), 0);
    assert_eq!(engine.searches[0].unresolved_addresses.len(), 1);
    assert!(engine.resolve_table_addresses(&catalog(0x50000), false));
    assert_eq!(engine.searches[0].collect_results()[0].addr, 0x50020);
    assert!(engine.searches[0].unresolved_addresses.is_empty());
    assert!(engine.searches[0].freezed_addresses.is_empty());
}

#[cfg(target_os = "linux")]
#[test]
fn own_executable_data_mapping_is_suggested_and_saved_as_a_module() {
    static VALUE: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(314159);
    let address = &VALUE as *const _ as usize;
    let modules = ModuleCatalog::for_process(std::process::id() as _).unwrap();
    let spec = modules.suggest(&result(address));
    assert!(matches!(spec, AddressSpec::Module { .. }), "{spec:?}");
    assert_eq!(modules.resolve(&spec, 4).unwrap(), address);
    let mut engine = GameCheetahEngine::default();
    engine.pid = std::process::id() as _;
    engine.searches[0].set_cached_results(vec![result(address)]);
    let file = TableFile::new();
    game_cheetah::save_cheat_table(&engine, &file.0).unwrap();
    let saved: CheatTable = toml::from_str(&std::fs::read_to_string(&file.0).unwrap()).unwrap();
    assert!(matches!(saved.searches[0].entries[0].address, AddressSpec::Module { .. }));
}

#[cfg(target_os = "linux")]
#[test]
fn editing_an_address_is_read_only_reversible_and_rejects_duplicates() {
    let first = Box::new(42_i32);
    let second = Box::new(99_i32);
    let a = &*first as *const i32 as usize;
    let b = &*second as *const i32 as usize;
    let mut app = App::default();
    app.set_persistence_enabled(true);
    app.state.pid = std::process::id() as _;
    app.state.searches[0].set_cached_results(vec![result(a)]);
    app.begin_address_edit(0);
    assert!(app.apply_address_definition(AddressSpec::Absolute("wrong".into())).is_err());
    assert_eq!(app.state.searches[0].collect_results()[0].addr, a);
    app.apply_address_definition(AddressSpec::absolute(b)).unwrap();
    assert_eq!(app.state.searches[0].collect_results()[0].addr, b);
    assert_eq!((*first, *second), (42, 99));
    app.undo_search();
    assert_eq!(app.state.searches[0].collect_results()[0].addr, a);
    assert!(app.state.searches[0].address_overrides.is_empty());
    app.state.searches[0].set_cached_results(vec![result(a), result(b)]);
    let index = app.state.searches[0].collect_results().iter().position(|r| r.addr == a).unwrap();
    app.begin_address_edit(index);
    assert!(app.apply_address_definition(AddressSpec::absolute(b)).is_err());
    assert_eq!(app.state.searches[0].get_result_count(), 2);
}

#[cfg(target_os = "linux")]
#[test]
fn missing_module_never_writes_or_freezes_the_last_known_address() {
    let value = Box::new(42_i32);
    let address = &*value as *const i32 as usize;
    let mut app = App::default();
    app.state.pid = std::process::id() as _;
    app.state.searches[0].set_cached_results(vec![result(address)]);
    app.state.searches[0]
        .address_overrides
        .insert((address, SearchType::Int), relative("__missing_game__.so", 0x20));
    assert!(!app.try_write_result_value(0, "99"));
    app.toggle_freeze(0);
    app.toggle_freeze_all();
    assert!(app.state.searches[0].freezed_addresses.is_empty());
    assert_eq!(*value, 42);
}
