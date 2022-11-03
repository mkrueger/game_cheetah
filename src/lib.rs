#![warn(clippy::all, rust_2018_idioms)]

pub mod search_type;
pub use search_type::*;

pub mod search_value;
pub use search_value::*;

mod app;
pub use app::GameCheetahEngine;
