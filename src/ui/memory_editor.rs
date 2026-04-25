use icy_ui::{
    Element, Length, Task, alignment,
    border::Radius,
    widget::{Id, operation, rule, scrollable::Viewport, text_input},
};
use process_memory::{PutAddress, TryIntoProcessHandle, copy_address};

use crate::{DIALOG_PADDING, SearchType, app::App, message::Message};

pub const BYTES_PER_ROW: usize = 16;
/// Total navigable rows from the current window base address. The view is
/// virtualized so only the visible rows are laid out and read from process
/// memory; the remaining rows are just an empty scroll range.
pub const WINDOW_ROWS: usize = 65_536; // 1 MB navigable from the base address
pub const ROW_HEIGHT: f32 = 22.0;
pub const PAGE_ROWS: usize = 16;

const SCROLL_ID: &str = "memory-editor-scroll";

fn scroll_id() -> Id {
    Id::new(SCROLL_ID)
}

/// Snaps the editor scroll view back to the top of the window.
pub fn snap_to_top<T>() -> Task<T>
where
    T: 'static,
{
    operation::snap_to(scroll_id(), operation::RelativeOffset::START)
}

#[derive(Default)]
pub struct MemoryEditor {
    pub address_text: String,
    cursor_row: usize,
    cursor_col: usize,
    cursor_nibble: usize, // 0 = high nibble, 1 = low nibble

    editor_initial_address: usize,
    editor_initial_size: usize,

    /// Latest viewport reported by the scroll area. Used to decide whether the
    /// cursor row is on-screen and to compute the minimal scroll required to
    /// keep it visible.
    viewport: Option<Viewport>,
}

impl MemoryEditor {
    pub fn cursor_row(&self) -> usize {
        self.cursor_row
    }

