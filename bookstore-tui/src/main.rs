#![deny(unused_must_use)]

mod ui;

use std::env;
use std::io::stdout;
use std::num::NonZeroU64;
use std::path::PathBuf;
use std::process::exit;

use clap::Clap;
use crossterm::event::EventStream;
use crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::{cursor, event::DisableMouseCapture, event::EnableMouseCapture, execute};

use tui::backend::CrosstermBackend;
use tui::Terminal;

use bookstore_app::table_view::TableView;
use bookstore_app::{parse_args, App, Settings};
use bookstore_database::paginator::Variable;
use bookstore_database::AppDatabase;
use bookstore_database::SQLiteDatabase;

use crate::ui::terminal_ui::UIState;
use crate::ui::views::{run_command, AppView, ApplicationTask};
use crate::ui::{AppInterface, TuiError};

#[derive(Clap)]
#[clap(version = "0.1", author = "?")]
struct Opts {
    #[clap(short, long)]
    settings: Option<PathBuf>,
    #[clap(short, long)]
    database: Option<PathBuf>,
}

// #[tokio::main]
// async fn main() -> Result<(), TuiError<<SQLiteDatabase as AppDatabase>::Error>> {
//     let (opts, commands) = {
//         let args: Vec<_> = env::args().collect();
//         if args.is_empty() {
//             (Opts::parse_from(Vec::<String>::new()), vec![])
//         } else {
//             let before_index = args.iter().position(|s| "--".eq(s)).unwrap_or(args.len());
//             let (args, command) = args.split_at(before_index);
//             if command.is_empty() {
//                 (Opts::parse_from(args), command.to_owned())
//             } else {
//                 (Opts::parse_from(args), command[1..].to_owned())
//             }
//         }
//     };
//
//     let Opts { settings, database } = opts;
//     let ((interface_settings, mut app_settings), settings_path) = if let Some(path) = settings {
//         (
//             Settings::open(&path).unwrap_or_default().split(),
//             Some(path),
//         )
//     } else if let Some(mut path) = dirs::config_dir() {
//         path.push("bookstore/settings.toml");
//         (
//             Settings::open(&path).unwrap_or_default().split(),
//             Some(path),
//         )
//     } else {
//         (Settings::default().split(), None)
//     };
//
//     if let Some(path) = database {
//         app_settings.database_settings.path = path;
//     }
//
//     let mut db = std::sync::Arc::new(tokio::sync::RwLock::new(
//         SQLiteDatabase::open(&app_settings.database_settings.path).await?,
//     ));
//     use bookstore_records::book::{BookID, ColumnIdentifier};
//     use bookstore_records::{Book, BookVariant, ColumnOrder};
//     use std::convert::TryFrom;
//
//     let mut paginator = bookstore_database::paginator::Paginator::new(
//         db.clone(),
//         5,
//         vec![(ColumnIdentifier::ID, ColumnOrder::Ascending)].into_boxed_slice(),
//     );
//     paginator.scroll_down(0).await?;
//
//     for _ in 0..3 {
//         println!("SCROLL DOWN BY 1");
//         paginator.scroll_down(1).await?;
//         for book in paginator.window() {
//             println!(
//                 "{}|{:?}|{:?}",
//                 book.id(),
//                 book.title,
//                 book.authors().map(|x| x.first()).flatten()
//             );
//         }
//     }
//
//     for _ in 0..5 {
//         println!("SCROLL UP BY 10");
//         paginator.scroll_up(10).await?;
//         for book in paginator.window() {
//             println!(
//                 "{}|{:?}|{:?}",
//                 book.id(),
//                 book.title,
//                 book.authors().map(|x| x.first()).flatten()
//             );
//         }
//     }
//
//     println!("END");
//     paginator.end().await?;
//     for book in paginator.window() {
//         println!(
//             "{}|{:?}|{:?}",
//             book.id(),
//             book.title,
//             book.authors().map(|x| x.first()).flatten()
//         );
//     }
//
//     println!("update window size");
//     paginator.update_window_size(10).await?;
//     for book in paginator.window().iter() {
//         println!(
//             "{}|{:?}|{:?}",
//             book.id(),
//             book.title,
//             book.authors().map(|x| x.first()).flatten()
//         );
//     }
//     for _ in 0..2 {
//         println!("SCROLL UP BY 25");
//         paginator.scroll_up(25).await?;
//         for book in paginator.window().iter() {
//             println!(
//                 "{}|{:?}|{:?}",
//                 book.id(),
//                 book.title,
//                 book.authors().map(|x| x.first()).flatten()
//             );
//         }
//     }
//
//     Ok(())
// }
#[tokio::main]
async fn main() -> Result<(), TuiError<<SQLiteDatabase as AppDatabase>::Error>> {
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
    // match r {
    //     Ok(res) => res,
    //     Err(e) => match e.downcast_ref::<&'static str>() {
    //         Some(s) => {
    //             println!("Error occurred during execution: {}", s);
    //             Ok(())
    //         }
    //         None => match e.downcast_ref::<String>() {
    //             Some(s) => {
    //                 println!("Error occurred during execution: {}", s);
    //                 Ok(())
    //             }
    //             None => {
    //                 println!("Unknown error occurred during execution.");
    //                 Ok(())
    //             }
    //         },
    //     },
    // }
}

// TODO:
//  Live search, fuzzy text search? meillisearch?
//  Cloud sync support (eg. upload database to Google Drive / read from Google Drive)
//  File conversion (mainly using calibre?)
//  Help menu
//  Splash screen
//  New database button / screen
//  Copy books to central directory: -c flag && set dir in settings.toml
//  Duplicate detection on import
//  Add automatic date column?
//  Convert format to media, convert book to something else
//  Infinite undo redo (:u, :r)
//  Pop-up notifications
//  Documentation
//  Testing
//  Provide peeking access to text widgets
//  Make text widgets
