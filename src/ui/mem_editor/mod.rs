//! Live-process memory editor.
//!
//! Thin integration layer on top of the vendored hex editor in
//! [`raw`] (originally `egui_memory_editor` by Hirtol, MIT/Apache-2.0).
//!
//! Responsibilities of this wrapper:
//!
//! - Translate the target's `proc_maps` regions into the vendored editor's
//!   address ranges so the user gets a region dropdown with a meaningful
//!   label (`name@start  size`).
//! - Maintain a `byte cache` populated once per tick from the editor's
//!   visible range. The vendored editor calls `read_fn(addr)` many times
//!   per frame — without caching this would translate to one
//!   `copy_address` syscall per visible byte every frame; with caching
//!   we issue exactly one syscall covering the entire visible window.
//! - Track per-byte change timestamps for fade-in highlights.
//! - Wire writes back through `process_memory::PutAddress`.

mod navigation;
pub mod raw;

use std::collections::HashMap;
use std::ops::Range;
use std::time::{Duration, Instant};

use i18n_embed_fl::fl;
use proc_maps::get_process_maps;
use process_memory::{ProcessHandle, PutAddress, TryIntoProcessHandle, copy_address};

use crate::{SearchType, ui::app::App};

use self::raw::{
    Address, MemoryEditor as RawEditor,
    option_data::{Endianness, MemoryEditorOptions},
};

/// Highlight fade duration for recently-changed bytes.
const CHANGE_FADE: Duration = Duration::from_millis(1500);

/// Outer padding when refreshing the cache — read a little above and
/// below the visible range so light scrolling doesn't show `--` flicker.
const CACHE_PREFETCH: usize = 256;

/// Maximum size of a single cache fill in bytes. Guards against accidental
/// reads of huge regions if `visible_range()` ever returns something
/// degenerate.
const MAX_CACHE_READ: usize = 64 * 1024;

/// Maximum number of write operations remembered for undo/redo. Older
/// entries are dropped from the bottom of the stack once the cap is hit.
const UNDO_STACK_LIMIT: usize = 256;

/// One reversible write: the bytes that lived at `addr` before the write
/// and the bytes the user (or undo/redo) replaced them with. Multi-byte
/// writes (inspector edits) are stored as a single record so undo
/// restores the entire value atomically. The caret state at the moment
/// the write happened is also captured so undo/redo restores the user's
/// editing context, not just the bytes.
#[derive(Debug, Clone)]
struct WriteRecord {
    addr: Address,
    before: Vec<u8>,
    after: Vec<u8>,
    /// Caret position before the write (so undo can put the cursor back
    /// where the user was when they made the change).
    caret_before: Option<(Address, bool)>,
    /// Caret position right after the write (so redo restores the
    /// post-write cursor — typically one nibble past `addr`).
    caret_after: Option<(Address, bool)>,
}

#[derive(Debug, Clone)]
struct RegionInfo {
    range: Range<Address>,
    label: String,
    name: String,
    readable: bool,
    writable: bool,
}

pub struct MemoryEditor {
    raw: RawEditor,
    data: EditorData,
}

struct EditorData {
    /// Process handle captured at the start of each frame. The vendored
    /// closures need access via the `mem: &mut T` parameter, so we stash
    /// it here and clear it on close.
    handle: Option<ProcessHandle>,
    /// Cache populated by [`MemoryEditor::tick`] covering the visible
    /// range + padding. `None` for bytes that failed to read.
    cache: HashMap<Address, Option<u8>>,
    /// Per-byte last value + time it most recently changed. Drives the
    /// short orange highlight after a write or natural change.
    change_tracker: HashMap<Address, (u8, Instant)>,
    /// Regions parsed from `proc_maps`, kept around so the region info
    /// panel can describe the address the editor is focused on.
    regions: Vec<RegionInfo>,
    /// Address the editor was opened at — used for the "back to origin"
    /// jump button and for region info.
    origin_address: Address,
    /// In-progress inspector edit: which address/kind the user is typing
    /// into and the partial text. Cleared on submit, on Escape, or
    /// whenever the highlighted address moves away.
    inspector_edit: Option<InspectorEdit>,
    /// Failed inspector submission, retained alongside the edit for retry.
    inspector_error: Option<String>,
    /// Address navigation is independent of memory-write undo/redo.
    navigation: navigation::NavigationHistory,
    /// Collapsed state of the bottom inspector panel, so the hex grid can
    /// reclaim its height.
    inspector_collapsed: bool,
    /// Search type of the result the editor was opened on. Drives the
    /// type-picker in the toolbar and the size of the result-range
    /// highlight.
    current_result_type: Option<SearchType>,
    /// Byte length of the result the editor was opened on. Together
    /// with `origin_address` this gives the address range painted with
    /// the result-highlight accent.
    current_result_byte_length: usize,
    /// Reversible writes performed since the editor was opened.
    /// Pushed by [`apply_write`]; popped by `Ctrl+Z`.
    undo_stack: Vec<WriteRecord>,
    /// Writes that were just undone and can be re-applied with `Ctrl+Y`.
    /// Cleared as soon as the user makes a fresh write.
    redo_stack: Vec<WriteRecord>,
    /// Caret position captured at the start of the current frame so the
    /// hex-grid write closure (which only sees `EditorData`, not the raw
    /// editor) can stamp it onto new undo records.
    caret_snapshot: Option<(Address, bool)>,
}

/// The numeric interpretations exposed by the bottom data-inspector
/// panel. Each kind is editable: typing a value and pressing Enter
/// writes the equivalent bytes back to the target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum InspectorKind {
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

const INSPECTOR_NUMERIC_TYPES: &[SearchType] = &[
    SearchType::Byte,
    SearchType::Short,
    SearchType::Int,
    SearchType::Int64,
    SearchType::Float,
    SearchType::Double,
];

impl InspectorKind {
    const fn byte_count(self) -> usize {
        match self {
            InspectorKind::U8 | InspectorKind::I8 => 1,
            InspectorKind::U16 | InspectorKind::I16 => 2,
            InspectorKind::U32 | InspectorKind::I32 | InspectorKind::F32 => 4,
            InspectorKind::U64 | InspectorKind::I64 | InspectorKind::F64 => 8,
        }
    }

