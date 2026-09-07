//! Save a snapshot, automatically replacing absolute numeric entries with
//! live-checked executable-rooted pointer chains. Never mutate live search tabs.
use std::{
    collections::HashMap,
    io::{Read, Seek, SeekFrom},
    path::PathBuf,
    sync::atomic::Ordering,
};

use i18n_embed_fl::fl;

use crate::{
    AddressSpec, App, CheatTable, ModuleCatalog, PointerWidth, SearchResult, SearchType,
    pointer_scan::{ProcessIdentity, ScanJob, ScanOptions, ScanReport},
};

const MAX_AUTO_TARGETS: usize = 8;

pub(crate) struct AutomaticSave {
    identity: ProcessIdentity,
    path: PathBuf,
    table: CheatTable,
    live_counts: Vec<usize>,
    targets: Vec<SearchResult>,
    next: usize,
    job: Option<ScanJob>,
    options: ScanOptions,
    found: HashMap<(usize, SearchType), AddressSpec>,
    incomplete: bool,
}

impl AutomaticSave {
    fn new(identity: ProcessIdentity, path: PathBuf, table: CheatTable, live_counts: Vec<usize>) -> Self {
        let mut targets: Vec<SearchResult> = Vec::new();
        let mut incomplete = false;
        for entry in table
            .searches
            .iter()
            .zip(&live_counts)
            .flat_map(|(search, &count)| search.entries.iter().take(count))
        {
            if let AddressSpec::Absolute(text) = &entry.address
                && entry.search_type.fixed_byte_length().is_some()
                && let Ok(addr) = crate::address::parse_hex(text)
            {
                let target = SearchResult::new(addr, entry.search_type);
                if !targets.iter().any(|old| old.addr == target.addr && old.search_type == target.search_type) {
                    if targets.len() < MAX_AUTO_TARGETS {
                        targets.push(target);
                    } else {
                        incomplete = true;
                    }
                }
            }
        }
        let width = executable_pointer_width(&identity.executable);
        if width.is_none() && !targets.is_empty() {
            // Do not guess host architecture for an unknown/cross-bitness target.
            targets.clear();
            incomplete = true;
        }
        Self {
            identity,
            path,
            table,
            live_counts,
            targets,
            next: 0,
            job: None,
            options: ScanOptions {
                pointer_width: width.unwrap_or(PointerWidth::Bits64),
                ..Default::default()
            },
            found: HashMap::new(),
            incomplete,
        }
    }

    fn accept(&mut self, target: SearchResult, report: ScanReport) {
        self.incomplete |= report.limited || report.failed_reads > 0;
        // Prefer short paths rooted in the actual game executable. Do not
        // automatically pick incidental allocator/system-library references.
        let mut candidates: Vec<_> = report
            .candidates
            .into_iter()
            .filter(|spec| matches!(spec, AddressSpec::Pointer { module, .. } if module == &self.identity.executable))
            .collect();
        candidates.sort_by_key(|spec| match spec {
            AddressSpec::Pointer { offsets, .. } => (offsets.len(), spec.label()),
            _ => unreachable!(),
        });
        if let Some(spec) = candidates.into_iter().next() {
            self.found.insert((target.addr, target.search_type), spec);
        }
    }

    fn finish(mut self) -> Result<String, String> {
        if ProcessIdentity::capture(self.identity.pid)? != self.identity {
            return Err(fl!(crate::LANGUAGE_LOADER, "pointer-scan-wrong-target"));
        }
        let modules = ModuleCatalog::for_process(self.identity.pid)?;
        // An earlier target may have moved while scanning a later one.
        self.found.retain(|&(addr, kind), spec| {
            modules
                .resolve_for_process(spec, kind.fixed_byte_length().unwrap(), self.identity.pid)
                .is_ok_and(|address| address == addr)
        });
        let mut automatic = 0;
        let mut absolute = 0;
        for (search, &count) in self.table.searches.iter_mut().zip(&self.live_counts) {
            for entry in search.entries.iter_mut().take(count) {
                if let AddressSpec::Absolute(text) = &entry.address {
                    let candidate = crate::address::parse_hex(text).ok().and_then(|addr| self.found.get(&(addr, entry.search_type)));
                    if let Some(spec) = candidate {
                        entry.address = spec.clone();
                        automatic += 1;
                    }
                }
            }
        }
        for entry in self.table.searches.iter().flat_map(|search| &search.entries) {
            absolute += usize::from(matches!(entry.address, AddressSpec::Absolute(_)));
        }
        if self
            .table
            .searches
            .iter()
            .any(|search| search.entries.iter().any(|entry| entry.address.is_pointer()))
        {
            self.table.version = 3;
        }
        self.table.save(&self.path)?;
        let mut message = fl!(
            crate::LANGUAGE_LOADER,
            "auto-save-done",
            count = automatic.to_string(),
            absolute = absolute.to_string()
        );
        if self.incomplete {
            message.push_str(&format!("\n{}", fl!(crate::LANGUAGE_LOADER, "auto-save-limited")));
        }
        Ok(message)
    }
}

