#![deny(unused_must_use)]
#![deny(unused_imports)]
#[allow(clippy::module_inception)]
pub mod app;
pub mod autocomplete;
mod edit;
mod help_strings;
mod open;
pub mod parser;
pub mod settings;
pub mod table_view;
pub mod user_input;

pub use app::{App, ApplicationError};
pub use autocomplete::AutoCompleter;
#[allow(unused_imports)]
pub use parser::{parse_args, BookIndex, Command};
pub use settings::Settings;

fn log(s: impl AsRef<str>) {
    use std::io::Write;

    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(true)
        .open("log.txt")
    {
        let _ = f.write_all(s.as_ref().as_bytes());
        let _ = f.write_all(b"\n");
    }
}
