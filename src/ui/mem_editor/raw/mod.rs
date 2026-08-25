//! # Egui Memory Editor
//!
//! Vendored from <https://github.com/Hirtol/egui_memory_editor> v0.2.14
//! (MIT/Apache-2.0). Minor adjustments for egui 0.34: `id_source` → `id_salt`.
//!
//! Provides a memory editor to be used with `egui`. Originally written for
//! emulator development; game-cheetah uses it as the hex/ASCII back-end for
//! the in-process memory editor view, with per-frame batched reads to limit
//! the number of `process_memory::copy_address` syscalls.
//!
//! Look at [`MemoryEditor`] to get started.
use std::collections::BTreeMap;
use std::ops::Range;

use egui::{Align2, Context, Rect, ScrollArea, Sense, Ui, Window, pos2, vec2};

use self::option_data::{BetweenFrameData, MemoryEditorOptions};

pub mod option_data;

// Vendored data-preview helper. Not yet wired into the UI, but kept compiled
// (and thus type-checked) so it can't silently rot.
#[allow(dead_code)]
mod utilities;

/// A memory address that should be read from/written to.
pub type Address = usize;

const BYTE_GROUP_SIZE: usize = 8;
const MIN_COLUMN_COUNT: usize = BYTE_GROUP_SIZE;
const MAX_COLUMN_COUNT: usize = 64;
const BYTE_GAP: f32 = 6.0;
const GRID_COLUMN_GAP: f32 = 15.0;
const ASCII_SEPARATOR_WIDTH: f32 = 8.0;
const SCROLLBAR_RESERVE: f32 = 18.0;
/// Padding added to the tallest glyph box to form the pitch of one row.
const ROW_GAP: f32 = 3.0;

/// Horizontal placement of one memory row. Rendering and the adaptive column
/// count share this, so the chosen column count always matches the width a row
/// really occupies.
#[derive(Clone, Copy)]
struct RowLayout {
    column_count: usize,
    show_ascii: bool,
    address_width: f32,
    hex_cell_width: f32,
    ascii_cell_width: f32,
}

impl RowLayout {
    fn group_width(&self) -> f32 {
        BYTE_GROUP_SIZE as f32 * self.hex_cell_width + (BYTE_GROUP_SIZE - 1) as f32 * BYTE_GAP
    }

    fn hex_x(&self, index: usize) -> f32 {
        let group = index / BYTE_GROUP_SIZE;
        let within_group = index % BYTE_GROUP_SIZE;
        self.address_width + GRID_COLUMN_GAP + group as f32 * (self.group_width() + GRID_COLUMN_GAP) + within_group as f32 * (self.hex_cell_width + BYTE_GAP)
    }

    fn hex_end(&self) -> f32 {
        let groups = self.column_count.div_ceil(BYTE_GROUP_SIZE);
        self.address_width + GRID_COLUMN_GAP + groups as f32 * self.group_width() + groups.saturating_sub(1) as f32 * GRID_COLUMN_GAP
    }

    fn separator_x(&self) -> f32 {
        self.hex_end() + GRID_COLUMN_GAP
    }

    fn ascii_x(&self, index: usize) -> f32 {
        self.separator_x() + ASCII_SEPARATOR_WIDTH + index as f32 * self.ascii_cell_width
    }

    fn total_width(&self) -> f32 {
        if self.show_ascii { self.ascii_x(self.column_count) } else { self.hex_end() }
    }
}

/// Fonts and metrics measured once per frame and reused by every row.
struct RowStyle {
    layout: RowLayout,
    hex_font: egui::FontId,
    address_font: egui::FontId,
    ascii_font: egui::FontId,
    hex_char_width: f32,
    text_height: f32,
}

fn adaptive_column_count(available_width: f32, address_width: f32, hex_cell_width: f32, ascii_cell_width: f32, show_ascii: bool) -> usize {
    let usable_width = (available_width - SCROLLBAR_RESERVE).max(0.0);
    let mut best = MIN_COLUMN_COUNT;

    for column_count in (MIN_COLUMN_COUNT..=MAX_COLUMN_COUNT).step_by(BYTE_GROUP_SIZE) {
        let layout = RowLayout {
            column_count,
            show_ascii,
            address_width,
            hex_cell_width,
            ascii_cell_width,
        };
        if layout.total_width() > usable_width {
            break;
        }
        best = column_count;
    }

    best
}

fn centered_top_line(target_line: usize, max_lines: usize, visible_rows: usize) -> usize {
    let visible_rows = visible_rows.max(1).min(max_lines.max(1));
    target_line.saturating_sub(visible_rows / 2).min(max_lines.saturating_sub(visible_rows))
}