    const fn label(self) -> &'static str {
        match self {
            InspectorKind::U8 => "u8",
            InspectorKind::I8 => "i8",
            InspectorKind::U16 => "u16",
            InspectorKind::I16 => "i16",
            InspectorKind::U32 => "u32",
            InspectorKind::I32 => "i32",
            InspectorKind::U64 => "u64",
            InspectorKind::I64 => "i64",
            InspectorKind::F32 => "f32",
            InspectorKind::F64 => "f64",
        }
    }

    /// Row matching the search type the editor was opened on.
    fn from_search_type(search_type: SearchType) -> Option<Self> {
        Some(match search_type {
            SearchType::Byte => InspectorKind::U8,
            SearchType::Short => InspectorKind::I16,
            SearchType::Int => InspectorKind::I32,
            SearchType::Int64 => InspectorKind::I64,
            SearchType::Float => InspectorKind::F32,
            SearchType::Double => InspectorKind::F64,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone)]
struct InspectorEdit {
    address: Address,
    kind: InspectorKind,
    text: String,
}

impl Default for MemoryEditor {
    fn default() -> Self {
        Self {
            raw: RawEditor::new().with_options(default_options()),
            data: EditorData {
                handle: None,
                cache: HashMap::new(),
                change_tracker: HashMap::new(),
                regions: Vec::new(),
                origin_address: 0,
                inspector_edit: None,
                inspector_error: None,
                navigation: navigation::NavigationHistory::default(),
                inspector_collapsed: false,
                current_result_type: None,
                current_result_byte_length: 1,
                undo_stack: Vec::new(),
                redo_stack: Vec::new(),
                caret_snapshot: None,
            },
        }
    }
}

fn default_options() -> MemoryEditorOptions {
    // The crate's defaults assume a light theme and clash with our dark
    // visuals. Tweak the colours to read well on dark backgrounds.
    MemoryEditorOptions {
        address_text_colour: egui::Color32::from_rgb(150, 160, 200),
        highlight_text_colour: egui::Color32::from_rgb(255, 180, 130),
        zero_colour: egui::Color32::from_gray(90),
        ..MemoryEditorOptions::default()
    }
}

impl MemoryEditor {
    /// (Re-)scan the target's memory map and configure the editor to open
    /// on the region containing `address`. `byte_length` is the size of the
    /// result currently being edited (used for the result-range highlight).
    /// Returns an error string ready to be pushed onto the app's error state
    /// if the map can't be read.
    pub fn initialize(&mut self, pid: process_memory::Pid, address: usize, search_type: SearchType, byte_length: usize) -> Result<(), String> {
        let pid_t = pid;
        let maps = get_process_maps(pid_t).map_err(|e| fl!(crate::LANGUAGE_LOADER, "memory-editor-error-read-map", pid = pid, error = e.to_string()))?;

        self.data.regions.clear();
        self.raw.clear_address_ranges();

        for m in maps {
            let start = m.start();
            let size = m.size();
            if size == 0 || !m.is_read() {
                continue;
            }
            let end = start.saturating_add(size);
            let name = m
                .filename()
                .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
                .unwrap_or_else(|| fl!(crate::LANGUAGE_LOADER, "memory-editor-region-anonymous"));

            // Combo-box label: address-prefixed name so duplicates from the
            // same shared library are still individually addressable, and
            // size shown with binary prefix.
            let label = format!("{name} @ 0x{start:X}  ({})", human_bytes(size));

            self.data.regions.push(RegionInfo {
                range: start..end,
                label: label.clone(),
                name,
                readable: m.is_read(),
                writable: m.is_write(),
            });
            self.raw.set_address_range(label, start..end);
        }

        if self.data.regions.is_empty() {
            return Err(fl!(crate::LANGUAGE_LOADER, "memory-editor-error-no-regions", pid = pid));
        }

        self.data.origin_address = address;
        self.data.navigation = navigation::NavigationHistory::default();
        self.data.inspector_edit = None;
        self.data.inspector_error = None;
        self.data.cache.clear();
        self.data.change_tracker.clear();
        self.data.undo_stack.clear();
        self.data.redo_stack.clear();
        self.data.current_result_type = Some(search_type);
        self.data.current_result_byte_length = byte_length.max(1);
        self.raw.goto_address(address);
        self.raw
            .set_result_highlight_range(Some(address..address.saturating_add(self.data.current_result_byte_length)));

        // Warm the cache once so the first frame already shows bytes
        // rather than the `--` placeholder.
        self.fill_cache_around(pid_t, address.saturating_sub(CACHE_PREFETCH)..address.saturating_add(CACHE_PREFETCH));

        Ok(())
    }

    /// Public accessor for the search type the editor was opened on.
    pub fn current_result_type(&self) -> Option<SearchType> {
        self.data.current_result_type
    }

    /// Update the in-editor representation of the current result's type.
    /// Refreshes the highlight range that paints the bytes belonging to
    /// the result. Called from [`App::change_result_type`] after the
    /// cached search results have been mutated.
    pub fn update_result_type(&mut self, search_type: SearchType, byte_length: usize) {
        self.data.current_result_type = Some(search_type);
        self.data.current_result_byte_length = byte_length.max(1);
        let address = self.data.origin_address;
        self.raw
            .set_result_highlight_range(Some(address..address.saturating_add(self.data.current_result_byte_length)));
    }

    pub fn reset_change_tracker(&mut self) {
        self.data.change_tracker.clear();
        self.data.cache.clear();
        self.data.handle = None;
        self.data.inspector_edit = None;
        self.data.inspector_error = None;
        self.data.navigation = navigation::NavigationHistory::default();
        self.data.current_result_type = None;
        self.raw.set_result_highlight_range(None);
    }

    /// Per-frame syscall: re-read the bytes the editor was showing last
    /// frame plus a small padding band. Called from `App::tick` while in
    /// the [`crate::ui::app::AppState::MemoryEditor`] state.
    pub fn tick(&mut self, pid: process_memory::Pid) {
        let visible = self.raw.visible_range().clone();
        if visible.is_empty() {
            // First frame after open: no visible range yet. Cache was
            // already warmed in `initialize` so just return.
            return;
        }
        let start = visible.start.saturating_sub(CACHE_PREFETCH);
        let end = visible.end.saturating_add(CACHE_PREFETCH);
        self.fill_cache_around(pid, start..end);

        // Don't let the change tracker grow indefinitely while the editor
        // is open on a long session.
        self.data.change_tracker.retain(|_, (_, t)| t.elapsed() < Duration::from_secs(60));
    }

    /// Read `range` into `data.cache` in as few syscalls as possible.
    /// The range is clipped to readable regions; bytes that fall outside
    /// any region are marked `None`.
    fn fill_cache_around(&mut self, pid: process_memory::Pid, range: Range<Address>) {
        let Ok(handle) = pid.try_into_process_handle() else {
            return;
        };
        // Forget bytes far outside the new window so the cache stays bounded.
        let keep_start = range.start.saturating_sub(CACHE_PREFETCH);
        let keep_end = range.end.saturating_add(CACHE_PREFETCH);
        self.data.cache.retain(|addr, _| *addr >= keep_start && *addr < keep_end);

        let now = Instant::now();
        let mut cursor = range.start;
        while cursor < range.end {
            // Find a region we can read from at `cursor`. If there isn't
            // one, advance to the next region's start (or end of range).
            let region = self.data.regions.iter().find(|r| r.range.contains(&cursor) && r.readable);
            let segment_end = match region {
                Some(r) => range.end.min(r.range.end),
                None => self
                    .data
                    .regions
                    .iter()
                    .filter(|r| r.range.start > cursor && r.readable)
                    .map(|r| r.range.start)
                    .min()
                    .unwrap_or(range.end)
                    .min(range.end),
            };

            if let Some(_r) = region {
                let mut chunk_start = cursor;
                while chunk_start < segment_end {
                    let chunk_end = (chunk_start + MAX_CACHE_READ).min(segment_end);
                    let len = chunk_end - chunk_start;
                    match copy_address(chunk_start, len, &handle) {
                        Ok(buf) => {
                            for (i, b) in buf.iter().enumerate() {
                                let addr = chunk_start + i;
                                match self.data.change_tracker.get(&addr).map(|(v, _)| *v) {
                                    Some(prev) if prev != *b => {
                                        self.data.change_tracker.insert(addr, (*b, now));
                                    }
                                    None => {
                                        // First time we see this byte —
                                        // don't flash it.
                                        self.data.change_tracker.insert(addr, (*b, now - CHANGE_FADE));
                                    }
                                    _ => {}
                                }
                                self.data.cache.insert(addr, Some(*b));
                            }
                        }
                        Err(_) => {
                            for offset in 0..len {
                                self.data.cache.insert(chunk_start + offset, None);
                            }
                        }
                    }
                    chunk_start = chunk_end;
                }
            } else {
                // Gap between regions — mark as unreadable.
                for addr in cursor..segment_end {
                    self.data.cache.insert(addr, None);
                }
            }

            // Avoid getting stuck if find() returned an empty segment.
            cursor = segment_end.max(cursor + 1);
        }
    }

    /// Region currently containing `address` (if any), for the info bar.
    fn region_of(&self, address: Address) -> Option<&RegionInfo> {
        self.data.regions.iter().find(|r| r.range.contains(&address))
    }

    /// Programmatically jump to and highlight the original opening address.
    pub fn goto_origin(&mut self) {
        let addr = self.data.origin_address;
        self.navigate_to(addr);
    }

    pub fn origin_address(&self) -> Address {
        self.data.origin_address
    }

    pub fn can_undo(&self) -> bool {
        !self.data.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.data.redo_stack.is_empty()
    }

    /// Revert the most recent write. The byte(s) are written back to the
    /// target with their pre-write value and the record is moved onto
    /// the redo stack. No-op if [`Self::can_undo`] is `false` or the
    /// process handle is missing.
    pub fn undo(&mut self) -> bool {
        let Some(record) = self.data.undo_stack.pop() else {
            return false;
        };
        if write_raw(&self.data, record.addr, &record.before) {
            apply_to_cache(&mut self.data, record.addr, &record.before);
            self.raw.set_caret(record.caret_before);
            self.data.redo_stack.push(record);
            cap_stack(&mut self.data.redo_stack);
            true
        } else {
            // Push the record back so the user can retry once the write
            // failure is resolved (e.g. region became writable again).
            self.data.undo_stack.push(record);
            false
        }
    }

    /// Re-apply the most recently undone write.
    pub fn redo(&mut self) -> bool {
        let Some(record) = self.data.redo_stack.pop() else {
            return false;
        };
        if write_raw(&self.data, record.addr, &record.after) {
            apply_to_cache(&mut self.data, record.addr, &record.after);
            self.raw.set_caret(record.caret_after);
            self.data.undo_stack.push(record);
            cap_stack(&mut self.data.undo_stack);
            true
        } else {
            self.data.redo_stack.push(record);
            false
        }
    }
}

fn human_bytes(n: usize) -> String {
    const KIB: usize = 1024;
    const MIB: usize = KIB * 1024;
    const GIB: usize = MIB * 1024;
    if n >= GIB {
        format!("{:.1} GiB", n as f64 / GIB as f64)
    } else if n >= MIB {
        format!("{:.1} MiB", n as f64 / MIB as f64)
    } else if n >= KIB {
        format!("{:.1} KiB", n as f64 / KIB as f64)
    } else {
        format!("{n} B")
    }
}

/// Read up to 8 bytes starting at `addr` from the cache (with a one-shot
/// `copy_address` fallback). The returned tuple is the buffer (zero-padded
/// when bytes are missing) and the number of bytes that were actually
/// available, so the inspector can render `--` when the requested type is
/// wider than the readable window.
fn read_inspector_bytes(data: &mut EditorData, addr: Address) -> ([u8; 8], usize) {
    let mut out = [0u8; 8];
    let mut available = 0usize;
    for (i, slot) in out.iter_mut().enumerate() {
        let target = match addr.checked_add(i) {
            Some(a) => a,
            None => break,
        };
        let byte = match data.cache.get(&target).copied() {
            Some(Some(b)) => Some(b),
            Some(None) => None,
            None => {
                // Fall back to a one-shot read so the inspector still
                // works on the very first frame after the highlight
                // moves to a fresh address.
                let handle = data.handle.as_ref();
                handle
                    .and_then(|h| copy_address(target, 1, h).ok().and_then(|buf| buf.first().copied()))
                    .inspect(|b| {
                        data.cache.insert(target, Some(*b));
                    })
            }
        };
        match byte {
            Some(b) => {
                *slot = b;
                available = i + 1;
            }
            None => break,
        }
    }
    (out, available)
}

fn format_float_f32(val: f32) -> String {
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
}

fn format_float_f64(val: f64) -> String {
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
}

fn format_kind(kind: InspectorKind, bytes: &[u8; 8], available: usize, endian: Endianness) -> Option<String> {
    if available < kind.byte_count() {
        return None;
    }
    let be = matches!(endian, Endianness::Big);
    Some(match kind {
        InspectorKind::U8 => u8::from_le_bytes([bytes[0]]).to_string(),
        InspectorKind::I8 => (bytes[0] as i8).to_string(),
        InspectorKind::U16 => {
            let arr = [bytes[0], bytes[1]];
            if be { u16::from_be_bytes(arr) } else { u16::from_le_bytes(arr) }.to_string()
        }
        InspectorKind::I16 => {
            let arr = [bytes[0], bytes[1]];
            if be { i16::from_be_bytes(arr) } else { i16::from_le_bytes(arr) }.to_string()
        }
        InspectorKind::U32 => {
            let arr = [bytes[0], bytes[1], bytes[2], bytes[3]];
            if be { u32::from_be_bytes(arr) } else { u32::from_le_bytes(arr) }.to_string()
        }
        InspectorKind::I32 => {
            let arr = [bytes[0], bytes[1], bytes[2], bytes[3]];
            if be { i32::from_be_bytes(arr) } else { i32::from_le_bytes(arr) }.to_string()
        }
        InspectorKind::U64 => if be { u64::from_be_bytes(*bytes) } else { u64::from_le_bytes(*bytes) }.to_string(),
        InspectorKind::I64 => if be { i64::from_be_bytes(*bytes) } else { i64::from_le_bytes(*bytes) }.to_string(),
        InspectorKind::F32 => {
            let arr = [bytes[0], bytes[1], bytes[2], bytes[3]];
            format_float_f32(if be { f32::from_be_bytes(arr) } else { f32::from_le_bytes(arr) })
        }
        InspectorKind::F64 => format_float_f64(if be { f64::from_be_bytes(*bytes) } else { f64::from_le_bytes(*bytes) }),
    })
}

fn read_uint(bytes: &[u8; 8], width: usize, endian: Endianness) -> u64 {
    let slice = &bytes[..width.min(8)];
    match endian {
        Endianness::Big => slice.iter().fold(0u64, |acc, &b| (acc << 8) | b as u64),
        Endianness::Little => slice.iter().rev().fold(0u64, |acc, &b| (acc << 8) | b as u64),
    }
}

/// The widest unsigned value that fits the readable window, as hex.
fn format_hex(bytes: &[u8; 8], available: usize, endian: Endianness) -> Option<String> {
    let width = match available {
        0 => return None,
        1 => 1,
        2..=3 => 2,
        4..=7 => 4,
        _ => 8,
    };
    let value = read_uint(bytes, width, endian);
    Some(format!("0x{value:0>digits$X}", digits = width * 2))
}

/// The readable window interpreted as a pointer and resolved against the
/// target's memory map, so following a struct doesn't need a manual lookup.
fn format_pointer(regions: &[RegionInfo], bytes: &[u8; 8], available: usize, endian: Endianness) -> Option<String> {
    let value = pointer_address(bytes, available, endian)?;
    let target = regions
        .iter()
        .find(|region| region.range.contains(&value))
        .map(|region| region.name.clone())
        .unwrap_or_else(|| fl!(crate::LANGUAGE_LOADER, "memory-editor-region-unmapped"));
    Some(format!("0x{value:X}  {target}"))
}

fn pointer_address(bytes: &[u8; 8], available: usize, endian: Endianness) -> Option<Address> {
    if available < 8 {
        return None;
    }
    Address::try_from(read_uint(bytes, 8, endian)).ok()
}

fn quiet_field_visuals(ui: &mut egui::Ui) {
    let visuals = ui.visuals_mut();
    visuals.extreme_bg_color = egui::Color32::TRANSPARENT;
    visuals.widgets.inactive.bg_stroke = egui::Stroke::NONE;
    visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(4);
    visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(4);
    visuals.widgets.active.corner_radius = egui::CornerRadius::same(4);
}

/// The inspector stacks many short rows, so the app-wide control metrics
/// would cost the hex grid several lines.
fn compact_spacing(ui: &mut egui::Ui) {
    let spacing = ui.spacing_mut();
    spacing.item_spacing.y = 3.0;
    spacing.interact_size.y = 18.0;
    spacing.button_padding = egui::vec2(6.0, 1.0);
}

/// Painted rather than drawn from a font so the arrow can't turn into tofu.
fn collapse_arrow(ui: &mut egui::Ui, collapsed: bool, color: egui::Color32) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::click());
    let center = rect.center();
    let size = 4.0;
    let points = if collapsed {
        vec![
            egui::pos2(center.x - size * 0.5, center.y - size),
            egui::pos2(center.x - size * 0.5, center.y + size),
            egui::pos2(center.x + size, center.y),
        ]
    } else {
        vec![
            egui::pos2(center.x - size, center.y - size * 0.5),
            egui::pos2(center.x + size, center.y - size * 0.5),
            egui::pos2(center.x, center.y + size),
        ]
    };
    ui.painter().add(egui::Shape::convex_polygon(points, color, egui::Stroke::NONE));
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Everything a single inspector row needs about the current address.
struct InspectorContext {
    addr: Address,
    bytes: [u8; 8],
    available: usize,
    endian: Endianness,
    active_kind: Option<InspectorKind>,
}

