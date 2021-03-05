use std::cell::RefCell;
use std::ops::DerefMut;
use std::path::Path;
#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
use std::process::Command as ProcessCommand;
use std::rc::Rc;
use std::sync::{Arc, RwLock};

use rayon::prelude::*;
#[cfg(feature = "sqlite")]
use sqlx::Error as SQLError;
use unicase::UniCase;

use bookstore_database::bookview::BookViewIndex;
use bookstore_database::{
    bookview::BookViewError, AppDatabase, BookView, DatabaseError, IndexableDatabase,
    NestedBookView, SearchableBookView,
};
use bookstore_records::book::BookID;
use bookstore_records::{book::RawBook, Book, BookError};

use crate::help_strings::{help_strings, GENERAL_HELP};
use crate::parser;
use crate::parser::{BookIndex, Command};
use crate::settings::SortSettings;
use crate::table_view::TableView;
use crate::user_input::CommandStringError;

#[cfg(target_os = "windows")]
const OPEN_BOOK_IN_DIR_PY: &str = r#"import sys
import subprocess
import os

path = os.path.join(os.getenv('WINDIR'), 'explorer.exe')
subprocess.Popen(f'{path} /select,"{sys.argv[1]}"')
"#;

macro_rules! book {
    ($book: ident) => {
        $book.as_ref().read().unwrap()
    };
}

#[derive(Debug)]
pub enum ApplicationError {
    IO(std::io::Error),
    Book(BookError),
    Database(DatabaseError),
    BookView(BookViewError),
    NoBookSelected,
    Err(()),
    UserInput(CommandStringError),
    #[cfg(feature = "sqlite")]
    SQL(SQLError),
}

impl From<std::io::Error> for ApplicationError {
    fn from(e: std::io::Error) -> Self {
        ApplicationError::IO(e)
    }
}

impl From<()> for ApplicationError {
    fn from(_: ()) -> Self {
        ApplicationError::Err(())
    }
}

impl From<DatabaseError> for ApplicationError {
    fn from(e: DatabaseError) -> Self {
        match e {
            DatabaseError::Io(e) => ApplicationError::IO(e),
            DatabaseError::Book(e) => ApplicationError::Book(e),
            e => ApplicationError::Database(e),
        }
    }
}

impl From<BookError> for ApplicationError {
    fn from(e: BookError) -> Self {
        ApplicationError::Book(e)
    }
}

impl From<BookViewError> for ApplicationError {
    fn from(e: BookViewError) -> Self {
        match e {
            BookViewError::NoBookSelected => ApplicationError::NoBookSelected,
            x => ApplicationError::BookView(x),
        }
    }
}

impl From<CommandStringError> for ApplicationError {
    fn from(e: CommandStringError) -> Self {
        ApplicationError::UserInput(e)
    }
}

#[cfg(feature = "sqlite")]
impl From<SQLError> for ApplicationError {
    fn from(e: SQLError) -> Self {
        ApplicationError::SQL(e)
    }
}

// 0.75
fn books_in_dir<P: AsRef<Path>>(dir: P, depth: u8) -> Result<Vec<RawBook>, std::io::Error> {
    // TODO: Handle errored reads somehow.
    Ok(jwalk::WalkDir::new(dir)
        .min_depth(0)
        .max_depth(depth as usize)
        .into_iter()
        .filter_map(|res| res.map(|e| e.path()).ok())
        .collect::<Vec<_>>()
        .par_iter()
        .filter_map(|path| RawBook::generate_from_file(path).ok())
        .collect::<Vec<_>>())
}

/// Returns the first available path amongst the variants of the book, or None if no such
/// path exists.
///
/// # Arguments
///
/// * ` book ` - The book to find a path for.
fn get_book_path(book: &Book, index: usize) -> Option<&Path> {
    Some(book.get_variants()?.get(index)?.path())
}

#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
/// Opens the book in the native system viewer.
///
/// # Arguments
///
/// * ` book ` - The book to open.
///
/// # Errors
/// This function may error if the book's variants do not exist,
/// or if the command itself fails.
fn open_book(book: &Book, index: usize) -> Result<(), ApplicationError> {
    if let Some(path) = get_book_path(book, index) {
        #[cfg(target_os = "windows")]
        {
            ProcessCommand::new("cmd.exe")
                .args(&["/C", "start", "explorer"])
                .arg(path)
                .spawn()?;
        }
        #[cfg(target_os = "linux")]
        {
            ProcessCommand::new("xdg-open").arg(path).spawn()?;
        }
        #[cfg(target_os = "macos")]
        {
            ProcessCommand::new("open").arg(path).spawn()?;
        }
    }
    Ok(())
}

