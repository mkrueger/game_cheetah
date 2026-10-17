use i18n_embed_fl::fl;

use crate::ui::app::{App, AppState};

const MAIN_MENU_CARD_WIDTH: f32 = 380.0;
const MAIN_MENU_BUTTON_WIDTH: f32 = 320.0;
const MAIN_MENU_BUTTON_HEIGHT: f32 = 44.0;
const SETTINGS_MAX_WIDTH: f32 = 820.0;
const ABOUT_CARD_WIDTH: f32 = 620.0;

pub fn view_main_window(app: &mut App, ui: &mut egui::Ui) {
    egui::CentralPanel::default().show(ui, |ui| {
        animated_background(ui, app.menu_background);

        let top_space = (ui.available_height() * 0.07).clamp(24.0, 72.0);
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.add_space(top_space);
            ui.vertical_centered(|ui| {
                // Outer card with a subtle drop shadow for depth.
                card_frame(22, 30).show(ui, |ui| {
                    ui.set_width(MAIN_MENU_CARD_WIDTH);
                    ui.vertical_centered(|ui| {
                        main_menu_header(ui);
                        // Optional "update available" pill — visible only when the
                        // background update check found a newer release.
                        if let Some(latest) = app.latest_version.clone() {
                            ui.add_space(14.0);
                            let label = fl!(crate::LANGUAGE_LOADER, "update-available", version = latest.trim_start_matches('v'));
                            if ui
                                .add(
                                    egui::Button::new(egui::RichText::new(label).size(14.0))
                                        .fill(egui::Color32::from_rgb(56, 78, 126))
                                        .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(96, 124, 182)))
                                        .corner_radius(egui::CornerRadius::same(14)),
                                )
                                .clicked()
                            {
                                let _ = webbrowser::open("https://github.com/mkrueger/game_cheetah/releases/latest");
                            }
                        }

                        ui.add_space(22.0);

                        if menu_button(ui, &fl!(crate::LANGUAGE_LOADER, "attach-button"), true).clicked() {
                            app.attach_action();
                        }
                        ui.add_space(8.0);
                        if menu_button(ui, &fl!(crate::LANGUAGE_LOADER, "settings-button"), false).clicked() {
                            app.app_state = AppState::Settings;
                        }
                        ui.add_space(8.0);
                        if menu_button(ui, &fl!(crate::LANGUAGE_LOADER, "about-button"), false).clicked() {
                            app.app_state = AppState::About;
                        }

                        // Subtle divider before the destructive action.
                        ui.add_space(14.0);
                        thin_divider(ui, MAIN_MENU_BUTTON_WIDTH);
                        ui.add_space(10.0);

                        if menu_button(ui, &fl!(crate::LANGUAGE_LOADER, "quit-button"), false).clicked() {
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                        }

                        ui.add_space(22.0);
                        main_menu_footer(ui);
                    });
                });
            });
            ui.add_space(24.0);
        });
    });
}

fn main_menu_header(ui: &mut egui::Ui) {
    let logo_size = egui::Vec2::splat(96.0);
    let title = ui
        .painter()
        .layout_no_wrap(crate::APP_NAME.to_owned(), egui::FontId::proportional(34.0), ui.visuals().strong_text_color());
    let size = egui::vec2(logo_size.x + ui.spacing().item_spacing.x + title.size().x, logo_size.y);
    ui.allocate_ui_with_layout(size, egui::Layout::left_to_right(egui::Align::Center), |ui| {
        ui.add(
            egui::Image::new(egui::include_image!("../../assets/cheetah-menu.png"))
                .fit_to_exact_size(logo_size)
                .alt_text(crate::APP_NAME),
        );
        ui.label(title);
    });
}