/// Parse `input` as the given kind and return the bytes that should be
/// written to memory in the given endianness.
fn parse_kind(kind: InspectorKind, input: &str, endian: Endianness) -> Result<Vec<u8>, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("empty".to_string());
    }
    let be = matches!(endian, Endianness::Big);
    // `to_le_bytes()` is the canonical layout; reverse to swap to big.
    let swap = |mut v: Vec<u8>| -> Vec<u8> {
        if be {
            v.reverse();
        }
        v
    };
    match kind {
        InspectorKind::U8 => trimmed.parse::<u8>().map(|v| vec![v]).map_err(|e| e.to_string()),
        InspectorKind::I8 => trimmed.parse::<i8>().map(|v| vec![v as u8]).map_err(|e| e.to_string()),
        InspectorKind::U16 => trimmed.parse::<u16>().map(|v| swap(v.to_le_bytes().to_vec())).map_err(|e| e.to_string()),
        InspectorKind::I16 => trimmed.parse::<i16>().map(|v| swap(v.to_le_bytes().to_vec())).map_err(|e| e.to_string()),
        InspectorKind::U32 => trimmed.parse::<u32>().map(|v| swap(v.to_le_bytes().to_vec())).map_err(|e| e.to_string()),
        InspectorKind::I32 => trimmed.parse::<i32>().map(|v| swap(v.to_le_bytes().to_vec())).map_err(|e| e.to_string()),
        InspectorKind::U64 => trimmed.parse::<u64>().map(|v| swap(v.to_le_bytes().to_vec())).map_err(|e| e.to_string()),
        InspectorKind::I64 => trimmed.parse::<i64>().map(|v| swap(v.to_le_bytes().to_vec())).map_err(|e| e.to_string()),
        InspectorKind::F32 => trimmed.parse::<f32>().map(|v| swap(v.to_le_bytes().to_vec())).map_err(|e| e.to_string()),
        InspectorKind::F64 => trimmed.parse::<f64>().map(|v| swap(v.to_le_bytes().to_vec())).map_err(|e| e.to_string()),
    }
}

