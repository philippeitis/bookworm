use std::borrow::BorrowMut;
use std::path::{Path, PathBuf};
#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
use std::process::Command as ProcessCommand;
use std::sync::Arc;

use glob::PatternError;
use rayon::prelude::*;

use tokio::sync::mpsc::{Receiver, Sender};
use tokio::sync::RwLock;
use unicase::UniCase;

use bookstore_database::{
    bookview::BookViewError, Book, BookView, DatabaseError, IndexableDatabase,
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

pub enum AppTask<D: IndexableDatabase> {
    ApplySort(BookView<D>),
    RunCommand(parser::Command, TableView, BookView<D>),
    Save,
    IsSaved,
    GetDbPath,
    GetSortSettings,
    TakeUpdate,
    TakeHelpInformation,
    GetBookView,
}

pub enum AppResponse<D: IndexableDatabase> {
    Sorted(Result<BookView<D>, ApplicationError<D::Error>>),
    IsSaved(bool),
    HelpInformation(Option<String>),
    Updated(bool),
    DbPath(PathBuf),
    SortSettings(SortSettings),
    CommandResult(Result<(bool, TableView, BookView<D>), ApplicationError<D::Error>>),
    BookView(BookView<D>),
}

pub struct App<D: IndexableDatabase> {
    db: Arc<RwLock<D>>,
    active_help_string: Option<&'static str>,
    sort_settings: SortSettings,
    updated: bool,
    event_receiver: Receiver<AppTask<D>>,
    result_sender: Sender<AppResponse<D>>,
    is_sorted: bool,
}

pub struct AppChannel<D: IndexableDatabase> {
    sender: Sender<AppTask<D>>,
    receiver: Arc<RwLock<Receiver<AppResponse<D>>>>,
}

impl<D: IndexableDatabase> AppChannel<D> {
    pub async fn send(&self, app_task: AppTask<D>) -> bool {
        self.sender.send(app_task).await.is_ok()
    }

    pub async fn receive(&self) -> Option<AppResponse<D>> {
        self.receiver.as_ref().write().await.recv().await
    }

    pub async fn apply_sort(
        &self,
        book_view: BookView<D>,
    ) -> Result<BookView<D>, ApplicationError<D::Error>> {
        self.send(AppTask::ApplySort(book_view)).await;
        match self.receive().await.unwrap() {
            AppResponse::Sorted(result) => result,
            _ => panic!("Expected sorted response from application"),
        }
    }

    pub async fn take_update(&self) -> bool {
        self.send(AppTask::TakeUpdate).await;
        match self.receive().await.unwrap() {
            AppResponse::Updated(result) => result,
            _ => panic!("Expected IsSaved response from application"),
        }
    }

    pub async fn saved(&self) -> bool {
        self.send(AppTask::IsSaved).await;
        match self.receive().await.unwrap() {
            AppResponse::IsSaved(result) => result,
            _ => panic!("Expected IsSaved response from application"),
        }
    }

    pub async fn save(&self) -> bool {
        self.send(AppTask::Save).await;
        match self.receive().await.unwrap() {
            AppResponse::IsSaved(result) => result,
            _ => panic!("Expected IsSaved response from application"),
        }
    }

    pub async fn db_path(&self) -> PathBuf {
        self.send(AppTask::GetDbPath).await;
        match self.receive().await.unwrap() {
            AppResponse::DbPath(result) => result,
            _ => panic!("Expected GetDbPath response from application"),
        }
    }

    pub async fn sort_settings(&self) -> SortSettings {
        self.send(AppTask::GetSortSettings).await;
        match self.receive().await.unwrap() {
            AppResponse::SortSettings(result) => result,
            _ => panic!("Expected GetSortSettings response from application"),
        }
    }

    pub async fn run_command(
        &self,
        command: Command,
        table_view: TableView,
        book_view: BookView<D>,
    ) -> Result<(bool, TableView, BookView<D>), ApplicationError<D::Error>> {
        self.send(AppTask::RunCommand(command, table_view, book_view))
            .await;
        match self.receive().await.unwrap() {
            AppResponse::CommandResult(result) => result,
            _ => panic!("Expected CommandResult response from application"),
        }
    }

    pub async fn take_help_string(&self) -> Option<String> {
        self.send(AppTask::TakeHelpInformation).await;
        match self.receive().await.unwrap() {
            AppResponse::HelpInformation(result) => result,
            _ => panic!("Expected HelpInformation response from application"),
        }
    }

    pub async fn new_book_view(&self) -> BookView<D> {
        self.send(AppTask::GetBookView).await;
        match self.receive().await.unwrap() {
            AppResponse::BookView(result) => result,
            _ => panic!("Expected BookView response from application"),
        }
    }
}

impl<D: IndexableDatabase + Send + Sync> App<D> {
    pub fn new(db: D, sort_settings: SortSettings) -> (Self, AppChannel<D>) {
        let (event_sender, event_receiver) = tokio::sync::mpsc::channel(100);
        let (result_sender, result_receiver) = tokio::sync::mpsc::channel(100);

        (
            App {
                db: Arc::new(RwLock::new(db)),
                is_sorted: sort_settings.columns.is_empty(),
                sort_settings,
                updated: true,
                event_receiver,
                active_help_string: None,
                result_sender,
            },
            AppChannel {
                sender: event_sender,
                receiver: Arc::new(RwLock::new(result_receiver)),
            },
        )
    }

    pub async fn db_path(&self) -> std::path::PathBuf {
        self.db.read().await.path().to_path_buf()
    }

    pub fn sort_settings(&self) -> &SortSettings {
        &self.sort_settings
    }

    /// Returns a BookView, allowing reads of all available books.
    pub async fn new_book_view(&self) -> BookView<D> {
        BookView::new(self.db.clone()).await
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
        bv: &BookView<D>,
    ) -> Result<Vec<Arc<Book>>, ApplicationError<D::Error>> {
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
    async fn edit_selected_book(
        &mut self,
        edits: &[(ColumnIdentifier, Edit)],
        book_view: &mut BookView<D>,
    ) -> Result<(), ApplicationError<D::Error>> {
        for book in book_view.get_selected_books().await? {
            self.edit_book_with_id(book.id(), edits).await?;
        }
        Ok(())
    }

    async fn edit_book_with_id(
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

    // TODO: Remove this
    async fn remove_selected_books(
        &mut self,
        book_view: &mut BookView<D>,
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

    async fn remove_book(
        &mut self,
        id: BookID,
        // TODO: Remove this as argument
        book_view: &mut BookView<D>,
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
        // TODO: These should be removed as arguments
        table: &mut TableView,
        book_view: &mut BookView<D>,
    ) -> Result<bool, ApplicationError<D::Error>> {
        match command {
            Command::DeleteSelected => {
                self.remove_selected_books(book_view).await?;
            }
            Command::DeleteMatching(matches) => {
                let targets = self.db.read().await.find_matches(&matches).await?;
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
                self.is_sorted = false;
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
                self.is_sorted = false;
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
    async fn save(&mut self) -> Result<(), DatabaseError<D::Error>> {
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
        self.is_sorted = false;
    }

    // used in AppInterface::run
    async fn apply_sort(
        &mut self,
        book_view: &mut BookView<D>,
    ) -> Result<(), DatabaseError<D::Error>> {
        if !self.is_sorted {
            self.db
                .write()
                .await
                .sort_books_by_cols(&self.sort_settings.columns)
                .await?;
            book_view
                .sort_by_columns(&self.sort_settings.columns)
                .await?;
            self.is_sorted = true;
            self.register_update();
        }
        Ok(())
    }

    pub async fn event_loop(&mut self) -> Option<()> {
        loop {
            let val = match self.event_receiver.recv().await? {
                AppTask::ApplySort(mut book_view) => match self.apply_sort(&mut book_view).await {
                    Ok(_) => AppResponse::Sorted(Ok(book_view)),
                    Err(e) => AppResponse::Sorted(Err(ApplicationError::Database(e))),
                },
                AppTask::IsSaved => AppResponse::IsSaved(self.saved().await),
                AppTask::GetDbPath => AppResponse::DbPath(self.db_path().await),
                AppTask::RunCommand(command, mut table_view, mut book_view) => {
                    let res = self
                        .run_command(command, &mut table_view, &mut book_view)
                        .await;
                    AppResponse::CommandResult(res.map(|val| (val, table_view, book_view)))
                }
                AppTask::Save => {
                    self.save().await;
                    AppResponse::IsSaved(self.saved().await)
                }
                AppTask::TakeUpdate => AppResponse::Updated(self.take_update()),
                AppTask::TakeHelpInformation => {
                    AppResponse::HelpInformation(self.active_help_string.map(String::from))
                }
                AppTask::GetSortSettings => AppResponse::SortSettings(self.sort_settings.clone()),
                AppTask::GetBookView => AppResponse::BookView(self.new_book_view().await),
            };
            self.result_sender.send(val).await.ok();
        }
    }
    fn register_update(&mut self) {
        self.updated = true;
    }

    fn take_update(&mut self) -> bool {
        std::mem::replace(&mut self.updated, false)
    }

    async fn saved(&mut self) -> bool {
        self.db.read().await.saved().await
    }

    pub fn has_help_string(&self) -> bool {
        self.active_help_string.is_some()
    }

    pub fn take_help_string(&mut self) -> &'static str {
        std::mem::take(&mut self.active_help_string).unwrap_or(GENERAL_HELP)
    }
}

// TODO: UI should update immediately, but have a ... in the corner
//  Maybe ... updates
//  to indicate that background tasks are running
//  Wait until all expected messages are received to remove
//  When adding books:
//  Return an appropriately sorted list of book ids for each scope
//  -> app needs to maintain scopes?
//  Local copy of BookView needs to do the following:
//  Maintain selections
//  Maintain position:
//  eg. top of window, or currently selected item?
//  Update underlying size
//  Need to make sure that sorting and scopes are maintained
//  When editing:
//  Changes can be made locally
//  When deleting books:
//  Return a hashset of all bookids to remove
//  If selections, remove all selections. No other changes made.
//  If schematic, need to remove own book indices.
//  > Make sure to update table view & top cursor position
//  > make sure all scopes are updated