/// Version plus secondary community links in one centered row.
fn main_menu_footer(ui: &mut egui::Ui) {
    let links = [
        ("GitHub".to_owned(), "https://github.com/mkrueger/game_cheetah"),
        (
            fl!(crate::LANGUAGE_LOADER, "discuss-button"),
            "https://github.com/mkrueger/game_cheetah/discussions",
        ),
        (fl!(crate::LANGUAGE_LOADER, "bug-button"), "https://github.com/mkrueger/game_cheetah/issues/new"),
    ];
    let font = egui::FontId::proportional(13.0);
    let version = format!("v{}", crate::VERSION);
    let separator = "·";
    let measure = |text: &str| ui.painter().layout_no_wrap(text.to_owned(), font.clone(), egui::Color32::PLACEHOLDER).size().x;
    let spacing = ui.spacing().item_spacing.x;
    let width = measure(&version) + links.iter().map(|(label, _)| measure(label) + measure(separator) + 2.0 * spacing).sum::<f32>();
    ui.horizontal(|ui| {
        ui.add_space(((ui.available_width() - width) * 0.5).max(0.0));
        ui.label(egui::RichText::new(version).font(font.clone()).weak());
        for (label, url) in links {
            ui.label(egui::RichText::new(separator).font(font.clone()).weak());
            if ui.link(egui::RichText::new(label).font(font.clone())).on_hover_text(url).clicked() {
                let _ = webbrowser::open(url);
            }
        }
    });
}

/// Render a main-menu button with consistent sizing and styling.
fn menu_button(ui: &mut egui::Ui, label: &str, primary: bool) -> egui::Response {
    let accent = ui.visuals().selection.bg_fill;
    let fill = if primary { accent } else { egui::Color32::from_rgb(36, 40, 46) };
    let stroke = if primary {
        egui::Stroke::NONE
    } else {
        egui::Stroke::new(1.0, egui::Color32::from_rgb(58, 64, 74))
    };
    let text = if primary {
        egui::RichText::new(label).size(18.0).strong().color(egui::Color32::WHITE)
    } else {
        egui::RichText::new(label).size(16.0).color(egui::Color32::from_rgb(228, 232, 238))
    };

    ui.add(
        egui::Button::new(text)
            .fill(fill)
            .stroke(stroke)
            .corner_radius(egui::CornerRadius::same(12))
            .min_size(egui::vec2(MAIN_MENU_BUTTON_WIDTH, MAIN_MENU_BUTTON_HEIGHT)),
    )
    .on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn thin_divider(ui: &mut egui::Ui, width: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 1.0), egui::Sense::hover());
    let color = egui::Color32::from_rgb(52, 58, 68);
    ui.painter().hline(rect.x_range(), rect.center().y, egui::Stroke::new(1.0, color));
}

pub fn view_settings(app: &mut App, ui: &mut egui::Ui) {
    // Pinned so the way back never scrolls out of view on small windows.
    egui::Panel::bottom("settings_footer")
        .show_separator_line(false)
        .frame(egui::Frame::new().fill(ui.visuals().panel_fill).inner_margin(egui::Margin::symmetric(20, 14)))
        .show(ui, |ui| {
            ui.vertical_centered(|ui| {
                if menu_button(ui, &fl!(crate::LANGUAGE_LOADER, "back-to-main-button"), false).clicked() {
                    app.app_state = AppState::MainWindow;
                }
            });
        });

    egui::CentralPanel::default().show(ui, |ui| {
        ui.add_space(28.0);
        ui.vertical_centered(|ui| {
            ui.heading(egui::RichText::new(fl!(crate::LANGUAGE_LOADER, "settings-title")).size(34.0).strong());
            ui.add_space(10.0);
            accent_rule(ui);
        });
        ui.add_space(22.0);

        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            settings_container(ui, |ui| {
                section(ui, |ui| {
                    if toggle_row(
                        ui,
                        &mut app.auto_reconnect,
                        fl!(crate::LANGUAGE_LOADER, "automatic-reconnect-label"),
                        fl!(crate::LANGUAGE_LOADER, "automatic-reconnect-description"),
                    ) {
                        app.persist_settings();
                    }
                    row_divider(ui);
                    if toggle_row(
                        ui,
                        &mut app.check_for_updates,
                        fl!(crate::LANGUAGE_LOADER, "check-for-updates-label"),
                        fl!(crate::LANGUAGE_LOADER, "check-for-updates-description"),
                    ) {
                        app.persist_settings();
                    }
                    row_divider(ui);
                    if toggle_row(
                        ui,
                        &mut app.confirm_value_writes,
                        fl!(crate::LANGUAGE_LOADER, "confirm-value-writes-label"),
                        fl!(crate::LANGUAGE_LOADER, "confirm-value-writes-description"),
                    ) {
                        app.persist_settings();
                    }
                    row_divider(ui);
                    if float_tolerance_row(ui, app) {
                        app.persist_settings();
                    }
                    row_divider(ui);
                    let mut persistence = app.enable_persistence;
                    if toggle_row(
                        ui,
                        &mut persistence,
                        fl!(crate::LANGUAGE_LOADER, "persistence-label"),
                        fl!(crate::LANGUAGE_LOADER, "persistence-description"),
                    ) {
                        app.set_persistence_enabled(persistence);
                        app.persist_settings();
                    }
                });
                config_directory_section(ui);
            });
        });
    });
}

