#![warn(clippy::all, clippy::pedantic)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

const APP_NAME: &str = "Game Cheetah";
const VERSION: &str = env!("CARGO_PKG_VERSION");

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

    let mut options = eframe::NativeOptions {
        multisampling: 0,
        hardware_acceleration: eframe::HardwareAcceleration::Preferred,
        centered: true,
        ..Default::default()
    };
    let icon_data = eframe::icon_data::from_png_bytes(&include_bytes!("../build/linux/256x256.png")[..]).unwrap();
    options.viewport = options.viewport.with_icon(icon_data);
    
    eframe::run_native(
        format!("{APP_NAME} {VERSION}").as_str(),
        options,
        Box::new(|cc| {
            Ok(Box::new(game_cheetah::GameCheetahEngine::new(cc)))
        }),
    )
    .unwrap();
}