/// Opens the book and selects it, in File Explorer on Windows, or in Nautilus on Linux.
/// Other operating systems not currently supported
///
/// # Arguments
///
/// * ` book ` - The book to open.
/// * ` index ` - The index of the path to open.
///
/// # Errors
/// This function may error if the book's variants do not exist,
/// or if the command itself fails.
fn open_book_in_dir(book: &Book, index: usize) -> Result<(), ApplicationError> {
    // TODO: This doesn't work when run with install due to relative paths.
    #[cfg(target_os = "windows")]
    if let Some(path) = get_book_path(book, index) {
        use std::io::Write;

        let mut open_book_path = std::env::current_dir()?;
        open_book_path.push("open_book_in_dir.py");

        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .create(true)
            .open(&open_book_path)?;

        file.write_all(OPEN_BOOK_IN_DIR_PY.as_bytes())?;

        // TODO: Find a way to do this entirely in Rust
        ProcessCommand::new("python")
            .args(&[
                open_book_path.display().to_string().as_str(),
                path.display().to_string().as_str(),
            ])
            .spawn()?;
    }
    #[cfg(target_os = "linux")]
    if let Some(path) = get_book_path(book, index) {
        ProcessCommand::new("nautilus")
            .arg("--select")
            .arg(path)
            .spawn()?;
    }
    Ok(())
}

// fn books_in_dir<P: AsRef<Path>>(dir: P) -> Result<Vec<RawBook>, std::io::Error> {
//     // TODO: Look at libraries with parallel directory reading.
//     //  Handle errored reads somehow.
//     use futures::future::join_all;
//     use futures::executor::block_on;
//
//     let start = std::time::Instant::now();
//     let books = block_on(join_all(fs::read_dir(dir)?
//         .filter_map(|res| res.map(|e| e.path()).ok())
//         .map(|p| async move { RawBook::generate_from_file(p) })))
//         .into_iter()
//         .filter_map(|x| x.ok())
//         .collect();
//
//     let elapsed = start.elapsed().as_secs_f32();
//     println!("{}", elapsed);
//
//     Ok(books)
// }

pub struct App<D: AppDatabase> {
    db: Rc<RefCell<D>>,
    active_help_string: Option<&'static str>,
    sort_settings: SortSettings,
    updated: bool,
}

impl<D: IndexableDatabase> App<D> {
    pub fn new(db: D) -> Self {
        App {
            db: Rc::new(RefCell::new(db)),
            sort_settings: SortSettings::default(),
            updated: true,
            active_help_string: None,
        }
    }

    pub fn new_book_view(&self) -> SearchableBookView<D> {
        SearchableBookView::new(self.db.clone())
    }

    /// Gets the book specified by the `BookIndex`,
    /// or None if the particular book does not exist.
    ///
    /// # Arguments
    ///
    /// * ` b ` - A `BookIndex` to get a book by ID or by current selection.
    pub fn get_book(
        b: BookIndex,
        bv: &SearchableBookView<D>,
    ) -> Result<Arc<RwLock<Book>>, ApplicationError> {
        match b {
            BookIndex::Selected => Ok(bv.get_selected_book()?),
            BookIndex::ID(id) => Ok(bv.get_book(id)?),
        }
    }

    pub fn edit_selected_book<S0: AsRef<str>, S1: AsRef<str>>(
        &mut self,
        column: S0,
        new_value: S1,
        book_view: &mut SearchableBookView<D>,
    ) -> Result<(), ApplicationError> {
        let id = book_view
            .get_selected_book()?
            .as_ref()
            .read()
            .unwrap()
            .get_id();
        self.edit_book_with_id(id, column, new_value)
    }

    pub fn edit_book_with_id<S0: AsRef<str>, S1: AsRef<str>>(
        &mut self,
        id: BookID,
        column: S0,
        new_value: S1,
    ) -> Result<(), ApplicationError> {
        Ok(self.write(|db| db.edit_book_with_id(id, &column, &new_value))?)
    }

    pub fn remove_selected_book(
        &mut self,
        book_view: &mut SearchableBookView<D>,
    ) -> Result<(), ApplicationError> {
        match book_view.remove_selected_book()? {
            BookViewIndex::ID(id) => self.write(|db| db.remove_book(id))?,
            BookViewIndex::Index(index) => {
                self.write(|db| db.remove_book_indexed(index))?;
                book_view.refresh_db_size();
            }
        }
        Ok(())
    }

    pub fn remove_book(
        &mut self,
        id: BookID,
        book_view: &mut SearchableBookView<D>,
    ) -> Result<(), ApplicationError> {
        book_view.remove_book(id);
        Ok(self.write(|db| db.remove_book(id))?)
    }

    fn write<B>(&mut self, f: impl Fn(&mut D) -> B) -> B {
        let v = f(self.db.as_ref().borrow_mut().deref_mut());
        self.register_update();
        v
    }