/// Keep navigation/actions separate from location details so long region
/// names cannot push the Close button out of the window.
fn editor_header(editor: &mut MemoryEditor, pid: process_memory::Pid, ui: &mut egui::Ui) -> bool {
    let accent = ui.visuals().selection.bg_fill;
    let mut close = false;

    egui::Panel::top("memory_editor_top")
        .frame(
            egui::Frame::new()
                .fill(egui::Color32::from_rgb(22, 25, 29))
                .inner_margin(egui::Margin::symmetric(20, 10))
                .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(45, 50, 60))),
        )
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(fl!(crate::LANGUAGE_LOADER, "memory-editor-title"))
                        .size(15.0)
                        .strong()
                        .color(accent),
                );
                ui.add_space(6.0);
                ui.label(egui::RichText::new(format!("PID {pid}")).size(13.0).weak().monospace());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(fl!(crate::LANGUAGE_LOADER, "close-button")).clicked() {
                        close = true;
                    }
                });
            });
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().button_padding = egui::vec2(8.0, 4.0);
                ui.spacing_mut().interact_size.y = 28.0;
                let button = |text: String| egui::Button::new(egui::RichText::new(text).size(13.0)).frame(false);
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(editor.can_go_back(), button("⏴".to_owned()))
                        .on_hover_text(fl!(crate::LANGUAGE_LOADER, "memory-editor-nav-back"))
                        .clicked()
                    {
                        editor.go_back();
                    }
                    if ui
                        .add_enabled(editor.can_go_forward(), button("⏵".to_owned()))
                        .on_hover_text(fl!(crate::LANGUAGE_LOADER, "memory-editor-nav-forward"))
                        .clicked()
                    {
                        editor.go_forward();
                    }
                    if ui
                        .add(button(fl!(crate::LANGUAGE_LOADER, "memory-editor-origin-button")))
                        .on_hover_text(format!(
                            "{}\n0x{:X}",
                            fl!(crate::LANGUAGE_LOADER, "memory-editor-origin-tooltip"),
                            editor.origin_address()
                        ))
                        .clicked()
                    {
                        editor.goto_origin();
                    }
                });
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(editor.can_undo(), button(fl!(crate::LANGUAGE_LOADER, "undo-button")))
                        .on_hover_text(fl!(crate::LANGUAGE_LOADER, "memory-editor-undo-tooltip"))
                        .clicked()
                    {
                        editor.undo();
                    }
                    if ui
                        .add_enabled(editor.can_redo(), button(fl!(crate::LANGUAGE_LOADER, "memory-editor-redo-button")))
                        .on_hover_text(fl!(crate::LANGUAGE_LOADER, "memory-editor-redo-tooltip"))
                        .clicked()
                    {
                        editor.redo();
                    }
                });
            });
            ui.separator();
            let address = editor.current_address();
            let region = editor.region_of(address);
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(fl!(crate::LANGUAGE_LOADER, "memory-editor-address-label"))
                        .size(12.0)
                        .weak(),
                );
                ui.add(
                    egui::Label::new(egui::RichText::new(format!("0x{address:X}")).size(13.0).monospace())
                        .wrap_mode(egui::TextWrapMode::Extend)
                        .selectable(true),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(region) = region {
                        let (access, tooltip) = match (region.readable, region.writable) {
                            (true, true) => ("rw", fl!(crate::LANGUAGE_LOADER, "memory-editor-access-rw")),
                            (true, false) => ("r-", fl!(crate::LANGUAGE_LOADER, "memory-editor-access-r")),
                            (false, true) => ("-w", fl!(crate::LANGUAGE_LOADER, "memory-editor-access-w")),
                            (false, false) => ("--", fl!(crate::LANGUAGE_LOADER, "memory-editor-access-none")),
                        };
                        ui.label(egui::RichText::new(format!("[{access}]")).size(12.0).monospace())
                            .on_hover_text(tooltip);
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            ui.add(egui::Label::new(egui::RichText::new(&region.label).size(12.0).weak()).truncate())
                                .on_hover_text(&region.label);
                        });
                    } else {
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(fl!(crate::LANGUAGE_LOADER, "memory-editor-region-unmapped"))
                                    .size(12.0)
                                    .weak(),
                            )
                            .truncate(),
                        );
                    }
                });
            });
        });
    close
}

