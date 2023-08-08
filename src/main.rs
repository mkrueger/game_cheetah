#![warn(clippy::all, clippy::pedantic)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

const APP_NAME: &str = "Game Cheetah";
const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(target_os = "linux")]
fn main() {
    use std::process::Command;

    if !cfg!(target_os = "linux") && sudo::check() != sudo::RunningAs::Root {
        Command::new("pkexec")
            .args([
                "env",
                format!("DISPLAY={}", &std::env::var("DISPLAY").unwrap()).as_str(),
                format!("XAUTHORITY={}", &std::env::var("XAUTHORITY").unwrap()).as_str(),
                std::env::current_exe().unwrap().to_str().unwrap(),
            ])
            .output()
            .expect("failed to execute process");
        return;
    }

    // Log to stdout (if you run with `RUST_LOG=debug`).
    tracing_subscriber::fmt::init();

    let native_options = eframe::NativeOptions::default();
    eframe::run_native(
        format!("{APP_NAME} {VERSION}").as_str(),
        native_options,
        Box::new(|cc| Box::new(game_cheetah::GameCheetahEngine::new(cc))),
    )
    .unwrap();
}

#[cfg(target_os = "macos")]
fn main() {
    // Log to stdout (if you run with `RUST_LOG=debug`).
    tracing_subscriber::fmt::init();

    let native_options = eframe::NativeOptions::default();
    eframe::run_native(
        format!("{} {}", APP_NAME, VERSION).as_str(),
        native_options,
        Box::new(|cc| Box::new(game_cheetah::GameCheetahEngine::new(cc))),
    );
}

#[cfg(target_os = "windows")]
fn main() {
    // Log to stdout (if you run with `RUST_LOG=debug`).
    tracing_subscriber::fmt::init();

    let native_options = eframe::NativeOptions::default();
    eframe::run_native(
        format!("{} {}", APP_NAME, VERSION).as_str(),
        native_options,
        Box::new(|cc| Box::new(game_cheetah::GameCheetahEngine::new(cc))),
    )
    .unwrap();
}
