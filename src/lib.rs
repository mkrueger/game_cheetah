#![warn(clippy::all, rust_2018_idioms)]

pub mod search_type;
pub use search_type::*;

pub mod search_value;
pub use search_value::*;

pub mod search_context;
pub use search_context::*;

pub mod app;
pub use app::*;

mod state;
pub use state::*;
mod about_dialog;

pub enum MessageCommand {
    // Quit,
    Freeze,
    Unfreeze,
    Pid,
}

pub struct FreezeMessage {
    msg: MessageCommand,
    addr: usize,
    value: SearchValue,
}

impl FreezeMessage {
    pub fn from_addr(cmd: MessageCommand, addr: usize) -> Self {
        FreezeMessage {
            msg: cmd,
            addr,
            value: SearchValue(SearchType::Guess, Vec::new()),
        }
    }
}

use rust_embed::RustEmbed;
#[derive(RustEmbed)]
#[folder = "i18n"] // path to the compiled localization resources
struct Localizations;

use i18n_embed::{
    DesktopLanguageRequester,
    fluent::{FluentLanguageLoader, fluent_language_loader},
};

use once_cell::sync::Lazy;
pub static LANGUAGE_LOADER: Lazy<FluentLanguageLoader> = Lazy::new(|| {
    let loader = fluent_language_loader!();
    let requested_languages = DesktopLanguageRequester::requested_languages();
    let _result = i18n_embed::select(&loader, &Localizations, &requested_languages);
    loader
});