pub fn view_memory_editor(app: &mut App, ui: &mut egui::Ui) {
    let pid_t = app.state.pid as process_memory::Pid;
    if editor_header(&mut app.memory_editor, pid_t, ui) {
        app.close_memory_editor();
        return;
    }

    // Establish the process handle for this frame at the outer level so
    // both the inspector bottom panel and the hex grid central panel can
    // see it. Cleared each frame; closures inside `draw_editor_contents`
    // read it via the shared `data.handle` field.
    app.memory_editor.data.handle = pid_t.try_into_process_handle().ok();
    let handle_attached = app.memory_editor.data.handle.is_some();

    // Ctrl+Z / Ctrl+Y / Ctrl+Shift+Z — undo & redo for memory writes.
    // Suppressed while a text field has focus so the shortcut still
    // performs in-field undo on Goto / Inspector edits.
    if handle_attached && !ui.ctx().text_edit_focused() {
        let (undo, redo) = ui.ctx().input(|i| {
            let ctrl = i.modifiers.command;
            let shift = i.modifiers.shift;
            let undo = ctrl && !shift && i.key_pressed(egui::Key::Z);
            let redo = ctrl && (i.key_pressed(egui::Key::Y) || (shift && i.key_pressed(egui::Key::Z)));
            (undo, redo)
        });
        if undo {
            app.memory_editor.undo();
        }
        if redo {
            app.memory_editor.redo();
        }
    }

    // Inspector lives in its own bottom panel so the hex grid scroll
    // area knows its exact height and `visible_range` matches what the
    // user actually sees. Without this, the scroll area inside the
    // central panel grew taller than the visible viewport, which made
    // arrow-key scrolling stutter when moving down past the last row.
    if handle_attached {
        let mut requested_type = None;
        egui::Panel::bottom("memory_editor_inspector")
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(egui::Color32::from_rgb(22, 25, 30))
                    .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(55, 62, 72)))
                    .inner_margin(egui::Margin::symmetric(16, 6)),
            )
            .show(ui, |ui| {
                requested_type = inspector_body(&mut app.memory_editor, ui);
            });
        if let Some(new_type) = requested_type {
            app.change_result_type(new_type);
        }
    }

    egui::CentralPanel::default()
        .frame(egui::Frame::central_panel(ui.style()).inner_margin(egui::Margin::symmetric(20, 14)))
        .show(ui, |ui| {
            if !handle_attached {
                ui.colored_label(
                    egui::Color32::from_rgb(220, 120, 120),
                    fl!(crate::LANGUAGE_LOADER, "memory-editor-error-attach", error = "process unavailable".to_string()),
                );
                return;
            }

            // Destructure so we can borrow the raw editor and the data
            // separately — the vendored API takes `&mut T` and we want T
            // to be our `EditorData` so the closures can mutate the cache.
            let MemoryEditor { raw, data } = &mut app.memory_editor;

            // Capture caret state so the per-byte write closure (which
            // only sees `EditorData`) can stamp it onto new undo
            // records as `caret_before`.
            data.caret_snapshot = raw.caret();
            let undo_len_before = data.undo_stack.len();

            raw.draw_editor_contents_with_highlight(
                ui,
                data,
                |d, addr| match d.cache.get(&addr).copied() {
                    Some(Some(b)) => Some(b),
                    Some(None) => None,
                    None => {
                        // Cache miss — fall back to a one-shot read so
                        // the very first frame after opening isn't
                        // blank. The batched tick() will pick this up
                        // afterwards.
                        let handle = d.handle.as_ref()?;
                        match copy_address(addr, 1, handle) {
                            Ok(buf) if !buf.is_empty() => {
                                d.cache.insert(addr, Some(buf[0]));
                                Some(buf[0])
                            }
                            _ => {
                                d.cache.insert(addr, None);
                                None
                            }
                        }
                    }
                },
                |d, addr, byte| {
                    let caret = d.caret_snapshot;
                    apply_write(d, addr, &[byte], caret);
                },
                |d, addr| change_intensity(d, addr),
            );

            // The hex grid advances the caret one nibble after a write;
            // patch the post-write caret onto every undo record added
            // this frame so redo restores the user's editing context.
            let caret_after = raw.caret();
            for record in &mut data.undo_stack[undo_len_before..] {
                record.caret_after = caret_after;
            }

            // Animate the change-flash overlay smoothly even when there's
            // no user input. Only request a repaint while at least one
            // tracked byte is still inside the fade window.
            let now = Instant::now();
            let needs_repaint = data.change_tracker.values().any(|(_, ts)| now.duration_since(*ts) < CHANGE_FADE);
            if needs_repaint {
                ui.ctx().request_repaint();
            }
        });
}

