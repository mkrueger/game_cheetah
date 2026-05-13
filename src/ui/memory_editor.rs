//! Minimal hex/ASCII memory editor.
//!
//! The pre-port editor was ~1700 lines of retained-mode plumbing (custom
//! focus state, virtualized scrolling, inspector, undo/redo, fade animations).
//! This egui port keeps the essential viewer/editor behavior — load the region
//! containing an address, show a hex grid with ASCII column, click-to-edit a
//! byte, periodic refresh, and a jump-to-address bar. Inspector / undo can be
//! re-added incrementally; the underlying [`crate::state`] code that does the
//! actual memory reads/writes is unchanged.
use std::collections::HashMap;
use std::time::{Duration, Instant};

use i18n_embed_fl::fl;
use proc_maps::get_process_maps;
use process_memory::{PutAddress, TryIntoProcessHandle, copy_address};

use crate::{SearchType, ui::app::App};

pub const BYTES_PER_ROW: usize = 16;
pub const ROW_HEIGHT: f32 = 22.0;
const VISIBLE_ROWS_DEFAULT: usize = 32;
const CHANGE_FADE: Duration = Duration::from_millis(1500);

#[derive(Debug, Clone)]
struct Region {
    start: usize,
    size: usize,
    readable: bool,
    writable: bool,
    name: String,
}

#[derive(Default)]
pub struct MemoryEditor {
    regions: Vec<Region>,
    /// Origin address the editor was opened at (highlighted in the grid).
    origin_address: usize,
    /// First byte address currently shown in the grid view.
    view_address: usize,
    /// Number of bytes to load per refresh.
    view_bytes: usize,
    /// Most recent snapshot of the visible bytes. `None` for bytes that
    /// failed to read.
    snapshot: Vec<Option<u8>>,
    /// Last observed byte and timestamp of last change at each address.
    /// Drives the brief "changed" highlight.
    change_tracker: HashMap<usize, (u8, Instant)>,

    /// Text content of the "Go to address" input box.
    address_input: String,

    /// Last selected data type used by the inspector / SearchType label.
    data_type: Option<SearchType>,

    /// Pending edit: (offset within view, partial hex text, original byte).
    editing: Option<(usize, String)>,
}

impl MemoryEditor {
    pub fn initialize(
        &mut self,
        pid: process_memory::Pid,
        address: usize,
        search_type: SearchType,
    ) -> Result<(), String> {
        let maps = get_process_maps(pid).map_err(|e| {
            fl!(
                crate::LANGUAGE_LOADER,
                "memory-editor-error-read-map",
                pid = pid,
                error = e.to_string()
            )
        })?;
        self.regions.clear();
        for m in maps {
            let start = m.start();
            let size = m.size();
            if size == 0 {
                continue;
            }
            self.regions.push(Region {
                start,
                size,
                readable: m.is_read(),
                writable: m.is_write(),
                name: m
                    .filename()
                    .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
                    .unwrap_or_else(|| fl!(crate::LANGUAGE_LOADER, "memory-editor-region-anonymous")),
            });
        }
        self.regions.sort_by_key(|r| r.start);

        if self.regions.is_empty() {
            return Err(fl!(
                crate::LANGUAGE_LOADER,
                "memory-editor-error-no-regions",
                pid = pid
            ));
        }

        self.origin_address = address;
        // Center the address on the grid.
        let bytes_visible = BYTES_PER_ROW * VISIBLE_ROWS_DEFAULT;
        let half = bytes_visible / 2;
        self.view_address = address.saturating_sub(half) / BYTES_PER_ROW * BYTES_PER_ROW;
        self.view_bytes = bytes_visible;
        self.address_input = format!("0x{:X}", address);
        self.data_type = Some(search_type);
        self.snapshot.clear();
        self.change_tracker.clear();
        self.editing = None;
        Ok(())
    }

    pub fn reset_change_tracker(&mut self) {
        self.change_tracker.clear();
    }

    pub fn data_type(&self) -> Option<SearchType> {
        self.data_type
    }

