use std::io;

mod book;
mod database;
#[cfg(feature = "cloud-google")]
mod google_cloud_lib;
#[allow(dead_code)]
mod isbn;
mod parser;
mod ui;

use tui::backend::CrosstermBackend;
use tui::Terminal;

use crate::database::{AppDatabase, BasicDatabase};
use crate::ui::{settings::Settings, terminal_ui};

fn main() -> Result<(), terminal_ui::ApplicationError> {
    // fn main() {
    // println!("{:?}", parser::parser::parse_command_string("!a Hello -d"));

    #[cfg(feature = "cloud-google")]
    {
        google_cloud_lib::CloudDatabase::open_database();
        return Ok(());
    }

    let stdout = io::stdout();

    let backend = if cfg!(windows) {
        CrosstermBackend::new(stdout)
    } else {
        println!("Current backend may not offer best results on current operating system.");
        CrosstermBackend::new(stdout)
    };

    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;
    // terminal_ui::App::splash(InterfaceStyle::default(), &mut terminal);
    terminal_ui::App::new(
        "Really Cool Library",
        Settings::open("settings.toml").unwrap_or(Settings::default()),
        BasicDatabase::open("books.db")?,
    )?
    .run(&mut terminal)
}
