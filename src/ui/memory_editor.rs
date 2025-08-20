use iced::{
    Element, Length, alignment,
    border::Radius,
    widget::{column, horizontal_rule, row, text_input},
};
use process_memory::{PutAddress, TryIntoProcessHandle, copy_address};

use crate::{DIALOG_PADDING, SearchType, app::App, message::Message};

#[derive(Default)]
pub struct MemoryEditor {
    pub address_text: String,
    cursor_row: usize,
    cursor_col: usize,
    cursor_nibble: usize, // 0 = high nibble, 1 = low nibble

    editor_initial_address: usize, // Add this to track the initial address
    editor_initial_size: usize,    // Add this to track the size of the initial value
}

impl MemoryEditor {
    // Update the show_memory_editor function to highlight the initial bytes:
    pub fn show_memory_editor(&self, app: &App) -> Element<'_, Message> {
        use iced::widget::{button, column, container, mouse_area, row, scrollable, text};

        const BYTES_PER_ROW: usize = 16;
        const MAX_ROWS: usize = 24;

        let address = app.state.edit_address;
        let total_bytes = BYTES_PER_ROW * MAX_ROWS;
        let mut memory = vec![0u8; total_bytes];

        if let Ok(handle) = (app.state.pid as process_memory::Pid).try_into_process_handle() {
            if let Ok(buf) = copy_address(address, total_bytes, &handle) {
                memory[..buf.len().min(total_bytes)].copy_from_slice(&buf[..buf.len().min(total_bytes)]);
            }
        }

        // Calculate which bytes to highlight
        let highlight_start = self.editor_initial_address;
        let highlight_end = highlight_start + self.editor_initial_size;

        // Build header row (unchanged)
        let header = row![
            container(text("Address").size(14).font(iced::Font::MONOSPACE))
                .width(Length::Fixed(95.0))
                .padding(0),
            container({
                let mut hex_headers = row![];
                for i in 0..BYTES_PER_ROW {
                    hex_headers = hex_headers.push(
                        container(text(format!("{:02X}", i)).size(14).font(iced::Font::MONOSPACE))
                            .width(Length::Fixed(30.0))
                            .align_x(alignment::Alignment::Center),
                    );
                }
                hex_headers
            })
            .width(Length::Fixed(480.0))
            .padding(0),
            container(text("ASCII").size(14).font(iced::Font::MONOSPACE))
                .width(Length::Fixed(140.0))
                .padding(0),
        ]
        .spacing(0);

        // Build hex rows
        let mut rows: Vec<iced::widget::Row<'_, Message>> = Vec::new();
        let actual_rows = memory.len() / BYTES_PER_ROW;

