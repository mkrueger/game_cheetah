use std::{cell::RefCell, collections::BTreeMap, io};

use game_cheetah::{AddressSpec, LoadedModule, ModuleCatalog, PointerWidth};
use process_memory::{Architecture, CopyAddress};

#[derive(Default)]
struct Memory {
    blocks: BTreeMap<usize, Vec<u8>>,
    reads: RefCell<Vec<(usize, usize)>>,
}

impl CopyAddress for Memory {
    fn copy_address(&self, address: usize, bytes: &mut [u8]) -> io::Result<()> {
        self.reads.borrow_mut().push((address, bytes.len()));
        let (base, data) = self.blocks.range(..=address).next_back().ok_or(io::ErrorKind::NotFound)?;
        let offset = address - base;
        let end = offset.checked_add(bytes.len()).ok_or(io::ErrorKind::UnexpectedEof)?;
        bytes.copy_from_slice(data.get(offset..end).ok_or(io::ErrorKind::UnexpectedEof)?);
        Ok(())
    }

    fn get_pointer_width(&self) -> Architecture {
        // Deliberately irrelevant: the stored target width must be used.
        Architecture::Arch32Bit
    }
}

fn catalog() -> ModuleCatalog {
    ModuleCatalog {
        modules: vec![LoadedModule {
            path: "game.exe".into(),
            base: 0x1000,
            ranges: std::iter::once(0x1000..0x1100).collect(),
            ambiguous: false,
        }],
    }
}

fn chain(width: PointerWidth, offsets: &[&str]) -> AddressSpec {
    AddressSpec::Pointer {
        module: "game.exe".into(),
        offset: "0x20".into(),
        offsets: offsets.iter().map(|offset| (*offset).into()).collect(),
        pointer_width: width,
    }
}

#[test]
fn target_width_and_offset_order_are_explicit_and_last_target_is_a_value() {
    for width in [PointerWidth::Bits32, PointerWidth::Bits64] {
        let mut memory = Memory::default();
        memory.blocks.insert(0x1020, 0x2000_u64.to_le_bytes()[..width.bytes()].to_vec());
        memory.blocks.insert(0x2080, 0x3000_u64.to_le_bytes()[..width.bytes()].to_vec());
        memory.blocks.insert(0x2ff0, 0_i32.to_le_bytes().to_vec());
        let resolved = catalog().resolve_with_reader(&chain(width, &["80", "-0x10"]), 4, &memory).unwrap();
        assert_eq!(resolved.address, 0x2ff0);
        assert_eq!(resolved.steps.len(), 2);
        assert_eq!(resolved.steps[0].location, 0x1020);
        assert_eq!(resolved.steps[0].target, 0x2080);
        assert_eq!(resolved.steps[1].pointer, 0x3000);
        assert_eq!(*memory.reads.borrow(), vec![(0x1020, width.bytes()), (0x2080, width.bytes()), (0x2ff0, 4)]);
    }
}

#[test]
fn zero_offset_is_one_dereference_and_missing_or_short_reads_fail() {
    let mut memory = Memory::default();
    let spec = chain(PointerWidth::Bits64, &["0"]);
    assert!(catalog().resolve_with_reader(&spec, 4, &memory).is_err());
    memory.blocks.insert(0x1020, 0x2000_u32.to_le_bytes().to_vec());
    assert!(catalog().resolve_with_reader(&spec, 4, &memory).is_err());
    memory.blocks.insert(0x1020, 0x2000_u64.to_le_bytes().to_vec());
    memory.blocks.insert(0x2000, vec![42; 3]);
    assert!(catalog().resolve_with_reader(&spec, 4, &memory).is_err());
    memory.blocks.insert(0x2000, vec![42; 4]);
    assert_eq!(catalog().resolve_with_reader(&spec, 4, &memory).unwrap().address, 0x2000);
}