/// The main struct for the editor window.
/// This should persist between frames as it keeps track of quite a bit of state.
#[derive(Clone)]
pub struct MemoryEditor {
    /// The name of the `egui` window, can be left blank.
    window_name: String,
    /// The collection of address ranges, the GUI will start at the lower bound and go up to the upper bound.
    pub(crate) address_ranges: BTreeMap<String, Range<Address>>,
    /// A collection of options relevant for the `MemoryEditor` window.
    pub options: MemoryEditorOptions,
    /// Data for layout between frames, rather hacky.
    frame_data: BetweenFrameData,
    /// The visible range of addresses from the last frame.
    visible_range: Range<Address>,
}

impl MemoryEditor {
    pub fn new() -> Self {
        MemoryEditor {
            window_name: "Memory Editor".to_string(),
            address_ranges: BTreeMap::new(),
            options: Default::default(),
            frame_data: Default::default(),
            visible_range: Default::default(),
        }
    }

    /// Returns the visible range of the last frame.
    pub fn visible_range(&self) -> &Range<Address> {
        &self.visible_range
    }

    /// Remove every previously-registered address range.
    pub fn clear_address_ranges(&mut self) {
        self.address_ranges.clear();
        self.options.selected_address_range.clear();
    }

    /// Currently selected address range name.
    pub fn selected_range_name(&self) -> &str {
        &self.options.selected_address_range
    }

    /// Force-select a range by name (no-op if it doesn't exist).
    pub fn select_range(&mut self, name: &str) {
        if self.address_ranges.contains_key(name) {
            self.options.selected_address_range = name.to_string();
        }
    }

    /// The address most recently highlighted by the user (right-click in
    /// the grid, the Goto bar, or a programmatic [`Self::goto_address`]).
    /// Useful for a data-inspector panel that needs to read bytes at the
    /// caret position.
    pub fn highlighted_address(&self) -> Option<Address> {
        self.frame_data.selected_highlight_address.or(self.frame_data.selected_edit_address)
    }

    /// Endianness configured in the data-preview section. Inspector UIs
    /// outside the vendored editor can use this to mirror the user's
    /// choice.
    pub fn endianness(&self) -> option_data::Endianness {
        self.options.data_preview.selected_endianness
    }

    /// Caret position in the hex grid, if any: `(address, on_low_nibble)`.
    pub fn caret(&self) -> Option<(Address, bool)> {
        self.frame_data.selected_edit_address.map(|a| (a, self.frame_data.selected_low_nibble))
    }

    /// Restore a previously captured caret position. The address is
    /// validated against the currently-selected range; an out-of-range
    /// address simply clears the selection.
    pub fn set_caret(&mut self, caret: Option<(Address, bool)>) {
        if let Some(range) = self.address_ranges.get(&self.options.selected_address_range).cloned() {
            match caret {
                Some((addr, on_low)) => {
                    self.frame_data.set_selected_edit_address(Some(addr), &range);
                    self.frame_data.selected_low_nibble = on_low;
                }
                None => {
                    self.frame_data.set_selected_edit_address(None, &range);
                }
            }
        }
    }

    pub fn window_ui_read_only<T: ?Sized>(&mut self, ctx: &Context, is_open: &mut bool, mem: &mut T, read_fn: impl FnMut(&mut T, Address) -> Option<u8>) {
        type DummyWriteFunction<T> = fn(&mut T, Address, u8);
        self.window_ui_impl(ctx, is_open, mem, read_fn, None::<DummyWriteFunction<T>>);
    }

    pub fn window_ui<T: ?Sized>(
        &mut self,
        ctx: &Context,
        is_open: &mut bool,
        mem: &mut T,
        read_fn: impl FnMut(&mut T, Address) -> Option<u8>,
        write_fn: impl FnMut(&mut T, Address, u8),
    ) {
        self.window_ui_impl(ctx, is_open, mem, read_fn, Some(write_fn));
    }

    fn window_ui_impl<T: ?Sized>(
        &mut self,
        ctx: &Context,
        is_open: &mut bool,
        mem: &mut T,
        read_fn: impl FnMut(&mut T, Address) -> Option<u8>,
        write_fn: Option<impl FnMut(&mut T, Address, u8)>,
    ) {
        Window::new(self.window_name.clone())
            .open(is_open)
            .hscroll(false)
            .vscroll(false)
            .resizable(true)
            .show(ctx, |ui| {
                self.shrink_window_ui(ui);
                type DummyHighlightFunction<T> = fn(&mut T, Address) -> f32;
                self.draw_editor_contents_impl(ui, mem, read_fn, write_fn, None::<DummyHighlightFunction<T>>);
            });
    }