        for row_idx in 0..actual_rows {
            let offset = row_idx * BYTES_PER_ROW;
            let row_bytes = &memory[offset..offset + BYTES_PER_ROW];

            // Hex cells with nibble highlighting
            let mut hex_cells = row![];
            for (col_idx, byte) in row_bytes.iter().enumerate() {
                let is_selected_byte = self.cursor_row == row_idx && self.cursor_col == col_idx;
                let current_address = address + offset + col_idx;
                let is_initial_location = current_address >= highlight_start && current_address < highlight_end;

                let high_nibble = (byte >> 4) & 0x0F;
                let low_nibble = byte & 0x0F;

                let hex_display = if is_selected_byte {
                    row![
                        text(format!("{:X}", high_nibble))
                            .size(14)
                            .font(iced::Font::MONOSPACE)
                            .style(move |theme: &iced::Theme| {
                                if self.cursor_nibble == 0 {
                                    iced::widget::text::Style {
                                        color: Some(theme.palette().primary),
                                    }
                                } else {
                                    iced::widget::text::Style {
                                        color: Some(theme.palette().text),
                                    }
                                }
                            }),
                        text(format!("{:X}", low_nibble))
                            .size(14)
                            .font(iced::Font::MONOSPACE)
                            .style(move |theme: &iced::Theme| {
                                if self.cursor_nibble == 1 {
                                    iced::widget::text::Style {
                                        color: Some(theme.palette().primary),
                                    }
                                } else {
                                    iced::widget::text::Style {
                                        color: Some(theme.palette().text),
                                    }
                                }
                            }),
                    ]
                    .spacing(0)
                } else {
                    row![
                        text(format!("{:X}", high_nibble)).size(14).font(iced::Font::MONOSPACE),
                        text(format!("{:X}", low_nibble)).size(14).font(iced::Font::MONOSPACE),
                    ]
                    .spacing(0)
                };

                hex_cells = hex_cells.push(
                    mouse_area(container(hex_display).width(Length::Fixed(30.0)).padding(2).style(move |theme: &iced::Theme| {
                        if is_selected_byte {
                            container::Style {
                                background: Some(theme.palette().background.into()),
                                text_color: Some(theme.palette().text),
                                border: iced::Border {
                                    color: theme.palette().primary,
                                    width: 2.0,
                                    radius: Radius::new(4.0),
                                },
                                ..Default::default()
                            }
                        } else if is_initial_location {
                            // Highlight initial location with a different background
                            container::Style {
                                background: Some(theme.extended_palette().primary.weak.color.into()),
                                text_color: Some(theme.palette().text),
                                border: iced::Border {
                                    color: theme.extended_palette().primary.weak.color,
                                    width: 1.0,
                                    radius: Radius::new(0.0),
                                },
                                ..Default::default()
                            }
                        } else {
                            container::Style::default()
                        }
                    }))
                    .on_press(Message::MemoryEditorSetCursor(row_idx, col_idx)),
                );
            }

            // ASCII representation
            let ascii = row_bytes
                .iter()
                .enumerate()
                .map(|(i, b)| {
                    let c = *b as char;
                    let display_char = if c.is_ascii_graphic() || c == ' ' { c.to_string() } else { ".".to_string() };
                    let is_selected = self.cursor_row == row_idx && self.cursor_col == i;
                    let current_address = address + offset + i;
                    let is_initial_location = current_address >= highlight_start && current_address < highlight_end;

                    container(text(display_char).size(14).font(iced::Font::MONOSPACE))
                        .width(Length::Fixed(8.0))
                        .align_x(alignment::Alignment::Center)
                        .style(move |theme: &iced::Theme| {
                            if is_selected {
                                container::Style {
                                    background: Some(theme.palette().primary.into()),
                                    text_color: Some(theme.palette().background),
                                    ..Default::default()
                                }
                            } else if is_initial_location {
                                container::Style {
                                    background: Some(theme.extended_palette().success.weak.color.into()),
                                    text_color: Some(theme.palette().text),
                                    ..Default::default()
                                }
                            } else {
                                container::Style::default()
                            }
                        })
                })
                .fold(row![], |row, elem| row.push(elem));

            rows.push(
                row![
                    container(text(format!("{:08X}", address + offset as usize)).size(14).font(iced::Font::MONOSPACE))
                        .width(Length::Fixed(100.0))
                        .padding(0),
                    container(hex_cells).width(Length::Fixed(480.0)).padding(0),
                    container(ascii).width(Length::Fixed(140.0)).padding(0),
                ]
                .spacing(0),
            );
        }

