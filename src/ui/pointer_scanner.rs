use std::sync::atomic::Ordering;

use i18n_embed_fl::fl;

use crate::{
    App, ModuleCatalog, PointerWidth, SearchMode, SearchResult,
    pointer_scan::{CandidateSet, ProcessIdentity, ScanJob, ScanOptions},
};

#[derive(Clone)]
pub struct ScanTarget {
    pub identity: ProcessIdentity,
    pub result: SearchResult,
}

pub struct PointerScanner {
    pub open: bool,
    pub target: Option<ScanTarget>,
    pub options: ScanOptions,
    pub offset_text: String,
    pub candidates: Option<CandidateSet>,
    pub job: Option<ScanJob>,
    pub error: Option<String>,
    pub status: String,
    pub limited: bool,
    pub failed_reads: usize,
}

impl Default for PointerScanner {
    fn default() -> Self {
        Self {
            open: false,
            target: None,
            options: ScanOptions::default(),
            offset_text: "1000".into(),
            candidates: None,
            job: None,
            error: None,
            status: String::new(),
            limited: false,
            failed_reads: 0,
        }
    }
}

impl PointerScanner {
    pub fn cancel(&mut self) {
        if self.job.take().is_some() {
            self.status = fl!(crate::LANGUAGE_LOADER, "pointer-scan-cancelled");
        }
    }

    pub fn target_is_current(&self, app: &App) -> bool {
        self.target.as_ref().is_some_and(|target| {
            app.state.pid != 0
                && target.identity.pid == app.state.pid
                && (app.state.attached_start_time == 0 || target.identity.start_time == app.state.attached_start_time)
        })
    }
}

impl App {
    pub fn begin_pointer_scan(&mut self, result_index: usize) {
        if !self.enable_persistence {
            return;
        }
        if self.is_saving_cheat_table() {
            return;
        }
        let search = &self.state.searches[self.state.current_search];
        if search.searching != SearchMode::None {
            return;
        }
        let Some(result) = search.collect_results().get(result_index).copied() else {
            return;
        };
        self.pointer_scanner.cancel();
        self.pointer_scanner.open = true;
        self.pointer_scanner.target = None;
        self.pointer_scanner.error = None;
        self.clear_result_interaction();
        let target = (|| {
            if result.search_type.fixed_byte_length().is_none() {
                return Err(fl!(crate::LANGUAGE_LOADER, "pointer-scan-numeric-only"));
            }
            self.state.validate_result_address(self.state.current_search, &result)?;
            Ok(ScanTarget {
                identity: ProcessIdentity::capture(self.state.pid)?,
                result,
            })
        })();
        match target {
            Ok(target) => self.pointer_scanner.target = Some(target),
            Err(error) => self.pointer_scanner.error = Some(error),
        }
    }

    pub fn start_pointer_scan(&mut self, filter: bool) -> Result<(), String> {
        if !self.enable_persistence {
            return Err(fl!(crate::LANGUAGE_LOADER, "persistence-disabled"));
        }
        if self.is_saving_cheat_table() {
            return Err(fl!(crate::LANGUAGE_LOADER, "auto-save-busy"));
        }
        if self.pointer_scanner.job.is_some() {
            return Ok(());
        }
        if !self.pointer_scanner.target_is_current(self) {
            return Err(fl!(crate::LANGUAGE_LOADER, "pointer-scan-no-target"));
        }
        let target = self.pointer_scanner.target.as_ref().unwrap();
        if ProcessIdentity::capture(self.state.pid)? != target.identity {
            return Err(fl!(crate::LANGUAGE_LOADER, "pointer-scan-wrong-target"));
        }
        let previous = if filter {
            Some(
                self.pointer_scanner
                    .candidates
                    .clone()
                    .ok_or_else(|| fl!(crate::LANGUAGE_LOADER, "pointer-scan-file-error"))?,
            )
        } else {
            None
        };
        self.pointer_scanner.options.max_offset = crate::address::parse_hex(&self.pointer_scanner.offset_text)?;
        self.pointer_scanner.job = Some(ScanJob::start(
            target.identity.clone(),
            target.result,
            self.pointer_scanner.options.clone(),
            previous,
        )?);
        self.pointer_scanner.error = None;
        self.pointer_scanner.status.clear();
        Ok(())
    }

