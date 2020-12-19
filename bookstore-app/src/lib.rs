#[allow(clippy::module_inception)]
pub mod app;
pub mod autocomplete;
mod help_strings;
pub mod parser;
pub mod settings;
pub mod user_input;

pub use app::{App, ApplicationError};
pub use autocomplete::AutoCompleter;
#[allow(unused_imports)]
pub use parser::{parse_args, parse_command_string, BookIndex, Command};
pub use settings::Settings;