pub fn view_about(app: &mut App, ui: &mut egui::Ui) {
    egui::CentralPanel::default().show(ui, |ui| {
        animated_background(ui, app.menu_background.alternate());

        let top_space = (ui.available_height() * 0.07).clamp(24.0, 72.0);
        ui.add_space(top_space);
        ui.vertical_centered(|ui| {
            card_frame(22, 30).show(ui, |ui| {
                ui.set_width(ABOUT_CARD_WIDTH.min(ui.available_width()));
                ui.vertical_centered(|ui| {
                    ui.heading(egui::RichText::new(fl!(crate::LANGUAGE_LOADER, "about-dialog-heading")).size(34.0).strong());
                    ui.add_space(10.0);
                    accent_rule(ui);
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new(format!("v{}", crate::VERSION)).size(13.0).weak());
                    ui.add_space(20.0);
                    ui.label(egui::RichText::new(fl!(crate::LANGUAGE_LOADER, "about-dialog-description")).size(15.0));
                    ui.add_space(20.0);
                    if ui.link(egui::RichText::new("github.com/mkrueger/game_cheetah").size(13.0)).clicked() {
                        let _ = webbrowser::open("https://github.com/mkrueger/game_cheetah");
                    }
                    ui.add_space(22.0);
                    if menu_button(ui, &fl!(crate::LANGUAGE_LOADER, "close-button"), false).clicked() {
                        app.app_state = AppState::MainWindow;
                    }
                });
            });
        });
    });
}

/// Paint a full-window game animation behind the current screen.
fn animated_background(ui: &egui::Ui, background: crate::ui::menu_background::MenuBackground) {
    let screen = ui.ctx().content_rect();
    background.paint(&ui.painter().with_clip_rect(screen), screen, ui.input(|input| input.time));
    // Unfocused or minimized windows keep the last frame instead of animating.
    if !crate::ui::refresh::backgrounded(ui.ctx()) {
        ui.ctx().request_repaint_after(crate::ui::refresh::LIVE_INTERVAL);
    }
}

/// Raised card shared by the main menu and About screen.
fn card_frame(radius: u8, margin: i8) -> egui::Frame {
    egui::Frame::new()
        .fill(egui::Color32::from_rgb(26, 29, 34))
        .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(56, 62, 72)))
        .corner_radius(egui::CornerRadius::same(radius))
        .inner_margin(egui::Margin::symmetric(margin + 4, margin))
        .shadow(egui::epaint::Shadow {
            offset: [0, 6],
            blur: 24,
            spread: 0,
            color: egui::Color32::from_black_alpha(110),
        })
}

/// Short accent underline beneath a page heading.
fn accent_rule(ui: &mut egui::Ui) {
    let accent = ui.visuals().selection.bg_fill;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(56.0, 3.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, egui::CornerRadius::same(2), accent);
}

fn settings_container(ui: &mut egui::Ui, contents: impl FnOnce(&mut egui::Ui)) {
    let width = ui.available_width().min(SETTINGS_MAX_WIDTH);
    ui.horizontal(|ui| {
        let margin = ((ui.available_width() - width) * 0.5).max(0.0);
        ui.add_space(margin);
        ui.vertical(|ui| {
            ui.set_width(width);
            contents(ui);
        });
    });
}

/// Card-style section container used to group settings. The card width is
/// pinned to the surrounding container's width so all sections render at
/// exactly the same width — even if a child widget would otherwise grow
/// the row beyond the available space.
fn section(ui: &mut egui::Ui, contents: impl FnOnce(&mut egui::Ui)) {
    let outer_width = ui.available_width();
    // Account for the frame's inner_margin (20 per side) + stroke (1 per
    // side) so the inner content area we hand out is exact.
    let inner_width = (outer_width - 20.0 * 2.0 - 1.0 * 2.0).max(0.0);
    egui::Frame::new()
        .fill(egui::Color32::from_rgb(26, 29, 34))
        .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(56, 62, 72)))
        .corner_radius(egui::CornerRadius::same(14))
        .inner_margin(egui::Margin::symmetric(20, 18))
        .shadow(egui::epaint::Shadow {
            offset: [0, 3],
            blur: 14,
            spread: 0,
            color: egui::Color32::from_black_alpha(70),
        })
        .show(ui, |ui| {
            ui.set_width(inner_width);
            contents(ui);
        });
    ui.add_space(14.0);
}