    pub fn poll_pointer_scan(&mut self) {
        if !self.enable_persistence {
            self.pointer_scanner.cancel();
            return;
        }
        if self.pointer_scanner.job.is_none() {
            return;
        }
        if !self.pointer_scanner.target_is_current(self) || !self.state.is_process_running() {
            self.pointer_scanner.cancel();
            self.pointer_scanner.target = None;
            return;
        }
        if let Some(result) = self.pointer_scanner.job.as_ref().and_then(ScanJob::poll) {
            self.pointer_scanner.job = None;
            match result {
                Ok(report) => {
                    let target = self.pointer_scanner.target.as_ref().unwrap();
                    self.pointer_scanner.candidates = Some(CandidateSet {
                        version: 1,
                        executable: target.identity.executable.clone(),
                        value_type: target.result.search_type,
                        options: self.pointer_scanner.options.clone(),
                        candidates: report.candidates,
                        limited: report.limited,
                    });
                    self.pointer_scanner.limited = report.limited;
                    self.pointer_scanner.failed_reads = report.failed_reads;
                    self.pointer_scanner.status.clear();
                }
                Err(error) => self.pointer_scanner.error = Some(error),
            }
        }
    }

    /// Freshly validate the candidate against the selected current target.
    /// Add a dedicated tab, never rewrite the old scan or enable any freeze.
    pub fn adopt_pointer_candidate(&mut self, index: usize) -> Result<(), String> {
        if !self.enable_persistence {
            return Err(fl!(crate::LANGUAGE_LOADER, "persistence-disabled"));
        }
        if self.pointer_scanner.job.is_some() || !self.pointer_scanner.target_is_current(self) {
            return Err(fl!(crate::LANGUAGE_LOADER, "pointer-scan-no-target"));
        }
        let target = self.pointer_scanner.target.as_ref().unwrap().clone();
        let set = self
            .pointer_scanner
            .candidates
            .as_ref()
            .ok_or_else(|| fl!(crate::LANGUAGE_LOADER, "pointer-scan-file-error"))?;
        if set.executable != target.identity.executable
            || set.value_type != target.result.search_type
            || ProcessIdentity::capture(self.state.pid)? != target.identity
        {
            return Err(fl!(crate::LANGUAGE_LOADER, "pointer-scan-wrong-target"));
        }
        let spec = set
            .candidates
            .get(index)
            .ok_or_else(|| fl!(crate::LANGUAGE_LOADER, "pointer-scan-file-error"))?
            .clone();
        let modules = ModuleCatalog::for_process(self.state.pid)?;
        if modules.resolve_for_process(&spec, target.result.search_type.fixed_byte_length().unwrap(), self.state.pid)? != target.result.addr {
            return Err(fl!(crate::LANGUAGE_LOADER, "address-changed"));
        }
        self.new_search();
        let search = &mut self.state.searches[self.state.current_search];
        search.description = fl!(crate::LANGUAGE_LOADER, "pointer-scan-tab");
        search.search_type = target.result.search_type;
        search.set_cached_results(vec![target.result]);
        search.address_overrides.insert((target.result.addr, target.result.search_type), spec);
        search.search_complete.store(true, Ordering::Release);
        self.pointer_scanner.open = false;
        Ok(())
    }
}