        // Build info area showing values at cursor position
        let info_area = {
            let cursor_offset = self.cursor_row * BYTES_PER_ROW + self.cursor_col;
            let cursor_address = address + cursor_offset;

            // Get enough bytes for the largest type (8 bytes for u64/double)
            let mut value_bytes = vec![0u8; 8];
            let bytes_available = memory.len().saturating_sub(cursor_offset).min(8);
            if bytes_available > 0 {
                value_bytes[..bytes_available].copy_from_slice(&memory[cursor_offset..cursor_offset + bytes_available]);
            }

            // Calculate values in different formats
            let byte_val = value_bytes[0];
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
            let u64_val = if bytes_available >= 8 {
                u64::from_le_bytes([
                    value_bytes[0],
                    value_bytes[1],
                    value_bytes[2],
                    value_bytes[3],
                    value_bytes[4],
                    value_bytes[5],
                    value_bytes[6],
                    value_bytes[7],
                ])
            } else {
                0
            };

            // Add float and double representations
            let float_val = if bytes_available >= 4 {
                f32::from_le_bytes([value_bytes[0], value_bytes[1], value_bytes[2], value_bytes[3]])
            } else {
                0.0
            };
            let double_val = if bytes_available >= 8 {
                f64::from_le_bytes([
                    value_bytes[0],
                    value_bytes[1],
                    value_bytes[2],
                    value_bytes[3],
                    value_bytes[4],
                    value_bytes[5],
                    value_bytes[6],
                    value_bytes[7],
                ])
            } else {
                0.0
            };

            container(
                column![
                    row![text(format!("Cursor: 0x{:08X}", cursor_address)).size(14).font(iced::Font::MONOSPACE),].spacing(20),
                    row![
                        column![
                            text("Byte:").size(14).font(iced::Font::MONOSPACE),
                            text("U16:").size(14).font(iced::Font::MONOSPACE),
                            text("U32:").size(14).font(iced::Font::MONOSPACE),
                            text("U64:").size(14).font(iced::Font::MONOSPACE),
                        ]
                        .width(Length::Fixed(60.0))
                        .spacing(5),
                        column![
                            text(format!("{}", byte_val)).size(14).font(iced::Font::MONOSPACE),
                            text(format!("{}", u16_val)).size(14).font(iced::Font::MONOSPACE),
                            text(format!("{}", u32_val)).size(14).font(iced::Font::MONOSPACE),
                            text(format!("{}", u64_val)).size(14).font(iced::Font::MONOSPACE),
                        ]
                        .width(Length::Fixed(200.0))
                        .spacing(5),
                        column![
                            text("Float:").size(14).font(iced::Font::MONOSPACE),
                            text("Double:").size(14).font(iced::Font::MONOSPACE),
                        ]
                        .width(Length::Fixed(60.0))
                        .spacing(5),
                        column![
                            text(if bytes_available >= 4 {
                                if float_val.is_finite() {
                                    // Format float value more intelligently
                                    let abs_val = float_val.abs();
                                    if abs_val == 0.0 {
                                        "0.0".to_string()
                                    } else if abs_val >= 1e6 || abs_val <= 1e-3 {
                                        // Use scientific notation for very large or very small numbers
                                        format!("{:.3e}", float_val)
                                    } else if abs_val >= 1000.0 {
                                        // For large numbers, show fewer decimal places
                                        format!("{:.1}", float_val)
                                    } else if abs_val >= 1.0 {
                                        // For medium numbers, show up to 3 decimal places
                                        format!("{:.3}", float_val)
                                    } else {
                                        // For small numbers, show up to 4 decimal places
                                        format!("{:.4}", float_val)
                                    }
                                } else {
                                    format!("{}", float_val)
                                }
                            } else {
                                "N/A".to_string()
                            })
                            .size(14)
                            .font(iced::Font::MONOSPACE),
                            text(if bytes_available >= 8 {
                                if double_val.is_finite() {
                                    // Format double value more intelligently
                                    let abs_val = double_val.abs();
                                    if abs_val == 0.0 {
                                        "0.0".to_string()
                                    } else if abs_val >= 1e7 || abs_val <= 1e-4 {
                                        // Use scientific notation for very large or very small numbers
                                        format!("{:.4e}", double_val)
                                    } else if abs_val >= 1000.0 {
                                        // For large numbers, show fewer decimal places
                                        format!("{:.2}", double_val)
                                    } else if abs_val >= 1.0 {
                                        // For medium numbers, show up to 4 decimal places
                                        format!("{:.4}", double_val)
                                    } else {
                                        // For small numbers, show up to 6 decimal places
                                        format!("{:.6}", double_val)
                                    }
                                } else {
                                    format!("{}", double_val)
                                }
                            } else {
                                "N/A".to_string()
                            })
                            .size(14)
                            .font(iced::Font::MONOSPACE),
                        ]
                        .spacing(5),
                    ]
                    .spacing(20)
                ]
                .spacing(10)
                .padding(10),
            )
            .width(Length::Fill)
            .style(|theme: &iced::Theme| container::Style {
                background: Some(theme.extended_palette().background.weak.color.into()),
                border: iced::Border {
                    color: theme.extended_palette().background.strong.color,
                    width: 1.0,
                    radius: Radius::new(4.0),
                },
                ..Default::default()
            })
        };

