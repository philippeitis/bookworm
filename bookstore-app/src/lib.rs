#![deny(unused_must_use)]
#![deny(unused_imports)]

pub use app::{App, ApplicationError};
#[allow(unused_imports)]
pub use parser::{parse_args, BookIndex, Command};
pub use settings::Settings;

#[allow(clippy::module_inception)]
pub mod app;
pub mod columns;
pub mod parser;
pub mod settings;
