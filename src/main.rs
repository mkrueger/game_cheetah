#![warn(clippy::all, clippy::pedantic)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

fn main() -> eframe::Result<()> {
    use game_cheetah::App;

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 720.0])
            .with_min_inner_size([640.0, 420.0])
            .with_title(format!("{} {}", game_cheetah::APP_NAME, game_cheetah::VERSION)),
        ..Default::default()
    };

    eframe::run_native(
        &format!("{} {}", game_cheetah::APP_NAME, game_cheetah::VERSION),
        options,
        Box::new(|cc| {
            // Use a dark visuals theme by default and slightly larger spacing
            // so the dense tables read comfortably.
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            Ok(Box::new(App::new()))
        }),
    )
}