    pub fn draw_editor_contents_read_only<T: ?Sized>(&mut self, ui: &mut Ui, mem: &mut T, read_fn: impl FnMut(&mut T, Address) -> Option<u8>) {
        type DummyWriteFunction<T> = fn(&mut T, Address, u8);
        type DummyHighlightFunction<T> = fn(&mut T, Address) -> f32;
        self.draw_editor_contents_impl(ui, mem, read_fn, None::<DummyWriteFunction<T>>, None::<DummyHighlightFunction<T>>);
    }

    pub fn draw_editor_contents<T: ?Sized>(
        &mut self,
        ui: &mut Ui,
        mem: &mut T,
        read_fn: impl FnMut(&mut T, Address) -> Option<u8>,
        write_fn: impl FnMut(&mut T, Address, u8),
    ) {
        type DummyHighlightFunction<T> = fn(&mut T, Address) -> f32;
        self.draw_editor_contents_impl(ui, mem, read_fn, Some(write_fn), None::<DummyHighlightFunction<T>>);
    }

    /// Same as [`Self::draw_editor_contents`] but also takes a closure
    /// that returns a per-byte fade intensity in `0.0..=1.0`. Bytes with
    /// a non-zero intensity get a translucent orange overlay so the user
    /// notices them blinking when the target process modifies them.
    pub fn draw_editor_contents_with_highlight<T: ?Sized>(
        &mut self,
        ui: &mut Ui,
        mem: &mut T,
        read_fn: impl FnMut(&mut T, Address) -> Option<u8>,
        write_fn: impl FnMut(&mut T, Address, u8),
        highlight_fn: impl FnMut(&mut T, Address) -> f32,
    ) {
        self.draw_editor_contents_impl(ui, mem, read_fn, Some(write_fn), Some(highlight_fn));
    }

    fn draw_editor_contents_impl<T: ?Sized>(
        &mut self,
        ui: &mut Ui,
        mem: &mut T,
        mut read_fn: impl FnMut(&mut T, Address) -> Option<u8>,
        mut write_fn: Option<impl FnMut(&mut T, Address, u8)>,
        mut highlight_fn: Option<impl FnMut(&mut T, Address) -> f32>,
    ) {
        assert!(
            !self.address_ranges.is_empty(),
            "At least one address range needs to be added to render the contents!"
        );

        // Keep the dense byte grid independent from the larger monospace
        // size used by text fields elsewhere in the application.
        let selected_address_range = self.options.selected_address_range.clone();
        let address_space = self.address_ranges.get(&selected_address_range).unwrap().clone();
        let address_characters = address_space.end.next_power_of_two().ilog2() as usize / 4;

        let hex_font = self.options.memory_editor_text_style.resolve(ui.style());
        let address_font = self.options.memory_editor_address_text_style.resolve(ui.style());
        let ascii_font = self.options.memory_editor_ascii_text_style.resolve(ui.style());
        let hex_char_width = ui.fonts_mut(|fonts| fonts.glyph_width(&hex_font, '0'));
        let address_char_width = ui.fonts_mut(|fonts| fonts.glyph_width(&address_font, '0'));
        let ascii_char_width = ui.fonts_mut(|fonts| fonts.glyph_width(&ascii_font, '0'));
        let address_width = (address_characters + 3) as f32 * address_char_width;
        let hex_cell_width = hex_char_width * 2.0 + 4.0;

        if self.options.is_resizable_column {
            let column_count = adaptive_column_count(ui.available_width(), address_width, hex_cell_width, ascii_char_width, self.options.show_ascii);
            if column_count != self.options.column_count {
                let anchor = self.visible_range.start.max(address_space.start).min(address_space.end.saturating_sub(1));
                self.options.column_count = column_count;
                if self.frame_data.center_address_on_next_frame.is_none() {
                    self.frame_data.goto_address_line = anchor.checked_sub(address_space.start).map(|offset| offset / column_count);
                }
            }
        }

        let column_count = self.options.column_count;
        let row_style = RowStyle {
            layout: RowLayout {
                column_count,
                show_ascii: self.options.show_ascii,
                address_width,
                hex_cell_width,
                ascii_cell_width: ascii_char_width,
            },
            hex_font,
            address_font,
            ascii_font,
            hex_char_width,
            text_height: self.get_line_height(ui),
        };

        // `show_rows` scrolls by exactly this pitch, so each row below is
        // allocated at the same height. Any mismatch would desynchronise the
        // scroll offset from the rendered content.
        let row_height = row_style.text_height + ROW_GAP;
        let max_lines = address_space.len().div_ceil(column_count);

        self.handle_keyboard_edit_input(&address_space, ui.ctx());

        // Hex input is independent of arrow-key navigation: typing `0`..`9`
        // / `a`..`f` while a cell is selected updates the byte. Done once
        // per frame here (not per row) so an event is only consumed once.
        if write_fn.is_some()
            && let Some(edit_addr) = self.frame_data.selected_edit_address
        {
            self.consume_hex_input(ui.ctx(), mem, &mut read_fn, &mut write_fn, edit_addr, &address_space);
        }

        // `show_rows` derives its pitch from `row_height + item_spacing.y`.
        ui.spacing_mut().item_spacing.y = 0.0;

        let mut scroll = ScrollArea::vertical()
            .id_salt(selected_address_range)
            .max_height(f32::INFINITY)
            .auto_shrink([false, false]);

        let mut refine_center = None;
        if let Some(address) = self.frame_data.center_address_on_next_frame.take() {
            let target_line = address.saturating_sub(address_space.start) / column_count;
            // A bottom panel has no measured height on its first frame, so
            // `available_height` overshoots; the refine pass below corrects it.
            let visible_rows = match self.rendered_rows() {
                0 => (ui.available_height() / row_height).floor().max(1.0) as usize,
                rows => rows,
            };
            let line = centered_top_line(target_line, max_lines, visible_rows);
            self.frame_data.goto_address_line = None;
            scroll = scroll.vertical_scroll_offset(row_height * line as f32);
            refine_center = Some(address);
        } else if let Some(line) = self.frame_data.goto_address_line.take() {
            let new_offset = row_height * (line as f32);
            scroll = scroll.vertical_scroll_offset(new_offset);
        }

        scroll.show_rows(ui, row_height, max_lines, |ui, line_range| {
            let start_address_range = address_space.start + (line_range.start * column_count);
            let end_address_range = address_space.start + (line_range.end * column_count);
            self.visible_range = start_address_range..end_address_range;

            ui.spacing_mut().item_spacing.y = 0.0;
            let row_width = row_style.layout.total_width();

            // `show_rows` stops at the last row whose start is inside the
            // viewport, which leaves the strip below it blank. Drawing one
            // more row keeps that edge filled; the scroll area clips it.
            let draw_range = line_range.start..(line_range.end + 1).min(max_lines);

            for row_index in draw_range {
                let start_address = address_space.start + (row_index * column_count);
                let (row_rect, _) = ui.allocate_exact_size(vec2(row_width, row_height), Sense::hover());

                if row_index % 2 == 1 {
                    ui.painter().rect_filled(row_rect, 0.0, ui.visuals().faint_bg_color);
                }

                self.draw_row(
                    ui,
                    mem,
                    &mut read_fn,
                    &mut write_fn,
                    &mut highlight_fn,
                    &row_style,
                    row_rect,
                    start_address,
                    address_characters,
                    &address_space,
                );
            }

            self.frame_data.previous_frame_editor_width = row_width;
        });

        // Set after the pass that forced an explicit offset, so the refine
        // scroll doesn't fight the jump it is meant to correct.
        if refine_center.is_some() {
            self.frame_data.center_refine_address = refine_center;
        }
    }

