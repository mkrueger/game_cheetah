use i18n_embed_fl::fl;

use crate::{AddressSpec, App, ModuleCatalog, PendingAddress, PointerWidth, SearchMode, SearchResult};

#[derive(Clone, Copy)]
pub enum AddressTarget {
    Result(SearchResult),
    Pending(usize),
}

pub struct AddressEditor {
    pub search_index: usize,
    pub target: AddressTarget,
    pub relative: bool,
    pub pointer: bool,
    pub offsets: String,
    pub pointer_width: PointerWidth,
    pub module: String,
    pub value: String,
    pub modules: ModuleCatalog,
    pub error: Option<String>,
}

impl AddressEditor {
    pub fn definition(&self) -> AddressSpec {
        if self.pointer {
            AddressSpec::Pointer {
                module: self.module.clone(),
                offset: self.value.clone(),
                offsets: self
                    .offsets
                    .split(|c: char| c == ',' || c == ';' || c.is_whitespace())
                    .filter(|part| !part.is_empty())
                    .map(str::to_owned)
                    .collect(),
                pointer_width: self.pointer_width,
            }
        } else if self.relative {
            AddressSpec::Module {
                module: self.module.clone(),
                offset: self.value.clone(),
            }
        } else {
            AddressSpec::Absolute(self.value.clone())
        }
    }
}

impl App {
    pub fn begin_address_edit(&mut self, index: usize) {
        let Some(result) = self.state.searches[self.state.current_search].collect_results().get(index).copied() else {
            return;
        };
        self.open_address_definition(AddressTarget::Result(result));
    }

    pub fn begin_pending_address_edit(&mut self, index: usize) {
        self.open_address_definition(AddressTarget::Pending(index));
    }

    fn open_address_definition(&mut self, target: AddressTarget) {
        if !self.enable_persistence {
            return;
        }
        let search = &self.state.searches[self.state.current_search];
        if search.searching != SearchMode::None {
            return;
        }
        let modules = ModuleCatalog::for_process(self.state.pid);
        let error = modules.as_ref().err().cloned();
        let modules = modules.unwrap_or_default();
        let spec = match target {
            AddressTarget::Result(result) => search
                .address_overrides
                .get(&(result.addr, result.search_type))
                .cloned()
                .unwrap_or_else(|| modules.suggest(&result)),
            AddressTarget::Pending(index) => {
                let Some(entry) = search.unresolved_addresses.get(index) else { return };
                entry.address.clone()
            }
        };
        let mut pointer = false;
        let mut offsets_text = "0".to_owned();
        let mut width = PointerWidth::default();
        let (relative, module, value) = match spec {
            AddressSpec::Absolute(address) => (false, String::new(), address),
            AddressSpec::Module { module, offset } => (true, module, offset),
            AddressSpec::Pointer {
                module,
                offset,
                offsets,
                pointer_width,
            } => {
                pointer = true;
                offsets_text = offsets.join(", ");
                width = pointer_width;
                (true, module, offset)
            }
        };
        self.editing_result = None;
        self.address_editor = Some(AddressEditor {
            search_index: self.state.current_search,
            target,
            relative,
            pointer,
            offsets: offsets_text,
            pointer_width: width,
            module,
            value,
            modules,
            error,
        });
    }

    /// Editing an address changes metadata and releases the old freeze. It
    /// never writes memory; valid but unavailable module definitions stay pending.
    pub fn apply_address_definition(&mut self, spec: AddressSpec) -> Result<(), String> {
        if !self.enable_persistence {
            return Err(fl!(crate::LANGUAGE_LOADER, "persistence-disabled"));
        }
        spec.validate()?;
        let Some(editor) = &self.address_editor else { return Ok(()) };
        let index = editor.search_index;
        let target = editor.target;
        let Some(search) = self.state.searches.get(index) else { return Ok(()) };
        if search.searching != SearchMode::None {
            return Ok(());
        }
        let old = search.collect_results();
        let ty = match target {
            AddressTarget::Result(result) => result.search_type,
            AddressTarget::Pending(i) => search.unresolved_addresses[i].search_type,
        };
        let modules = ModuleCatalog::for_process(self.state.pid)?;
        let resolved = if self.state.pid == 0 {
            Err(fl!(crate::LANGUAGE_LOADER, "address-no-process"))
        } else {
            modules.resolve_for_process(&spec, ty.fixed_byte_length().unwrap_or(1), self.state.pid)
        };
        let mut results: Vec<_> = old
            .iter()
            .copied()
            .filter(|result| !matches!(target, AddressTarget::Result(old) if old.addr == result.addr && old.search_type == result.search_type))
            .collect();
        if let Ok(addr) = resolved
            && results.iter().any(|result| result.addr == addr && result.search_type == ty)
        {
            return Err(fl!(crate::LANGUAGE_LOADER, "address-duplicate"));
        }
        self.state.remove_freezes(index);
        let search = &mut self.state.searches[index];
        search.push_undo_state(old);
        match target {
            AddressTarget::Result(result) => {
                search.address_overrides.remove(&(result.addr, result.search_type));
            }
            AddressTarget::Pending(i) => {
                search.unresolved_addresses.remove(i);
            }
        }
        match resolved {
            Ok(addr) => {
                results.push(SearchResult::new(addr, ty));
                search.address_overrides.insert((addr, ty), spec);
            }
            Err(reason) => search.unresolved_addresses.push(PendingAddress {
                address: spec,
                search_type: ty,
                reason,
            }),
        }
        search.set_cached_results(results);
        search.clear_memory_snapshot();
        search.clear_previous_unknown_values();
        search.unknown_comparison = None;
        self.clear_result_interaction();
        self.clear_change_tracker();
        Ok(())
    }

