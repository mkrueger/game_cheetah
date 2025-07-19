#![warn(clippy::all, clippy::pedantic)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

fn main() {
    use game_cheetah::app::App;

    iced::application(App::title, App::update, App::view)
        .theme(App::theme)
        .run()
        .expect("Failed to run application");
}