/// Render the inspector body. The caller is responsible for wrapping
/// this in a `Panel::bottom` (or similar) so the hex grid above gets a
/// well-defined remaining height. Shows the bytes at the currently
/// highlighted address decoded as every common numeric type; each
/// entry is editable, Enter writes the value to the target.
fn inspector_body(editor: &mut MemoryEditor, ui: &mut egui::Ui) -> Option<SearchType> {
    compact_spacing(ui);
    let accent = ui.visuals().selection.bg_fill;
    let highlight = editor.raw.highlighted_address();
    let endian = editor.raw.endianness();
    let collapsed = editor.data.inspector_collapsed;
    let mut toggle_collapsed = false;
    let mut toggle_endian = false;
    let mut requested_type = None;

    ui.horizontal(|ui| {
        if collapse_arrow(ui, collapsed, accent).clicked() {
            toggle_collapsed = true;
        }
        let title = egui::Button::new(
            egui::RichText::new(fl!(crate::LANGUAGE_LOADER, "memory-editor-inspector-title"))
                .size(14.0)
                .strong()
                .color(accent),
        )
        .frame(false);
        if ui.add(title).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            toggle_collapsed = true;
        }
        ui.add_space(4.0);
        match highlight {
            Some(addr) => {
                ui.label(egui::RichText::new(format!("@ 0x{addr:X}")).size(13.0).monospace().weak());
            }
            None => {
                ui.label(
                    egui::RichText::new(fl!(crate::LANGUAGE_LOADER, "memory-editor-inspector-select-hint"))
                        .size(13.0)
                        .weak()
                        .italics(),
                );
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(fl!(crate::LANGUAGE_LOADER, "memory-editor-inspector-edit-hint"))
                    .size(12.0)
                    .weak(),
            );
            ui.add_space(10.0);
            if let Some(current_type) = editor.current_result_type() {
                let mut selected = current_type;
                let combo = egui::ComboBox::from_id_salt("memory_editor_result_type")
                    .selected_text(current_type.get_short_description_text())
                    .width(84.0);
                if INSPECTOR_NUMERIC_TYPES.contains(&current_type) {
                    let response = combo.show_ui(ui, |ui| {
                        ui.set_max_width(320.0);
                        ui.add(egui::Label::new(fl!(crate::LANGUAGE_LOADER, "type-width-warning")).wrap());
                        ui.separator();
                        for &ty in INSPECTOR_NUMERIC_TYPES {
                            ui.selectable_value(&mut selected, ty, ty.get_short_description_text());
                        }
                    });
                    response
                        .response
                        .on_hover_text(fl!(crate::LANGUAGE_LOADER, "memory-editor-inspector-type-tooltip"));
                    if selected != current_type {
                        requested_type = Some(selected);
                    }
                } else {
                    ui.add_enabled_ui(false, |ui| {
                        let _ = combo.show_ui(ui, |_| {});
                    })
                    .response
                    .on_hover_text(fl!(crate::LANGUAGE_LOADER, "memory-editor-inspector-variable-type-tooltip"));
                }
                ui.label(
                    egui::RichText::new(fl!(crate::LANGUAGE_LOADER, "memory-editor-inspector-type-label"))
                        .size(12.0)
                        .weak(),
                );
                ui.add_space(10.0);
            }
            let label = match endian {
                Endianness::Little => "little-endian",
                Endianness::Big => "big-endian",
            };
            let toggle = egui::Button::new(egui::RichText::new(label).size(12.0).color(accent)).frame(false);
            if ui
                .add(toggle)
                .on_hover_text(fl!(crate::LANGUAGE_LOADER, "memory-editor-inspector-endian-tooltip"))
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                toggle_endian = true;
            }
        });
    });

    if toggle_collapsed {
        editor.data.inspector_collapsed = !collapsed;
    }
    if toggle_endian {
        editor.raw.set_endianness(match endian {
            Endianness::Little => Endianness::Big,
            Endianness::Big => Endianness::Little,
        });
        editor.data.inspector_edit = None;
        editor.data.inspector_error = None;
    }

    let Some(addr) = highlight.filter(|_| !editor.data.inspector_collapsed) else {
        return requested_type;
    };

    // Read 8 bytes (cache-first, syscall fallback) and forget any
    // stale edit buffer that doesn't match this address.
    let (bytes, available) = read_inspector_bytes(&mut editor.data, addr);
    if let Some(edit) = editor.data.inspector_edit.as_ref()
        && edit.address != addr
    {
        editor.data.inspector_edit = None;
        editor.data.inspector_error = None;
    }

    let endian = editor.raw.endianness();
    let context = InspectorContext {
        addr,
        bytes,
        available,
        endian,
        active_kind: editor.data.current_result_type.and_then(InspectorKind::from_search_type),
    };
    let hex = format_hex(&bytes, available, endian);

    ui.add_space(2.0);
    ui.columns(3, |cols| {
        cols[0].label(
            egui::RichText::new(fl!(crate::LANGUAGE_LOADER, "memory-editor-inspector-unsigned"))
                .size(12.0)
                .weak(),
        );
        cols[1].label(
            egui::RichText::new(fl!(crate::LANGUAGE_LOADER, "memory-editor-inspector-signed"))
                .size(12.0)
                .weak(),
        );
        cols[2].label(
            egui::RichText::new(fl!(crate::LANGUAGE_LOADER, "memory-editor-inspector-float-raw"))
                .size(12.0)
                .weak(),
        );
        for col in &mut cols[..] {
            col.separator();
        }
        inspector_column(
            &mut cols[0],
            &mut editor.data,
            &context,
            &[InspectorKind::U8, InspectorKind::U16, InspectorKind::U32, InspectorKind::U64],
        );
        inspector_column(
            &mut cols[1],
            &mut editor.data,
            &context,
            &[InspectorKind::I8, InspectorKind::I16, InspectorKind::I32, InspectorKind::I64],
        );
        inspector_column(&mut cols[2], &mut editor.data, &context, &[InspectorKind::F32, InspectorKind::F64]);
        inspector_readonly_row(&mut cols[2], "hex", hex);
    });
    ui.add_space(3.0);
    ui.separator();
    inspector_pointer_row(editor, ui, &bytes, available, endian);
    if let Some(error) = &editor.data.inspector_error {
        ui.colored_label(ui.visuals().error_fg_color, error);
    }
    requested_type
}

