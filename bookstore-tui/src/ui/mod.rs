#![deny(unused_imports)]

mod scrollable_text;
pub(crate) mod terminal_ui;
mod tui_widgets;
pub(crate) mod views;
pub mod widgets;

pub(crate) use terminal_ui::{AppInterface, TuiError};

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
