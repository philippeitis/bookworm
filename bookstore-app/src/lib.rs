#![deny(unused_must_use)]
#![deny(unused_imports)]

pub use app::{App, ApplicationError};
pub use autocomplete::AutoCompleter;
#[allow(unused_imports)]
pub use parser::{parse_args, BookIndex, Command};
pub use settings::Settings;

#[allow(clippy::module_inception)]
pub mod app;
pub mod autocomplete;
mod edit;
mod open;
pub mod parser;
pub mod settings;
pub mod table_view;
pub mod user_input;
