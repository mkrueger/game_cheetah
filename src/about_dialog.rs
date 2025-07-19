/*
use egui::Vec2;
use i18n_embed_fl::fl;

use crate::GameCheetahEngine;

impl GameCheetahEngine {
    pub fn show_about_dialog(&mut self, ctx: &egui::Context) {
        let mut open = true;

        egui::Window::new(fl!(crate::LANGUAGE_LOADER, "about-dialog-title"))
            .resizable(false)
            .movable(false)
            .collapsible(false)
            .open(&mut open)
            .default_width(500.)
            .anchor(egui::Align2::CENTER_CENTER, [0., 0.])
            .show(ctx, |ui| {
                self.show_contents(ui);
            });
        self.show_about_dialog &= open;
    }

    fn show_contents(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default().show_inside(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.heading(fl!(crate::LANGUAGE_LOADER, "about-dialog-heading"));
            });

            ui.label(fl!(crate::LANGUAGE_LOADER, "about-dialog-description"));
            ui.add_space(12.0); // ui.separator();
            ui.label(fl!(crate::LANGUAGE_LOADER, "about-dialog-created_by", authors = env!("CARGO_PKG_AUTHORS")));

            ui.add_space(8.0); // ui.separator();
        });

        egui::TopBottomPanel::bottom("bottom_panel")
            .resizable(false)
            .min_height(0.0)
            .show_inside(ui, |ui| {
                ui.add_space(8.0); // ui.separator();
                ui.vertical_centered(|ui| {
                    let button= egui::Button::new(fl!(crate::LANGUAGE_LOADER, "about-dialog-ok")).min_size(Vec2::new(100.0, 24.0));
                    if ui.add(button).clicked() {
                        self.show_about_dialog = false;
                    }
                });
            });
    }
}

*/
