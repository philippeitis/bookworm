#![deny(unused_must_use)]
#![deny(unused_imports)]

mod ui;

use std::env;
use std::io::stdout;
use std::path::PathBuf;
use std::process::exit;

use clap::Parser;
use crossterm::event::EventStream;
use crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::{cursor, event::DisableMouseCapture, event::EnableMouseCapture, execute};

use tui::backend::CrosstermBackend;
use tui::Terminal;

use tracing::subscriber::set_global_default;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{fmt, EnvFilter, Registry};

use bookstore_app::{parse_args, App, Settings};
use bookstore_database::AppDatabase;
use bookstore_database::SQLiteDatabase;

use crate::ui::terminal_ui::UIState;
use crate::ui::views::{run_command, AppView, ApplicationTask};
use crate::ui::{AppInterface, TuiError};

#[derive(Parser)]
#[clap(version = "0.1", author = "?")]
struct Opts {
    #[clap(short, long)]
    settings: Option<PathBuf>,
    #[clap(short, long)]
    database: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<(), TuiError<<SQLiteDatabase as AppDatabase>::Error>> {
    let logging_dir = if let Some(mut dir) = dirs::data_local_dir() {
        dir.push("bookstore/logs/");
        dir
    } else {
        PathBuf::from("./bookstore/logs/")
    };

    println!("Writing logs to {}", logging_dir.display());

    let (file_appender, _guard) = tracing_appender::non_blocking::NonBlocking::new(
        tracing_appender::rolling::hourly(logging_dir, "log"),
    );

    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let subscriber = Registry::default()
        .with(env_filter)
        .with(fmt::layer().json().with_writer(file_appender));
    set_global_default(subscriber).expect("Failed to set subscriber");

    let (opts, commands) = {
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
    } else if let Some(mut path) = dirs::config_dir() {
        path.push("bookstore/settings.toml");
        (
            Settings::open(&path).unwrap_or_default().split(),
            Some(path),
        )
    } else {
        (Settings::default().split(), None)
    };

    if let Some(path) = database {
        app_settings.database_settings.path = path;
    }

    let db = SQLiteDatabase::open(&app_settings.database_settings.path).await?;

    let (mut app, mut receiver) = App::new(db);
    let mut placeholder_state = UIState {
        style: Default::default(),
        nav_settings: Default::default(),
        curr_command: Default::default(),
        selected_column: 0,
        table_view: Default::default(),
        book_view: app.new_book_view().await,
        sort_settings: Default::default(),
    };

    tokio::spawn(async move {
        let _ = app.event_loop().await;
    });

    if !commands.is_empty() {
        for command in commands.split(|v| v == "--") {
            if let Ok(command) = parse_args(command.to_owned()) {
                if command.requires_ui() {
                    println!(
                        "The selected command ({:?}) requires opening the user interface.",
                        command
                    );
                    return Ok(());
                }
                match run_command(&mut receiver, command, &mut placeholder_state).await? {
                    ApplicationTask::Quit => return Ok(()),
                    ApplicationTask::SwitchView(AppView::Help(msg)) => println!("{}", msg),
                    _ => {}
                }
            }
        }
    }

    // Goes before due to lifetime issues.
    let stdout_ = stdout();

    let mut app = AppInterface::new(
        "Really Cool Library",
        interface_settings,
        settings_path,
        receiver,
        EventStream::new(),
        app_settings.sort_settings,
    )
    .await;

    let backend = CrosstermBackend::new(&stdout_);
    let mut terminal = Terminal::new(backend)?;
    crossterm::terminal::enable_raw_mode()?;
    execute!(
        &stdout_,
        EnterAlternateScreen,
        EnableMouseCapture,
        cursor::Hide
    )?;
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let stdout_ = stdout();
        let _ = execute!(
            &stdout_,
            cursor::Show,
            DisableMouseCapture,
            LeaveAlternateScreen
        );
        let _ = crossterm::terminal::disable_raw_mode();
        default_panic(info);
        exit(1)
    }));

    let r = app.run(&mut terminal).await;
    execute!(
        &stdout_,
        cursor::Show,
        DisableMouseCapture,
        LeaveAlternateScreen
    )?;
    crossterm::terminal::disable_raw_mode()?;
    r
}

// TODO:
//  Live search, fuzzy text search? meillisearch?
//  Cloud sync support (eg. upload database to Google Drive / read from Google Drive)
//  File conversion (mainly using calibre?)
//  Help menu
//  Splash screen
//  New database button / screen
//  Copy books to central directory: -c flag && set dir in settings.toml
//  Add automatic date column?
//  Convert format to media, convert book to something else
//  Infinite undo redo (:u, :r)
//  Pop-up notifications
//  Documentation
//  Testing
//  Provide peeking access to text widgets
//  Make text widgets