    pub fn retry_module_addresses(&mut self) {
        if !self.enable_persistence {
            return;
        }
        match ModuleCatalog::for_process(self.state.pid) {
            Ok(modules) => {
                if self.state.resolve_table_addresses(&modules, false) {
                    self.clear_result_interaction();
                    self.clear_change_tracker();
                }
            }
            Err(error) => self.state.push_error(error),
        }
    }
}

pub fn show(app: &mut App, ctx: &egui::Context) {
    if !app.enable_persistence {
        return;
    }
    let Some(mut editor) = app.address_editor.take() else { return };
    let mut open = true;
    let mut apply = None;
    let mut cancel = false;
    egui::Window::new(fl!(crate::LANGUAGE_LOADER, "address-editor-title"))
        .id(egui::Id::new("address_definition_editor"))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_width(440.0)
        .max_height((ctx.content_rect().height() - 64.0).max(220.0))
        .vscroll(true)
        .show(ctx, |ui| {
            ui.set_max_width((ctx.content_rect().width() - 64.0).clamp(220.0, 440.0));
            let ty = match editor.target {
                AddressTarget::Result(result) => result.search_type,
                AddressTarget::Pending(i) => app.state.searches[editor.search_index].unresolved_addresses[i].search_type,
            };
            let was_relative = editor.relative;
            let old_spec = editor.definition();
            let mut mode = if editor.pointer { 2 } else { i32::from(editor.relative) };
            ui.horizontal(|ui| {
                ui.radio_value(&mut mode, 0, fl!(crate::LANGUAGE_LOADER, "address-absolute"));
                ui.radio_value(&mut mode, 1, fl!(crate::LANGUAGE_LOADER, "address-relative"));
                ui.radio_value(&mut mode, 2, fl!(crate::LANGUAGE_LOADER, "pointer-mode"));
            });
            editor.relative = mode != 0;
            editor.pointer = mode == 2;
            if was_relative != editor.relative {
                if editor.relative {
                    let suggestion = crate::address::parse_hex(&editor.value)
                        .ok()
                        .map(|addr| editor.modules.suggest(&SearchResult::new(addr, ty)));
                    if let Some(AddressSpec::Module { module, offset }) = suggestion {
                        editor.module = module;
                        editor.value = offset;
                    } else {
                        editor.module.clear();
                        editor.value.clear();
                    }
                } else {
                    editor.value = editor
                        .modules
                        .resolve_for_process(&old_spec, ty.fixed_byte_length().unwrap_or(1), app.state.pid)
                        .map(|addr| format!("0x{addr:X}"))
                        .unwrap_or_default();
                }
                editor.error = None;
            }
            if editor.relative {
                ui.label(fl!(crate::LANGUAGE_LOADER, "address-module"));
                egui::ComboBox::from_id_salt("address_module_picker")
                    .selected_text(&editor.module)
                    .width(300.0)
                    .show_ui(ui, |ui| {
                        for module in &editor.modules.modules {
                            ui.selectable_value(&mut editor.module, module.path.clone(), &module.path);
                        }
                    });
                ui.add(egui::TextEdit::singleline(&mut editor.module).desired_width(f32::INFINITY));
            }
            ui.label(if editor.relative {
                fl!(crate::LANGUAGE_LOADER, "address-offset")
            } else {
                fl!(crate::LANGUAGE_LOADER, "address-value")
            });
            ui.add(
                egui::TextEdit::singleline(&mut editor.value)
                    .font(egui::TextStyle::Monospace)
                    .desired_width(f32::INFINITY),
            );
            if editor.pointer {
                ui.label(fl!(crate::LANGUAGE_LOADER, "pointer-width"));
                ui.horizontal(|ui| {
                    ui.radio_value(&mut editor.pointer_width, PointerWidth::Bits32, "32 bit");
                    ui.radio_value(&mut editor.pointer_width, PointerWidth::Bits64, "64 bit");
                });
                ui.label(fl!(crate::LANGUAGE_LOADER, "pointer-offsets"));
                ui.add(
                    egui::TextEdit::singleline(&mut editor.offsets)
                        .font(egui::TextStyle::Monospace)
                        .desired_width(f32::INFINITY),
                );
                ui.label(egui::RichText::new(fl!(crate::LANGUAGE_LOADER, "pointer-help")).small().weak());
                if ui.button(fl!(crate::LANGUAGE_LOADER, "pointer-refresh")).clicked() {
                    match ModuleCatalog::for_process(app.state.pid) {
                        Ok(modules) => {
                            editor.modules = modules;
                            editor.error = None;
                        }
                        Err(error) => {
                            editor.modules = ModuleCatalog::default();
                            editor.error = Some(error);
                        }
                    }
                }
            }
            let spec = editor.definition();
            match editor.modules.trace_for_process(&spec, ty.fixed_byte_length().unwrap_or(1), app.state.pid) {
                Ok(resolved) => {
                    ui.label(fl!(crate::LANGUAGE_LOADER, "address-resolved", address = format!("0x{:X}", resolved.address)));
                    if !resolved.steps.is_empty() {
                        egui::CollapsingHeader::new(fl!(crate::LANGUAGE_LOADER, "pointer-trace")).show(ui, |ui| {
                            for (index, step) in resolved.steps.iter().enumerate() {
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{}: [0x{:X}] = 0x{:X}; {} => 0x{:X}",
                                        index + 1,
                                        step.location,
                                        step.pointer,
                                        step.offset,
                                        step.target
                                    ))
                                    .monospace()
                                    .small(),
                                );
                            }
                        });
                    }
                }
                Err(error) => {
                    ui.colored_label(ui.visuals().warn_fg_color, error);
                }
            }
            ui.label(
                egui::RichText::new(if editor.pointer {
                    fl!(crate::LANGUAGE_LOADER, "pointer-safety")
                } else if editor.relative {
                    fl!(crate::LANGUAGE_LOADER, "address-relative-warning")
                } else {
                    fl!(crate::LANGUAGE_LOADER, "address-absolute-warning")
                })
                .small()
                .weak(),
            );
            if let Some(error) = &editor.error {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(spec.validate().is_ok(), egui::Button::new(fl!(crate::LANGUAGE_LOADER, "address-apply")))
                    .clicked()
                {
                    apply = Some(spec);
                }
                if ui.button(fl!(crate::LANGUAGE_LOADER, "address-cancel")).clicked() {
                    cancel = true;
                }
            });
        });
    if open && !cancel && !ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
        app.address_editor = Some(editor);
        if let Some(spec) = apply
            && let Err(error) = app.apply_address_definition(spec)
            && let Some(editor) = &mut app.address_editor
        {
            editor.error = Some(error);
        }
    }
}

