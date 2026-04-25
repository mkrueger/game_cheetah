use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use icy_ui::{
    Element, Length, Task, alignment,
    border::Radius,
    widget::{Id, operation, rule, scrollable::Viewport, text_input},
};
use proc_maps::get_process_maps;
use process_memory::{PutAddress, TryIntoProcessHandle, copy_address};

use crate::{DIALOG_PADDING, SearchType, app::App, message::Message};

pub const BYTES_PER_ROW: usize = 16;
pub const ROW_HEIGHT: f32 = 22.0;
pub const PAGE_ROWS: usize = 16;
/// Cadence at which the editor re-reads visible bytes to drive change
/// highlighting. Trades responsiveness against the cost of issuing one
/// `copy_address` per visible row.
pub const TICK_INTERVAL: Duration = Duration::from_millis(150);
/// How long a byte change stays visibly tinted before fading back to the
/// regular cell appearance.
const CHANGE_FADE: Duration = Duration::from_millis(1500);
/// Cap on the number of remembered byte observations. Bounded so a long
/// session that scrolls through many regions can't grow unbounded.
const CHANGE_TRACKER_CAP: usize = 16384;

/// Map of every recently-observed address to the most recent byte value seen
/// there and, when known, the timestamp of the last observed transition. The
/// timestamp is `None` for cells we've only ever observed once — those have
/// no observed change yet and therefore must not flash.
type ChangeTracker = HashMap<usize, (u8, Option<Instant>)>;

/// Cap on the number of remembered undo/redo entries. Each entry is at most
/// 8 bytes of payload, so the bound is generous.
const UNDO_STACK_CAP: usize = 1024;

/// A single user-initiated write that can be undone or redone.
#[derive(Debug, Clone)]
struct UndoEntry {
    address: usize,
    /// Bytes that were at `address` *before* the write — replaying these
    /// restores the previous state.
    before: Vec<u8>,
    /// Bytes that the user wrote — replaying these reapplies the change on
    /// redo.
    after: Vec<u8>,
}

const SCROLL_ID: &str = "memory-editor-scroll";

fn scroll_id() -> Id {
    Id::new(SCROLL_ID)
}