/// The pointer gets the full inspector width: its region name need not fight
/// with the numeric columns, and following it never writes to the process.
fn inspector_pointer_row(editor: &mut MemoryEditor, ui: &mut egui::Ui, bytes: &[u8; 8], available: usize, endian: Endianness) {
    let target = pointer_address(bytes, available, endian);
    let readable = target.and_then(|address| editor.region_of(address)).is_some_and(|region| region.readable);
    let text = format_pointer(&editor.data.regions, bytes, available, endian).unwrap_or_else(|| "--".to_owned());
    let mut follow = false;
    ui.horizontal(|ui| {
        ui.add_sized(egui::vec2(30.0, 18.0), egui::Label::new(egui::RichText::new("ptr").monospace().weak()));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            follow = ui
                .add_enabled(readable, egui::Button::new(fl!(crate::LANGUAGE_LOADER, "memory-editor-follow-pointer")))
                .on_hover_text(if readable {
                    fl!(crate::LANGUAGE_LOADER, "memory-editor-follow-pointer-tooltip")
                } else {
                    fl!(crate::LANGUAGE_LOADER, "memory-editor-pointer-unavailable")
                })
                .clicked();
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.add(egui::Label::new(egui::RichText::new(&text).monospace()).truncate().selectable(true))
                    .on_hover_text(&text);
            });
        });
    });
    if follow && let Some(address) = target {
        editor.navigate_to(address);
    }
}

/// Derived value that has no meaningful write path, shown in the same grid
/// as the editable rows.
fn inspector_readonly_row(ui: &mut egui::Ui, label: &str, value: Option<String>) {
    ui.horizontal(|ui| {
        ui.add_sized(egui::vec2(30.0, 18.0), egui::Label::new(egui::RichText::new(label).monospace().weak()));
        match value {
            Some(text) => {
                ui.add(egui::Label::new(egui::RichText::new(&text).monospace()).truncate().selectable(true))
                    .on_hover_text(text);
            }
            None => {
                let color = ui.visuals().widgets.inactive.fg_stroke.color;
                ui.label(egui::RichText::new("--").monospace().color(color));
            }
        }
    });
}

fn inspector_column(ui: &mut egui::Ui, data: &mut EditorData, ctx: &InspectorContext, kinds: &[InspectorKind]) {
    quiet_field_visuals(ui);
    let accent = ui.visuals().selection.bg_fill;

    for &kind in kinds {
        ui.horizontal(|ui| {
            let label = egui::RichText::new(kind.label()).monospace();
            let label = if ctx.active_kind == Some(kind) {
                label.strong().color(accent)
            } else {
                label.weak()
            };
            ui.add_sized(egui::vec2(30.0, 18.0), egui::Label::new(label));

            let displayed_value = format_kind(kind, &ctx.bytes, ctx.available, ctx.endian);
            let editing = matches!(&data.inspector_edit, Some(edit) if edit.kind == kind && edit.address == ctx.addr);

            // The displayed buffer is either the live edit text or the
            // current value rendered as a string. `--` when the type
            // doesn't fit in the readable window.
            let mut buf = if editing {
                data.inspector_edit.as_ref().map(|e| e.text.clone()).unwrap_or_default()
            } else {
                displayed_value.clone().unwrap_or_else(|| "--".to_string())
            };

            let parse_error = editing && parse_kind(kind, &buf, ctx.endian).is_err();
            let text_color = if parse_error {
                Some(egui::Color32::from_rgb(220, 120, 120))
            } else if displayed_value.is_none() {
                Some(ui.visuals().widgets.inactive.fg_stroke.color)
            } else {
                None
            };

            let response = ui.add(
                egui::TextEdit::singleline(&mut buf)
                    .id_salt(("inspector-value", ctx.addr, kind))
                    .interactive(displayed_value.is_some())
                    .desired_width((ui.available_width() - 4.0).max(60.0))
                    .margin(egui::Margin::symmetric(4, 1))
                    .font(egui::TextStyle::Monospace)
                    .text_color_opt(text_color),
            );

            if displayed_value.is_none() {
                response
                    .clone()
                    .on_hover_text(fl!(crate::LANGUAGE_LOADER, "memory-editor-inspector-readable-hint", count = kind.byte_count()));
            }

            if response.changed() {
                data.inspector_error = None;
                data.inspector_edit = Some(InspectorEdit {
                    address: ctx.addr,
                    kind,
                    text: buf.clone(),
                });
            }

            // A single-line TextEdit surrenders focus on Enter before it
            // returns its Response. Include lost_focus or the commit is lost.
            let owns_edit = matches!(&data.inspector_edit, Some(edit) if edit.kind == kind && edit.address == ctx.addr);
            let had_focus = response.has_focus() || response.lost_focus();
            let enter_pressed = owns_edit && had_focus && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let escape_pressed = owns_edit && had_focus && ui.input(|i| i.key_pressed(egui::Key::Escape));

            // Commit only on Enter. Live-writing while typing is too easy
            // to trigger accidentally and can produce surprising transient
            // values in the target process.
            if enter_pressed {
                match parse_kind(kind, &buf, ctx.endian) {
                    Ok(write_bytes) => {
                        // Only discard the edit after a successful write.
                        // A failure must remain visible and retryable.
                        if apply_write(data, ctx.addr, &write_bytes, Some((ctx.addr, false))) {
                            data.inspector_edit = None;
                            data.inspector_error = None;
                            response.surrender_focus();
                        } else {
                            data.inspector_error = Some(fl!(
                                crate::LANGUAGE_LOADER,
                                "memory-editor-inspector-write-failed",
                                address = format!("{:X}", ctx.addr)
                            ));
                            response.request_focus();
                        }
                    }
                    Err(error) => {
                        data.inspector_error = Some(fl!(
                            crate::LANGUAGE_LOADER,
                            "memory-editor-error-invalid-value",
                            kind = kind.label(),
                            input = buf.clone(),
                            error = error
                        ));
                        response.request_focus();
                    }
                }
            } else if owns_edit && (escape_pressed || response.lost_focus()) {
                data.inspector_edit = None;
                data.inspector_error = None;
            }
        });
    }
}