    /// Paint one memory row into `row_rect` and handle clicks on its bytes.
    #[allow(clippy::too_many_arguments)]
    fn draw_row<T: ?Sized>(
        &mut self,
        ui: &mut Ui,
        mem: &mut T,
        read_fn: &mut impl FnMut(&mut T, Address) -> Option<u8>,
        write_fn: &mut Option<impl FnMut(&mut T, Address, u8)>,
        highlight_fn: &mut Option<impl FnMut(&mut T, Address) -> f32>,
        style: &RowStyle,
        row_rect: Rect,
        start_address: Address,
        address_characters: usize,
        address_space: &Range<Address>,
    ) {
        let frame_data = &mut self.frame_data;
        let options = &self.options;
        let layout = &style.layout;
        let writable = write_fn.is_some();

        let accent = ui.visuals().selection.bg_fill;
        let default_text_color = ui.visuals().text_color();
        let highlight_bg = ui.visuals().code_bg_color;
        let result_bg = egui::Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 96);
        let center_y = row_rect.center().y;
        let cell_size = vec2(layout.hex_cell_width, style.text_height);

        let row_addresses = start_address..start_address + layout.column_count;
        if matches!(frame_data.center_refine_address, Some(address) if row_addresses.contains(&address)) {
            ui.scroll_to_rect(row_rect, Some(egui::Align::Center));
            frame_data.center_refine_address = None;
        }
        let highlight_in_row = matches!(frame_data.selected_highlight_address, Some(address) if row_addresses.contains(&address));
        let address_color = if highlight_in_row {
            options.highlight_text_colour
        } else {
            options.address_text_colour
        };
        ui.painter().text(
            pos2(row_rect.left(), center_y),
            Align2::LEFT_CENTER,
            format!("0x{:01$X}:", start_address, address_characters),
            style.address_font.clone(),
            address_color,
        );