fn format_relative_offset(origin: usize, address: usize) -> String {
    let delta = address as i128 - origin as i128;
    if delta < 0 { format!("-0x{:X}", -delta) } else { format!("+0x{delta:X}") }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InspectorValueKind {
    U8,
    I8,
    U16,
    I16,
    U32,
    I32,
    U64,
    I64,
    F32,
    F64,
}

#[derive(Debug, Clone)]
struct MemoryRegion {
    start: usize,
    size: usize,
    row_start: usize,
    row_count: usize,
    readable: bool,
    writable: bool,
    executable: bool,
    name: String,
}

impl MemoryRegion {
    fn end(&self) -> usize {
        self.start.saturating_add(self.size)
    }

    fn contains_address(&self, address: usize) -> bool {
        address >= self.start && address < self.end()
    }

    fn contains_row(&self, row: usize) -> bool {
        row >= self.row_start && row < self.row_start + self.row_count
    }

    fn row_address(&self, row: usize) -> usize {
        self.start.saturating_add((row - self.row_start) * BYTES_PER_ROW)
    }
}

#[derive(Default)]
pub struct MemoryEditor {
    pub address_text: String,
    regions: Vec<MemoryRegion>,
    cursor_row: usize,
    cursor_col: usize,
    cursor_nibble: usize, // 0 = high nibble, 1 = low nibble

    editor_initial_address: usize,
    editor_initial_size: usize,

    /// Latest viewport reported by the scroll area. Used to decide whether the
    /// cursor row is on-screen and to compute the minimal scroll required to
    /// keep it visible.
    viewport: Option<Viewport>,

    inspector_edit: Option<(InspectorValueKind, String)>,

    /// Stack of recent writes for `Ctrl+Z`. Each entry remembers the bytes
    /// that were *replaced* by a single user-initiated write so undoing can
    /// put them back. Bounded to keep memory bounded across long sessions.
    undo_stack: Vec<UndoEntry>,
    /// Writes that were undone and can be replayed via `Shift+Ctrl+Z`.
    /// Cleared whenever a fresh write happens.
    redo_stack: Vec<UndoEntry>,

    /// Last observed byte value and, when applicable, the moment it last
    /// transitioned to that value. The view re-reads the visible rows and
    /// updates this map so cells that just changed get a brief highlight
    /// that fades out over [`CHANGE_FADE`]. The timestamp is `None` for
    /// cells we've only ever observed once — those have no observed
    /// transition and therefore must not flash.
    change_tracker: Rc<RefCell<ChangeTracker>>,
}

impl MemoryEditor {
    pub fn cursor_row(&self) -> usize {
        self.cursor_row
    }

    fn total_rows(&self) -> usize {
        self.regions.last().map_or(1, |region| region.row_start + region.row_count).max(1)
    }

    fn region_for_row_in(regions: &[MemoryRegion], row: usize) -> Option<&MemoryRegion> {
        regions.iter().find(|region| region.contains_row(row))
    }

    fn region_for_row(&self, row: usize) -> Option<&MemoryRegion> {
        Self::region_for_row_in(&self.regions, row)
    }

    pub fn address_for_offset(&self, offset: usize) -> Option<usize> {
        let row = offset / BYTES_PER_ROW;
        let col = offset % BYTES_PER_ROW;
        let region = self.region_for_row(row)?;
        let address = region.row_address(row).saturating_add(col);
        (address < region.end()).then_some(address)
    }

    pub fn set_viewport(&mut self, viewport: Viewport) {
        self.viewport = Some(viewport);
    }

    fn should_skip_region(map: &proc_maps::MapRange) -> bool {
        if map.size() == 0 || !map.is_read() {
            return true;
        }

        if map.start() == 0xffffffffff600000 || map.start() > 0x7fffffffffff {
            return true;
        }

        if let Some(file_name) = map.filename() {
            let file_str = file_name.to_string_lossy();
            if file_str == "[vvar]" || file_str == "[vdso]" || file_str == "[vsyscall]" {
                return true;
            }
        }

        false
    }

    pub fn refresh_regions(&mut self, pid: process_memory::Pid) -> Result<(), String> {
        let mut maps = get_process_maps(pid).map_err(|err| format!("Failed to read memory map of PID {pid}: {err}"))?;
        maps.sort_by_key(proc_maps::MapRange::start);
        let mut regions = Vec::new();
        let mut row_start = 0usize;

        for map in maps {
            if Self::should_skip_region(&map) {
                continue;
            }

            let row_count = map.size().div_ceil(BYTES_PER_ROW).max(1);
            let name = map
                .filename()
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_else(|| "anonymous".to_string());

            regions.push(MemoryRegion {
                start: map.start(),
                size: map.size(),
                row_start,
                row_count,
                readable: map.is_read(),
                writable: map.is_write(),
                executable: map.is_exec(),
                name,
            });
            row_start = row_start.saturating_add(row_count);
        }

        if regions.is_empty() {
            return Err(format!("PID {pid} reports no readable memory regions"));
        }

        self.regions = regions;
        Ok(())
    }

    /// Places the cursor on `focus_addr` inside the current region map. If the
    /// exact address is not mapped, jumps to the nearest following region (or
    /// the last available region when the address is past the map).
    pub fn focus_on(&mut self, focus_addr: usize) -> Result<(), String> {
        let Some((row, col)) = self.address_to_cursor(focus_addr) else {
            return Err(format!("0x{focus_addr:X} is not in a readable memory region"));
        };

        self.cursor_row = row;
        self.cursor_col = col;
        self.cursor_nibble = 0;
        self.viewport = None;
        self.inspector_edit = None;
        Ok(())
    }

    fn address_to_cursor(&self, address: usize) -> Option<(usize, usize)> {
        if let Some(region) = self.regions.iter().find(|region| region.contains_address(address)) {
            let offset = address - region.start;
            return Some((region.row_start + offset / BYTES_PER_ROW, offset % BYTES_PER_ROW));
        }

        let nearest = self.regions.iter().find(|region| region.start > address).or_else(|| self.regions.last())?;
        Some((nearest.row_start, 0))
    }

    /// Animated scroll task that places the cursor row a few lines below the
    /// top of the viewport. Used right after a jump/open before any viewport
    /// has been measured.
    pub fn snap_to_cursor<T>(&self) -> Task<T>
    where
        T: 'static,
    {
        let target_y = ((self.cursor_row as f32 - 4.0) * ROW_HEIGHT).max(0.0);
        operation::scroll_to(scroll_id(), operation::AbsoluteOffset { x: None, y: Some(target_y) })
    }

    fn cursor_address(&self) -> Option<usize> {
        let region = self.region_for_row(self.cursor_row)?;
        let address = region.row_address(self.cursor_row).saturating_add(self.cursor_col);
        (address < region.end()).then_some(address)
    }

    pub fn set_inspector_value_text(&mut self, kind: InspectorValueKind, value: String) {
        self.inspector_edit = Some((kind, value));
    }

    fn parse_unsigned(input: &str, max: u64, label: &str) -> Result<u64, String> {
        let text = input.trim().replace('_', "");
        let value = if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
            u64::from_str_radix(hex, 16)
        } else {
            text.parse::<u64>()
        }
        .map_err(|err| format!("Invalid {label} value '{input}': {err}"))?;

        if value <= max {
            Ok(value)
        } else {
            Err(format!("{label} value {value} is out of range (max {max})"))
        }
    }

    fn parse_signed(input: &str, min: i64, max: i64, label: &str) -> Result<i64, String> {
        let text = input.trim().replace('_', "");
        let value = if let Some(hex) = text.strip_prefix("-0x").or_else(|| text.strip_prefix("-0X")) {
            let magnitude = i64::from_str_radix(hex, 16).map_err(|err| format!("Invalid {label} value '{input}': {err}"))?;
            -magnitude
        } else if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
            i64::from_str_radix(hex, 16).map_err(|err| format!("Invalid {label} value '{input}': {err}"))?
        } else {
            text.parse::<i64>().map_err(|err| format!("Invalid {label} value '{input}': {err}"))?
        };

        if (min..=max).contains(&value) {
            Ok(value)
        } else {
            Err(format!("{label} value {value} is out of range ({min}..={max})"))
        }
    }

    fn inspector_bytes(kind: InspectorValueKind, input: &str) -> Result<Vec<u8>, String> {
        match kind {
            InspectorValueKind::U8 => Ok(vec![Self::parse_unsigned(input, u8::MAX as u64, "u8")? as u8]),
            InspectorValueKind::I8 => Ok(vec![(Self::parse_signed(input, i8::MIN as i64, i8::MAX as i64, "i8")? as i8) as u8]),
            InspectorValueKind::U16 => Ok((Self::parse_unsigned(input, u16::MAX as u64, "u16")? as u16).to_le_bytes().to_vec()),
            InspectorValueKind::I16 => Ok((Self::parse_signed(input, i16::MIN as i64, i16::MAX as i64, "i16")? as i16)
                .to_le_bytes()
                .to_vec()),
            InspectorValueKind::U32 => Ok((Self::parse_unsigned(input, u32::MAX as u64, "u32")? as u32).to_le_bytes().to_vec()),
            InspectorValueKind::I32 => Ok((Self::parse_signed(input, i32::MIN as i64, i32::MAX as i64, "i32")? as i32)
                .to_le_bytes()
                .to_vec()),
            InspectorValueKind::U64 => Ok(Self::parse_unsigned(input, u64::MAX, "u64")?.to_le_bytes().to_vec()),
            InspectorValueKind::I64 => Ok(Self::parse_signed(input, i64::MIN, i64::MAX, "i64")?.to_le_bytes().to_vec()),
            InspectorValueKind::F32 => Ok(input
                .trim()
                .parse::<f32>()
                .map_err(|err| format!("Invalid f32 value '{input}': {err}"))?
                .to_le_bytes()
                .to_vec()),
            InspectorValueKind::F64 => Ok(input
                .trim()
                .parse::<f64>()
                .map_err(|err| format!("Invalid f64 value '{input}': {err}"))?
                .to_le_bytes()
                .to_vec()),
        }
    }

    pub fn submit_inspector_value(&mut self, pid: process_memory::Pid, kind: InspectorValueKind) -> Result<(), String> {
        let Some((edit_kind, input)) = self.inspector_edit.as_ref() else {
            return Ok(());
        };
        if *edit_kind != kind {
            return Ok(());
        }

        let Some(address) = self.cursor_address() else {
            return Err("Cursor is not in a readable memory region".to_string());
        };
        let bytes = Self::inspector_bytes(kind, input)?;
        self.write_with_undo(pid, address, &bytes)?;
        self.inspector_edit = None;
        Ok(())
    }

    /// Performs a write at `address` and records an entry on the undo stack.
    /// Reads the existing bytes first so undo can restore them. The redo
    /// stack is cleared because a fresh user write invalidates the redo
    /// branch.
    fn write_with_undo(&mut self, pid: process_memory::Pid, address: usize, after: &[u8]) -> Result<(), String> {
        let handle = pid.try_into_process_handle().map_err(|e| format!("Failed to attach to process: {e}"))?;
        let before = copy_address(address, after.len(), &handle).map_err(|e| format!("Failed to read 0x{address:X}: {e}"))?;
        handle.put_address(address, after).map_err(|e| format!("Failed to write 0x{address:X}: {e}"))?;

        // Skip recording no-op writes so redundant submits don't pollute the
        // history.
        if before != after {
            self.push_undo(UndoEntry {
                address,
                before,
                after: after.to_vec(),
            });
            self.redo_stack.clear();
        }
        Ok(())
    }

    fn push_undo(&mut self, entry: UndoEntry) {
        self.undo_stack.push(entry);
        if self.undo_stack.len() > UNDO_STACK_CAP {
            // Drop the oldest entries while keeping the newest ones.
            let drop = self.undo_stack.len() - UNDO_STACK_CAP;
            self.undo_stack.drain(..drop);
        }
    }

    /// Restores the bytes from the most recent undo entry. Returns the
    /// address that was modified (so the caller can move the cursor / make
    /// the change visible) or `None` if there's nothing to undo.
    pub fn undo(&mut self, pid: process_memory::Pid) -> Result<Option<usize>, String> {
        let Some(entry) = self.undo_stack.pop() else {
            return Ok(None);
        };
        let handle = pid.try_into_process_handle().map_err(|e| format!("Failed to attach to process: {e}"))?;
        handle
            .put_address(entry.address, &entry.before)
            .map_err(|e| format!("Failed to write 0x{:X}: {e}", entry.address))?;
        let address = entry.address;
        self.redo_stack.push(entry);
        Ok(Some(address))
    }

    /// Re-applies the most recently undone write.
    pub fn redo(&mut self, pid: process_memory::Pid) -> Result<Option<usize>, String> {
        let Some(entry) = self.redo_stack.pop() else {
            return Ok(None);
        };
        let handle = pid.try_into_process_handle().map_err(|e| format!("Failed to attach to process: {e}"))?;
        handle
            .put_address(entry.address, &entry.after)
            .map_err(|e| format!("Failed to write 0x{:X}: {e}", entry.address))?;
        let address = entry.address;
        self.undo_stack.push(entry);
        Ok(Some(address))
    }

    /// Returns an animated scroll task that brings the cursor row into view if
    /// it is currently outside the tracked viewport. Returns `Task::none()`
    /// when the cursor is already visible or the viewport hasn't been measured
    /// yet.
    pub fn ensure_cursor_visible<T>(&self) -> Task<T>
    where
        T: 'static,
    {
        let Some(viewport) = self.viewport else {
            return Task::none();
        };

        let cursor_top = self.cursor_row as f32 * ROW_HEIGHT;
        let cursor_bottom = cursor_top + ROW_HEIGHT;
        let view_top = viewport.absolute_offset().y;
        let view_height = viewport.bounds().height;
        let view_bottom = view_top + view_height;

        let target_y = if cursor_top < view_top {
            cursor_top
        } else if cursor_bottom > view_bottom {
            (cursor_bottom - view_height).max(0.0)
        } else {
            return Task::none();
        };

        operation::scroll_to_animated(scroll_id(), operation::AbsoluteOffset { x: None, y: Some(target_y) })
    }

    pub fn show_memory_editor<'a>(&'a self, app: &'a App) -> Element<'a, Message> {
        use icy_ui::widget::{button, column, container, mouse_area, row, scroll_area, text};

        // ---- Layout constants -------------------------------------------------
        // Address: 16 hex digits + one space worth of padding -> wide enough for
        // 64-bit addresses without overflowing into the hex grid.
        const ADDRESS_WIDTH: f32 = 150.0;
        const HEX_CELL_WIDTH: f32 = 26.0;
        const HEX_GROUP_GAP: f32 = 12.0;
        const ASCII_CELL_WIDTH: f32 = 10.0;
        const ASCII_GROUP_GAP: f32 = 6.0;
        const GUTTER: f32 = 14.0;
        const HEX_GROUP_WIDTH: f32 = HEX_CELL_WIDTH * 8.0;
        const HEX_BLOCK_WIDTH: f32 = HEX_GROUP_WIDTH * 2.0 + HEX_GROUP_GAP;
        const ASCII_GROUP_WIDTH: f32 = ASCII_CELL_WIDTH * 8.0;
        const ASCII_BLOCK_WIDTH: f32 = ASCII_GROUP_WIDTH * 2.0 + ASCII_GROUP_GAP;

        let pid = app.state.pid;
        let highlight_start = self.editor_initial_address;
        let highlight_end = highlight_start + self.editor_initial_size;
        let cursor_row = self.cursor_row;
        let cursor_col = self.cursor_col;
        let cursor_nibble = self.cursor_nibble;
        let regions = self.regions.clone();
        let total_rows = self.total_rows();

        // ---- Header -----------------------------------------------------------
        // Theming model:
        //   * The editor sits inside a `primary` panel; text on it uses
        //     `theme.primary.on` and dimmed text uses `primary.on.scale_alpha(...)`.
        //   * Highlights (cursor, selected byte, search hit) are *translucent*
        //     overlays so text remains readable on top.
        let dim = |theme: &icy_ui::Theme| icy_ui::widget::text::Style {
            color: Some(theme.primary.on.scale_alpha(0.55)),
        };

        let make_hex_header_group = |start: usize| {
            let mut group = row![].spacing(0);
            for i in start..start + 8 {
                group = group.push(
                    container(text(format!("{i:02X}")).size(13).font(icy_ui::Font::MONOSPACE).style(dim))
                        .width(Length::Fixed(HEX_CELL_WIDTH))
                        .align_x(alignment::Alignment::Center),
                );
            }
            container(group).width(Length::Fixed(HEX_GROUP_WIDTH))
        };

        let header = container(
            row![
                container(text("Address").size(12).font(icy_ui::Font::MONOSPACE).style(dim))
                    .width(Length::Fixed(ADDRESS_WIDTH))
                    .padding([0, 8])
                    .align_x(alignment::Alignment::Start),
                row![make_hex_header_group(0), make_hex_header_group(8),].spacing(HEX_GROUP_GAP),
                container(text("ASCII").size(12).font(icy_ui::Font::MONOSPACE).style(dim))
                    .width(Length::Fixed(ASCII_BLOCK_WIDTH))
                    .padding([0, 4])
                    .align_x(alignment::Alignment::Start),
            ]
            .spacing(GUTTER)
            .align_y(alignment::Alignment::Center),
        )
        .padding([6, 8])
        .style(|theme: &icy_ui::Theme| container::Style {
            background: Some(theme.primary.on.scale_alpha(0.04).into()),
            border: icy_ui::Border {
                color: theme.primary.divider,
                width: 0.0,
                radius: Radius::new(0.0),
            },
            ..Default::default()
        });

        // ---- Virtualized memory rows -----------------------------------------
        let tracker = self.change_tracker.clone();
        let memory_view =
            scroll_area()
                .id(scroll_id())
                .height(Length::FillPortion(3))
                .show_rows(ROW_HEIGHT, total_rows, move |range| {
                    let range_start = range.start;
                    let range_end = range.end;
                    let handle = (pid as process_memory::Pid).try_into_process_handle().ok();

                    let make_hex_cell =
                        |absolute_row: usize, col_idx: usize, byte: u8, current_address: usize, is_valid: bool, change_alpha: f32| -> Element<'_, Message> {
                            if !is_valid {
                                return container(text("  ").size(14).font(icy_ui::Font::MONOSPACE))
                                    .width(Length::Fixed(HEX_CELL_WIDTH))
                                    .into();
                            }

                            let is_selected_byte = cursor_row == absolute_row && cursor_col == col_idx;
                            let is_initial = current_address >= highlight_start && current_address < highlight_end;
                            let is_zero = byte == 0;

                            let high = (byte >> 4) & 0x0F;
                            let low = byte & 0x0F;
                            let nibble_color = move |theme: &icy_ui::Theme, is_active_nibble: bool| {
                                if is_active_nibble {
                                    theme.accent.base
                                } else if is_zero {
                                    theme.primary.on.scale_alpha(0.35)
                                } else {
                                    theme.primary.on
                                }
                            };
                            let hi_text = text(format!("{high:X}"))
                                .size(14)
                                .font(icy_ui::Font::MONOSPACE)
                                .style(move |theme: &icy_ui::Theme| icy_ui::widget::text::Style {
                                    color: Some(nibble_color(theme, is_selected_byte && cursor_nibble == 0)),
                                });
                            let lo_text = text(format!("{low:X}"))
                                .size(14)
                                .font(icy_ui::Font::MONOSPACE)
                                .style(move |theme: &icy_ui::Theme| icy_ui::widget::text::Style {
                                    color: Some(nibble_color(theme, is_selected_byte && cursor_nibble == 1)),
                                });

                            let hex_pair = row![hi_text, lo_text].spacing(0);

                            mouse_area(
                                container(hex_pair)
                                    .width(Length::Fixed(HEX_CELL_WIDTH))
                                    .padding([1, 0])
                                    .align_x(alignment::Alignment::Center)
                                    .style(move |theme: &icy_ui::Theme| {
                                        if is_selected_byte {
                                            container::Style {
                                                background: Some(theme.accent.base.scale_alpha(0.30).into()),
                                                border: icy_ui::Border {
                                                    color: theme.accent.base,
                                                    width: 1.0,
                                                    radius: Radius::new(3.0),
                                                },
                                                ..Default::default()
                                            }
                                        } else if change_alpha > 0.0 {
                                            container::Style {
                                                background: Some(theme.destructive.base.scale_alpha(change_alpha).into()),
                                                ..Default::default()
                                            }
                                        } else if is_initial {
                                            container::Style {
                                                background: Some(theme.success.base.scale_alpha(0.22).into()),
                                                ..Default::default()
                                            }
                                        } else {
                                            container::Style::default()
                                        }
                                    }),
                            )
                            .on_press(Message::MemoryEditorSetCursor(absolute_row, col_idx))
                            .into()
                        };

                    let make_ascii_cell =
                        |absolute_row: usize, i: usize, byte: u8, current_address: usize, is_valid: bool, change_alpha: f32| -> Element<'_, Message> {
                            if !is_valid {
                                return container(text(" ").size(14).font(icy_ui::Font::MONOSPACE))
                                    .width(Length::Fixed(ASCII_CELL_WIDTH))
                                    .into();
                            }

                            let c = byte as char;
                            let is_printable = c.is_ascii_graphic() || c == ' ';
                            let display_char = if is_printable { c.to_string() } else { "·".to_string() };
                            let is_selected = cursor_row == absolute_row && cursor_col == i;
                            let is_initial = current_address >= highlight_start && current_address < highlight_end;

                            container(text(display_char).size(14).font(icy_ui::Font::MONOSPACE).style(move |theme: &icy_ui::Theme| {
                                icy_ui::widget::text::Style {
                                    color: Some(if is_selected {
                                        theme.accent.base
                                    } else if is_printable {
                                        theme.primary.on
                                    } else {
                                        theme.primary.on.scale_alpha(0.35)
                                    }),
                                }
                            }))
                            .width(Length::Fixed(ASCII_CELL_WIDTH))
                            .align_x(alignment::Alignment::Center)
                            .style(move |theme: &icy_ui::Theme| {
                                if is_selected {
                                    container::Style {
                                        background: Some(theme.accent.base.scale_alpha(0.30).into()),
                                        ..Default::default()
                                    }
                                } else if change_alpha > 0.0 {
                                    container::Style {
                                        background: Some(theme.destructive.base.scale_alpha(change_alpha).into()),
                                        ..Default::default()
                                    }
                                } else if is_initial {
                                    container::Style {
                                        background: Some(theme.success.base.scale_alpha(0.22).into()),
                                        ..Default::default()
                                    }
                                } else {
                                    container::Style::default()
                                }
                            })
                            .into()
                        };

                    let col = column(
                        (range_start..range_end)
                            .map(|absolute_row| {
                                let Some(region) = Self::region_for_row_in(&regions, absolute_row) else {
                                    return container(text("No readable memory regions").size(14))
                                        .height(Length::Fixed(ROW_HEIGHT))
                                        .padding([0, 16])
                                        .into();
                                };

                                let row_addr = region.row_address(absolute_row);
                                let bytes_in_region = region.end().saturating_sub(row_addr).min(BYTES_PER_ROW);
                                let mut row_bytes = [0u8; BYTES_PER_ROW];
                                if bytes_in_region > 0
                                    && let Some(handle) = &handle
                                    && let Ok(buf) = copy_address(row_addr, bytes_in_region, handle)
                                {
                                    let n = buf.len().min(bytes_in_region);
                                    row_bytes[..n].copy_from_slice(&buf[..n]);
                                }
                                let zebra = absolute_row % 2 == 1;
                                let is_cursor_row = cursor_row == absolute_row;

                                // Update the change tracker with the bytes we just
                                // observed and compute a fade alpha for each cell.
                                // Cells we have only ever observed once are recorded
                                // with no transition timestamp, so they cannot flash
                                // until they actually change in the target process.
                                let mut change_alphas = [0.0f32; BYTES_PER_ROW];
                                {
                                    let mut tracker_borrow = tracker.borrow_mut();
                                    let now = Instant::now();
                                    for col_idx in 0..bytes_in_region {
                                        let cell_addr = row_addr.saturating_add(col_idx);
                                        let new_byte = row_bytes[col_idx];
                                        match tracker_borrow.get(&cell_addr).copied() {
                                            Some((prev_byte, last_change)) => {
                                                if prev_byte != new_byte {
                                                    tracker_borrow.insert(cell_addr, (new_byte, Some(now)));
                                                    change_alphas[col_idx] = 0.55;
                                                } else if let Some(last_change) = last_change {
                                                    let elapsed = now.saturating_duration_since(last_change);
                                                    if elapsed < CHANGE_FADE {
                                                        let frac = elapsed.as_secs_f32() / CHANGE_FADE.as_secs_f32();
                                                        change_alphas[col_idx] = 0.55 * (1.0 - frac);
                                                    }
                                                }
                                            }
                                            None => {
                                                tracker_borrow.insert(cell_addr, (new_byte, None));
                                            }
                                        }
                                    }
                                }

                                let mut hex_left = row![].spacing(0);
                                let mut hex_right = row![].spacing(0);
                                for (col_idx, byte) in row_bytes.iter().enumerate().take(8) {
                                    let cell_addr = row_addr.saturating_add(col_idx);
                                    hex_left = hex_left.push(make_hex_cell(
                                        absolute_row,
                                        col_idx,
                                        *byte,
                                        cell_addr,
                                        col_idx < bytes_in_region,
                                        change_alphas[col_idx],
                                    ));
                                }
                                for (col_idx, byte) in row_bytes.iter().enumerate().skip(8) {
                                    let cell_addr = row_addr.saturating_add(col_idx);
                                    hex_right = hex_right.push(make_hex_cell(
                                        absolute_row,
                                        col_idx,
                                        *byte,
                                        cell_addr,
                                        col_idx < bytes_in_region,
                                        change_alphas[col_idx],
                                    ));
                                }
                                let hex_block = row![
                                    container(hex_left).width(Length::Fixed(HEX_GROUP_WIDTH)),
                                    container(hex_right).width(Length::Fixed(HEX_GROUP_WIDTH)),
                                ]
                                .spacing(HEX_GROUP_GAP);

                                let mut ascii_left = row![].spacing(0);
                                let mut ascii_right = row![].spacing(0);
                                for (i, byte) in row_bytes.iter().enumerate().take(8) {
                                    let cell_addr = row_addr.saturating_add(i);
                                    ascii_left = ascii_left.push(make_ascii_cell(absolute_row, i, *byte, cell_addr, i < bytes_in_region, change_alphas[i]));
                                }
                                for (i, byte) in row_bytes.iter().enumerate().skip(8) {
                                    let cell_addr = row_addr.saturating_add(i);
                                    ascii_right = ascii_right.push(make_ascii_cell(absolute_row, i, *byte, cell_addr, i < bytes_in_region, change_alphas[i]));
                                }
                                let ascii_block = row![
                                    container(ascii_left).width(Length::Fixed(ASCII_GROUP_WIDTH)),
                                    container(ascii_right).width(Length::Fixed(ASCII_GROUP_WIDTH)),
                                ]
                                .spacing(ASCII_GROUP_GAP);

                                let address_label =
                                    text(format!("{row_addr:016X}"))
                                        .size(13)
                                        .font(icy_ui::Font::MONOSPACE)
                                        .style(move |theme: &icy_ui::Theme| icy_ui::widget::text::Style {
                                            color: Some(if is_cursor_row {
                                                theme.accent.base
                                            } else {
                                                theme.primary.on.scale_alpha(0.55)
                                            }),
                                        });

                                container(
                                    row![
                                        container(address_label)
                                            .width(Length::Fixed(ADDRESS_WIDTH))
                                            .padding([0, 8])
                                            .align_x(alignment::Alignment::Start),
                                        container(hex_block).width(Length::Fixed(HEX_BLOCK_WIDTH)),
                                        container(ascii_block).width(Length::Fixed(ASCII_BLOCK_WIDTH)).padding([0, 4]),
                                    ]
                                    .spacing(GUTTER)
                                    .align_y(alignment::Alignment::Center),
                                )
                                .height(Length::Fixed(ROW_HEIGHT))
                                .padding([0, 8])
                                .style(move |theme: &icy_ui::Theme| {
                                    if is_cursor_row {
                                        container::Style {
                                            background: Some(theme.accent.base.scale_alpha(0.10).into()),
                                            ..Default::default()
                                        }
                                    } else if zebra {
                                        container::Style {
                                            background: Some(theme.primary.on.scale_alpha(0.04).into()),
                                            ..Default::default()
                                        }
                                    } else {
                                        container::Style::default()
                                    }
                                })
                                .into()
                            })
                            .collect::<Vec<Element<'_, Message>>>(),
                    )
                    .spacing(0);

                    // Cap the tracker so we never grow it unbounded across long
                    // browsing sessions. When over the cap, drop the oldest
                    // entries (by last-change timestamp; entries we've only
                    // observed once count as oldest).
                    {
                        let mut tracker_borrow = tracker.borrow_mut();
                        if tracker_borrow.len() > CHANGE_TRACKER_CAP {
                            let target = CHANGE_TRACKER_CAP * 3 / 4;
                            let mut entries: Vec<(usize, Option<Instant>)> = tracker_borrow.iter().map(|(&addr, &(_, ts))| (addr, ts)).collect();
                            entries.sort_by_key(|(_, ts)| *ts);
                            let drop_count = tracker_borrow.len().saturating_sub(target);
                            for (addr, _) in entries.into_iter().take(drop_count) {
                                tracker_borrow.remove(&addr);
                            }
                        }
                    }

                    col.into()
                })
                .on_scroll(Message::MemoryEditorScrolled);

        // ---- Status strip -----------------------------------------------------
        // One slim bar showing the things that change as the cursor moves:
        //   - the absolute address under the cursor,
        //   - the signed offset from the original search hit (only when the
        //     editor was opened on a hit and the cursor has moved away),
        //   - the enclosing region (name, range, access).
        let cursor_address = self.cursor_address();
        let cursor_region = self.region_for_row(cursor_row);
        let cursor_address_text = cursor_address.map_or_else(|| "—".to_string(), |address| format!("0x{address:016X}"));
        let access_text = cursor_region.map_or_else(
            || "unmapped".to_string(),
            |region| match (region.readable, region.writable, region.executable) {
                (true, true, true) => "read / write / execute".to_string(),
                (true, true, false) => "read / write".to_string(),
                (true, false, true) => "read / execute".to_string(),
                (true, false, false) => "read-only".to_string(),
                (false, true, _) => "write-only".to_string(),
                (false, false, true) => "execute-only".to_string(),
                (false, false, false) => "no access".to_string(),
            },
        );
        let region_summary_text = cursor_region.map_or_else(
            || "no mapped region".to_string(),
            |region| {
                let name = if region.name.is_empty() { "<unnamed>" } else { region.name.as_str() };
                format!("{name}   0x{:X}–0x{:X}   ({})", region.start, region.end(), access_text)
            },
        );
        let show_offset = cursor_address.is_some_and(|address| address != self.editor_initial_address) && self.editor_initial_address != 0;
        let offset_text = if show_offset {
            cursor_address.map(|address| format_relative_offset(self.editor_initial_address, address))
        } else {
            None
        };

        let status_label = |s: String| -> Element<'_, Message> {
            text(s)
                .size(11)
                .font(icy_ui::Font::MONOSPACE)
                .style(|theme: &icy_ui::Theme| icy_ui::widget::text::Style {
                    color: Some(theme.secondary.on.scale_alpha(0.55)),
                })
                .into()
        };
        let status_value = |s: String| -> Element<'_, Message> {
            text(s)
                .size(12)
                .font(icy_ui::Font::MONOSPACE)
                .style(|theme: &icy_ui::Theme| icy_ui::widget::text::Style {
                    color: Some(theme.secondary.on),
                })
                .into()
        };

        // Region band: sits between the toolbar and the hex grid header so the
        // (potentially long) region description gets its own line and never
        // pushes the address/offset off-screen.
        let region_strip = container(
            row![status_label("Region".to_string()), status_value(region_summary_text)]
                .spacing(8)
                .align_y(alignment::Alignment::Center),
        )
        .width(Length::Fill)
        .padding([6, 12])
        .style(|theme: &icy_ui::Theme| container::Style {
            background: Some(theme.secondary.base.into()),
            text_color: Some(theme.secondary.on),
            ..Default::default()
        });

        // Status strip below the grid: just the things that change when the
        // cursor moves byte-by-byte.
        let mut status_items: Vec<Element<'_, Message>> = Vec::new();
        status_items.push(
            row![status_label("Address".to_string()), status_value(cursor_address_text)]
                .spacing(6)
                .align_y(alignment::Alignment::Center)
                .into(),
        );
        if let Some(offset) = offset_text {
            status_items.push(
                row![status_label("from search hit".to_string()), status_value(offset)]
                    .spacing(6)
                    .align_y(alignment::Alignment::Center)
                    .into(),
            );
        }

        let status_strip = container(row(status_items).spacing(24).align_y(alignment::Alignment::Center))
            .width(Length::Fill)
            .padding([6, 12])
            .style(|theme: &icy_ui::Theme| container::Style {
                background: Some(theme.secondary.base.into()),
                text_color: Some(theme.secondary.on),
                ..Default::default()
            });

        // ---- Info area --------------------------------------------------------
        let info_area = {
            let mut value_bytes = [0u8; 8];
            let bytes_available = if let Some(cursor_address) = cursor_address
                && let Ok(handle) = (pid as process_memory::Pid).try_into_process_handle()
                && let Ok(buf) = copy_address(cursor_address, 8, &handle)
            {
                let n = buf.len().min(8);
                value_bytes[..n].copy_from_slice(&buf[..n]);
                n
            } else {
                0
            };

            let na = || "—".to_string();

            let byte_str = if bytes_available >= 1 { value_bytes[0].to_string() } else { na() };
            let i8_str = if bytes_available >= 1 { format!("{}", value_bytes[0] as i8) } else { na() };
            let u16_str = if bytes_available >= 2 {
                u16::from_le_bytes([value_bytes[0], value_bytes[1]]).to_string()
            } else {
                na()
            };
            let i16_str = if bytes_available >= 2 {
                format!("{}", i16::from_le_bytes([value_bytes[0], value_bytes[1]]))
            } else {
                na()
            };
            let u32_str = if bytes_available >= 4 {
                u32::from_le_bytes([value_bytes[0], value_bytes[1], value_bytes[2], value_bytes[3]]).to_string()
            } else {
                na()
            };
            let i32_str = if bytes_available >= 4 {
                format!("{}", i32::from_le_bytes([value_bytes[0], value_bytes[1], value_bytes[2], value_bytes[3]]))
            } else {
                na()
            };
            let u64_str = if bytes_available >= 8 {
                u64::from_le_bytes(value_bytes).to_string()
            } else {
                na()
            };
            let i64_str = if bytes_available >= 8 {
                format!("{}", i64::from_le_bytes(value_bytes))
            } else {
                na()
            };

            let format_float = |val: f32| -> String {
                if !val.is_finite() {
                    return format!("{val}");
                }
                let abs = val.abs();
                if abs == 0.0 {
                    "0.0".to_string()
                } else if abs >= 1e6 || abs <= 1e-3 {
                    format!("{val:.3e}")
                } else {
                    format!("{val:.4}")
                }
            };
            let format_double = |val: f64| -> String {
                if !val.is_finite() {
                    return format!("{val}");
                }
                let abs = val.abs();
                if abs == 0.0 {
                    "0.0".to_string()
                } else if abs >= 1e7 || abs <= 1e-4 {
                    format!("{val:.4e}")
                } else {
                    format!("{val:.6}")
                }
            };
            let f32_str = if bytes_available >= 4 {
                format_float(f32::from_le_bytes([value_bytes[0], value_bytes[1], value_bytes[2], value_bytes[3]]))
            } else {
                na()
            };
            let f64_str = if bytes_available >= 8 {
                format_double(f64::from_le_bytes(value_bytes))
            } else {
                na()
            };

            let label = |s: &'static str| -> Element<'_, Message> {
                text(s)
                    .size(13)
                    .font(icy_ui::Font::MONOSPACE)
                    .style(|theme: &icy_ui::Theme| icy_ui::widget::text::Style {
                        color: Some(theme.secondary.on.scale_alpha(0.6)),
                    })
                    .into()
            };
            let make_pair = |kind: InspectorValueKind, lbl: &'static str, val: String| -> Element<'_, Message> {
                let edited = self.inspector_edit.as_ref().filter(|(edit_kind, _)| *edit_kind == kind);
                let input_value = edited.map(|(_, t)| t.clone()).unwrap_or(val);
                let parse_error = edited.is_some_and(|(_, t)| Self::inspector_bytes(kind, t).is_err());

                let mut input = text_input("—", &input_value)
                    .on_input(move |value| Message::MemoryEditorInspectorValueChanged(kind, value))
                    .on_submit(Message::MemoryEditorInspectorValueSubmit(kind))
                    .font(icy_ui::Font::MONOSPACE)
                    .size(13)
                    .padding([2, 6])
                    .width(Length::Fixed(170.0));
                if parse_error {
                    input = input.style(|theme: &icy_ui::Theme, status| {
                        let mut style = icy_ui::widget::text_input::default(theme, status);
                        style.value = theme.destructive.base;
                        style.border.color = theme.destructive.base;
                        style
                    });
                }

                row![container(label(lbl)).width(Length::Fixed(44.0)), input]
                    .spacing(8)
                    .align_y(alignment::Alignment::Center)
                    .into()
            };

            let unsigned_col = column![
                make_pair(InspectorValueKind::U8, "u8", byte_str),
                make_pair(InspectorValueKind::U16, "u16", u16_str),
                make_pair(InspectorValueKind::U32, "u32", u32_str),
                make_pair(InspectorValueKind::U64, "u64", u64_str),
            ]
            .spacing(4);
            let signed_col = column![
                make_pair(InspectorValueKind::I8, "i8", i8_str),
                make_pair(InspectorValueKind::I16, "i16", i16_str),
                make_pair(InspectorValueKind::I32, "i32", i32_str),
                make_pair(InspectorValueKind::I64, "i64", i64_str),
            ]
            .spacing(4);
            let float_col = column![
                make_pair(InspectorValueKind::F32, "f32", f32_str),
                make_pair(InspectorValueKind::F64, "f64", f64_str),
            ]
            .spacing(4);

            container(column![row![unsigned_col, signed_col, float_col,].spacing(24)].spacing(10).padding(12))
                .width(Length::Fill)
                .style(|theme: &icy_ui::Theme| container::Style {
                    background: Some(theme.secondary.base.into()),
                    text_color: Some(theme.secondary.on),
                    border: icy_ui::Border {
                        color: theme.primary.divider,
                        width: 1.0,
                        radius: Radius::new(6.0),
                    },
                    ..Default::default()
                })
        };

        // ---- Toolbar ----------------------------------------------------------
        let toolbar = container(
            row![
                text("Memory Editor")
                    .size(18)
                    .style(|theme: &icy_ui::Theme| icy_ui::widget::text::Style { color: Some(theme.primary.on) }),
                text(format!("PID {}", app.state.pid))
                    .size(13)
                    .font(icy_ui::Font::MONOSPACE)
                    .style(|theme: &icy_ui::Theme| icy_ui::widget::text::Style {
                        color: Some(theme.primary.on.scale_alpha(0.55)),
                    }),
                container(
                    row![
                        text("Address").size(13).style(|theme: &icy_ui::Theme| icy_ui::widget::text::Style {
                            color: Some(theme.primary.on.scale_alpha(0.7)),
                        }),
                        text_input("0x…", &self.address_text)
                            .on_input(Message::MemoryEditorAddressChanged)
                            .on_submit(Message::MemoryEditorJumpToAddress)
                            .font(icy_ui::Font::MONOSPACE)
                            .width(Length::Fixed(200.0)),
                        button(text("Go")).on_press(Message::MemoryEditorJumpToAddress).padding([4, 12]),
                    ]
                    .spacing(8)
                    .align_y(alignment::Alignment::Center)
                )
                .width(Length::Fill)
                .align_x(alignment::Alignment::End),
                button(text("Close")).on_press(Message::CloseMemoryEditor).padding([4, 12]),
            ]
            .spacing(16)
            .align_y(alignment::Alignment::Center),
        )
        .padding([10, 12]);

        // ---- Body wrapper -----------------------------------------------------
        // The whole editor is presented as a single primary panel so the
        // toolbar, header, body and info area share one consistent surface.
        container(
            column![
                toolbar,
                rule::horizontal(1),
                region_strip,
                rule::horizontal(1),
                header,
                container(memory_view).style(|theme: &icy_ui::Theme| container::Style {
                    text_color: Some(theme.primary.on),
                    ..Default::default()
                }),
                rule::horizontal(1),
                status_strip,
                rule::horizontal(1),
                container(info_area).padding([10, 12]),
            ]
            .spacing(0),
        )
        .padding(DIALOG_PADDING)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|theme: &icy_ui::Theme| container::Style {
            background: Some(theme.primary.base.into()),
            text_color: Some(theme.primary.on),
            ..Default::default()
        })
        .into()
    }

    pub fn initialize(&mut self, pid: process_memory::Pid, addr: usize, search_type: SearchType) -> Result<(), String> {
        self.address_text = format!("{addr:X}");
        self.editor_initial_address = addr;
        self.editor_initial_size = if search_type == SearchType::String || search_type == SearchType::StringUtf16 {
            1
        } else {
            search_type.fixed_byte_length().unwrap_or(1)
        };

        self.reset_change_tracker();
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.refresh_regions(pid)?;
        self.focus_on(addr)
    }

    /// Clears the change tracker. Called when the editor is opened or closed
    /// so a re-open does not flash bytes that happened to be different from
    /// the values seen during the previous session.
    pub fn reset_change_tracker(&mut self) {
        self.change_tracker.borrow_mut().clear();
    }

    /// Moves the cursor within the virtualized window. Returns `true` when the
    /// cursor row changed (so the caller can ensure it stays visible).
    pub fn move_cursor(&mut self, row_delta: i32, col_delta: i32) -> bool {
        if col_delta != 0 {
            let total_nibbles = BYTES_PER_ROW * 2;
            let current_nibble_pos = self.cursor_col * 2 + self.cursor_nibble;
            let new_nibble_pos = (current_nibble_pos as i32 + col_delta).clamp(0, total_nibbles as i32 - 1) as usize;
            let new_col = new_nibble_pos / 2;
            let new_nibble = new_nibble_pos % 2;
            if new_col != self.cursor_col || new_nibble != self.cursor_nibble {
                self.inspector_edit = None;
            }
            self.cursor_col = new_col;
            self.cursor_nibble = new_nibble;
        }

        if row_delta == 0 {
            return false;
        }

        let new_row = (self.cursor_row as i32 + row_delta).clamp(0, self.total_rows() as i32 - 1) as usize;
        let changed = new_row != self.cursor_row;
        self.cursor_row = new_row;
        if changed {
            self.inspector_edit = None;
        }
        changed
    }

    pub fn set_cursor(&mut self, row: usize, col: usize) {
        self.cursor_row = row.min(self.total_rows() - 1);
        self.cursor_col = col.min(BYTES_PER_ROW - 1);
        self.cursor_nibble = 0;
        self.inspector_edit = None;
    }

    pub fn reset_cursor(&mut self) {
        self.cursor_row = 0;
        self.cursor_col = 0;
        self.cursor_nibble = 0;
        self.inspector_edit = None;
    }

    pub fn edit_hex(&mut self, pid: process_memory::Pid, hex_digit: u8) -> Result<(), String> {
        let Some(address) = self.cursor_address() else {
            return Err("Cursor is not in a readable memory region".to_string());
        };
        let handle = pid.try_into_process_handle().map_err(|e| format!("Failed to attach to process: {e}"))?;
        let buf = copy_address(address, 1, &handle).map_err(|e| format!("Failed to read 0x{address:X}: {e}"))?;
        let current_byte = buf[0];
        let new_byte = if self.cursor_nibble == 0 {
            (hex_digit << 4) | (current_byte & 0x0F)
        } else {
            (current_byte & 0xF0) | hex_digit
        };
        self.write_with_undo(pid, address, &[new_byte])?;

        if self.cursor_nibble == 0 {
            self.cursor_nibble = 1;
        } else {
            self.cursor_nibble = 0;
            self.cursor_col += 1;
            if self.cursor_col >= BYTES_PER_ROW {
                self.cursor_col = 0;
                self.cursor_row = (self.cursor_row + 1).min(self.total_rows() - 1);
            }
        }
        self.inspector_edit = None;
        Ok(())
    }
}
