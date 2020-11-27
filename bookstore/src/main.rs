mod database;
mod parser;
mod record;
mod ui;

use std::env;
use std::io::{stdout, Write};

use crossterm::{event::DisableMouseCapture, event::EnableMouseCapture, execute};

use tui::backend::CrosstermBackend;
use tui::Terminal;

use crate::database::{AppDatabase, BasicDatabase};
use crate::parser::parse_args;
use crate::ui::{App, AppInterface, ApplicationError, Settings};

fn main() -> Result<(), ApplicationError> {
    #[cfg(feature = "cloud")]
    {
        database::google_cloud_database::CloudDatabase::open_database();
        return Ok(());
    }

    let (args, command) = {
        let args: Vec<_> = env::args().skip(1).collect();
        if args.is_empty() {
            (vec![], vec![])
        } else {
            let before_index = args.iter().position(|s| "--".eq(s)).unwrap_or(args.len());
            let (args, command) = args.split_at(before_index);
            if command.is_empty() {
                (args.to_owned(), command.to_owned())
            } else {
                (args.to_owned(), command[1..].to_owned())
            }
        }
    };

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
    }
    .unwrap_or_default();

    let mut app = App::new(db);
    // let mut app = App::new("Really Cool Library", settings, db)?;

    if !command.is_empty() {
        let command = parse_args(&command);
        if command.requires_ui() {
            println!(
                "The selected command ({:?}) requires opening the user interface.",
                command
            );
            return Ok(());
        }
        app.run_command(command)?;
    }

    // TODO: Make -h do something interesting, like open a server in the background.
    if args.contains(&"-h".to_string()) {
        return Ok(());
    }

    if !cfg!(windows) {
        println!("Current backend may not offer best results on current operating system.");
    };

    let stdout = stdout();
    let mut app = AppInterface::new("Really Cool Library", settings, app)?;

    let backend = CrosstermBackend::new(&stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;
    execute!(&stdout, EnableMouseCapture)?;
    let r = app.run(&mut terminal);
    execute!(&stdout, DisableMouseCapture)?;
    r
}
