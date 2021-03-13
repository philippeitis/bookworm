mod ui;

use std::env;
use std::io::stdout;
use std::path::PathBuf;

use crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::{cursor, event::DisableMouseCapture, event::EnableMouseCapture, execute};

use clap::Clap;

use tui::backend::CrosstermBackend;
use tui::Terminal;

use bookstore_app::table_view::TableView;
use bookstore_app::{parse_args, App, Settings};
use bookstore_database::AppDatabase;
#[cfg(feature = "rustbreak")]
use bookstore_database::BasicDatabase as Database;
#[cfg(feature = "sqlite")]
use bookstore_database::SQLiteDatabase as Database;

use crate::ui::{AppInterface, TuiError};

#[derive(Clap)]
#[clap(version = "0.1", author = "?")]
struct Opts {
    #[clap(short, long)]
    settings: Option<PathBuf>,
    #[clap(short, long)]
    database: Option<PathBuf>,
}

fn main() -> Result<(), TuiError<<Database as AppDatabase>::Error>> {
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

    let Opts { settings, database } = opts;
    let ((interface_settings, mut app_settings), settings_path) = if let Some(path) = settings {
        (
            Settings::open(&path).unwrap_or_default().split(),
            Some(path),
        )
    } else {
        if let Some(mut path) = dirs::config_dir() {
            path.push("bookstore/settings.toml");
            (
                Settings::open(&path).unwrap_or_default().split(),
                Some(path),
            )
        } else {
            (Settings::default().split(), None)
        }
    };

    if let Some(path) = database {
        app_settings.database_settings.path = path;
    }

    let db = Database::open(&app_settings.database_settings.path)?;

    let mut app = App::new(db, app_settings.sort_settings);
    let mut placeholder_table_view = TableView::default();
    let mut book_view = app.new_book_view();
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
                if !app.run_command(command, &mut placeholder_table_view, &mut book_view)? {
                    return Ok(());
                }
                if app.has_help_string() {
                    println!("{}", app.take_help_string());
                }
            }
        }
    }

    // Goes before due to lifetime issues.
    let stdout = stdout();

    let mut app = AppInterface::new(
        "Really Cool Library",
        interface_settings,
        settings_path,
        app,
    );

    let backend = CrosstermBackend::new(&stdout);
    let mut terminal = Terminal::new(backend)?;
    crossterm::terminal::enable_raw_mode()?;
    execute!(
        &stdout,
        EnterAlternateScreen,
        EnableMouseCapture,
        cursor::Hide
    )?;
    let r = app.run(&mut terminal);
    execute!(
        &stdout,
        cursor::Show,
        DisableMouseCapture,
        LeaveAlternateScreen
    )?;
    crossterm::terminal::disable_raw_mode()?;
    r
}

// TODO:
//  Live search & search by tags? meillisearch?
//  Cloud sync support (eg. upload database to Google Drive / read from Google Drive)
//  File conversion (mainly using calibre?)
//  Help menu
//  Splash screen
//  New database button / screen
//  Copy books to central directory: -c flag && set dir in settings.toml
//  Duplicate detection - use blake3 to hash first 4kb or something?
//  Add automatic date column?
//  Convert format to media, convert book to something else
//  Infinite undo redo (:u, :r)
//  Pop-up notifications
//  Documentation
//  Testing