    pub fn set_viewport(&mut self, viewport: Viewport) {
        self.viewport = Some(viewport);
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

        let address = app.state.edit_address;
        let pid = app.state.pid;
        let highlight_start = self.editor_initial_address;
        let highlight_end = highlight_start + self.editor_initial_size;
        let cursor_row = self.cursor_row;
        let cursor_col = self.cursor_col;
        let cursor_nibble = self.cursor_nibble;

        // Header row with column labels
        let header = row![
            container(text("Address").size(14).font(icy_ui::Font::MONOSPACE))
                .width(Length::Fixed(95.0))
                .padding(0),
            container({
                let mut hex_headers = row![];
                for i in 0..BYTES_PER_ROW {
                    hex_headers = hex_headers.push(
                        container(text(format!("{i:02X}")).size(14).font(icy_ui::Font::MONOSPACE))
                            .width(Length::Fixed(30.0))
                            .align_x(alignment::Alignment::Center),
                    );
                }
                hex_headers
            })
            .width(Length::Fixed(480.0))
            .padding(0),
            container(text("ASCII").size(14).font(icy_ui::Font::MONOSPACE))
                .width(Length::Fixed(140.0))
                .padding(0),
        ]
        .spacing(0);

        // Virtualized memory view: only visible rows are read and laid out.
        let memory_view = scroll_area()
            .id(scroll_id())
            .height(Length::FillPortion(3))
            .show_rows(ROW_HEIGHT, WINDOW_ROWS, move |range| {
                let range_start = range.start;
                let range_end = range.end;
                let row_count = range_end.saturating_sub(range_start);
                let mut memory = vec![0u8; row_count * BYTES_PER_ROW];
                let start_address = address.saturating_add(range_start * BYTES_PER_ROW);
                if let Ok(handle) = (pid as process_memory::Pid).try_into_process_handle()
                    && let Ok(buf) = copy_address(start_address, memory.len(), &handle)
                {
                    let n = buf.len().min(memory.len());
                    memory[..n].copy_from_slice(&buf[..n]);
                }

                column(
                    (range_start..range_end)
                        .map(|absolute_row| {
                            let local_idx = absolute_row - range_start;
                            let row_offset = local_idx * BYTES_PER_ROW;
                            let row_bytes = &memory[row_offset..row_offset + BYTES_PER_ROW];
                            let row_addr = address.saturating_add(absolute_row * BYTES_PER_ROW);

                            let mut hex_cells = row![];
                            for (col_idx, byte) in row_bytes.iter().enumerate() {
                                let is_selected_byte = cursor_row == absolute_row && cursor_col == col_idx;
                                let current_address = row_addr + col_idx;
                                let is_initial_location = current_address >= highlight_start && current_address < highlight_end;

                                let high_nibble = (byte >> 4) & 0x0F;
                                let low_nibble = byte & 0x0F;

                                let hex_display = if is_selected_byte {
                                    row![
                                        text(format!("{high_nibble:X}"))
                                            .size(14)
                                            .font(icy_ui::Font::MONOSPACE)
                                            .style(move |theme: &icy_ui::Theme| {
                                                if cursor_nibble == 0 {
                                                    icy_ui::widget::text::Style {
                                                        color: Some(theme.accent.base),
                                                    }
                                                } else {
                                                    icy_ui::widget::text::Style {
                                                        color: Some(theme.background.on),
                                                    }
                                                }
                                            }),
                                        text(format!("{low_nibble:X}"))
                                            .size(14)
                                            .font(icy_ui::Font::MONOSPACE)
                                            .style(move |theme: &icy_ui::Theme| {
                                                if cursor_nibble == 1 {
                                                    icy_ui::widget::text::Style {
                                                        color: Some(theme.accent.base),
                                                    }
                                                } else {
                                                    icy_ui::widget::text::Style {
                                                        color: Some(theme.background.on),
                                                    }
                                                }
                                            }),
                                    ]
                                    .spacing(0)
                                } else {
                                    row![
                                        text(format!("{high_nibble:X}")).size(14).font(icy_ui::Font::MONOSPACE),
                                        text(format!("{low_nibble:X}")).size(14).font(icy_ui::Font::MONOSPACE),
                                    ]
                                    .spacing(0)
                                };

                                hex_cells = hex_cells.push(
                                    mouse_area(
                                        container(hex_display)
                                            .width(Length::Fixed(30.0))
                                            .padding(2)
                                            .style(move |theme: &icy_ui::Theme| {
                                                if is_selected_byte {
                                                    container::Style {
                                                        background: Some(theme.background.base.into()),
                                                        text_color: Some(theme.background.on),
                                                        border: icy_ui::Border {
                                                            color: theme.accent.base,
                                                            width: 2.0,
                                                            radius: Radius::new(4.0),
                                                        },
                                                        ..Default::default()
                                                    }
                                                } else if is_initial_location {
                                                    container::Style {
                                                        background: Some(theme.accent.hover.into()),
                                                        text_color: Some(theme.background.on),
                                                        border: icy_ui::Border {
                                                            color: theme.accent.hover,
                                                            width: 1.0,
                                                            radius: Radius::new(0.0),
                                                        },
                                                        ..Default::default()
                                                    }
                                                } else {
                                                    container::Style::default()
                                                }
                                            }),
                                    )
                                    .on_press(Message::MemoryEditorSetCursor(absolute_row, col_idx)),
                                );
                            }

                            let ascii = row_bytes
                                .iter()
                                .enumerate()
                                .map(|(i, b)| {
                                    let c = *b as char;
                                    let display_char = if c.is_ascii_graphic() || c == ' ' { c.to_string() } else { ".".to_string() };
                                    let is_selected = cursor_row == absolute_row && cursor_col == i;
                                    let current_address = row_addr + i;
                                    let is_initial_location = current_address >= highlight_start && current_address < highlight_end;

                                    container(text(display_char).size(14).font(icy_ui::Font::MONOSPACE))
                                        .width(Length::Fixed(8.0))
                                        .align_x(alignment::Alignment::Center)
                                        .style(move |theme: &icy_ui::Theme| {
                                            if is_selected {
                                                container::Style {
                                                    background: Some(theme.accent.base.into()),
                                                    text_color: Some(theme.background.base),
                                                    ..Default::default()
                                                }
                                            } else if is_initial_location {
                                                container::Style {
                                                    background: Some(theme.success.hover.into()),
                                                    text_color: Some(theme.background.on),
                                                    ..Default::default()
                                                }
                                            } else {
                                                container::Style::default()
                                            }
                                        })
                                })
                                .fold(row![], |row, elem| row.push(elem));

                            row![
                                container(text(format!("{row_addr:08X}")).size(14).font(icy_ui::Font::MONOSPACE))
                                    .width(Length::Fixed(100.0))
                                    .padding(0),
                                container(hex_cells).width(Length::Fixed(480.0)).padding(0),
                                container(ascii).width(Length::Fixed(140.0)).padding(0),
                            ]
                            .height(Length::Fixed(ROW_HEIGHT))
                            .spacing(0)
                            .into()
                        })
                        .collect::<Vec<Element<'_, Message>>>(),
                )
                .spacing(0)
                .into()
            })
            .on_scroll(Message::MemoryEditorScrolled);

        // Info area showing values at the cursor position
        let info_area = {
            let cursor_address = address.saturating_add(cursor_row * BYTES_PER_ROW + cursor_col);

            let mut value_bytes = [0u8; 8];
            let bytes_available = if let Ok(handle) = (pid as process_memory::Pid).try_into_process_handle()
                && let Ok(buf) = copy_address(cursor_address, 8, &handle)
            {
                let n = buf.len().min(8);
                value_bytes[..n].copy_from_slice(&buf[..n]);
                n
            } else {
                0
            };

            let byte_val = if bytes_available >= 1 { value_bytes[0] } else { 0 };
            let u16_val = if bytes_available >= 2 {
                u16::from_le_bytes([value_bytes[0], value_bytes[1]])
            } else {
                0
            };
            let u32_val = if bytes_available >= 4 {
                u32::from_le_bytes([value_bytes[0], value_bytes[1], value_bytes[2], value_bytes[3]])
            } else {
                0
            };
            let u64_val = if bytes_available >= 8 { u64::from_le_bytes(value_bytes) } else { 0 };
            let float_val = if bytes_available >= 4 {
                f32::from_le_bytes([value_bytes[0], value_bytes[1], value_bytes[2], value_bytes[3]])
            } else {
                0.0
            };
            let double_val = if bytes_available >= 8 { f64::from_le_bytes(value_bytes) } else { 0.0 };

            container(
                column![
                    row![text(format!("Cursor: 0x{cursor_address:08X}")).size(14).font(icy_ui::Font::MONOSPACE),].spacing(20),
                    row![
                        column![
                            text("Byte:").size(14).font(icy_ui::Font::MONOSPACE),
                            text("U16:").size(14).font(icy_ui::Font::MONOSPACE),
                            text("U32:").size(14).font(icy_ui::Font::MONOSPACE),
                            text("U64:").size(14).font(icy_ui::Font::MONOSPACE),
                        ]
                        .width(Length::Fixed(60.0))
                        .spacing(5),
                        column![
                            text(format!("{byte_val}")).size(14).font(icy_ui::Font::MONOSPACE),
                            text(format!("{u16_val}")).size(14).font(icy_ui::Font::MONOSPACE),
                            text(format!("{u32_val}")).size(14).font(icy_ui::Font::MONOSPACE),
                            text(format!("{u64_val}")).size(14).font(icy_ui::Font::MONOSPACE),
                        ]
                        .width(Length::Fixed(200.0))
                        .spacing(5),
                        column![
                            text("Float:").size(14).font(icy_ui::Font::MONOSPACE),
                            text("Double:").size(14).font(icy_ui::Font::MONOSPACE),
                        ]
                        .width(Length::Fixed(60.0))
                        .spacing(5),
                        column![
                            text(if bytes_available >= 4 {
                                if float_val.is_finite() {
                                    let abs_val = float_val.abs();
                                    if abs_val == 0.0 {
                                        "0.0".to_string()
                                    } else if abs_val >= 1e6 || abs_val <= 1e-3 {
                                        format!("{float_val:.3e}")
                                    } else if abs_val >= 1000.0 {
                                        format!("{float_val:.1}")
                                    } else if abs_val >= 1.0 {
                                        format!("{float_val:.3}")
                                    } else {
                                        format!("{float_val:.4}")
                                    }
                                } else {
                                    format!("{float_val}")
                                }
                            } else {
                                "N/A".to_string()
                            })
                            .size(14)
                            .font(icy_ui::Font::MONOSPACE),
                            text(if bytes_available >= 8 {
                                if double_val.is_finite() {
                                    let abs_val = double_val.abs();
                                    if abs_val == 0.0 {
                                        "0.0".to_string()
                                    } else if abs_val >= 1e7 || abs_val <= 1e-4 {
                                        format!("{double_val:.4e}")
                                    } else if abs_val >= 1000.0 {
                                        format!("{double_val:.2}")
                                    } else if abs_val >= 1.0 {
                                        format!("{double_val:.4}")
                                    } else {
                                        format!("{double_val:.6}")
                                    }
                                } else {
                                    format!("{double_val}")
                                }
                            } else {
                                "N/A".to_string()
                            })
                            .size(14)
                            .font(icy_ui::Font::MONOSPACE),
                        ]
                        .spacing(5),
                    ]
                    .spacing(20)
                ]
                .spacing(10)
                .padding(10),
            )
            .width(Length::Fill)
            .style(|theme: &icy_ui::Theme| container::Style {
                background: Some(theme.primary.base.into()),
                border: icy_ui::Border {
                    color: theme.secondary.base,
                    width: 1.0,
                    radius: Radius::new(4.0),
                },
                ..Default::default()
            })
        };

        container(
            column![
                row![
                    text(format!("Memory Editor - PID: {}", app.state.pid)).size(20),
                    container(
                        row![
                            text("Address:"),
                            text_input("0x", &self.address_text)
                                .on_input(Message::MemoryEditorAddressChanged)
                                .on_submit(Message::MemoryEditorJumpToAddress)
                                .width(Length::Fixed(120.0)),
                            button(text("Go")).on_press(Message::MemoryEditorJumpToAddress).padding(5),
                        ]
                        .spacing(10)
                        .align_y(alignment::Alignment::Center)
                    )
                    .width(Length::Fill),
                    button(text("Close")).on_press(Message::CloseMemoryEditor).padding(10)
                ]
                .spacing(20)
                .align_y(alignment::Alignment::Center),
                rule::horizontal(1),
                container(header).style(|theme: &icy_ui::Theme| container::Style {
                    background: Some(theme.primary.base.into()),
                    ..Default::default()
                }),
                memory_view,
                info_area,
            ]
            .spacing(0)
            .padding(DIALOG_PADDING),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    pub fn initialize(&mut self, addr: usize, search_type: SearchType) {
        self.address_text = format!("{addr:X}");
        self.editor_initial_address = addr;
        self.editor_initial_size = if search_type == SearchType::String || search_type == SearchType::StringUtf16 {
            1
        } else {
            search_type.fixed_byte_length().unwrap_or(1)
        };

        self.cursor_row = 0;
        self.cursor_col = 0;
        self.cursor_nibble = 0;
        self.viewport = None;
    }

    /// Moves the cursor within the virtualized window. Returns `true` when the
    /// cursor row changed (so the caller can ensure it stays visible).
    pub fn move_cursor(&mut self, row_delta: i32, col_delta: i32) -> bool {
        if col_delta != 0 {
            let total_nibbles = BYTES_PER_ROW * 2;
            let current_nibble_pos = self.cursor_col * 2 + self.cursor_nibble;
            let new_nibble_pos = (current_nibble_pos as i32 + col_delta).clamp(0, total_nibbles as i32 - 1) as usize;
            self.cursor_col = new_nibble_pos / 2;
            self.cursor_nibble = new_nibble_pos % 2;
        }

        if row_delta == 0 {
            return false;
        }

        let new_row = (self.cursor_row as i32 + row_delta).clamp(0, WINDOW_ROWS as i32 - 1) as usize;
        let changed = new_row != self.cursor_row;
        self.cursor_row = new_row;
        changed
    }

    pub fn set_cursor(&mut self, row: usize, col: usize) {
        self.cursor_row = row.min(WINDOW_ROWS - 1);
        self.cursor_col = col.min(BYTES_PER_ROW - 1);
        self.cursor_nibble = 0;
    }

    pub fn reset_cursor(&mut self) {
        self.cursor_row = 0;
        self.cursor_col = 0;
        self.cursor_nibble = 0;
    }

    pub fn edit_hex(&mut self, edit_address: usize, pid: process_memory::Pid, hex_digit: u8) -> Result<(), String> {
        let offset = self.cursor_row * BYTES_PER_ROW + self.cursor_col;
        let address = edit_address.saturating_add(offset);
        let handle = pid.try_into_process_handle().map_err(|e| format!("Failed to attach to process: {e}"))?;
        let buf = copy_address(address, 1, &handle).map_err(|e| format!("Failed to read 0x{address:X}: {e}"))?;
        let current_byte = buf[0];
        let new_byte = if self.cursor_nibble == 0 {
            (hex_digit << 4) | (current_byte & 0x0F)
        } else {
            (current_byte & 0xF0) | hex_digit
        };
        handle
            .put_address(address, &[new_byte])
            .map_err(|e| format!("Failed to write 0x{address:X}: {e}"))?;

        if self.cursor_nibble == 0 {
            self.cursor_nibble = 1;
        } else {
            self.cursor_nibble = 0;
            self.cursor_col += 1;
            if self.cursor_col >= BYTES_PER_ROW {
                self.cursor_col = 0;
                self.cursor_row = (self.cursor_row + 1).min(WINDOW_ROWS - 1);
            }
        }
        Ok(())
    }
}