    /// Read the visible window and update [`snapshot`] + [`change_tracker`].
    pub fn tick(&mut self, pid: process_memory::Pid) {
        let Ok(handle) = pid.try_into_process_handle() else {
            return;
        };
        let mut new_snapshot: Vec<Option<u8>> = Vec::with_capacity(self.view_bytes);
        let now = Instant::now();
        // Walk regions overlapping the view, reading contiguous segments.
        let view_end = self.view_address.saturating_add(self.view_bytes);
        let mut cursor = self.view_address;
        while cursor < view_end {
            // Find the region containing `cursor`, if any.
            let region = self
                .regions
                .iter()
                .find(|r| cursor >= r.start && cursor < r.start + r.size);
            let next_region_start = self
                .regions
                .iter()
                .filter(|r| r.start > cursor)
                .map(|r| r.start)
                .min()
                .unwrap_or(view_end);
            let segment_end = if let Some(r) = region {
                view_end.min(r.start + r.size)
            } else {
                view_end.min(next_region_start)
            };
            let len = segment_end - cursor;
            if let Some(r) = region
                && r.readable
            {
                match copy_address(cursor, len, &handle) {
                    Ok(buf) => {
                        for (i, b) in buf.iter().enumerate() {
                            let addr = cursor + i;
                            let prev = self.change_tracker.get(&addr).map(|(b, _)| *b);
                            if prev.is_some_and(|p| p != *b) {
                                self.change_tracker.insert(addr, (*b, now));
                            } else if prev.is_none() {
                                self.change_tracker.insert(addr, (*b, now - CHANGE_FADE));
                            } else {
                                // unchanged
                            }
                            new_snapshot.push(Some(*b));
                        }
                    }
                    Err(_) => {
                        for _ in 0..len {
                            new_snapshot.push(None);
                        }
                    }
                }
            } else {
                for _ in 0..len {
                    new_snapshot.push(None);
                }
            }
            cursor = segment_end;
        }
        self.snapshot = new_snapshot;

        // Drop expired highlight timestamps so the map doesn't grow forever.
        self.change_tracker
            .retain(|_, (_, t)| t.elapsed() < Duration::from_secs(60));
    }

    fn write_byte(&mut self, pid: process_memory::Pid, offset: usize, byte: u8) -> Result<(), String> {
        let handle = pid.try_into_process_handle().map_err(|e| {
            fl!(
                crate::LANGUAGE_LOADER,
                "memory-editor-error-attach",
                error = e.to_string()
            )
        })?;
        let addr = self.view_address + offset;
        handle.put_address(addr, &[byte]).map_err(|e| {
            fl!(
                crate::LANGUAGE_LOADER,
                "memory-editor-error-write-address",
                address = format!("{addr:X}"),
                error = e.to_string()
            )
        })?;
        Ok(())
    }

    fn region_of(&self, address: usize) -> Option<&Region> {
        self.regions
            .iter()
            .find(|r| address >= r.start && address < r.start + r.size)
    }

    fn jump_to_address(&mut self) {
        let raw = self.address_input.trim();
        let parsed = if let Some(rest) = raw.strip_prefix("0x").or_else(|| raw.strip_prefix("0X")) {
            usize::from_str_radix(rest, 16).ok()
        } else if raw.chars().all(|c| c.is_ascii_hexdigit()) {
            usize::from_str_radix(raw, 16).ok()
        } else {
            raw.parse::<usize>().ok()
        };
        if let Some(addr) = parsed {
            self.origin_address = addr;
            let half = self.view_bytes / 2;
            self.view_address = addr.saturating_sub(half) / BYTES_PER_ROW * BYTES_PER_ROW;
        }
    }
}