#[test]
fn null_pointers_depth_limits_and_offset_overflows_are_rejected() {
    let mut memory = Memory::default();
    memory.blocks.insert(0x1020, 0_u64.to_le_bytes().to_vec());
    assert!(catalog().resolve_with_reader(&chain(PointerWidth::Bits64, &["80"]), 4, &memory).is_err());
    for offsets in [vec![], vec!["0"; 17], vec!["-"], vec!["xyz"], vec!["0xFFFFFFFFFFFFFFFFFFFFFFFF"]] {
        memory.reads.borrow_mut().clear();
        assert!(catalog().resolve_with_reader(&chain(PointerWidth::Bits64, &offsets), 4, &memory).is_err());
        assert!(memory.reads.borrow().is_empty());
    }
    memory.blocks.insert(0x1020, 8_u64.to_le_bytes().to_vec());
    assert!(catalog().resolve_with_reader(&chain(PointerWidth::Bits64, &["-10"]), 4, &memory).is_err());
    memory.blocks.insert(0x1020, u64::MAX.to_le_bytes().to_vec());
    assert!(catalog().resolve_with_reader(&chain(PointerWidth::Bits64, &["1"]), 4, &memory).is_err());
    memory.blocks.insert(0x1020, u32::MAX.to_le_bytes().to_vec());
    assert!(catalog().resolve_with_reader(&chain(PointerWidth::Bits32, &["1"]), 4, &memory).is_err());
    assert!(catalog().resolve_with_reader(&chain(PointerWidth::Bits32, &["0"]), 4, &memory).is_err());
}

#[cfg(target_pointer_width = "64")]
#[test]
fn sixty_four_bit_pointers_are_not_truncated_to_thirty_two_bits() {
    let mut memory = Memory::default();
    let address = 0x1_0000_2000_u64;
    memory.blocks.insert(0x1020, address.to_le_bytes().to_vec());
    memory.blocks.insert(address as usize, vec![1; 4]);
    assert_eq!(
        catalog()
            .resolve_with_reader(&chain(PointerWidth::Bits64, &["+0"]), 4, &memory)
            .unwrap()
            .address,
        address as usize
    );
}

#[test]
fn moving_pointer_resolves_again_without_cached_fallback() {
    let mut memory = Memory::default();
    let spec = chain(PointerWidth::Bits64, &["0"]);
    memory.blocks.insert(0x1020, 0x2000_u64.to_le_bytes().to_vec());
    memory.blocks.insert(0x2000, vec![1; 4]);
    assert_eq!(catalog().resolve_with_reader(&spec, 4, &memory).unwrap().address, 0x2000);
    memory.blocks.insert(0x1020, 0x3000_u64.to_le_bytes().to_vec());
    memory.blocks.insert(0x3000, vec![2; 4]);
    assert_eq!(catalog().resolve_with_reader(&spec, 4, &memory).unwrap().address, 0x3000);
    memory.blocks.insert(0x1020, 0_u64.to_le_bytes().to_vec());
    assert!(catalog().resolve_with_reader(&spec, 4, &memory).is_err());
}

#[cfg(target_os = "linux")]
mod live {
    use super::*;
    use game_cheetah::{App, CheatTable, GameCheetahEngine, SearchResult, SearchType};
    use std::sync::{
        Mutex, MutexGuard,
        atomic::{AtomicUsize, Ordering},
    };

    // Nonzero initializer: file-backed data rather than anonymous BSS.
    static LOCK: Mutex<()> = Mutex::new(());
    static ROOT: AtomicUsize = AtomicUsize::new(1);

    struct Fixture {
        _guard: MutexGuard<'static, ()>,
        object: Box<[usize; 4]>,
        values: Box<[i32; 4]>,
        spec: AddressSpec,
        modules: ModuleCatalog,
    }

    impl Fixture {
        fn new() -> Self {
            let guard = LOCK.lock().unwrap();
            let values = Box::new([17, 42, 19, 20]);
            let mut object = Box::new([0; 4]);
            object[2] = values.as_ptr() as usize;
            ROOT.store(object.as_ptr() as usize, Ordering::Release);
            let modules = ModuleCatalog::for_process(std::process::id() as _).unwrap();
            let AddressSpec::Module { module, offset } = modules.suggest(&SearchResult::new(&ROOT as *const _ as usize, SearchType::Int64)) else {
                panic!("test root must be module-backed");
            };
            let spec = AddressSpec::Pointer {
                module,
                offset,
                offsets: vec![format!("0x{:X}", 2 * size_of::<usize>()), "0x4".into()],
                pointer_width: if size_of::<usize>() == 8 {
                    PointerWidth::Bits64
                } else {
                    PointerWidth::Bits32
                },
            };
            Self {
                _guard: guard,
                object,
                values,
                spec,
                modules,
            }
        }