    // Used in main.rs, ColumnWidget::handle_input
    /// Runs the command currently in the current command string. On success, returns a bool
    /// indicating whether to continue or not.
    ///
    /// # Arguments
    ///
    /// * ` command ` - The command to run.
    pub fn run_command(
        &mut self,
        command: parser::Command,
        table: &mut TableView,
        book_view: &mut SearchableBookView<D>,
    ) -> Result<bool, ApplicationError> {
        match command {
            Command::DeleteBook(b) => {
                match b {
                    BookIndex::Selected => self.remove_selected_book(book_view)?,
                    BookIndex::ID(id) => self.remove_book(id, book_view)?,
                };
            }
            Command::DeleteAll => {
                self.write(|db| db.clear())?;
                book_view.clear();
            }
            Command::EditBook(b, field, new_value) => {
                match b {
                    BookIndex::Selected => self.edit_selected_book(field, new_value, book_view)?,
                    BookIndex::ID(id) => self.edit_book_with_id(id, &field, &new_value)?,
                };
                self.sort_settings.is_sorted = false;
            }
            Command::AddBookFromFile(f) => {
                self.write(|db| db.insert_book(RawBook::generate_from_file(&f)?))?;
                self.sort_settings.is_sorted = false;
            }
            Command::AddBooksFromDir(dir, depth) => {
                // TODO: Handle failed reads.
                self.write(|db| db.insert_books(books_in_dir(&dir, depth)?))?;
                self.sort_settings.is_sorted = false;
            }
            Command::AddColumn(column) => {
                let column = UniCase::new(column);
                if book_view.has_column(&column) {
                    table.add_column(column);
                }
            }
            Command::RemoveColumn(column) => {
                table.remove_column(&UniCase::new(column));
            }
            Command::SortColumn(column, rev) => {
                self.update_selected_column(column, rev, table);
            }
            #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
            Command::OpenBookInApp(b, index) => {
                if let Ok(b) = Self::get_book(b, book_view) {
                    open_book(&book!(b), index)?;
                }
            }
            #[cfg(any(target_os = "windows", target_os = "linux"))]
            Command::OpenBookInExplorer(b, index) => {
                if let Ok(b) = Self::get_book(b, book_view) {
                    open_book_in_dir(&book!(b), index)?;
                }
            }
            Command::FindMatches(search) => {
                book_view.push_scope(search)?;
                self.register_update();
            }
            Command::Write => self.write(|d| d.save())?,
            // TODO: A warning pop-up when user is about to exit
            //  with unsaved changes.
            Command::Quit => return Ok(false),
            Command::WriteAndQuit => {
                self.write(|d| d.save())?;
                return Ok(false);
            }
            Command::TryMergeAllBooks => {
                self.write(|db| db.merge_similar())?;
            }
            Command::Help(flag) => {
                if let Some(s) = help_strings(&flag) {
                    self.active_help_string = Some(s);
                } else {
                    self.active_help_string = Some(GENERAL_HELP);
                }
            }
            Command::GeneralHelp => {
                self.active_help_string = Some(GENERAL_HELP);
            }
            #[cfg(all(not(target_os = "windows"), not(target_os = "linux")))]
            _ => return Ok(true),
        }
        book_view.refresh_db_size();
        Ok(true)
    }

    /// Updates the required sorting settings if the column changes.
    ///
    /// # Arguments
    ///
    /// * ` word ` - The column to sort the table on.
    /// * ` reverse ` - Whether to reverse the sort.
    fn update_selected_column(&mut self, word: String, reverse: bool, table: &mut TableView) {
        let word = UniCase::new(match word.to_ascii_lowercase().as_str() {
            "author" => String::from("authors"),
            _ => word,
        });

        if table.selected_cols().contains(&word) {
            self.sort_settings.column = word;
            self.sort_settings.is_sorted = false;
            self.sort_settings.reverse = reverse;
        }
    }

    // used in AppInterface::run
    pub fn apply_sort(
        &mut self,
        book_view: &mut SearchableBookView<D>,
    ) -> Result<(), DatabaseError> {
        if !self.sort_settings.is_sorted {
            let col = &self.sort_settings.column;
            let reverse = self.sort_settings.reverse;
            self.db
                .as_ref()
                .borrow_mut()
                .sort_books_by_col(col, reverse)?;
            book_view.sort_by_column(col, reverse)?;
            self.sort_settings.is_sorted = true;
            self.register_update();
        }
        Ok(())
    }

    fn register_update(&mut self) {
        self.updated = true;
    }

    pub fn take_update(&mut self) -> bool {
        std::mem::replace(&mut self.updated, false)
    }

    pub fn saved(&mut self) -> bool {
        self.db.as_ref().borrow().saved()
    }

    pub fn has_help_string(&self) -> bool {
        self.active_help_string.is_some()
    }

    pub fn take_help_string(&mut self) -> &'static str {
        std::mem::take(&mut self.active_help_string).unwrap_or(GENERAL_HELP)
    }
}
