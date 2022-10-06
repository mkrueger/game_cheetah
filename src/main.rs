#![warn(clippy::all, clippy::pedantic)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

const APP_NAME: &str = "Game Cheetah";
const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(target_os = "linux")]
fn main() -> Result<(), Box<dyn std::error::Error>> 
{
    sudo::escalate_if_needed()?;

    // Log to stdout (if you run with `RUST_LOG=debug`).
    tracing_subscriber::fmt::init();

    let native_options = eframe::NativeOptions::default();
    eframe::run_native(
        format!("{} {}", APP_NAME, VERSION).as_str(),
        native_options,
        Box::new(|cc| Box::new(game_cheetah::GameCheetahEngine::new(cc))),
    );

    Ok(())
}

#[cfg(target_os = "macos")]
fn main() -> Result<(), Box<dyn std::error::Error>> 
{
    // Log to stdout (if you run with `RUST_LOG=debug`).
    tracing_subscriber::fmt::init();

    let native_options = eframe::NativeOptions::default();
    eframe::run_native(
        format!("{} {}", APP_NAME, VERSION).as_str(),
        native_options,
        Box::new(|cc| Box::new(game_cheetah::GameCheetahEngine::new(cc))),
    );
    Ok(())
}

#[cfg(target_os = "windows")]
fn main() -> Result<(), Box<dyn std::error::Error>> 
{
    // Log to stdout (if you run with `RUST_LOG=debug`).
    tracing_subscriber::fmt::init();

    let native_options = eframe::NativeOptions::default();
    eframe::run_native(
        format!("{} {}", APP_NAME, VERSION).as_str(),
        native_options,
        Box::new(|cc| Box::new(game_cheetah::GameCheetahEngine::new(cc))),
    );

    Ok(())
}