        fn address(&self) -> usize {
            &self.values[1] as *const i32 as usize
        }

        fn app(&self) -> App {
            let mut app = App::default();
            app.state.pid = std::process::id() as _;
            app.state.searches[0].set_cached_results(vec![SearchResult::new(self.address(), SearchType::Int)]);
            app.state.searches[0]
                .address_overrides
                .insert((self.address(), SearchType::Int), self.spec.clone());
            app
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            ROOT.store(0, Ordering::Release);
        }
    }

    #[test]
    fn real_chain_writes_only_current_target_and_rebinds_after_movement() {
        let mut fixture = Fixture::new();
        let mut app = fixture.app();
        assert!(app.try_write_result_value(0, "55"));
        assert_eq!(fixture.values[..], [17, 55, 19, 20]);
        let other = Box::new([1_i32, 77, 3, 4]);
        fixture.object[2] = other.as_ptr() as usize;
        assert!(!app.try_write_result_value(0, "99"));
        assert_eq!(fixture.values[1], 55);
        assert_eq!(other[1], 77);
        app.state.searches[0].freezed_addresses.insert(fixture.address());
        assert!(app.state.resolve_table_addresses(&fixture.modules, false));
        assert!(app.state.searches[0].freezed_addresses.is_empty());
        assert!(app.try_write_result_value(0, "88"));
        assert_eq!(other[1], 88);
        ROOT.store(0, Ordering::Release);
        assert!(!app.commit_result_value(0, "100"));
        assert!(app.state.resolve_table_addresses(&fixture.modules, false));
        assert_eq!(app.state.searches[0].get_result_count(), 0);
        assert_eq!(app.state.searches[0].unresolved_addresses[0].address, fixture.spec);
    }

    #[test]
    fn version_three_round_trip_preserves_chains_and_pending_entries() {
        let fixture = Fixture::new();
        let app = fixture.app();
        let path = std::env::temp_dir().join(format!("game-cheetah-pointer-{}.toml", std::process::id()));
        game_cheetah::save_cheat_table(&app.state, &path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let saved: CheatTable = toml::from_str(&text).unwrap();
        assert_eq!(saved.version, 3);
        assert_eq!(saved.searches[0].entries[0].address, fixture.spec);
        let loaded = game_cheetah::load_cheat_table_with_process(&path, "", &fixture.modules, std::process::id() as _).unwrap();
        assert_eq!(loaded[0].collect_results()[0].addr, fixture.address());
        ROOT.store(0, Ordering::Release);
        let pending = game_cheetah::load_cheat_table_with_process(&path, "", &fixture.modules, std::process::id() as _).unwrap();
        assert_eq!(pending[0].get_result_count(), 0);
        assert_eq!(pending[0].unresolved_addresses[0].address, fixture.spec);
        let mut engine = GameCheetahEngine::default();
        engine.searches = pending;
        game_cheetah::save_cheat_table(&engine, &path).unwrap();
        assert!(std::fs::read_to_string(&path).unwrap().contains("pointer_width"));
        std::fs::write(&path, text.replace("version = 3", "version = 2")).unwrap();
        assert!(game_cheetah::load_cheat_table_with_modules(&path, "", &fixture.modules).is_err());
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn pointer_edit_undo_and_removal_preserve_definition_lifecycle() {
        let fixture = Fixture::new();
        let mut app = fixture.app();
        app.set_persistence_enabled(true);
        app.begin_address_edit(0);
        assert!(app.address_editor.as_ref().unwrap().pointer);
        assert_eq!(app.address_editor.as_ref().unwrap().definition(), fixture.spec);
        app.apply_address_definition(AddressSpec::absolute(fixture.address())).unwrap();
        assert!(!app.state.searches[0].has_pointer_addresses());
        app.undo_search();
        assert_eq!(app.state.searches[0].address_overrides[&(fixture.address(), SearchType::Int)], fixture.spec);
        app.state.searches[0].search_value_text = "42".into();
        app.state.filter_searches(0);
        assert!(app.state.current_error().is_some());
        assert_eq!(app.state.searches[0].get_result_count(), 1);
        app.remove_result(0);
        assert!(!app.state.searches[0].has_pointer_addresses());
        app.undo_search();
        assert!(app.state.searches[0].has_pointer_addresses());
    }
}