        // Rest of the UI remains the same...
        container(
            column![
                // Title and navigation bar
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
                horizontal_rule(1),
                // Header row with column labels
                container(header).style(|theme: &iced::Theme| {
                    container::Style {
                        background: Some(theme.extended_palette().background.weak.color.into()),
                        ..Default::default()
                    }
                }),
                // Memory view
                scrollable(column(rows.into_iter().map(|r| r.into()).collect::<Vec<_>>()).spacing(0)).height(Length::FillPortion(3)),
                // Info area
                info_area,
            ]
            .spacing(0)
            .padding(DIALOG_PADDING),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    pub fn initalize(&mut self, addr: usize, search_type: SearchType) {
        self.address_text = format!("{:X}", addr);
        self.editor_initial_address = addr;
        self.editor_initial_size = if search_type == SearchType::String || search_type == SearchType::StringUtf16 {
            1
        } else {
            search_type.get_byte_length()
        };

        // Reset cursor to highlight the first byte
        self.cursor_row = 0;
        self.cursor_col = 0;
        self.cursor_nibble = 0;
    }

    pub fn move_cursor(&mut self, edit_address: usize, row_delta: i32, col_delta: i32) -> usize {
        const BYTES_PER_ROW: usize = 16;
        const MAX_VISIBLE_ROWS: usize = 24; // This should match MAX_ROWS in show_memory_editor
        let mut edit_address = edit_address;
        if col_delta != 0 {
            // Handle horizontal movement (nibble by nibble)
            let total_nibbles = BYTES_PER_ROW * 2;
            let current_nibble_pos = self.cursor_col * 2 + self.cursor_nibble;
            let new_nibble_pos = (current_nibble_pos as i32 + col_delta).clamp(0, total_nibbles as i32 - 1) as usize;

            self.cursor_col = new_nibble_pos / 2;
            self.cursor_nibble = new_nibble_pos % 2;
        }

        if row_delta != 0 {
            // Handle vertical movement with scrolling
            let new_row = self.cursor_row as i32 + row_delta;

            if new_row < 0 {
                // Cursor at top, scroll up
                let new_address = edit_address.saturating_sub(BYTES_PER_ROW);
                edit_address = new_address;
                self.address_text = format!("{:X}", new_address);
                self.cursor_row = 0;
            } else if new_row >= MAX_VISIBLE_ROWS as i32 {
                // Cursor would go beyond visible area, scroll down
                let new_address = edit_address.saturating_add(BYTES_PER_ROW);
                edit_address = new_address;
                self.address_text = format!("{:X}", new_address);
                // Keep cursor at last visible row
                self.cursor_row = MAX_VISIBLE_ROWS - 1;
            } else {
                // Normal cursor movement within visible area
                self.cursor_row = new_row as usize;
            }
        }
        edit_address
    }

    pub fn set_cursor(&mut self, row: usize, col: usize) {
        self.cursor_row = row;
        self.cursor_col = col;
        self.cursor_nibble = 0; // Always start at high nibble when clicking
    }

    pub fn edit_hex(&mut self, edit_address: usize, pid: process_memory::Pid, hex_digit: u8) {
        let offset = self.cursor_row * 16 + self.cursor_col;
        if let Ok(handle) = pid.try_into_process_handle() {
            let address = edit_address + offset;
            // Read current byte
            if let Ok(buf) = copy_address(address, 1, &handle) {
                let current_byte = buf[0];
                let new_byte = if self.cursor_nibble == 0 {
                    // Editing high nibble
                    (hex_digit << 4) | (current_byte & 0x0F)
                } else {
                    // Editing low nibble
                    (current_byte & 0xF0) | hex_digit
                };
                handle.put_address(address, &[new_byte]).unwrap_or_default();
            }
        }

        // Advance to next nibble
        if self.cursor_nibble == 0 {
            self.cursor_nibble = 1;
        } else {
            self.cursor_nibble = 0;
            self.cursor_col += 1;
            if self.cursor_col >= 16 {
                self.cursor_col = 0;
                self.cursor_row += 1;
                if self.cursor_row >= 16 {
                    self.cursor_row = 0;
                }
            }
        }
    }
}
