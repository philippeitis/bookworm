#![allow(clippy::single_match)]
mod app;
mod database;
mod record;
mod ui;

use std::env;
use std::io::{stdout, Write};

use crossterm::{event::DisableMouseCapture, event::EnableMouseCapture, execute};

use tui::backend::CrosstermBackend;
use tui::Terminal;

use crate::app::parse_args;
use crate::app::{App, ApplicationError, Settings};
use crate::database::{AppDatabase, BasicDatabase};
use crate::ui::AppInterface;

use clap::Clap;

#[derive(Clap)]
#[clap(version = "0.1", author = "?")]
struct Opts {
    #[clap(short, long, default_value = "settings.toml")]
    settings: String,
    #[clap(short, long, default_value = "books.db")]
    database: String,
}

fn main() -> Result<(), ApplicationError> {
    let (opts, command) = {
        let args: Vec<_> = env::args().collect();
        if args.is_empty() {
            (Opts::parse_from(Vec::<String>::new()), vec![])
        } else {
            let before_index = args.iter().position(|s| "--".eq(s)).unwrap_or(args.len());
            let (args, command) = args.split_at(before_index);
            if command.is_empty() {
                (Opts::parse_from(args), command.to_owned())
            } else {
                (Opts::parse_from(args), command[1..].to_owned())
            }
        }
    };

    let settings = Settings::open(opts.settings).unwrap_or_default();
    let db = BasicDatabase::open(opts.database)?;

    let mut app = App::new(db);

    if !command.is_empty() {
        for command in command.split(|v| v == "--") {
            if let Ok(command) = parse_args(command.to_owned()) {
                if command.requires_ui() {
                    println!(
                        "The selected command ({:?}) requires opening the user interface.",
                        command
                    );
                    return Ok(());
                }
                if !app.run_command(command)? {
                    return Ok(());
                }
            }
        }
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

// TODO:
//  Live search & search by tags - sqllite? meillisearch?
//  Cloud sync support (eg. upload database to Google Drive / read from Google Drive)
//  File conversion (mainly using calibre?)
//  Help menu
//  Splash screen
//  New database button / screen
//  Copy books to central directory: -c flag && set dir in settings.toml
//  Duplicate detection - use blake3 to hash first 4kb or something?
//  Add automatic date column?
//  Convert format to media, convert book to something else
//  Infinite undo redo (!u, !r)
//  Pop-up notifications
//  Documentation
//  Testing