        for index in 0..layout.column_count {
            let memory_address = start_address + index;
            if !address_space.contains(&memory_address) {
                break;
            }

            let cell_rect = Rect::from_center_size(pos2(row_rect.left() + layout.hex_x(index) + layout.hex_cell_width / 2.0, center_y), cell_size);
            let mem_val = read_fn(mem, memory_address);
            let is_selected = frame_data.selected_edit_address == Some(memory_address);

            let mut color = default_text_color;
            if options.show_zero_colour && !is_selected && (matches!(mem_val, Some(val) if val == 0) || mem_val.is_none()) {
                color = options.zero_colour;
            }
            if !is_selected && frame_data.should_highlight(memory_address) {
                color = options.highlight_text_colour;
            }

            if frame_data.should_subtle_highlight(memory_address, options.data_preview.selected_data_format) {
                ui.painter().rect_filled(cell_rect, 0.0, highlight_bg);
            }
            if frame_data.is_in_result_range(memory_address) {
                ui.painter().rect_filled(cell_rect, 0.0, result_bg);
            }
            if let Some(highlight) = highlight_fn.as_mut() {
                let intensity = highlight(mem, memory_address).clamp(0.0, 1.0);
                if intensity > 0.0 {
                    let alpha = (intensity * 180.0) as u8;
                    ui.painter()
                        .rect_filled(cell_rect, 0.0, egui::Color32::from_rgba_unmultiplied(255, 150, 60, alpha));
                }
            }

            let label_text = match mem_val {
                Some(val) => format!("{val:02X}"),
                None => options.none_display_value.clone(),
            };
            let text_left = cell_rect.center().x - style.hex_char_width;
            for (i, ch) in label_text.chars().take(2).enumerate() {
                ui.painter().text(
                    pos2(text_left + style.hex_char_width * (i as f32 + 0.5), center_y),
                    Align2::CENTER_CENTER,
                    ch,
                    style.hex_font.clone(),
                    color,
                );
            }

            if is_selected {
                // The row is on screen now, so this performs the exact
                // minimum scroll needed to reveal the caret.
                if frame_data.scroll_caret_into_view {
                    ui.scroll_to_rect(cell_rect, None);
                    frame_data.scroll_caret_into_view = false;
                }

                let border = cell_rect.expand2(vec2(1.0, 0.0));
                ui.painter()
                    .rect_stroke(border, egui::CornerRadius::ZERO, egui::Stroke::new(1.0, accent), egui::StrokeKind::Outside);

                let nibble_idx = if frame_data.selected_low_nibble { 1.0 } else { 0.0 };
                let nibble_x0 = text_left + style.hex_char_width * nibble_idx;
                let underline_y = cell_rect.bottom() - 1.0;
                ui.painter().line_segment(
                    [pos2(nibble_x0, underline_y), pos2(nibble_x0 + style.hex_char_width, underline_y)],
                    egui::Stroke::new(1.5, accent),
                );
            }

            let response = ui.interact(cell_rect, ui.id().with(("mem_edit_byte", memory_address)), Sense::click());
            if response.clicked() {
                if writable {
                    let click_pos = response.interact_pointer_pos().unwrap_or(cell_rect.center());
                    let on_low = click_pos.x > cell_rect.center().x;
                    frame_data.set_selected_edit_address(Some(memory_address), address_space);
                    frame_data.selected_low_nibble = on_low;
                } else {
                    frame_data.set_highlight_address(memory_address);
                }
            }
            if response.secondary_clicked() {
                frame_data.set_highlight_address(memory_address);
            }
        }

        if !layout.show_ascii {
            return;
        }

        let separator_x = row_rect.left() + layout.separator_x();
        ui.painter().line_segment(
            [pos2(separator_x, row_rect.top()), pos2(separator_x, row_rect.bottom())],
            egui::Stroke::new(1.0, ui.visuals().weak_text_color()),
        );