pub fn unresolved_panel(app: &mut App, ui: &mut egui::Ui) {
    let index = app.state.current_search;
    let count = app.state.searches[index].unresolved_addresses.len();
    if count == 0 {
        return;
    }
    let idle = app.state.searches[index].searching == SearchMode::None;
    egui::CollapsingHeader::new(fl!(crate::LANGUAGE_LOADER, "address-unresolved", count = count.to_string()))
        .default_open(true)
        .show(ui, |ui| {
            if ui.add_enabled(idle, egui::Button::new(fl!(crate::LANGUAGE_LOADER, "address-retry"))).clicked() {
                app.retry_module_addresses();
            }
            let mut edit = None;
            let mut remove = None;
            let entries = &app.state.searches[index].unresolved_addresses;
            egui::ScrollArea::vertical()
                .id_salt("unresolved_addresses")
                .max_height(120.0)
                .show_rows(ui, 50.0, entries.len(), |ui, range| {
                    for i in range {
                        let entry = &entries[i];
                        ui.horizontal(|ui| {
                            if ui.add_enabled(idle, egui::Button::new(fl!(crate::LANGUAGE_LOADER, "address-edit"))).clicked() {
                                edit = Some(i);
                            }
                            if ui.add_enabled(idle, egui::Button::new("×")).clicked() {
                                remove = Some(i);
                            }
                            ui.add(egui::Label::new(format!("{} ({:?})", entry.address.label(), entry.search_type)).truncate())
                                .on_hover_text(&entry.reason);
                        });
                        ui.add(egui::Label::new(egui::RichText::new(&entry.reason).small().color(ui.visuals().warn_fg_color)).truncate())
                            .on_hover_text(&entry.reason);
                    }
                });
            if let Some(i) = edit {
                app.begin_pending_address_edit(i);
            }
            if let Some(i) = remove {
                let search = &mut app.state.searches[index];
                search.push_undo_state(search.collect_results());
                search.unresolved_addresses.remove(i);
                app.address_editor = None;
            }
        });
}