impl App {
    pub fn is_saving_cheat_table(&self) -> bool {
        self.automatic_save.is_some()
    }

    pub fn cancel_cheat_table_save(&mut self) {
        if self.automatic_save.take().is_some() {
            self.automatic_save_notice = Some(fl!(crate::LANGUAGE_LOADER, "auto-save-cancelled"));
        }
    }

    pub(crate) fn start_automatic_save(&mut self, path: PathBuf) -> Result<(), String> {
        if !self.enable_persistence {
            return Err(fl!(crate::LANGUAGE_LOADER, "persistence-disabled"));
        }
        if self.is_saving_cheat_table() {
            return Ok(());
        }
        let identity = ProcessIdentity::capture(self.state.pid)?;
        let table = crate::snapshot_cheat_table(&self.state)?;
        let live_counts = self.state.searches.iter().map(|search| search.collect_results().len()).collect();
        self.pointer_scanner.cancel();
        self.automatic_save_notice = None;
        self.automatic_save = Some(AutomaticSave::new(identity, path, table, live_counts));
        self.poll_automatic_save();
        Ok(())
    }

    pub fn poll_automatic_save(&mut self) {
        if !self.enable_persistence {
            self.cancel_cheat_table_save();
            return;
        }
        let Some(save) = &mut self.automatic_save else { return };
        if save.identity.pid != self.state.pid
            || (self.state.attached_start_time != 0 && save.identity.start_time != self.state.attached_start_time)
            || !self.state.is_process_running()
        {
            self.cancel_cheat_table_save();
            return;
        }
        if let Some(job) = &save.job {
            let Some(result) = job.poll() else { return };
            save.job = None;
            match result {
                Ok(report) => save.accept(save.targets[save.next - 1], report),
                Err(_) => save.incomplete = true,
            }
        }
        if let Some(&target) = save.targets.get(save.next) {
            save.next += 1;
            match ScanJob::start(save.identity.clone(), target, save.options.clone(), None) {
                Ok(job) => save.job = Some(job),
                Err(_) => save.incomplete = true,
            }
            return;
        }
        let save = self.automatic_save.take().unwrap();
        self.automatic_save_notice = Some(match save.finish() {
            Ok(message) => message,
            Err(error) => fl!(crate::LANGUAGE_LOADER, "auto-save-error", error = error),
        });
    }
}

pub(crate) fn show(app: &mut App, ui: &mut egui::Ui) {
    if !app.enable_persistence {
        return;
    }
    if app.automatic_save.is_none() && app.automatic_save_notice.is_none() {
        return;
    }
    egui::Panel::top("automatic_save_status").show(ui, |ui| {
        // egui 0.36 small buttons zero their vertical inner margin, but keep
        // the negative outer margin from hover expansion. This changes their
        // layout height and shifts this auto-sized panel and all following tabs.
        // Keep the banner's geometry stable; retain hover colors and strokes.
        let widgets = &mut ui.style_mut().visuals.widgets;
        widgets.inactive.expansion = 0.0;
        widgets.hovered.expansion = 0.0;
        widgets.active.expansion = 0.0;
        if let Some(save) = &app.automatic_save {
            let mib = save.job.as_ref().map_or(0, |job| job.progress.bytes.load(Ordering::Relaxed) / (1024 * 1024));
            ui.horizontal_wrapped(|ui| {
                ui.spinner();
                ui.label(fl!(
                    crate::LANGUAGE_LOADER,
                    "auto-save-progress",
                    index = save.next.to_string(),
                    count = save.targets.len().to_string(),
                    mib = mib.to_string()
                ));
            });
            if ui.button(fl!(crate::LANGUAGE_LOADER, "auto-save-cancel")).clicked() {
                app.cancel_cheat_table_save();
            }
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
        } else if let Some(notice) = &app.automatic_save_notice {
            ui.add(egui::Label::new(notice).wrap());
            if ui.small_button(fl!(crate::LANGUAGE_LOADER, "auto-save-dismiss")).clicked() {
                app.automatic_save_notice = None;
            }
        }
    });
}

