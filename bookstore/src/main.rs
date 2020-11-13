use std::io;
use std::env;

mod book;
mod database;
#[cfg(feature = "cloud")]
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
    #[cfg(feature = "cloud")]
    {
        google_cloud_lib::CloudDatabase::open_database();
        return Ok(());
    }

    let args: Vec<_> = env::args().collect();
    let db = if let Some(i) = args.iter().position(|s| "--db".eq(s)) {
        if let Some(db) = args.get(i + 1) {
            BasicDatabase::open(db)?
        } else {
            BasicDatabase::open("books.db")?
        }
    } else {
        BasicDatabase::open("books.db")?
    };

    let settings = if let Some(i) = args.iter().position(|s| "--settings".eq(s)) {
        if let Some(db) = args.get(i + 1) {
            Settings::open(db)
        } else {
            Settings::open("settings.toml")
        }
    } else {
        Settings::open("settings.toml")
    }.unwrap_or(Settings::default());

    let stdout = io::stdout();

    let backend = if cfg!(windows) {
        CrosstermBackend::new(stdout)
    } else {
        println!("Current backend may not offer best results on current operating system.");
        CrosstermBackend::new(stdout)
    };

    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;
    terminal_ui::App::new(
        "Really Cool Library",
        settings,
        db,
    )?
        .run(&mut terminal)
}
