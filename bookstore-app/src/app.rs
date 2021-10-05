use std::borrow::BorrowMut;
use std::path::Path;
#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
use std::process::Command as ProcessCommand;
use std::sync::Arc;

use glob::PatternError;
use rayon::prelude::*;
use tokio::sync::RwLock as TokioRwLock;
use unicase::UniCase;

use bookstore_database::{
    bookview::BookViewError, AppDatabase, Book, BookView, DatabaseError, IndexableDatabase,
    NestedBookView, ScrollableBookView, SearchableBookView,
};
use bookstore_records::book::{BookID, ColumnIdentifier, RecordError};
use bookstore_records::{BookError, BookVariant, ColumnOrder, Edit};

use crate::autocomplete::AutoCompleteError;
use crate::help_strings::{help_strings, GENERAL_HELP};
use crate::parser;
use crate::parser::{BookIndex, Command, ModifyColumn, Source};
use crate::settings::SortSettings;
use crate::table_view::TableView;
use crate::user_input::CommandStringError;

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

#[cfg(target_os = "windows")]
const OPEN_BOOK_IN_DIR_PY: &str = r#"import sys
import subprocess
import os

path = os.path.join(os.getenv('WINDIR'), 'explorer.exe')
subprocess.Popen(f'{path} /select,"{sys.argv[1]}"')
"#;

macro_rules! async_write {
    ($self:ident, $id: ident, $op:expr) => {{
        let value = {
            let mut db = $self.db.write().await;
            #[allow(unused_mut)]
            let mut $id = db.borrow_mut();
            $op
        };
        $self.register_update();
        value
    }};
}

