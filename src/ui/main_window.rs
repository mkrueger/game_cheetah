use i18n_embed_fl::fl;

use crate::ui::app::{App, AppState};

const MAIN_MENU_BUTTON_WIDTH: f32 = 240.0;
const MAIN_MENU_BUTTON_HEIGHT: f32 = 36.0;

pub fn view_main_window(app: &mut App, ui: &mut egui::Ui) {
    egui::CentralPanel::default().show_inside(ui, |ui| {
        ui.add_space(40.0);
        ui.vertical_centered(|ui| {
            ui.heading(egui::RichText::new(crate::APP_NAME).size(32.0));
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(fl!(crate::LANGUAGE_LOADER, "main-menu-subtitle"))
                    .size(13.0)
                    .weak(),
            );

            // Optional "update available" pill — visible only when the
            // background update check found a newer release.
            if let Some(latest) = app.latest_version.clone() {
                ui.add_space(8.0);
                let label = fl!(
                    crate::LANGUAGE_LOADER,
                    "update-available",
                    version = latest.trim_start_matches('v')
                );
                if ui
                    .add(
                        egui::Button::new(egui::RichText::new(label).size(13.0))
                            .fill(egui::Color32::from_rgb(60, 80, 130)),
                    )
                    .clicked()
                {
                    let _ = webbrowser::open("https://github.com/mkrueger/game_cheetah/releases/latest");
                }
            }

            ui.add_space(28.0);

            let button = |ui: &mut egui::Ui, label: String, primary: bool| -> egui::Response {
                let mut btn = egui::Button::new(egui::RichText::new(label).size(if primary { 18.0 } else { 14.0 }))
                    .min_size(egui::vec2(MAIN_MENU_BUTTON_WIDTH, MAIN_MENU_BUTTON_HEIGHT));
                if primary {
                    btn = btn.fill(ui.visuals().selection.bg_fill);
                }
                ui.add(btn)
            };

            if button(ui, fl!(crate::LANGUAGE_LOADER, "attach-button"), true).clicked() {
                app.attach_action();
            }
            ui.add_space(6.0);
            if button(ui, fl!(crate::LANGUAGE_LOADER, "settings-button"), false).clicked() {
                app.app_state = AppState::Settings;
            }
            ui.add_space(6.0);
            if button(ui, fl!(crate::LANGUAGE_LOADER, "about-button"), false).clicked() {
                app.app_state = AppState::About;
            }
            ui.add_space(6.0);
            if button(ui, fl!(crate::LANGUAGE_LOADER, "discuss-button"), false).clicked() {
                let _ = webbrowser::open("https://github.com/mkrueger/game_cheetah/discussions");
            }
            ui.add_space(6.0);
            if button(ui, fl!(crate::LANGUAGE_LOADER, "bug-button"), false).clicked() {
                let _ = webbrowser::open("https://github.com/mkrueger/game_cheetah/issues/new");
            }
            ui.add_space(6.0);
            if button(ui, fl!(crate::LANGUAGE_LOADER, "quit-button"), false).clicked() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }

            ui.add_space(36.0);
            ui.label(
                egui::RichText::new(format!("v{}", crate::VERSION))
                    .size(11.0)
                    .weak(),
            );
            if ui
                .link(egui::RichText::new("github.com/mkrueger/game_cheetah").size(12.0))
                .clicked()
            {
                let _ = webbrowser::open("https://github.com/mkrueger/game_cheetah");
            }
        });
    });
}

pub fn view_settings(app: &mut App, ui: &mut egui::Ui) {
    egui::CentralPanel::default().show_inside(ui, |ui| {
        ui.add_space(20.0);
        ui.vertical_centered(|ui| {
            ui.heading(fl!(crate::LANGUAGE_LOADER, "settings-title"));
        });
        ui.add_space(10.0);
        ui.separator();

        egui::ScrollArea::vertical().show(ui, |ui| {
            section(ui, |ui| {
                let mut changed = false;
                ui.horizontal(|ui| {
                    if ui
                        .checkbox(
                            &mut app.auto_reconnect,
                            egui::RichText::new(fl!(crate::LANGUAGE_LOADER, "automatic-reconnect-label")).strong(),
                        )
                        .changed()
                    {
                        changed = true;
                    }
                });
                ui.label(
                    egui::RichText::new(fl!(crate::LANGUAGE_LOADER, "automatic-reconnect-description"))
                        .size(12.0)
                        .weak(),
                );
                if changed {
                    app.persist_settings();
                }
            });

            section(ui, |ui| {
                let mut changed = false;
                ui.horizontal(|ui| {
                    if ui
                        .checkbox(
                            &mut app.check_for_updates,
                            egui::RichText::new(fl!(crate::LANGUAGE_LOADER, "check-for-updates-label")).strong(),
                        )
                        .changed()
                    {
                        changed = true;
                    }
                });
                ui.label(
                    egui::RichText::new(fl!(crate::LANGUAGE_LOADER, "check-for-updates-description"))
                        .size(12.0)
                        .weak(),
                );
                if changed {
                    app.persist_settings();
                }
            });

            section(ui, |ui| {
                ui.label(
                    egui::RichText::new(fl!(crate::LANGUAGE_LOADER, "config-directory-label")).strong(),
                );
                let config_dir = crate::config_dir().display().to_string();
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut config_dir.clone())
                            .desired_width(ui.available_width() - 220.0)
                            .font(egui::TextStyle::Monospace),
                    );
                    if ui
                        .button(fl!(crate::LANGUAGE_LOADER, "open-config-directory-button"))
                        .clicked()
                    {
                        let path = crate::config_dir();
                        let _ = std::fs::create_dir_all(&path);
                        let _ = opener::open(&path);
                    }
                    if ui
                        .button(fl!(crate::LANGUAGE_LOADER, "copy-config-directory-button"))
                        .clicked()
                    {
                        ui.ctx().copy_text(config_dir.clone());
                    }
                });
                ui.label(
                    egui::RichText::new(fl!(crate::LANGUAGE_LOADER, "config-directory-description"))
                        .size(12.0)
                        .weak(),
                );
            });
        });

        ui.add_space(10.0);
        ui.separator();
        ui.vertical_centered(|ui| {
            if ui
                .button(fl!(crate::LANGUAGE_LOADER, "back-to-main-button"))
                .clicked()
            {
                app.app_state = AppState::MainWindow;
            }
        });
    });
}

pub fn view_about(app: &mut App, ui: &mut egui::Ui) {
    egui::CentralPanel::default().show_inside(ui, |ui| {
        ui.add_space(40.0);
        ui.vertical_centered(|ui| {
            ui.heading(fl!(crate::LANGUAGE_LOADER, "about-dialog-heading"));
            ui.add_space(16.0);
            ui.label(fl!(crate::LANGUAGE_LOADER, "about-dialog-description"));
            ui.add_space(24.0);
            ui.label(
                egui::RichText::new(format!("v{}", crate::VERSION))
                    .size(12.0)
                    .weak(),
            );
            ui.add_space(20.0);
            if ui
                .button(fl!(crate::LANGUAGE_LOADER, "close-button"))
                .clicked()
            {
                app.app_state = AppState::MainWindow;
            }
        });
    });
}

/// Card-style section container used to group settings.
fn section(ui: &mut egui::Ui, contents: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::group(ui.style())
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            contents(ui);
        });
    ui.add_space(8.0);
}
