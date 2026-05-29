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

use egui::{Align2, Context, RichText, ScrollArea, Sense, TextWrapMode, Ui, Vec2, Window};

use self::option_data::{BetweenFrameData, MemoryEditorOptions};

pub mod option_data;

// Vendored data-preview helper. Not yet wired into the UI, but kept compiled
// (and thus type-checked) so it can't silently rot.
#[allow(dead_code)]
mod utilities;

/// A memory address that should be read from/written to.
pub type Address = usize;

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

        let MemoryEditorOptions {
            show_ascii,
            column_count,
            address_text_colour,
            highlight_text_colour,
            selected_address_range,
            memory_editor_address_text_style,
            ..
        } = self.options.clone();

        let line_height = self.get_line_height(ui);
        let address_space = self.address_ranges.get(&selected_address_range).unwrap().clone();
        let address_characters = address_space.end.next_power_of_two().ilog2() as usize / 4;
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

        let mut scroll = ScrollArea::vertical()
            .id_salt(selected_address_range)
            .max_height(f32::INFINITY)
            .auto_shrink([false, true]);

        if let Some(line) = self.frame_data.goto_address_line.take() {
            let new_offset = (line_height + ui.spacing().item_spacing.y) * (line as f32);
            scroll = scroll.vertical_scroll_offset(new_offset);
        }

        scroll.show_rows(ui, line_height, max_lines, |ui, line_range| {
            let start_address_range = address_space.start + (line_range.start * column_count);
            let end_address_range = address_space.start + (line_range.end * column_count);
            self.visible_range = start_address_range..end_address_range;

            egui::Grid::new("mem_edit_grid")
                .striped(true)
                .spacing(Vec2::new(15.0, ui.style().spacing.item_spacing.y))
                .show(ui, |ui| {
                    ui.style_mut().wrap_mode = Some(TextWrapMode::Extend);
                    // Inter-byte gap: the selected byte is now rendered as a
                    // normal label with a painter-drawn caret rectangle, so no
                    // widget is wider than the surrounding bytes. Keep a small
                    // fixed gap for readability.
                    ui.style_mut().spacing.item_spacing.x = 6.0;

                    for start_row in line_range.clone() {
                        let start_address = address_space.start + (start_row * column_count);
                        let line_range = start_address..start_address + column_count;
                        let highlight_in_range = matches!(self.frame_data.selected_highlight_address, Some(address) if line_range.contains(&address));

                        let start_text = RichText::new(format!("0x{:01$X}:", start_address, address_characters))
                            .color(if highlight_in_range { highlight_text_colour } else { address_text_colour })
                            .text_style(memory_editor_address_text_style.clone());

                        ui.label(start_text);

                        self.draw_memory_values(ui, mem, &mut read_fn, &mut write_fn, &mut highlight_fn, start_address, &address_space);

                        if show_ascii {
                            self.draw_ascii_sidebar(ui, mem, &mut read_fn, &mut highlight_fn, start_address, &address_space);
                        }

                        ui.end_row();
                    }
                });
            self.frame_data.previous_frame_editor_width = ui.min_rect().width();
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_memory_values<T: ?Sized>(
        &mut self,
        ui: &mut Ui,
        mem: &mut T,
        read_fn: &mut impl FnMut(&mut T, Address) -> Option<u8>,
        write_fn: &mut Option<impl FnMut(&mut T, Address, u8)>,
        highlight_fn: &mut Option<impl FnMut(&mut T, Address) -> f32>,
        start_address: Address,
        address_space: &Range<Address>,
    ) {
        let frame_data = &mut self.frame_data;
        let options = &self.options;
        let writable = write_fn.is_some();

        // Uniform cell width derived directly from the monospace glyph width.
        // Every byte (selected or not) occupies an identical slot so the
        // layout never shifts. Rendering is painter-based (no `Label` widget)
        // so the cell never claims focus and we have full control over where
        // the nibble underline sits.
        let font_id = options.memory_editor_text_style.resolve(ui.style());
        let char_w = ui.fonts_mut(|f| f.glyph_width(&font_id, '0'));
        let cell_width = char_w * 2.0 + 4.0;
        let cell_height = ui.text_style_height(&options.memory_editor_text_style);
        let accent = ui.visuals().selection.bg_fill;

        for grid_column in 0..options.column_count.div_ceil(8) {
            let start_address = start_address + 8 * grid_column;

            ui.horizontal(|ui| {
                let column_count = (options.column_count - 8 * grid_column).min(8);

                for column_index in 0..column_count {
                    let memory_address = start_address + column_index;

                    if !address_space.contains(&memory_address) {
                        break;
                    }

                    let mem_val: Option<u8> = read_fn(mem, memory_address);
                    let is_selected = frame_data.selected_edit_address == Some(memory_address);

                    let label_text = match mem_val {
                        Some(val) => format!("{:02X}", val),
                        None => options.none_display_value.clone(),
                    };

                    // Resolve foreground colour.
                    let mut color = ui.style().visuals.text_color();
                    if options.show_zero_colour && !is_selected && (matches!(mem_val, Some(val) if val == 0) || mem_val.is_none()) {
                        color = options.zero_colour;
                    }
                    if !is_selected && frame_data.should_highlight(memory_address) {
                        color = options.highlight_text_colour;
                    }

                    // Allocate the cell rect and a click sense. We deliberately
                    // do NOT use a `Label` widget here: the previous Label-based
                    // implementation could keep a faint focus outline on the
                    // last-clicked widget after the caret moved away, which is
                    // exactly the "underline stays on the old cell" symptom.
                    let cell_size = egui::vec2(cell_width, cell_height);
                    let (rect, response) = ui.allocate_exact_size(cell_size, Sense::click());

                    // Background tints (subtle highlight + result range).
                    if frame_data.should_subtle_highlight(memory_address, options.data_preview.selected_data_format) {
                        ui.painter().rect_filled(rect, 0.0, ui.style().visuals.code_bg_color);
                    }
                    if frame_data.is_in_result_range(memory_address) {
                        let bg = egui::Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 96);
                        ui.painter().rect_filled(rect, 0.0, bg);
                    }

                    // Live-change flash: paint a translucent orange tint
                    // that fades out, sourced from the integration's
                    // change tracker via `highlight_fn`. This is the
                    // signal that the target process modified this byte.
                    if let Some(hf) = highlight_fn.as_mut() {
                        let intensity = hf(mem, memory_address).clamp(0.0, 1.0);
                        if intensity > 0.0 {
                            let alpha = (intensity * 180.0) as u8;
                            ui.painter().rect_filled(rect, 0.0, egui::Color32::from_rgba_unmultiplied(255, 150, 60, alpha));
                        }
                    }

                    // Paint the two hex digits centered in the cell. We split
                    // the text into two glyphs so each nibble has a known
                    // x-range and we can underline the active one precisely.
                    let text_w = char_w * 2.0;
                    let text_left = rect.center().x - text_w / 2.0;
                    let center_y = rect.center().y;
                    let chars: Vec<char> = label_text.chars().collect();
                    for (i, ch) in chars.iter().take(2).enumerate() {
                        ui.painter().text(
                            egui::pos2(text_left + char_w * (i as f32 + 0.5), center_y),
                            Align2::CENTER_CENTER,
                            ch,
                            font_id.clone(),
                            color,
                        );
                    }

                    if is_selected {
                        // Cell border so it's obvious which byte is active
                        // even when the user is between nibble strokes.
                        let border = rect.expand2(egui::vec2(1.0, 0.0));
                        ui.painter()
                            .rect_stroke(border, egui::CornerRadius::ZERO, egui::Stroke::new(1.0, accent), egui::StrokeKind::Outside);

                        // Underline the active nibble.
                        let nibble_idx = if frame_data.selected_low_nibble { 1.0 } else { 0.0 };
                        let nibble_x0 = text_left + char_w * nibble_idx;
                        let underline_y = rect.bottom() - 1.0;
                        ui.painter().line_segment(
                            [egui::pos2(nibble_x0, underline_y), egui::pos2(nibble_x0 + char_w, underline_y)],
                            egui::Stroke::new(1.5, accent),
                        );
                    }

                    if response.clicked() {
                        if writable {
                            // Click position selects high or low nibble.
                            let click_pos = response.interact_pointer_pos().unwrap_or(rect.center());
                            let on_low = click_pos.x > rect.center().x;
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
            });
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
        self.frame_data.set_selected_edit_address(Some(new_address), address_space);
        self.ensure_visible(new_address, address_space);
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
        let visible = &self.visible_range;
        if !address_space.contains(&address) {
            return;
        }
        let column_count = self.options.column_count;
        let next_line = address.saturating_sub(address_space.start) / column_count;
        let visible_top_line = visible.start.saturating_sub(address_space.start) / column_count;
        // `ScrollArea::show_rows` over-reports the visible row count by one
        // (it includes a partial trailing row that isn't fully on screen).
        // Treat that trailing row as not visible — otherwise pressing Down
        // when the caret is on the bottommost fully-visible row doesn't
        // scroll because the address still falls inside `visible_range`.
        let visible_bottom_line = visible.end.saturating_sub(address_space.start).saturating_sub(1) / column_count;
        let visible_bottom_full = visible_bottom_line.saturating_sub(1);

        if next_line >= visible_top_line && next_line <= visible_bottom_full {
            return;
        }

        let target_top_line = if next_line > visible_bottom_full {
            // Move the viewport down just enough so `next_line` is the new
            // last fully-visible row.
            visible_top_line + (next_line - visible_bottom_full)
        } else {
            next_line
        };
        self.frame_data.goto_address_line = Some(target_top_line);
    }

    fn draw_ascii_sidebar<T: ?Sized>(
        &mut self,
        ui: &mut Ui,
        mem: &mut T,
        read_fn: &mut impl FnMut(&mut T, Address) -> Option<u8>,
        highlight_fn: &mut Option<impl FnMut(&mut T, Address) -> f32>,
        start_address: Address,
        address_space: &Range<Address>,
    ) {
        let options = &self.options;

        // Painter-based render so adjacent highlighted glyphs share a flat,
        // contiguous background. The previous `RichText::background_color`
        // approach drew a tight per-glyph rect that left visible seams
        // ("borders") between neighbouring tinted bytes.
        let font_id = options.memory_editor_ascii_text_style.resolve(ui.style());
        let char_w = ui.fonts_mut(|f| f.glyph_width(&font_id, '0'));
        let cell_size = egui::vec2(char_w, ui.text_style_height(&options.memory_editor_ascii_text_style));
        let default_text_color = ui.style().visuals.text_color();
        let highlight_bg = ui.style().visuals.code_bg_color;
        let accent = ui.visuals().selection.bg_fill;
        let result_bg = egui::Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 96);
        let highlight_text_colour = options.highlight_text_colour;

        ui.horizontal(|ui| {
            ui.add(egui::Separator::default().vertical().spacing(3.0));
            ui.style_mut().spacing.item_spacing.x = 0.0;

            ui.horizontal(|ui| {
                for i in 0..options.column_count {
                    let memory_address = start_address + i;

                    if !address_space.contains(&memory_address) {
                        break;
                    }

                    let (rect, _response) = ui.allocate_exact_size(cell_size, egui::Sense::hover());

                    let highlighted = self.frame_data.should_highlight(memory_address);
                    if highlighted {
                        ui.painter().rect_filled(rect, 0.0, highlight_bg);
                    }
                    if self.frame_data.is_in_result_range(memory_address) {
                        ui.painter().rect_filled(rect, 0.0, result_bg);
                    }

                    // Live-change flash: translucent orange tint that fades
                    // out, sourced from the integration's change tracker via
                    // `highlight_fn`. Same colour/curve as the hex grid so
                    // both columns flash in lockstep.
                    if let Some(hf) = highlight_fn.as_mut() {
                        let intensity = hf(mem, memory_address).clamp(0.0, 1.0);
                        if intensity > 0.0 {
                            let alpha = (intensity * 180.0) as u8;
                            ui.painter().rect_filled(rect, 0.0, egui::Color32::from_rgba_unmultiplied(255, 150, 60, alpha));
                        }
                    }

                    let mem_val: u8 = read_fn(mem, memory_address).unwrap_or(0);
                    let character = if !(32..128).contains(&mem_val) { '.' } else { mem_val as char };
                    let color = if highlighted { highlight_text_colour } else { default_text_color };
                    ui.painter().text(rect.center(), Align2::CENTER_CENTER, character, font_id.clone(), color);
                }
            });
        });
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

        const ARROWS: [egui::Key; 4] = [ArrowLeft, ArrowRight, ArrowDown, ArrowUp];
        let key_pressed = ARROWS.iter().find(|&&k| ctx.input(|i| i.key_pressed(k)));
        if let Some(key) = key_pressed {
            match key {
                ArrowLeft => self.move_caret_nibble(current_address, -1, address_range),
                ArrowRight => self.move_caret_nibble(current_address, 1, address_range),
                ArrowDown => {
                    let next = current_address + self.options.column_count;
                    self.move_caret(next, address_range);
                }
                ArrowUp => {
                    let next = current_address.saturating_sub(self.options.column_count);
                    self.move_caret(next, address_range);
                }
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
    /// frame and highlights it.
    pub fn goto_address(&mut self, address: Address) {
        // Find which range contains the address (if any) and switch to it.
        let target_range = self
            .address_ranges
            .iter()
            .find(|(_, r)| r.contains(&address))
            .map(|(n, r)| (n.clone(), r.clone()));
        if let Some((name, range)) = target_range {
            self.options.selected_address_range = name;
            self.frame_data.goto_address_line = address.checked_sub(range.start).map(|o| o / self.options.column_count);
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
