#[allow(clippy::module_inception)]
pub mod app;
pub mod autocomplete;
pub(crate) mod parser;
pub mod settings;
pub mod user_input;

#[allow(unused_imports)]
pub(crate) use crate::app::parser::{parse_args, parse_command_string, BookIndex, Command};
pub(crate) use app::{App, ApplicationError};
pub(crate) use autocomplete::AutoCompleter;
pub(crate) use settings::Settings;