fn row_divider(ui: &mut egui::Ui) {
    ui.add_space(14.0);
    let width = ui.available_width();
    thin_divider(ui, width);
    ui.add_space(14.0);
}

/// Settings row: title and description on the left, switch on the right.
/// Clicking the title toggles as well, like a checkbox label would.
fn toggle_row(ui: &mut egui::Ui, value: &mut bool, title: String, description: String) -> bool {
    let row_width = ui.available_width();
    let text_width = (row_width - TOGGLE_SIZE.x - 24.0).max(120.0);
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.set_width(text_width);
            let title_response = ui.add(
                egui::Label::new(egui::RichText::new(&title).size(16.0).strong())
                    .selectable(false)
                    .sense(egui::Sense::click()),
            );
            if title_response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                *value = !*value;
                changed = true;
            }
            ui.add_space(4.0);
            ui.label(egui::RichText::new(description).size(13.5).weak());
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if toggle_switch(ui, value, &title).changed() {
                changed = true;
            }
        });
    });
    changed
}

const TOGGLE_SIZE: egui::Vec2 = egui::vec2(44.0, 24.0);
const TOLERANCE_FIELD_WIDTH: f32 = 84.0;
const TOLERANCE_MODE_WIDTH: f32 = 150.0;

/// Settings row with a number/percent switch and value field in place of the
/// on/off switch. Persists after editing finishes, not on every drag step.
fn float_tolerance_row(ui: &mut egui::Ui, app: &mut App) -> bool {
    let text_width = (ui.available_width() - TOLERANCE_CONTROLS_WIDTH - 32.0).max(120.0);
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.set_width(text_width);
            ui.label(egui::RichText::new(fl!(crate::LANGUAGE_LOADER, "float-tolerance-label")).size(16.0).strong());
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(fl!(crate::LANGUAGE_LOADER, "float-tolerance-description"))
                    .size(13.5)
                    .weak(),
            );
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            changed = float_tolerance_controls(ui, app);
        });
    });
    changed
}

const TOLERANCE_CONTROLS_WIDTH: f32 = TOLERANCE_FIELD_WIDTH + 8.0 + TOLERANCE_MODE_WIDTH;

/// Number/percent switch plus value field, shared by Settings and the result
/// toolbar popup. Returns `true` once a change should be persisted.
pub(crate) fn float_tolerance_controls(ui: &mut egui::Ui, app: &mut App) -> bool {
    use crate::{FloatTolerance, FloatToleranceMode};

    let size = egui::vec2(TOLERANCE_CONTROLS_WIDTH, TOGGLE_SIZE.y + 4.0);
    ui.allocate_ui_with_layout(size, egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let mode = app.float_tolerance_mode;
        let (value, decimals, suffix) = match mode {
            FloatToleranceMode::Absolute => (&mut app.float_tolerance_absolute, 3, ""),
            FloatToleranceMode::Percent => (&mut app.float_tolerance_percent, 2, " %"),
        };
        let response = ui.add_sized(
            egui::vec2(TOLERANCE_FIELD_WIDTH, TOGGLE_SIZE.y + 4.0),
            egui::DragValue::new(value)
                .range(0.0..=FloatTolerance::max_value(mode))
                .speed(0.1)
                .max_decimals(decimals)
                .prefix("± ")
                .suffix(suffix),
        );
        let mut changed = response.drag_stopped() || (response.changed() && !response.dragged());
        *value = FloatTolerance { mode, value: *value }.sanitized().value;
        ui.add_space(8.0);
        if tolerance_mode_switch(ui, &mut app.float_tolerance_mode) {
            changed = true;
        }
        changed
    })
    .inner
}

