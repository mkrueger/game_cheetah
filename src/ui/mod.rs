pub mod app;

pub mod in_process_view;
pub mod main_window;
pub mod memory_editor;
pub mod process_selection;

pub const APP_NAME: &str = "Game Cheetah";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub use app::{App, AppState};