pub fn show(app: &mut App, ctx: &egui::Context) {
    if !app.enable_persistence {
        return;
    }
    app.poll_pointer_scan();
    if !app.pointer_scanner.open {
        return;
    }
    let current = app.pointer_scanner.target_is_current(app);
    let path = crate::default_cheat_table_path(&app.state.process_name).with_extension("pointers.toml");
    let scanner = &mut app.pointer_scanner;
    let mut open = true;
    let mut start = None;
    let mut adopt = None;
    let mut close = false;
    egui::Window::new(fl!(crate::LANGUAGE_LOADER, "pointer-scan-title"))
        .id(egui::Id::new("pointer_scanner"))
        .open(&mut open)
        .collapsible(false)
        .default_width(540.0)
        .max_height((ctx.content_rect().height() - 64.0).max(180.0))
        .vscroll(true)
        .show(ctx, |ui| {
            ui.set_max_width((ctx.content_rect().width() - 64.0).clamp(220.0, 540.0));
            let idle = scanner.job.is_none();
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(idle && current, egui::Button::new(fl!(crate::LANGUAGE_LOADER, "pointer-scan-new")))
                    .clicked()
                {
                    start = Some(false);
                }
                if ui
                    .add_enabled(
                        idle && current && scanner.candidates.is_some(),
                        egui::Button::new(fl!(crate::LANGUAGE_LOADER, "pointer-scan-filter")),
                    )
                    .clicked()
                {
                    start = Some(true);
                }
                if !idle && ui.button(fl!(crate::LANGUAGE_LOADER, "pointer-scan-cancel")).clicked() {
                    scanner.cancel();
                }
                if ui.button(fl!(crate::LANGUAGE_LOADER, "pointer-scan-close")).clicked() {
                    close = true;
                }
            });
            if let Some(job) = &scanner.job {
                ui.spinner();
                let progress = &job.progress;
                ui.label(fl!(
                    crate::LANGUAGE_LOADER,
                    "pointer-scan-progress",
                    mib = (progress.bytes.load(Ordering::Relaxed) / (1024 * 1024)).to_string(),
                    pointers = progress.pointers.load(Ordering::Relaxed).to_string(),
                    depth = progress.depth.load(Ordering::Relaxed).to_string(),
                    count = progress.candidates.load(Ordering::Relaxed).to_string()
                ));
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
            if current {
                let target = scanner.target.as_ref().unwrap();
                ui.label(fl!(
                    crate::LANGUAGE_LOADER,
                    "pointer-scan-target",
                    address = format!("0x{:X}", target.result.addr),
                    kind = format!("{:?}", target.result.search_type)
                ))
                .on_hover_text(&target.identity.executable);
            } else {
                ui.label(fl!(crate::LANGUAGE_LOADER, "pointer-scan-no-target"));
            }
            if let Some(error) = &scanner.error {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
            if !scanner.status.is_empty() {
                ui.label(&scanner.status);
            }
            ui.label(egui::RichText::new(fl!(crate::LANGUAGE_LOADER, "pointer-scan-help")).small().weak());
            ui.add_enabled_ui(idle, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(fl!(crate::LANGUAGE_LOADER, "pointer-width"));
                    ui.radio_value(&mut scanner.options.pointer_width, PointerWidth::Bits32, "32 bit");
                    ui.radio_value(&mut scanner.options.pointer_width, PointerWidth::Bits64, "64 bit");
                });
                ui.horizontal_wrapped(|ui| {
                    ui.label(fl!(crate::LANGUAGE_LOADER, "pointer-scan-depth"));
                    ui.add(egui::DragValue::new(&mut scanner.options.depth).range(1..=6));
                    ui.label(fl!(crate::LANGUAGE_LOADER, "pointer-scan-offset"));
                    ui.add(egui::TextEdit::singleline(&mut scanner.offset_text).desired_width(90.0));
                });
                ui.horizontal_wrapped(|ui| {
                    ui.label(fl!(crate::LANGUAGE_LOADER, "pointer-scan-budget"));
                    ui.add(egui::DragValue::new(&mut scanner.options.scan_mib).range(1..=1024));
                    ui.label(fl!(crate::LANGUAGE_LOADER, "pointer-scan-limit"));
                    ui.add(egui::DragValue::new(&mut scanner.options.max_candidates).range(1..=512));
                });
                ui.checkbox(&mut scanner.options.aligned_only, fl!(crate::LANGUAGE_LOADER, "pointer-scan-aligned"));
                ui.checkbox(&mut scanner.options.include_readonly, fl!(crate::LANGUAGE_LOADER, "pointer-scan-readonly"));
                ui.checkbox(&mut scanner.options.negative_offsets, fl!(crate::LANGUAGE_LOADER, "pointer-scan-negative"));
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .add_enabled(
                            scanner.candidates.is_some(),
                            egui::Button::new(fl!(crate::LANGUAGE_LOADER, "pointer-scan-save")),
                        )
                        .on_hover_text(path.display().to_string())
                        .clicked()
                    {
                        match scanner.candidates.as_ref().unwrap().save(&path) {
                            Ok(()) => {
                                scanner.status = fl!(crate::LANGUAGE_LOADER, "pointer-scan-saved");
                                scanner.error = None;
                            }
                            Err(error) => scanner.error = Some(error),
                        }
                    }
                    if ui
                        .button(fl!(crate::LANGUAGE_LOADER, "pointer-scan-load"))
                        .on_hover_text(path.display().to_string())
                        .clicked()
                    {
                        match CandidateSet::load(&path) {
                            Ok(set) => {
                                scanner.options = set.options.clone();
                                scanner.offset_text = format!("{:X}", set.options.max_offset);
                                scanner.limited = set.limited;
                                scanner.candidates = Some(set);
                                scanner.failed_reads = 0;
                                scanner.status = fl!(crate::LANGUAGE_LOADER, "pointer-scan-loaded");
                                scanner.error = None;
                            }
                            Err(error) => scanner.error = Some(error),
                        }
                    }
                });
            });
            ui.collapsing(fl!(crate::LANGUAGE_LOADER, "pointer-scan-limit"), |ui| {
                ui.label(fl!(crate::LANGUAGE_LOADER, "pointer-scan-limits"));
            });
            if let Some(set) = &scanner.candidates {
                ui.separator();
                ui.label(fl!(
                    crate::LANGUAGE_LOADER,
                    "pointer-scan-result",
                    count = set.candidates.len().to_string(),
                    failed = scanner.failed_reads.to_string()
                ));
                if scanner.limited {
                    ui.colored_label(ui.visuals().warn_fg_color, fl!(crate::LANGUAGE_LOADER, "pointer-scan-truncated"));
                }
                ui.label(egui::RichText::new(fl!(crate::LANGUAGE_LOADER, "pointer-scan-unverified")).small().weak());
                egui::ScrollArea::vertical()
                    .id_salt("pointer_candidates")
                    .max_height(220.0)
                    .show_rows(ui, 56.0, set.candidates.len(), |ui, range| {
                        for i in range {
                            ui.push_id(i, |ui| {
                                ui.add(egui::Label::new(egui::RichText::new(set.candidates[i].label()).monospace()).truncate())
                                    .on_hover_text(format!("{:?}", set.candidates[i]));
                                if ui
                                    .add_enabled(idle && current, egui::Button::new(fl!(crate::LANGUAGE_LOADER, "pointer-scan-adopt")))
                                    .clicked()
                                {
                                    adopt = Some(i);
                                }
                            });
                        }
                    });
            }
        });
    if !open || close || ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
        app.pointer_scanner.cancel();
        app.pointer_scanner.open = false;
    } else if let Some(filter) = start {
        if let Err(error) = app.start_pointer_scan(filter) {
            app.pointer_scanner.error = Some(error);
        }
    } else if let Some(index) = adopt
        && let Err(error) = app.adopt_pointer_candidate(index)
    {
        app.pointer_scanner.error = Some(error);
    }
}