#[derive(Debug)]
pub enum ApplicationError<DBError> {
    IO(std::io::Error),
    Record(RecordError),
    Book(BookError),
    Database(DatabaseError<DBError>),
    BookView(BookViewError<DBError>),
    NoBookSelected,
    BadGlob(glob::PatternError),
    Unknown(&'static str),
}

impl<DBError> From<std::io::Error> for ApplicationError<DBError> {
    fn from(e: std::io::Error) -> Self {
        ApplicationError::IO(e)
    }
}

impl<DBError> From<DatabaseError<DBError>> for ApplicationError<DBError> {
    fn from(e: DatabaseError<DBError>) -> Self {
        match e {
            DatabaseError::Io(e) => ApplicationError::IO(e),
            e => ApplicationError::Database(e),
        }
    }
}

impl<DBError> From<BookError> for ApplicationError<DBError> {
    fn from(e: BookError) -> Self {
        ApplicationError::Book(e)
    }
}

impl<DBError> From<RecordError> for ApplicationError<DBError> {
    fn from(e: RecordError) -> Self {
        ApplicationError::Record(e)
    }
}

impl<DBError> From<BookViewError<DBError>> for ApplicationError<DBError> {
    fn from(e: BookViewError<DBError>) -> Self {
        match e {
            BookViewError::NoBookSelected => ApplicationError::NoBookSelected,
            x => ApplicationError::BookView(x),
        }
    }
}

impl<DBError> From<CommandStringError> for ApplicationError<DBError> {
    fn from(e: CommandStringError) -> Self {
        match e {
            CommandStringError::AutoComplete(ac) => match ac {
                AutoCompleteError::Glob(glob_err) => ApplicationError::BadGlob(glob_err),
            },
        }
    }
}

impl<DBError> From<PatternError> for ApplicationError<DBError> {
    fn from(e: PatternError) -> Self {
        ApplicationError::BadGlob(e)
    }
}

// Benchmarks:
// 5.3k books, Windows: 0.75s
// 332 books, Linux: ~0.042s
fn books_in_dir<P: AsRef<Path>>(dir: P, depth: u8) -> Result<Vec<BookVariant>, std::io::Error> {
    // TODO: Handle reads erroring out due to filesystem issues somehow.
    Ok(jwalk::WalkDir::new(std::fs::canonicalize(dir)?)
        .max_depth(depth as usize)
        .into_iter()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .collect::<Vec<_>>()
        .par_iter()
        .filter_map(|path| BookVariant::from_path(path).ok())
        .collect::<Vec<_>>())
}

fn books_globbed<S: AsRef<str>>(glob: S) -> Result<Vec<BookVariant>, glob::PatternError> {
    // TODO: Handle reads erroring out due to filesystem issues somehow.
    // TODO: Measure how well this performs - solutions for std::fs::canonicalize?
    // TODO: Create a new, better glob that does stuff like take AsRef<str> and AsRef<OsStr>
    //  and does parallelism like jwalk.
    Ok(glob::glob(glob.as_ref())?
        .into_iter()
        .filter_map(Result::ok)
        .collect::<Vec<_>>()
        .par_iter()
        .filter_map(|path| std::fs::canonicalize(path).ok())
        .filter_map(|path| BookVariant::from_path(path).ok())
        .collect::<Vec<_>>())
}

/// Returns the first available path amongst the variants of the book, or None if no such
/// path exists.
///
/// # Arguments
///
/// * ` book ` - The book to find a path for.
fn get_book_path(book: &Book, index: usize) -> Option<&Path> {
    Some(book.variants().get(index)?.path())
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
fn open_book(book: &Book, index: usize) -> Result<(), std::io::Error> {
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
fn open_book_in_dir(book: &Book, index: usize) -> Result<(), std::io::Error> {
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
            .args(&[open_book_path.as_path(), path])
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

pub struct App<D: AppDatabase> {
    db: Arc<TokioRwLock<D>>,
    active_help_string: Option<&'static str>,
    sort_settings: SortSettings,
    updated: bool,
}

impl<D: IndexableDatabase + Send + Sync> App<D> {
    pub fn new(db: D, sort_settings: SortSettings) -> Self {
        App {
            db: Arc::new(TokioRwLock::new(db)),
            sort_settings,
            updated: true,
            active_help_string: None,
        }
    }

    pub async fn db_path(&self) -> std::path::PathBuf {
        self.db.read().await.path().to_path_buf()
    }

    pub fn sort_settings(&self) -> &SortSettings {
        &self.sort_settings
    }

    /// Returns a BookView, allowing reads of all available books.
    pub async fn new_book_view(&self) -> SearchableBookView<D> {
        SearchableBookView::new(self.db.clone()).await
    }

    /// Gets the book specified by the `BookIndex` from the BookView.
    ///
    /// # Arguments
    ///
    /// * ` b ` - A `BookIndex` to get a book by ID or by current selection.
    ///
    /// # Errors
    /// If the database fails for any reason, an error will be returned.
    async fn get_book(
        b: BookIndex,
        bv: &SearchableBookView<D>,
    ) -> Result<Vec<Book>, ApplicationError<D::Error>> {
        match b {
            BookIndex::Selected => Ok(bv.get_selected_books().await?),
            BookIndex::ID(id) => Ok(vec![bv.get_book(id).await?]),
        }
    }

    /// Applies the specified edits to the provided book in the internal
    /// database.
    ///
    /// # Errors
    /// If no book is selected, or if editing the book fails, an error will be returned.
    pub async fn edit_selected_book(
        &mut self,
        edits: &[(ColumnIdentifier, Edit)],
        book_view: &mut SearchableBookView<D>,
    ) -> Result<(), ApplicationError<D::Error>> {
        for book in book_view.get_selected_books().await? {
            self.edit_book_with_id(book.id(), edits).await?;
        }
        Ok(())
    }

    pub async fn edit_book_with_id(
        &mut self,
        id: BookID,
        edits: &[(ColumnIdentifier, Edit)],
    ) -> Result<(), ApplicationError<D::Error>> {
        Ok(async_write!(
            self,
            db,
            db.edit_book_with_id(id, edits).await?
        ))
    }

    pub async fn remove_selected_books(
        &mut self,
        book_view: &mut SearchableBookView<D>,
    ) -> Result<(), ApplicationError<D::Error>> {
        log(format!(
            "Removing selected books: have {} books now.",
            self.db.read().await.size().await
        ));

        let books = book_view.remove_selected_books().await?;
        async_write!(self, db, db.remove_books(&books).await)?;
        book_view.refresh_db_size().await;
        Ok(())
    }

    pub async fn remove_book(
        &mut self,
        id: BookID,
        book_view: &mut SearchableBookView<D>,
    ) -> Result<(), ApplicationError<D::Error>> {
        book_view.remove_book(id);
        async_write!(self, db, db.remove_book(id).await)?;
        book_view.refresh_db_size().await;
        Ok(())
    }

    /// Runs the command currently in the current command string. On success, returns a bool
    /// indicating whether to continue or not.
    ///
    /// # Arguments
    ///
    /// * ` command ` - The command to run.
    pub async fn run_command(
        &mut self,
        command: parser::Command,
        table: &mut TableView,
        book_view: &mut SearchableBookView<D>,
    ) -> Result<bool, ApplicationError<D::Error>> {
        match command {
            Command::DeleteSelected => {
                self.remove_selected_books(book_view).await?;
            }
            Command::DeleteMatching(matches) => {
                let targets = async_write!(self, db, db.find_matches(&matches).await)?;
                let ids = targets.into_iter().map(|target| target.id()).collect();

                book_view.remove_books(&ids);
                async_write!(self, db, db.remove_books(&ids).await)?;
                book_view.refresh_db_size().await;
            }
            Command::DeleteAll => {
                async_write!(self, db, db.clear().await)?;
                book_view.clear().await;
            }
            Command::EditBook(b, edits) => {
                match b {
                    BookIndex::Selected => {
                        if book_view.make_selection_visible() {
                            table.regenerate_columns(book_view).await?;
                        }
                        self.edit_selected_book(&edits, book_view).await?
                    }
                    BookIndex::ID(id) => self.edit_book_with_id(id, &edits).await?,
                };
                self.sort_settings.is_sorted = false;
            }
            Command::AddBooks(sources) => {
                // TODO: Handle failed reads.
                for source in sources.into_vec() {
                    match source {
                        Source::File(f) => {
                            async_write!(self, db, {
                                let book = BookVariant::from_path(&f)?;
                                db.insert_book(book)
                                    .await
                                    .map_err(ApplicationError::Database)
                            })?;
                        }
                        Source::Dir(dir, depth) => {
                            async_write!(
                                self,
                                db,
                                db.insert_books(books_in_dir(&dir, depth)?.into_iter())
                                    .await
                            )?;
                        }
                        Source::Glob(glob) => {
                            async_write!(self, db, {
                                let books = books_globbed(&glob)?;
                                db.insert_books(books.into_iter()).await
                            })?;
                        }
                    }
                }

                book_view.refresh_db_size().await;
                self.sort_settings.is_sorted = false;
            }
            Command::ModifyColumns(columns) => {
                for column in columns.into_vec() {
                    match column {
                        ModifyColumn::Add(column) => {
                            let column = UniCase::new(column);
                            if book_view.has_column(&column).await? {
                                table.add_column(column);
                            }
                        }
                        ModifyColumn::Remove(column) => {
                            table.remove_column(&UniCase::new(column));
                        }
                    }
                }
            }
            Command::SortColumns(sort_cols) => {
                self.update_selected_columns(sort_cols);
            }
            #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
            Command::OpenBookInApp(b, index) => {
                if let Ok(b) = Self::get_book(b, book_view).await {
                    let b = &b[0];
                    open_book(b, index)?;
                }
            }
            #[cfg(any(target_os = "windows", target_os = "linux"))]
            Command::OpenBookInExplorer(b, index) => {
                if let Ok(b) = Self::get_book(b, book_view).await {
                    let b = &b[0];
                    open_book_in_dir(b, index)?;
                }
            }
            Command::FilterMatches(searches) => {
                book_view.push_scope(&searches).await?;
                self.register_update();
            }
            Command::JumpTo(searches) => {
                book_view.jump_to(&searches).await?;
                self.register_update();
            }
            Command::Write => {
                async_write!(self, db, db.save().await)?;
            }
            // TODO: A warning pop-up when user is about to exit
            //  with unsaved changes.
            Command::Quit => return Ok(false),
            Command::WriteAndQuit => {
                async_write!(self, db, db.save().await)?;
                return Ok(false);
            }
            Command::TryMergeAllBooks => {
                let ids = async_write!(self, db, db.merge_similar().await)?;
                book_view.remove_books(&ids);
                book_view.refresh_db_size().await;
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
        Ok(true)
    }

    /// Saves the internal database to disk. Note that with SQLite, all operations are saved
    /// immediately.
    ///
    /// # Errors
    /// If saving the database fails, an error will be returned.
    pub async fn save(&mut self) -> Result<(), DatabaseError<D::Error>> {
        async_write!(self, db, db.save().await)
    }

    /// Updates the required sorting settings if the column changes.
    ///
    /// # Arguments
    ///
    /// * ` word ` - The column to sort the table on.
    /// * ` reverse ` - Whether to reverse the sort.
    fn update_selected_columns(&mut self, cols: Box<[(ColumnIdentifier, ColumnOrder)]>) {
        self.sort_settings.columns = cols;
        self.sort_settings.is_sorted = false;
    }

    // used in AppInterface::run
    pub async fn apply_sort(
        &mut self,
        book_view: &mut SearchableBookView<D>,
    ) -> Result<(), DatabaseError<D::Error>> {
        if !self.sort_settings.is_sorted {
            self.db
                .write()
                .await
                .sort_books_by_cols(&self.sort_settings.columns)
                .await?;
            book_view
                .sort_by_columns(&self.sort_settings.columns)
                .await?;
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

    pub async fn saved(&mut self) -> bool {
        self.db.read().await.saved().await
    }

    pub fn has_help_string(&self) -> bool {
        self.active_help_string.is_some()
    }

    pub fn take_help_string(&mut self) -> &'static str {
        std::mem::take(&mut self.active_help_string).unwrap_or(GENERAL_HELP)
    }
}