pub fn view_memory_editor(app: &mut App, ui: &mut egui::Ui) {
    // Header
    egui::Panel::top("memory_editor_top").show_inside(ui, |ui| {
        ui.horizontal(|ui| {
            ui.heading(fl!(crate::LANGUAGE_LOADER, "memory-editor-title"));
            ui.label(
                fl!(
                    crate::LANGUAGE_LOADER,
                    "memory-editor-pid",
                    pid = app.state.pid
                )
                .chars()
                .filter(|c| c.is_ascii())
                .collect::<String>(),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(fl!(crate::LANGUAGE_LOADER, "close-button")).clicked() {
                    app.close_memory_editor();
                }
            });
        });
        ui.horizontal(|ui| {
            ui.label(fl!(crate::LANGUAGE_LOADER, "memory-editor-address-label"));
            let response = ui.add(
                egui::TextEdit::singleline(&mut app.memory_editor.address_input)
                    .hint_text(fl!(crate::LANGUAGE_LOADER, "memory-editor-address-hint"))
                    .desired_width(200.0)
                    .font(egui::TextStyle::Monospace),
            );
            if (response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                || ui.button(fl!(crate::LANGUAGE_LOADER, "memory-editor-go-button")).clicked()
            {
                app.memory_editor.jump_to_address();
            }

            // Region info for the currently focused address.
            let info = if let Some(r) = app.memory_editor.region_of(app.memory_editor.origin_address) {
                let access = match (r.readable, r.writable) {
                    (true, true) => "rw",
                    (true, false) => "r",
                    (false, true) => "w",
                    (false, false) => "—",
                };
                format!("{}  [{}]", r.name, access)
            } else {
                fl!(crate::LANGUAGE_LOADER, "memory-editor-region-unmapped")
            };
            ui.label(egui::RichText::new(info).weak());
        });
    });

    egui::CentralPanel::default().show_inside(ui, |ui| {
        // Buttons to scroll a row / page.
        ui.horizontal(|ui| {
            if ui.button("▲ Row").clicked() {
                app.memory_editor.view_address = app.memory_editor.view_address.saturating_sub(BYTES_PER_ROW);
            }
            if ui.button("▼ Row").clicked() {
                app.memory_editor.view_address = app.memory_editor.view_address.saturating_add(BYTES_PER_ROW);
            }
            if ui.button("⇞ Page").clicked() {
                app.memory_editor.view_address = app
                    .memory_editor
                    .view_address
                    .saturating_sub(BYTES_PER_ROW * VISIBLE_ROWS_DEFAULT);
            }
            if ui.button("⇟ Page").clicked() {
                app.memory_editor.view_address = app
                    .memory_editor
                    .view_address
                    .saturating_add(BYTES_PER_ROW * VISIBLE_ROWS_DEFAULT);
            }
        });

        // Snapshot view state for the grid rendering loop.
        let view_address = app.memory_editor.view_address;
        let origin = app.memory_editor.origin_address;
        let rows = app.memory_editor.view_bytes / BYTES_PER_ROW;
        let snapshot = app.memory_editor.snapshot.clone();

        let mut write_byte: Option<(usize, u8)> = None;

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Grid::new("memory_editor_grid")
                    .num_columns(BYTES_PER_ROW + 2)
                    .spacing(egui::vec2(4.0, 2.0))
                    .show(ui, |ui| {
                        for row in 0..rows {
                            let row_addr = view_address + row * BYTES_PER_ROW;
                            ui.monospace(format!("{:016X}", row_addr));

                            for col in 0..BYTES_PER_ROW {
                                let offset = row * BYTES_PER_ROW + col;
                                let addr = row_addr + col;
                                let byte = snapshot.get(offset).copied().unwrap_or(None);

                                let editing = matches!(app.memory_editor.editing, Some((o, _)) if o == offset);
                                if editing {
                                    if let Some((_, buf)) = app.memory_editor.editing.as_mut() {
                                        let response = ui.add(
                                            egui::TextEdit::singleline(buf)
                                                .desired_width(22.0)
                                                .font(egui::TextStyle::Monospace)
                                                .char_limit(2),
                                        );
                                        response.request_focus();
                                        if response.lost_focus() {
                                            if ui.input(|i| i.key_pressed(egui::Key::Enter))
                                                && let Ok(b) = u8::from_str_radix(buf, 16)
                                            {
                                                write_byte = Some((offset, b));
                                            }
                                            app.memory_editor.editing = None;
                                        }
                                    }
                                } else {
                                    let label = match byte {
                                        Some(b) => format!("{b:02X}"),
                                        None => "??".to_owned(),
                                    };
                                    let mut text = egui::RichText::new(label).monospace();
                                    if addr == origin {
                                        text = text.color(ui.visuals().selection.bg_fill).strong();
                                    }
                                    // Recent change → orange tint.
                                    if let Some((_, when)) = app.memory_editor.change_tracker.get(&addr)
                                        && when.elapsed() < CHANGE_FADE
                                    {
                                        text = text.color(egui::Color32::from_rgb(255, 180, 130));
                                    }
                                    let response = ui.add(
                                        egui::Label::new(text)
                                            .sense(egui::Sense::click()),
                                    );
                                    if response.clicked() && byte.is_some() {
                                        let initial = byte.map(|b| format!("{b:02X}")).unwrap_or_default();
                                        app.memory_editor.editing = Some((offset, initial));
                                    }
                                }
                            }

                            // ASCII column
                            let mut ascii = String::with_capacity(BYTES_PER_ROW);
                            for col in 0..BYTES_PER_ROW {
                                let offset = row * BYTES_PER_ROW + col;
                                match snapshot.get(offset).copied().unwrap_or(None) {
                                    Some(b) if (0x20..0x7f).contains(&b) => ascii.push(b as char),
                                    Some(_) => ascii.push('.'),
                                    None => ascii.push(' '),
                                }
                            }
                            ui.monospace(ascii);
                            ui.end_row();
                        }
                    });
            });

        if let Some((offset, byte)) = write_byte
            && let Err(err) = app.memory_editor.write_byte(app.state.pid as process_memory::Pid, offset, byte)
        {
            app.state.push_error(crate::AppError::memory_editor(err));
        }
    });
}
