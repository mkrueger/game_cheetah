#![warn(clippy::all, clippy::pedantic)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

fn main() -> eframe::Result<()> {
    use game_cheetah::App;

    let show_processes = std::env::args().any(|argument| argument == "--processes");
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/cheetah.png")).expect("embedded application icon must be a valid PNG");
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 720.0])
            .with_min_inner_size([640.0, 420.0])
            .with_icon(icon)
            .with_title(format!("{} {}", game_cheetah::APP_NAME, game_cheetah::VERSION)),
        ..Default::default()
    };

    eframe::run_native(
        &format!("{} {}", game_cheetah::APP_NAME, game_cheetah::VERSION),
        options,
        Box::new(move |cc| {
            game_cheetah::ui::theme::apply(&cc.egui_ctx);
            let mut app = App::new();
            if show_processes {
                app.attach_action();
            }
            Ok(Box::new(app))
        }),
    )
}