/// Read the target's binary, never infer pointer width from value type or host.
/// Unsupported/fat/big-endian binaries remain absolute rather than guessed.
fn executable_pointer_width(path: &str) -> Option<PointerWidth> {
    let mut file = std::fs::File::open(path).ok()?;
    let mut header = [0; 64];
    file.read_exact(&mut header).ok()?;
    if &header[..4] == b"\x7fELF" && header[5] == 1 {
        return match header[4] {
            1 => Some(PointerWidth::Bits32),
            2 => Some(PointerWidth::Bits64),
            _ => None,
        };
    }
    if &header[..2] == b"MZ" {
        let offset = u32::from_le_bytes(header[60..64].try_into().ok()?);
        file.seek(SeekFrom::Start(u64::from(offset))).ok()?;
        let mut pe = [0; 26];
        file.read_exact(&mut pe).ok()?;
        if &pe[..4] == b"PE\0\0" {
            return match u16::from_le_bytes([pe[24], pe[25]]) {
                0x10b => Some(PointerWidth::Bits32),
                0x20b => Some(PointerWidth::Bits64),
                _ => None,
            };
        }
    }
    match &header[..4] {
        [0xce, 0xfa, 0xed, 0xfe] => Some(PointerWidth::Bits32),
        [0xcf, 0xfa, 0xed, 0xfe] => Some(PointerWidth::Bits64),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SavedEntry, SavedSearch};

    fn table(entries: Vec<SavedEntry>) -> CheatTable {
        CheatTable {
            version: 2,
            process_name: "auto-save-test".into(),
            searches: vec![SavedSearch {
                description: "snapshot".into(),
                entries,
            }],
        }
    }

    fn absolute(addr: usize) -> SavedEntry {
        SavedEntry {
            address: AddressSpec::Absolute(format!("0x{addr:X}")),
            search_type: SearchType::Int,
        }
    }

    #[test]
    fn notice_layout_does_not_move_when_dismiss_is_hovered() {
        fn text_rect(shapes: &[egui::epaint::ClippedShape], label: &str) -> egui::Rect {
            fn find(shape: &egui::epaint::Shape, label: &str) -> Option<egui::Rect> {
                match shape {
                    egui::epaint::Shape::Text(text) if text.galley.job.text == label => Some(text.galley.rect.translate(text.pos.to_vec2())),
                    egui::epaint::Shape::Vec(children) => children.iter().find_map(|child| find(child, label)),
                    _ => None,
                }
            }
            shapes.iter().find_map(|shape| find(&shape.shape, label)).unwrap()
        }
        for width in [640.0, 1100.0, 1673.0] {
            let ctx = egui::Context::default();
            crate::ui::theme::apply(&ctx);
            let mut app = App::default();
            app.set_persistence_enabled(true);
            let notice = format!(
                "{}\n{}",
                fl!(crate::LANGUAGE_LOADER, "auto-save-done", count = "0", absolute = "1"),
                fl!(crate::LANGUAGE_LOADER, "auto-save-limited")
            );
            app.automatic_save_notice = Some(notice.clone());
            let dismiss = fl!(crate::LANGUAGE_LOADER, "auto-save-dismiss");
            let mut render = |events| {
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 800.0))),
                        events,
                        ..Default::default()
                    },
                    |ui| crate::ui::in_process_view::view_in_process(&mut app, ui),
                );
                output.textures_delta.clear();
                output
            };
            for _ in 0..5 {
                render(vec![]);
            }
            let initial = render(vec![]);
            let button = text_rect(&initial.shapes, &dismiss);
            let text = text_rect(&initial.shapes, &notice);
            let tab_label = fl!(crate::LANGUAGE_LOADER, "first-search-label");
            let tab = text_rect(&initial.shapes, &tab_label);
            for hovered in [true, false, true, false] {
                for _ in 0..4 {
                    let position = if hovered { button.center() } else { egui::pos2(width - 10.0, 790.0) };
                    let output = render(vec![egui::Event::PointerMoved(position)]);
                    assert_eq!(text_rect(&output.shapes, &notice), text, "notice moved at width {width}, hovered={hovered}");
                    assert_eq!(text_rect(&output.shapes, &dismiss), button, "button moved at width {width}, hovered={hovered}");
                    assert_eq!(text_rect(&output.shapes, &tab_label), tab, "tabs moved at width {width}, hovered={hovered}");
                }
            }
        }
    }

    #[test]
    fn binary_width_is_detected_from_target_not_host_or_value() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("binary");
        let path_str = path.to_str().unwrap();
        for (magic, width) in [
            (vec![0x7f, b'E', b'L', b'F', 1, 1], Some(PointerWidth::Bits32)),
            (vec![0x7f, b'E', b'L', b'F', 2, 1], Some(PointerWidth::Bits64)),
            (vec![0x7f, b'E', b'L', b'F', 2, 2], None),
            (vec![0xce, 0xfa, 0xed, 0xfe], Some(PointerWidth::Bits32)),
            (vec![0xcf, 0xfa, 0xed, 0xfe], Some(PointerWidth::Bits64)),
            (vec![0xca, 0xfe, 0xba, 0xbe], None),
        ] {
            let mut bytes = vec![0; 64];
            bytes[..magic.len()].copy_from_slice(&magic);
            std::fs::write(&path, bytes).unwrap();
            assert_eq!(executable_pointer_width(path_str), width);
        }
        for (magic, width) in [(0x10b_u16, PointerWidth::Bits32), (0x20b, PointerWidth::Bits64)] {
            let mut bytes = vec![0; 90];
            bytes[..2].copy_from_slice(b"MZ");
            bytes[60..64].copy_from_slice(&64_u32.to_le_bytes());
            bytes[64..68].copy_from_slice(b"PE\0\0");
            bytes[88..90].copy_from_slice(&magic.to_le_bytes());
            std::fs::write(&path, bytes).unwrap();
            assert_eq!(executable_pointer_width(path_str), Some(width));
        }
        std::fs::write(&path, b"short").unwrap();
        assert_eq!(executable_pointer_width(path_str), None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn automatic_targets_are_deduplicated_bounded_and_exclude_pending() {
        let identity = ProcessIdentity::capture(std::process::id() as _).unwrap();
        let mut entries: Vec<_> = (1..=10).map(absolute).collect();
        entries.insert(1, absolute(1));
        let save = AutomaticSave::new(identity.clone(), PathBuf::new(), table(entries), vec![11]);
        assert_eq!(save.targets.len(), MAX_AUTO_TARGETS);
        assert!(save.incomplete);
        let save = AutomaticSave::new(identity, PathBuf::new(), table(vec![absolute(1), absolute(2)]), vec![1]);
        assert_eq!(save.targets.len(), 1);
        assert_eq!(save.targets[0].addr, 1);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn only_short_main_executable_candidates_are_automatically_selected() {
        let identity = ProcessIdentity::capture(std::process::id() as _).unwrap();
        let candidate = |module: &str, depth: usize| AddressSpec::Pointer {
            module: module.into(),
            offset: "0x20".into(),
            offsets: vec!["0x0".into(); depth],
            pointer_width: PointerWidth::Bits64,
        };
        let preferred = candidate(&identity.executable, 2);
        let target = SearchResult::new(1, SearchType::Int);
        let mut save = AutomaticSave::new(identity.clone(), PathBuf::new(), table(vec![absolute(1)]), vec![1]);
        save.accept(
            target,
            ScanReport {
                candidates: vec![candidate("/lib/allocator.so", 1), candidate(&identity.executable, 3), preferred.clone()],
                ..Default::default()
            },
        );
        assert_eq!(save.found[&(target.addr, target.search_type)], preferred);
    }

    #[cfg(all(target_os = "linux", target_pointer_width = "64"))]
    #[test]
    fn save_scans_live_target_atomically_without_changing_tabs_or_memory() {
        use std::{sync::atomic::AtomicUsize, time::Instant};
        static ROOT: AtomicUsize = AtomicUsize::new(1);
        let value = Box::new(12345_i32);
        let target = SearchResult::new(&*value as *const i32 as usize, SearchType::Int);
        ROOT.store(target.addr, Ordering::Release);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("table.toml");
        std::fs::write(&path, "previous file").unwrap();
        let identity = ProcessIdentity::capture(std::process::id() as _).unwrap();
        let mut app = App::default();
        app.set_persistence_enabled(true);
        app.state.pid = identity.pid;
        app.state.searches[0].set_cached_results(vec![target]);
        let snapshot = crate::snapshot_cheat_table(&app.state).unwrap();
        let mut save = AutomaticSave::new(identity.clone(), path.clone(), snapshot, vec![1]);
        save.options.depth = 1;
        save.options.max_offset = 0;
        save.options.scan_mib = 1;
        app.automatic_save = Some(save);
        app.poll_automatic_save();
        assert!(app.is_saving_cheat_table());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "previous file");
        let deadline = Instant::now() + std::time::Duration::from_secs(10);
        while app.is_saving_cheat_table() {
            app.poll_automatic_save();
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        let saved: CheatTable = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(saved.version, 3);
        let spec = &saved.searches[0].entries[0].address;
        assert!(spec.is_pointer());
        assert_eq!(
            ModuleCatalog::for_process(identity.pid)
                .unwrap()
                .resolve_for_process(spec, 4, identity.pid)
                .unwrap(),
            target.addr
        );
        assert_eq!(*value, 12345);
        assert_eq!(app.state.searches.len(), 1);
        assert!(app.state.searches[0].address_overrides.is_empty());
        assert!(app.state.searches[0].freezed_addresses.is_empty());

        // Cancel immediately, including a worker that may already have finished.
        let before = std::fs::read(&path).unwrap();
        app.start_automatic_save(path.clone()).unwrap();
        app.cancel_cheat_table_save();
        app.poll_automatic_save();
        assert_eq!(std::fs::read(&path).unwrap(), before);
        app.start_automatic_save(path.clone()).unwrap();
        app.state.pid = 0;
        app.poll_automatic_save();
        assert!(!app.is_saving_cheat_table());
        assert_eq!(std::fs::read(&path).unwrap(), before);

        app.state.pid = identity.pid;
        app.start_automatic_save(path.clone()).unwrap();
        app.set_persistence_enabled(false);
        app.poll_automatic_save();
        assert!(!app.is_saving_cheat_table());
        assert!(app.automatic_save_notice.is_none());
        assert!(app.start_automatic_save(path.clone()).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);

        // A chain that moved since its scan must not be saved for this target.
        let mut save = AutomaticSave::new(identity, path.clone(), table(vec![absolute(target.addr), absolute(target.addr)]), vec![1]);
        save.found.insert((target.addr, target.search_type), spec.clone());
        ROOT.store(0, Ordering::Release);
        save.finish().unwrap();
        let saved: CheatTable = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(saved.searches[0].entries.iter().all(|entry| matches!(entry.address, AddressSpec::Absolute(_))));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn existing_definitions_and_pending_entries_save_without_scanning() {
        let identity = ProcessIdentity::capture(std::process::id() as _).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("table.toml");
        let module = AddressSpec::Module {
            module: identity.executable.clone(),
            offset: "0x10".into(),
        };
        let pointer = AddressSpec::Pointer {
            module: identity.executable.clone(),
            offset: "0x20".into(),
            offsets: vec!["0x0".into()],
            pointer_width: PointerWidth::Bits64,
        };
        let entries = vec![
            SavedEntry {
                address: module.clone(),
                search_type: SearchType::Int,
            },
            SavedEntry {
                address: pointer.clone(),
                search_type: SearchType::Int,
            },
            absolute(123),
        ];
        let save = AutomaticSave::new(identity, path.clone(), table(entries), vec![2]);
        assert!(save.targets.is_empty());
        save.finish().unwrap();
        let saved: CheatTable = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(saved.searches[0].entries[0].address, module);
        assert_eq!(saved.searches[0].entries[1].address, pointer);
        assert_eq!(saved.searches[0].entries[2].address, absolute(123).address);
    }
}
