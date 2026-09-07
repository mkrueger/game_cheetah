pub mod address_editor;
pub mod app;
pub(crate) mod auto_save;
pub(crate) mod error_notice;
pub mod pointer_scanner;

pub mod in_process_view;
pub mod main_window;
pub mod mem_editor;
pub(crate) mod notice;
pub mod process_selection;
pub mod theme;
pub mod value_cache;

pub const APP_NAME: &str = "Game Cheetah";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub use app::{App, AppState};