        let ascii_cell_size = vec2(layout.ascii_cell_width, style.text_height);
        for index in 0..layout.column_count {
            let memory_address = start_address + index;
            if !address_space.contains(&memory_address) {
                break;
            }

            let cell_rect = Rect::from_center_size(
                pos2(row_rect.left() + layout.ascii_x(index) + layout.ascii_cell_width / 2.0, center_y),
                ascii_cell_size,
            );
            let highlighted = frame_data.should_highlight(memory_address);
            if highlighted {
                ui.painter().rect_filled(cell_rect, 0.0, highlight_bg);
            }
            if frame_data.is_in_result_range(memory_address) {
                ui.painter().rect_filled(cell_rect, 0.0, result_bg);
            }
            if let Some(highlight) = highlight_fn.as_mut() {
                let intensity = highlight(mem, memory_address).clamp(0.0, 1.0);
                if intensity > 0.0 {
                    let alpha = (intensity * 180.0) as u8;
                    ui.painter()
                        .rect_filled(cell_rect, 0.0, egui::Color32::from_rgba_unmultiplied(255, 150, 60, alpha));
                }
            }

            let mem_val = read_fn(mem, memory_address).unwrap_or(0);
            let character = if !(32..128).contains(&mem_val) { '.' } else { mem_val as char };
            let color = if highlighted { options.highlight_text_colour } else { default_text_color };
            ui.painter()
                .text(cell_rect.center(), Align2::CENTER_CENTER, character, style.ascii_font.clone(), color);
        }
    }

    /// Read hex digits typed since last frame and apply them directly to the
    /// currently selected nibble. This behaves like a classic hex editor:
    /// the caret sits on either the high or the low nibble of a byte;
    /// typing a hex digit overwrites that nibble and advances the caret one
    /// nibble to the right (rolling over to the next byte's high nibble).
    fn consume_hex_input<T: ?Sized>(
        &mut self,
        ctx: &Context,
        mem: &mut T,
        read_fn: &mut impl FnMut(&mut T, Address) -> Option<u8>,
        write_fn: &mut Option<impl FnMut(&mut T, Address, u8)>,
        edit_addr: Address,
        address_space: &Range<Address>,
    ) {
        // Don't steal input from the Goto box or Inspector fields.
        if ctx.text_edit_focused() {
            return;
        }

        // Snapshot the textual events. egui maps typed characters to
        // `Event::Text(_)` regardless of the actual physical key — exactly
        // what we want for hex input.
        let typed: String = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Text(s) => Some(s.clone()),
                    _ => None,
                })
                .collect()
        });

        let mut current_addr = edit_addr;
        for ch in typed.chars() {
            let Some(digit) = ch.to_digit(16) else {
                continue;
            };
            let nibble = digit as u8;

            let old_value = read_fn(mem, current_addr).unwrap_or(0);
            let on_low = self.frame_data.selected_low_nibble;
            let new_value = if on_low {
                (old_value & 0xF0) | nibble
            } else {
                (nibble << 4) | (old_value & 0x0F)
            };
            if let Some(write) = write_fn.as_mut() {
                write(mem, current_addr, new_value);
            }

            // Advance one nibble. High → low (same byte); low → next byte's high.
            if on_low {
                let next_address = current_addr.saturating_add(1);
                self.frame_data.set_selected_edit_address(Some(next_address), address_space);
                self.frame_data.selected_low_nibble = false;
                self.ensure_visible(next_address, address_space);
                current_addr = next_address;
            } else {
                self.frame_data.selected_low_nibble = true;
            }
        }
    }

    /// Move the selected edit address and scroll the viewport just enough
    /// to bring the new position into view.
    fn move_caret(&mut self, new_address: Address, address_space: &Range<Address>) {
        let selected_low_nibble = self.frame_data.selected_low_nibble;
        self.frame_data.set_selected_edit_address(Some(new_address), address_space);
        if self.frame_data.selected_edit_address.is_some() {
            self.frame_data.selected_low_nibble = selected_low_nibble;
        }
        self.ensure_visible(new_address, address_space);
    }

    /// Rows currently rendered by the scroll area. `show_rows` renders a
    /// little beyond the viewport, so the last rows are not reliably visible.
    fn rendered_rows(&self) -> usize {
        self.visible_range.len().div_ceil(self.options.column_count)
    }

    fn page_rows(&self) -> usize {
        self.rendered_rows().saturating_sub(2).max(1)
    }

    fn page_caret(&mut self, current_address: Address, down: bool, address_space: &Range<Address>) {
        let column_count = self.options.column_count;
        let current_offset = current_address.saturating_sub(address_space.start);
        let current_row = current_offset / column_count;
        let column = current_offset % column_count;
        let last_offset = address_space.len().saturating_sub(1);
        let last_row = last_offset / column_count;
        let page_rows = self.page_rows();
        let target_row = if down {
            current_row.saturating_add(page_rows).min(last_row)
        } else {
            current_row.saturating_sub(page_rows)
        };
        let target_offset = target_row.saturating_mul(column_count).saturating_add(column).min(last_offset);
        self.move_caret(address_space.start.saturating_add(target_offset), address_space);
    }

    /// Move the caret by `delta` nibbles (signed). The nibble position is
    /// computed as `address * 2 + (low ? 1 : 0)`, so `+1` walks high→low
    /// within a byte and then onto the next byte's high nibble, and `-1`
    /// performs the inverse.
    fn move_caret_nibble(&mut self, current_address: Address, delta: i64, address_space: &Range<Address>) {
        let on_low = self.frame_data.selected_low_nibble;
        let nibble_pos = (current_address as i64) * 2 + if on_low { 1 } else { 0 };
        let new_pos = nibble_pos.saturating_add(delta);
        if new_pos < 0 {
            return;
        }
        let new_addr = (new_pos / 2) as Address;
        let new_low = (new_pos % 2) == 1;
        if !address_space.contains(&new_addr) {
            return;
        }
        // set_selected_edit_address resets selected_low_nibble; restore it.
        self.frame_data.set_selected_edit_address(Some(new_addr), address_space);
        self.frame_data.selected_low_nibble = new_low;
        self.ensure_visible(new_addr, address_space);
    }

    /// Schedule a scroll if `address` is outside the visible address window.
    /// Picks the smallest scroll that brings `address` back into view.
    fn ensure_visible(&mut self, address: Address, address_space: &Range<Address>) {
        if !address_space.contains(&address) {
            return;
        }
        // Once the target row is on screen, the renderer performs the exact
        // minimum scroll from the caret's real rectangle.
        self.frame_data.scroll_caret_into_view = true;

        let column_count = self.options.column_count;
        let target_row = address.saturating_sub(address_space.start) / column_count;
        let rendered_rows = self.rendered_rows();
        if rendered_rows == 0 {
            self.frame_data.goto_address_line = Some(target_row);
            return;
        }

        let top_row = self.visible_range.start.saturating_sub(address_space.start) / column_count;
        let last_full_row = top_row + rendered_rows.saturating_sub(2);

        if target_row < top_row {
            self.frame_data.goto_address_line = Some(target_row);
        } else if target_row > last_full_row {
            self.frame_data.goto_address_line = Some(top_row + (target_row - last_full_row));
        }
    }

    fn get_line_height(&self, ui: &mut Ui) -> f32 {
        let address_size = ui.text_style_height(&self.options.memory_editor_address_text_style);
        let body_size = ui.text_style_height(&self.options.memory_editor_text_style);
        let ascii_size = ui.text_style_height(&self.options.memory_editor_ascii_text_style);
        address_size.max(body_size).max(ascii_size)
    }

    fn shrink_window_ui(&self, ui: &mut Ui) {
        ui.set_max_width(self.frame_data.previous_frame_editor_width);
    }

    fn handle_keyboard_edit_input(&mut self, address_range: &Range<Address>, ctx: &Context) {
        use egui::Key::*;

        let Some(current_address) = self.frame_data.selected_edit_address else {
            return;
        };

        // If the user is typing in the Goto box or Inspector, don't steal
        // arrows/backspace/escape for the hex grid. Clicking a byte requests
        // focus for its label, which clears this condition on the next pass.
        if ctx.text_edit_focused() {
            return;
        }

        // Escape clears the selection so the user can use the rest of the
        // app's keyboard shortcuts again.
        if ctx.input(|i| i.key_pressed(Escape)) {
            self.frame_data.set_selected_edit_address(None, address_range);
            return;
        }

        // Backspace: step back one nibble. On a low nibble it lands on the
        // high nibble of the same byte; on a high nibble it lands on the
        // low nibble of the previous byte. This matches typical hex-editor
        // back-stepping behaviour.
        if ctx.input(|i| i.key_pressed(Backspace)) {
            self.move_caret_nibble(current_address, -1, address_range);
            return;
        }

        const NAVIGATION_KEYS: [egui::Key; 8] = [ArrowLeft, ArrowRight, ArrowDown, ArrowUp, Home, End, PageDown, PageUp];
        let key_pressed = NAVIGATION_KEYS.iter().find(|&&key| ctx.input(|input| input.key_pressed(key)));
        if let Some(key) = key_pressed {
            match key {
                ArrowLeft => self.move_caret_nibble(current_address, -1, address_range),
                ArrowRight => self.move_caret_nibble(current_address, 1, address_range),
                ArrowDown => {
                    let next = current_address.saturating_add(self.options.column_count);
                    self.move_caret(next, address_range);
                }
                ArrowUp => {
                    let next = current_address.saturating_sub(self.options.column_count);
                    self.move_caret(next, address_range);
                }
                Home => {
                    let target = if ctx.input(|input| input.modifiers.command) {
                        address_range.start
                    } else {
                        let row_offset = current_address.saturating_sub(address_range.start) % self.options.column_count;
                        current_address.saturating_sub(row_offset)
                    };
                    self.move_caret(target, address_range);
                    self.frame_data.selected_low_nibble = false;
                }
                End => {
                    let target = if ctx.input(|input| input.modifiers.command) {
                        address_range.end.saturating_sub(1)
                    } else {
                        let row_offset = current_address.saturating_sub(address_range.start) % self.options.column_count;
                        current_address
                            .saturating_sub(row_offset)
                            .saturating_add(self.options.column_count.saturating_sub(1))
                            .min(address_range.end.saturating_sub(1))
                    };
                    self.move_caret(target, address_range);
                    self.frame_data.selected_low_nibble = true;
                }
                PageDown => self.page_caret(current_address, true, address_range),
                PageUp => self.page_caret(current_address, false, address_range),
                _ => unreachable!(),
            }
        }
    }

    // ** Builder methods **

    #[must_use]
    pub fn with_window_title(mut self, title: impl Into<String>) -> Self {
        self.window_name = title.into();
        self
    }

    #[inline]
    #[must_use]
    pub fn with_address_range(mut self, range_name: impl Into<String>, address_range: Range<Address>) -> Self {
        self.set_address_range(range_name, address_range);
        self
    }

    pub fn set_address_range(&mut self, range_name: impl Into<String>, address_range: Range<Address>) {
        self.address_ranges.insert(range_name.into(), address_range);

        if self.options.selected_address_range.is_empty()
            && let Some((name, _)) = self.address_ranges.iter().next()
        {
            self.options.selected_address_range = name.clone();
        }
    }

    #[inline]
    #[must_use]
    pub fn with_options(mut self, options: MemoryEditorOptions) -> Self {
        self.set_options(options);
        self
    }

    pub fn set_options(&mut self, options: MemoryEditorOptions) {
        self.options = options;
    }

    /// Programmatically jump to a given address: scrolls to it on the next
    /// frame, centers its row, and highlights it.
    pub fn goto_address(&mut self, address: Address) {
        // Find which range contains the address (if any) and switch to it.
        let target_range = self.address_ranges.iter().find(|(_, r)| r.contains(&address)).map(|(name, _)| name.clone());
        if let Some(name) = target_range {
            self.options.selected_address_range = name;
            self.frame_data.center_address_on_next_frame = Some(address);
            self.frame_data.center_refine_address = None;
            self.frame_data.goto_address_line = None;
            self.frame_data.selected_highlight_address = Some(address);
        }
    }

    /// Mark a contiguous byte range as the "current cheat result" so the editor
    /// can render a persistent translucent accent background over it.
    pub fn set_result_highlight_range(&mut self, range: Option<Range<Address>>) {
        self.frame_data.result_highlight_range = range;
    }
}