/// Write `bytes` starting at `addr` to the target process and update the
/// cache/change-tracker so the UI reflects the write immediately. Also
/// captures the previous bytes from the cache, pushes an undo entry,
/// and clears the redo stack. Returns whether the write succeeded.
///
/// `caret_before` is the caret position at the moment the user
/// initiated the write — used by undo to restore the editing context.
/// The corresponding `caret_after` is filled in by the surrounding
/// frame logic once the post-write caret is known.
fn apply_write(data: &mut EditorData, addr: Address, bytes: &[u8], caret_before: Option<(Address, bool)>) -> bool {
    if bytes.is_empty() || data.handle.is_none() {
        return false;
    }

    // Snapshot what's currently at `addr..addr+bytes.len()` so the write
    // is reversible. Bytes that aren't cached fall back to a fresh read
    // from the target — this is the only path that needs the original
    // value, so we accept the extra syscall here.
    let before = read_current(data, addr, bytes.len());

    if !write_raw(data, addr, bytes) {
        return false;
    }
    apply_to_cache(data, addr, bytes);

    if before.as_slice() != bytes {
        data.redo_stack.clear();
        data.undo_stack.push(WriteRecord {
            addr,
            before,
            after: bytes.to_vec(),
            caret_before,
            // Filled in after the panel finishes drawing this frame, so
            // the post-write caret reflects any movement (e.g. nibble
            // advance) the editor performed in response to this write.
            caret_after: caret_before,
        });
        cap_stack(&mut data.undo_stack);
    }

    true
}

/// Read `len` bytes starting at `addr`, preferring cached values and
/// falling back to a one-shot `copy_address` for any byte not in cache.
/// Missing bytes are reported as `0`.
fn read_current(data: &EditorData, addr: Address, len: usize) -> Vec<u8> {
    let handle = data.handle.as_ref();
    (0..len)
        .map(|i| {
            let a = addr + i;
            match data.cache.get(&a).copied() {
                Some(Some(b)) => b,
                _ => handle.and_then(|h| copy_address(a, 1, h).ok()).and_then(|v| v.first().copied()).unwrap_or(0),
            }
        })
        .collect()
}

/// Best-effort raw write to the target. Does not touch undo/redo state.
fn write_raw(data: &EditorData, addr: Address, bytes: &[u8]) -> bool {
    match data.handle.as_ref() {
        Some(handle) => handle.put_address(addr, bytes).is_ok(),
        None => false,
    }
}

/// Mark `addr..addr+bytes.len()` as written in the cache and the
/// change-tracker so the highlight fade kicks in.
fn apply_to_cache(data: &mut EditorData, addr: Address, bytes: &[u8]) {
    let now = Instant::now();
    for (i, b) in bytes.iter().enumerate() {
        let a = addr + i;
        data.cache.insert(a, Some(*b));
        data.change_tracker.insert(a, (*b, now));
    }
}

fn cap_stack(stack: &mut Vec<WriteRecord>) {
    if stack.len() > UNDO_STACK_LIMIT {
        let overflow = stack.len() - UNDO_STACK_LIMIT;
        stack.drain(0..overflow);
    }
}

/// Fade intensity in `0.0..=1.0` for the byte at `addr`. Returns `0.0`
/// for bytes that haven't changed recently or aren't tracked yet. Drives
/// the orange overlay drawn by the hex grid and the ASCII sidebar.
fn change_intensity(data: &EditorData, addr: Address) -> f32 {
    let (_, ts) = match data.change_tracker.get(&addr) {
        Some(entry) => entry,
        None => return 0.0,
    };
    let elapsed = ts.elapsed();
    if elapsed >= CHANGE_FADE {
        return 0.0;
    }
    1.0 - (elapsed.as_secs_f32() / CHANGE_FADE.as_secs_f32())
}

#[cfg(test)]
mod interaction_tests;

#[cfg(test)]
mod tests {
    use super::*;

    const BYTES: [u8; 8] = [0x39, 0x30, 0x00, 0x00, 0x11, 0x22, 0x33, 0x44];

    #[test]
    fn hex_row_uses_the_widest_type_that_fits_the_readable_window() {
        assert_eq!(format_hex(&BYTES, 8, Endianness::Little).unwrap(), "0x4433221100003039");
        assert_eq!(format_hex(&BYTES, 4, Endianness::Little).unwrap(), "0x00003039");
        assert_eq!(format_hex(&BYTES, 2, Endianness::Little).unwrap(), "0x3039");
        assert_eq!(format_hex(&BYTES, 1, Endianness::Little).unwrap(), "0x39");
        assert!(format_hex(&BYTES, 0, Endianness::Little).is_none());
    }

    #[test]
    fn hex_row_follows_the_selected_byte_order() {
        assert_eq!(format_hex(&BYTES, 4, Endianness::Big).unwrap(), "0x39300000");
    }

    #[test]
    fn pointer_row_needs_a_full_address_width() {
        assert!(format_pointer(&[], &BYTES, 7, Endianness::Little).is_none());
        assert!(format_pointer(&[], &BYTES, 8, Endianness::Little).is_some());
    }

    #[test]
    fn pointer_row_names_the_region_it_points_into() {
        let regions = vec![RegionInfo {
            range: 0x1000..0x2000,
            label: "heap @ 0x1000".to_string(),
            name: "heap".to_string(),
            readable: true,
            writable: true,
        }];
        let mut bytes = [0u8; 8];
        bytes[..8].copy_from_slice(&0x1500u64.to_le_bytes());

        let rendered = format_pointer(&regions, &bytes, 8, Endianness::Little).unwrap();

        assert!(rendered.contains("0x1500"), "{rendered}");
        assert!(rendered.contains("heap"), "{rendered}");
    }

    #[test]
    fn inspector_highlights_the_row_matching_the_search_type() {
        assert_eq!(InspectorKind::from_search_type(SearchType::Int), Some(InspectorKind::I32));
        assert_eq!(InspectorKind::from_search_type(SearchType::Byte), Some(InspectorKind::U8));
        assert_eq!(InspectorKind::from_search_type(SearchType::Double), Some(InspectorKind::F64));
        assert_eq!(InspectorKind::from_search_type(SearchType::String), None);
    }
}
