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
///
/// 2^20 rows × 16 B = 16 MB navigable. The focus address is centered in this
/// window so the user can freely scroll both up (to lower addresses) and down
/// from the search hit.
pub const WINDOW_ROWS: usize = 1 << 20;
pub const ROW_HEIGHT: f32 = 22.0;
pub const PAGE_ROWS: usize = 16;

const SCROLL_ID: &str = "memory-editor-scroll";

fn scroll_id() -> Id {
    Id::new(SCROLL_ID)
}

#[derive(Default)]
pub struct MemoryEditor {
    pub address_text: String,
    /// Address mapped to row 0 of the virtualized window. Centered around the
    /// focus address so the user can scroll both up and down.
    base_address: usize,
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

    pub fn base_address(&self) -> usize {
        self.base_address
    }

    pub fn set_viewport(&mut self, viewport: Viewport) {
        self.viewport = Some(viewport);
    }

    /// Centers the navigable window on the given focus address and places the
    /// cursor on it.
    pub fn focus_on(&mut self, focus_addr: usize) {
        let half = (WINDOW_ROWS / 2) * BYTES_PER_ROW;
        self.base_address = focus_addr.saturating_sub(half);
        let row = (focus_addr - self.base_address) / BYTES_PER_ROW;
        self.cursor_row = row.min(WINDOW_ROWS - 1);
        self.cursor_col = 0;
        self.cursor_nibble = 0;
        self.viewport = None;
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

        let address = self.base_address;
        let pid = app.state.pid;
        let highlight_start = self.editor_initial_address;
        let highlight_end = highlight_start + self.editor_initial_size;
        let cursor_row = self.cursor_row;
        let cursor_col = self.cursor_col;
        let cursor_nibble = self.cursor_nibble;

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

                let make_hex_cell = |absolute_row: usize, col_idx: usize, byte: u8| -> Element<'_, Message> {
                    let is_selected_byte = cursor_row == absolute_row && cursor_col == col_idx;
                    let current_address = address.saturating_add(absolute_row * BYTES_PER_ROW + col_idx);
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

                let make_ascii_cell = |absolute_row: usize, i: usize, byte: u8| -> Element<'_, Message> {
                    let c = byte as char;
                    let is_printable = c.is_ascii_graphic() || c == ' ';
                    let display_char = if is_printable { c.to_string() } else { "·".to_string() };
                    let is_selected = cursor_row == absolute_row && cursor_col == i;
                    let current_address = address.saturating_add(absolute_row * BYTES_PER_ROW + i);
                    let is_initial = current_address >= highlight_start && current_address < highlight_end;

                    container(
                        text(display_char)
                            .size(14)
                            .font(icy_ui::Font::MONOSPACE)
                            .style(move |theme: &icy_ui::Theme| icy_ui::widget::text::Style {
                                color: Some(if is_selected {
                                    theme.accent.base
                                } else if is_printable {
                                    theme.primary.on
                                } else {
                                    theme.primary.on.scale_alpha(0.35)
                                }),
                            }),
                    )
                    .width(Length::Fixed(ASCII_CELL_WIDTH))
                    .align_x(alignment::Alignment::Center)
                    .style(move |theme: &icy_ui::Theme| {
                        if is_selected {
                            container::Style {
                                background: Some(theme.accent.base.scale_alpha(0.30).into()),
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

                column(
                    (range_start..range_end)
                        .map(|absolute_row| {
                            let local_idx = absolute_row - range_start;
                            let row_offset = local_idx * BYTES_PER_ROW;
                            let row_bytes = &memory[row_offset..row_offset + BYTES_PER_ROW];
                            let row_addr = address.saturating_add(absolute_row * BYTES_PER_ROW);
                            let zebra = absolute_row % 2 == 1;
                            let is_cursor_row = cursor_row == absolute_row;

                            let mut hex_left = row![].spacing(0);
                            let mut hex_right = row![].spacing(0);
                            for (col_idx, byte) in row_bytes.iter().enumerate().take(8) {
                                hex_left = hex_left.push(make_hex_cell(absolute_row, col_idx, *byte));
                            }
                            for (col_idx, byte) in row_bytes.iter().enumerate().skip(8) {
                                hex_right = hex_right.push(make_hex_cell(absolute_row, col_idx, *byte));
                            }
                            let hex_block = row![
                                container(hex_left).width(Length::Fixed(HEX_GROUP_WIDTH)),
                                container(hex_right).width(Length::Fixed(HEX_GROUP_WIDTH)),
                            ]
                            .spacing(HEX_GROUP_GAP);

                            let mut ascii_left = row![].spacing(0);
                            let mut ascii_right = row![].spacing(0);
                            for (i, byte) in row_bytes.iter().enumerate().take(8) {
                                ascii_left = ascii_left.push(make_ascii_cell(absolute_row, i, *byte));
                            }
                            for (i, byte) in row_bytes.iter().enumerate().skip(8) {
                                ascii_right = ascii_right.push(make_ascii_cell(absolute_row, i, *byte));
                            }
                            let ascii_block = row![
                                container(ascii_left).width(Length::Fixed(ASCII_GROUP_WIDTH)),
                                container(ascii_right).width(Length::Fixed(ASCII_GROUP_WIDTH)),
                            ]
                            .spacing(ASCII_GROUP_GAP);

                            let address_label = text(format!("{row_addr:016X}"))
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
                .spacing(0)
                .into()
            })
            .on_scroll(Message::MemoryEditorScrolled);

        // ---- Info area --------------------------------------------------------
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

            let fmt_dec_hex = |dec: String, hex: String| -> String { format!("{dec}  ({hex})") };
            let na = || "—".to_string();

            let byte_str = if bytes_available >= 1 {
                let v = value_bytes[0];
                fmt_dec_hex(format!("{v}"), format!("0x{v:02X}"))
            } else {
                na()
            };
            let i8_str = if bytes_available >= 1 { format!("{}", value_bytes[0] as i8) } else { na() };
            let u16_str = if bytes_available >= 2 {
                let v = u16::from_le_bytes([value_bytes[0], value_bytes[1]]);
                fmt_dec_hex(format!("{v}"), format!("0x{v:04X}"))
            } else {
                na()
            };
            let i16_str = if bytes_available >= 2 {
                format!("{}", i16::from_le_bytes([value_bytes[0], value_bytes[1]]))
            } else {
                na()
            };
            let u32_str = if bytes_available >= 4 {
                let v = u32::from_le_bytes([value_bytes[0], value_bytes[1], value_bytes[2], value_bytes[3]]);
                fmt_dec_hex(format!("{v}"), format!("0x{v:08X}"))
            } else {
                na()
            };
            let i32_str = if bytes_available >= 4 {
                format!("{}", i32::from_le_bytes([value_bytes[0], value_bytes[1], value_bytes[2], value_bytes[3]]))
            } else {
                na()
            };
            let u64_str = if bytes_available >= 8 {
                let v = u64::from_le_bytes(value_bytes);
                fmt_dec_hex(format!("{v}"), format!("0x{v:016X}"))
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
            let value = |s: String| -> Element<'_, Message> {
                text(s)
                    .size(14)
                    .font(icy_ui::Font::MONOSPACE)
                    .style(|theme: &icy_ui::Theme| icy_ui::widget::text::Style {
                        color: Some(theme.secondary.on),
                    })
                    .into()
            };

            let make_pair = |lbl: &'static str, val: String| -> Element<'_, Message> {
                row![container(label(lbl)).width(Length::Fixed(56.0)), value(val),]
                    .spacing(8)
                    .align_y(alignment::Alignment::Center)
                    .into()
            };

            let header_line = row![
                text("Cursor")
                    .size(12)
                    .font(icy_ui::Font::MONOSPACE)
                    .style(|theme: &icy_ui::Theme| icy_ui::widget::text::Style {
                        color: Some(theme.secondary.on.scale_alpha(0.6)),
                    }),
                text(format!("0x{cursor_address:016X}"))
                    .size(14)
                    .font(icy_ui::Font::MONOSPACE)
                    .style(|theme: &icy_ui::Theme| icy_ui::widget::text::Style {
                        color: Some(theme.accent.base),
                    }),
            ]
            .spacing(10)
            .align_y(alignment::Alignment::Center);

            let unsigned_col = column![
                make_pair("u8", byte_str),
                make_pair("u16", u16_str),
                make_pair("u32", u32_str),
                make_pair("u64", u64_str),
            ]
            .spacing(4);
            let signed_col = column![
                make_pair("i8", i8_str),
                make_pair("i16", i16_str),
                make_pair("i32", i32_str),
                make_pair("i64", i64_str),
            ]
            .spacing(4);
            let float_col = column![make_pair("f32", f32_str), make_pair("f64", f64_str),].spacing(4);

            container(
                column![header_line, row![unsigned_col, signed_col, float_col,].spacing(24),]
                    .spacing(10)
                    .padding(12),
            )
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
                header,
                container(memory_view).style(|theme: &icy_ui::Theme| container::Style {
                    text_color: Some(theme.primary.on),
                    ..Default::default()
                }),
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

    pub fn initialize(&mut self, addr: usize, search_type: SearchType) {
        self.address_text = format!("{addr:X}");
        self.editor_initial_address = addr;
        self.editor_initial_size = if search_type == SearchType::String || search_type == SearchType::StringUtf16 {
            1
        } else {
            search_type.fixed_byte_length().unwrap_or(1)
        };

        self.focus_on(addr);
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

    pub fn edit_hex(&mut self, pid: process_memory::Pid, hex_digit: u8) -> Result<(), String> {
        let offset = self.cursor_row * BYTES_PER_ROW + self.cursor_col;
        let address = self.base_address.saturating_add(offset);
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