impl Default for MemoryEditor {
    fn default() -> Self {
        MemoryEditor::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adaptive_columns_use_complete_eight_byte_groups() {
        let narrow = adaptive_column_count(700.0, 100.0, 24.0, 10.0, true);
        let medium = adaptive_column_count(900.0, 100.0, 24.0, 10.0, true);
        let wide = adaptive_column_count(1200.0, 100.0, 24.0, 10.0, true);

        assert_eq!(narrow, 8);
        assert_eq!(medium, 16);
        assert_eq!(wide, 24);
    }

    #[test]
    fn adaptive_columns_stay_within_supported_limits() {
        assert_eq!(adaptive_column_count(1.0, 100.0, 24.0, 10.0, true), MIN_COLUMN_COUNT);
        assert_eq!(adaptive_column_count(100_000.0, 100.0, 24.0, 10.0, true), MAX_COLUMN_COUNT);
    }

    #[test]
    fn hiding_ascii_allows_more_hex_columns() {
        let with_ascii = adaptive_column_count(900.0, 100.0, 24.0, 10.0, true);
        let without_ascii = adaptive_column_count(900.0, 100.0, 24.0, 10.0, false);

        assert!(without_ascii > with_ascii);
    }

    #[test]
    fn chosen_column_count_fits_the_rendered_row_width() {
        let available_width = 1200.0;
        let column_count = adaptive_column_count(available_width, 100.0, 24.0, 10.0, true);
        let layout = RowLayout {
            column_count,
            show_ascii: true,
            address_width: 100.0,
            hex_cell_width: 24.0,
            ascii_cell_width: 10.0,
        };

        assert!(layout.total_width() <= available_width - SCROLLBAR_RESERVE);
    }

    #[test]
    fn hex_cells_never_overlap_and_stay_left_of_the_ascii_column() {
        let layout = RowLayout {
            column_count: 24,
            show_ascii: true,
            address_width: 100.0,
            hex_cell_width: 24.0,
            ascii_cell_width: 10.0,
        };

        for index in 1..layout.column_count {
            let previous_end = layout.hex_x(index - 1) + layout.hex_cell_width;
            assert!(layout.hex_x(index) >= previous_end, "hex cell {index} overlaps its predecessor");
        }

        let last_hex_end = layout.hex_x(layout.column_count - 1) + layout.hex_cell_width;
        assert!(last_hex_end <= layout.hex_end());
        assert!(layout.hex_end() < layout.separator_x());
        assert!(layout.separator_x() < layout.ascii_x(0));
    }

    #[test]
    fn programmatic_jump_centers_target_row() {
        assert_eq!(centered_top_line(50, 100, 20), 40);
    }

    #[test]
    fn centered_jump_clamps_at_region_edges() {
        assert_eq!(centered_top_line(3, 100, 20), 0);
        assert_eq!(centered_top_line(98, 100, 20), 80);
    }
}