/// Two-segment switch; segments are laid out right-to-left by the caller.
fn tolerance_mode_switch(ui: &mut egui::Ui, mode: &mut crate::FloatToleranceMode) -> bool {
    use crate::FloatToleranceMode;

    let mut changed = false;
    egui::Frame::new()
        .fill(egui::Color32::from_rgb(20, 23, 27))
        .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(56, 62, 72)))
        .corner_radius(egui::CornerRadius::same(7))
        .inner_margin(egui::Margin::same(2))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.spacing_mut().button_padding = egui::vec2(10.0, 3.0);
            for (segment, label) in [
                (FloatToleranceMode::Percent, fl!(crate::LANGUAGE_LOADER, "float-tolerance-percent")),
                (FloatToleranceMode::Absolute, fl!(crate::LANGUAGE_LOADER, "float-tolerance-absolute")),
            ] {
                let selected = *mode == segment;
                let text = egui::RichText::new(&label).size(13.5);
                let text = if selected { text.strong().color(egui::Color32::WHITE) } else { text };
                let button = egui::Button::new(text)
                    .fill(if selected {
                        ui.visuals().selection.bg_fill
                    } else {
                        egui::Color32::TRANSPARENT
                    })
                    .stroke(egui::Stroke::NONE)
                    .corner_radius(egui::CornerRadius::same(5))
                    .min_size(egui::vec2(0.0, TOGGLE_SIZE.y));
                let response = ui.add(button);
                response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::RadioButton, ui.is_enabled(), selected, &label));
                if response.clicked() && !selected {
                    *mode = segment;
                    changed = true;
                }
            }
        });
    changed
}

/// iOS-style switch; reads as an on/off setting at a glance, unlike egui's
/// small checkbox box.
fn toggle_switch(ui: &mut egui::Ui, on: &mut bool, label: &str) -> egui::Response {
    let (rect, mut response) = ui.allocate_exact_size(TOGGLE_SIZE, egui::Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), *on, label));
    if ui.is_rect_visible(rect) {
        let how_on = ui.ctx().animate_bool_responsive(response.id, *on);
        let off_fill = egui::Color32::from_rgb(52, 58, 68);
        let on_fill = ui.visuals().selection.bg_fill;
        let fill = off_fill.lerp_to_gamma(on_fill, how_on);
        let stroke = if response.has_focus() {
            egui::Stroke::new(2.0, crate::ui::theme::ACCENT_TEXT)
        } else if response.hovered() {
            egui::Stroke::new(1.0, egui::Color32::from_rgb(110, 118, 130))
        } else {
            egui::Stroke::NONE
        };
        let radius = rect.height() * 0.5;
        ui.painter().rect(rect, radius, fill, stroke, egui::StrokeKind::Outside);
        let knob_x = egui::lerp((rect.left() + radius)..=(rect.right() - radius), how_on);
        ui.painter()
            .circle_filled(egui::pos2(knob_x, rect.center().y), radius - 4.0, egui::Color32::WHITE);
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn config_directory_section(ui: &mut egui::Ui) {
    section(ui, |ui| {
        ui.label(egui::RichText::new(fl!(crate::LANGUAGE_LOADER, "config-directory-label")).size(16.0).strong());
        ui.add_space(10.0);

        let config_dir = crate::config_dir().display().to_string();
        // Right-to-left: the buttons take their natural width (which varies
        // by language) and the path pill fills whatever is left, so the row
        // never exceeds the section width.
        let path_frame_overhead = 12.0 * 2.0 + 1.0 * 2.0;
        ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), 36.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let button = |label: String| egui::Button::new(label).min_size(egui::vec2(0.0, 36.0));
            if ui.add(button(fl!(crate::LANGUAGE_LOADER, "copy-config-directory-button"))).clicked() {
                ui.ctx().copy_text(config_dir.clone());
            }
            if ui.add(button(fl!(crate::LANGUAGE_LOADER, "open-config-directory-button"))).clicked() {
                let path = crate::config_dir();
                let _ = std::fs::create_dir_all(&path);
                let _ = opener::open(&path);
            }
            let path_width = (ui.available_width() - path_frame_overhead).max(120.0);
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                egui::Frame::new()
                    .fill(ui.visuals().text_edit_bg_color())
                    .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(64, 72, 84)))
                    .corner_radius(egui::CornerRadius::same(9))
                    .inner_margin(egui::Margin::symmetric(12, 8))
                    .show(ui, |ui| {
                        ui.set_width(path_width);
                        ui.add(
                            egui::Label::new(egui::RichText::new(&config_dir).monospace().size(15.0))
                                .selectable(true)
                                .truncate(),
                        );
                    });
            });
        });

        ui.add_space(10.0);
        ui.label(
            egui::RichText::new(fl!(crate::LANGUAGE_LOADER, "config-directory-description"))
                .size(13.5)
                .weak(),
        );
    });
}
