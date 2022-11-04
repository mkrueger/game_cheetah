#![warn(clippy::all, rust_2018_idioms)]

pub mod search_type;
pub use search_type::*;

pub mod search_value;
pub use search_value::*;

pub mod search_context;
pub use search_context::*;

mod app;
pub use app::GameCheetahEngine;

pub enum MessageCommand {
    // Quit,
    Freeze,
    Unfreeze,
    Pid
}

pub struct Message {
    msg: MessageCommand,
    addr: usize,
    value: SearchValue
}

impl Message {
    pub fn from_addr(cmd: MessageCommand, addr: usize) -> Self  {
        Message {
            msg: cmd,
            addr,
            value: SearchValue(SearchType::Guess, Vec::new())
        }
    }
}
