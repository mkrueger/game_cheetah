#![warn(clippy::all, clippy::pedantic)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

fn main() {
    use game_cheetah::app::App;

    iced::application(App::default, App::update, App::view)
        .theme(App::theme)
        .subscription(App::subscription) // Add this line
        .run()
        .expect("Failed to run application");
}
