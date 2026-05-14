//! Vendored from egui_memory_editor v0.2.14 (MIT/Apache-2.0).
use std::ops::Range;

use egui::Ui;

use super::option_data::Endianness;
use super::{Address, MemoryEditor};

fn short_label(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_owned();
    }
    let mut out = text.chars().take(max_chars.saturating_sub(3)).collect::<String>();
    out.push_str("...");
    out
}

impl MemoryEditor {
    pub(crate) fn draw_options_area<T: ?Sized>(&mut self, ui: &mut Ui, _mem: &mut T, _read: &mut impl FnMut(&mut T, Address) -> Option<u8>) {
        let current_address_range = self.address_ranges.get(&self.options.selected_address_range).unwrap().clone();

        egui::Frame::new()
            .fill(egui::Color32::from_rgb(22, 25, 30))
            .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(45, 50, 60)))
            .corner_radius(egui::CornerRadius::same(10))
            .inner_margin(egui::Margin::symmetric(14, 10))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let accent = ui.visuals().selection.bg_fill;
                    ui.label(egui::RichText::new("Options").size(14.0).strong().color(accent));
                    ui.add_space(8.0);
                    if !self.options.selected_address_range.is_empty() {
                        ui.label(egui::RichText::new(short_label(&self.options.selected_address_range, 64)).size(12.0).weak());
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let label = if self.options.is_options_collapsed { "Show" } else { "Hide" };
                        if ui.add(egui::Button::new(label).min_size(egui::vec2(56.0, 24.0))).clicked() {
                            self.options.is_options_collapsed = !self.options.is_options_collapsed;
                        }
                    });
                });

                if !self.options.is_options_collapsed {
                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(8.0);
                    self.draw_main_options(ui, &current_address_range);
                }
            });
    }

    fn draw_main_options(&mut self, ui: &mut Ui, current_address_range: &Range<Address>) {
        ui.horizontal_wrapped(|ui| {
            if self.frame_data.memory_range_combo_box_enabled {
                let selected_address_range = &mut self.options.selected_address_range;
                let address_ranges = &self.address_ranges;

                ui.horizontal(|ui| {
                    ui.label("Region:");

                    egui::ComboBox::from_id_salt("RegionCombo")
                        .width(320.0)
                        .selected_text(short_label(selected_address_range, 52))
                        .show_ui(ui, |ui| {
                            address_ranges.iter().for_each(|(range_name, _)| {
                                ui.selectable_value(selected_address_range, range_name.clone(), range_name);
                            });
                        });
                });
            };

            ui.add_space(10.0);

            let mut columns_u8 = self.options.column_count as u8;

            if self.options.is_resizable_column {
                ui.add(egui::DragValue::new(&mut columns_u8).range(1.0..=64.0).prefix("Columns: ").speed(0.5));
            } else {
                ui.add(egui::Label::new(format!("Columns: {}", columns_u8)));
            }

            self.options.column_count = columns_u8 as usize;

            ui.add_space(10.0);
            let data_preview_options = &mut self.options.data_preview;
            egui::ComboBox::from_id_salt("EndianCombo")
                .selected_text(match data_preview_options.selected_endianness {
                    Endianness::Little => "little-endian",
                    Endianness::Big => "big-endian",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut data_preview_options.selected_endianness, Endianness::Little, "little-endian");
                    ui.selectable_value(&mut data_preview_options.selected_endianness, Endianness::Big, "big-endian");
                })
                .response
                .on_hover_text("Select the endianness used by the inspector");

            ui.add_space(10.0);
            ui.label("Goto:");

            let response = ui
                .add_sized(
                    egui::vec2(150.0, 28.0),
                    egui::TextEdit::singleline(&mut self.frame_data.goto_address_string)
                        .hint_text(format!("0x{:X}", current_address_range.start))
                        .font(egui::TextStyle::Monospace),
                )
                .on_hover_text(
                    "Goto an address, format: \n\
                    * An address like `0xAA` can be written as `AA`\n\
                    * Offset from the base address, if the base is `0xFF00` then one can enter `5` to go to `0xFF05`\n\
                    Press enter to move to the address",
                );

            self.frame_data.goto_address_string.retain(|c| c.is_ascii_hexdigit());

            if response.clicked() && !ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                self.frame_data.goto_address_string.clear();
            }

            if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                let goto_address_string = &mut self.frame_data.goto_address_string;

                if goto_address_string.starts_with("0x") || goto_address_string.starts_with("0X") {
                    *goto_address_string = goto_address_string[2..].to_string();
                }

                let address = Address::from_str_radix(goto_address_string, 16).ok().and_then(|addr| {
                    if current_address_range.contains(&addr) {
                        Some(addr)
                    } else {
                        let offset_addr = addr.saturating_add(current_address_range.start);

                        if current_address_range.contains(&offset_addr) {
                            *goto_address_string = format!("{:X}", offset_addr);
                            Some(offset_addr)
                        } else {
                            None
                        }
                    }
                });

                self.frame_data.goto_address_line = address
                    .and_then(|addr| addr.checked_sub(current_address_range.start))
                    .map(|addr| addr / self.options.column_count);
                self.frame_data.selected_highlight_address = address;

                response.surrender_focus();
            }
        });
    }
}
