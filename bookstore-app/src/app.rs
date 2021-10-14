use std::borrow::BorrowMut;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
use std::process::Command as ProcessCommand;
use std::sync::Arc;

use glob::PatternError;
use rayon::prelude::*;

use tokio::sync::mpsc::{Receiver, Sender};
use tokio::sync::RwLock;
use unicase::UniCase;

use bookstore_database::search::Search;
use bookstore_database::{AppDatabase, Book, BookView, DatabaseError};
use bookstore_records::book::{BookID, ColumnIdentifier, RecordError};
use bookstore_records::{BookError, BookVariant, Edit};

use crate::help_strings::{help_strings, GENERAL_HELP};
use crate::parser::{ModifyColumn, Source, Target};
use crate::table_view::TableView;

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
            let $id = db.borrow_mut();
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
fn open_in_dir<P: AsRef<Path>>(path: P) -> Result<(), std::io::Error> {
    #[cfg(target_os = "windows")]
    {
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
            .args(&[open_book_path.as_path(), path.as_ref()])
            .spawn()?;
    }
    #[cfg(target_os = "linux")]
    ProcessCommand::new("nautilus")
        .arg("--select")
        .arg(path.as_ref())
        .spawn()?;
    #[cfg(target_os = "macos")]
    ProcessCommand::new("open")
        .arg("-R")
        .arg(path.as_ref())
        .spawn()?;

    Ok(())
}

pub enum AppTask {
    Save,
    IsSaved,
    GetDbPath,
    TakeUpdate,
    GetBookView,
    GetHelp(Option<String>),
    DeleteIds(HashSet<BookID>),
    DeleteMatching(Box<[Search]>),
    DeleteAll,
    EditBooks(Box<[BookID]>, Box<[(ColumnIdentifier, Edit)]>),
    AddBooks(Box<[Source]>),
    TryMergeAllBooks,
    #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
    OpenBookIn(BookID, usize, Target),
}

pub enum AppResponse<D: AppDatabase + 'static> {
    IsSaved(bool),
    HelpInformation(String),
    Updated(bool),
    DbPath(PathBuf),
    BookView(BookView<D>),
    // DeleteMatching
    Deleted(HashSet<BookID>),
    // AddBooks
    Created(Vec<BookID>),
    // Delete these ids, and refresh ids from DB
    MergeRefresh(HashSet<BookID>),
    Empty,
}

pub struct App<D: AppDatabase + 'static> {
    db: Arc<RwLock<D>>,
    updated: bool,
    event_receiver: Receiver<AppTask>,
    result_sender: Sender<AppResponse<D>>,
}

pub struct AppChannel<D: AppDatabase + 'static> {
    sender: Sender<AppTask>,
    receiver: Arc<RwLock<Receiver<AppResponse<D>>>>,
}

impl<D: AppDatabase + Send + Sync> AppChannel<D> {
    pub async fn send(&self, app_task: AppTask) -> bool {
        self.sender.send(app_task).await.is_ok()
    }

    pub async fn receive(&self) -> Option<AppResponse<D>> {
        self.receiver.as_ref().write().await.recv().await
    }

    pub async fn delete_ids(&self, ids: HashSet<BookID>) {
        self.send(AppTask::DeleteIds(ids)).await;
        match self.receive().await.unwrap() {
            AppResponse::Empty => {}
            _ => panic!("Expected AppResponse::Empty response from application"),
        }
    }

    pub async fn delete_matching(&self, matches: Box<[Search]>) -> HashSet<BookID> {
        self.send(AppTask::DeleteMatching(matches)).await;
        match self.receive().await.unwrap() {
            AppResponse::Deleted(deleted) => deleted,
            _ => panic!("Expected AppResponse::Empty response from application"),
        }
    }

    pub async fn delete_all(&self) {
        self.send(AppTask::DeleteAll).await;
        match self.receive().await.unwrap() {
            AppResponse::Empty => {}
            _ => panic!("Expected AppResponse::Empty response from application"),
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

    pub async fn new_book_view(&self) -> BookView<D> {
        self.send(AppTask::GetBookView).await;
        match self.receive().await.unwrap() {
            AppResponse::BookView(result) => result,
            _ => panic!("Expected BookView response from application"),
        }
    }

    pub async fn help(&self, target: Option<String>) -> String {
        self.send(AppTask::GetHelp(target)).await;
        match self.receive().await.unwrap() {
            AppResponse::HelpInformation(result) => result,
            _ => panic!("Expected HelpInformation response from application"),
        }
    }

    pub async fn edit_books(&self, books: Box<[BookID]>, edits: Box<[(ColumnIdentifier, Edit)]>) {
        self.send(AppTask::EditBooks(books, edits)).await;
        match self.receive().await.unwrap() {
            AppResponse::Empty => {}
            _ => panic!("Expected HelpInformation response from application"),
        }
    }

    pub async fn add_books(&self, sources: Box<[Source]>) -> Vec<BookID> {
        self.send(AppTask::AddBooks(sources)).await;
        match self.receive().await.unwrap() {
            AppResponse::Created(result) => result,
            _ => panic!("Expected HelpInformation response from application"),
        }
    }

    #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
    pub async fn open_book(&self, id: BookID, index: usize, target: Target) {
        self.send(AppTask::OpenBookIn(id, index, target)).await;
        match self.receive().await.unwrap() {
            AppResponse::Empty => {}
            _ => panic!("Expected Empty response from application"),
        }
    }

    pub async fn try_merge_all_books(&self) -> HashSet<BookID> {
        self.send(AppTask::TryMergeAllBooks).await;
        match self.receive().await.unwrap() {
            AppResponse::MergeRefresh(book_ids) => book_ids,
            _ => panic!("Expected Empty response from application"),
        }
    }

    pub async fn modify_columns(
        &self,
        columns: Box<[ModifyColumn]>,
        table: &mut TableView,
        book_view: &mut BookView<D>,
    ) -> Result<(), ApplicationError<D::Error>> {
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
        Ok(())
    }
}

impl<D: AppDatabase + Send + Sync> App<D> {
    pub fn new(db: D) -> (Self, AppChannel<D>) {
        let (event_sender, event_receiver) = tokio::sync::mpsc::channel(100);
        let (result_sender, result_receiver) = tokio::sync::mpsc::channel(100);

        (
            App {
                db: Arc::new(RwLock::new(db)),
                updated: true,
                event_receiver,
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

    /// Returns a BookView, allowing reads of all available books.
    pub async fn new_book_view(&self) -> BookView<D> {
        BookView::new(self.db.clone()).await
    }

    /// Applies the specified edits to the provided book in the internal
    /// database.
    ///
    /// # Errors
    /// If no book is selected, or if editing the book fails, an error will be returned.
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

    async fn remove_books(
        &mut self,
        ids: &HashSet<BookID>,
    ) -> Result<(), ApplicationError<D::Error>> {
        async_write!(self, db, db.remove_books(ids).await)?;
        Ok(())
    }

    /// Saves the internal database to disk. Note that with SQLite, all operations are saved
    /// immediately.
    ///
    /// # Errors
    /// If saving the database fails, an error will be returned.
    async fn save(&mut self) -> Result<(), DatabaseError<D::Error>> {
        async_write!(self, db, db.save().await)
    }

    pub async fn event_loop(&mut self) -> Option<()> {
        loop {
            let val = match self.event_receiver.recv().await? {
                AppTask::IsSaved => AppResponse::IsSaved(self.saved().await),
                AppTask::GetDbPath => AppResponse::DbPath(self.db_path().await),
                AppTask::Save => {
                    let _ = self.save().await;
                    AppResponse::IsSaved(self.saved().await)
                }
                AppTask::TakeUpdate => AppResponse::Updated(self.take_update()),
                AppTask::GetBookView => AppResponse::BookView(self.new_book_view().await),
                #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
                AppTask::OpenBookIn(book, index, target) => {
                    if let Ok(book) = self.db.read().await.get_book(book).await {
                        if let Some(path) = get_book_path(&book, index) {
                            match target {
                                Target::FileManager => {
                                    let _ = open_in_dir(path);
                                }
                                Target::DefaultApp => {
                                    let _ = opener::open(path);
                                }
                            }
                        }
                    }

                    AppResponse::Empty
                }
                AppTask::GetHelp(Some(target)) => {
                    if let Some(s) = help_strings(&target) {
                        AppResponse::HelpInformation(s.to_string())
                    } else {
                        AppResponse::HelpInformation(GENERAL_HELP.to_string())
                    }
                }
                AppTask::GetHelp(None) => AppResponse::HelpInformation(GENERAL_HELP.to_string()),
                AppTask::DeleteIds(ids) => {
                    let _ = self.remove_books(&ids).await;
                    AppResponse::Empty
                }
                AppTask::DeleteMatching(matches) => {
                    let res = self.db.read().await.find_matches(&matches).await;
                    if let Ok(targets) = res {
                        let ids = targets.into_iter().map(|target| target.id()).collect();
                        let _ = self.remove_books(&ids).await;
                        AppResponse::Deleted(ids)
                    } else {
                        AppResponse::Empty
                    }
                }
                AppTask::DeleteAll => {
                    let _ = async_write!(self, db, db.clear().await);
                    AppResponse::Empty
                }
                AppTask::EditBooks(books, edits) => {
                    for book in books.to_vec().into_iter() {
                        let _ = self.edit_book_with_id(book, &edits).await;
                    }
                    AppResponse::Empty
                }
                AppTask::AddBooks(sources) => {
                    // TODO: Handle failed reads.
                    let mut ids = vec![];
                    let mut futs = vec![];
                    for source in sources.into_vec() {
                        match source {
                            Source::File(f) => {
                                if let Ok(book) = BookVariant::from_path(&f) {
                                    if let Ok(id) =
                                        async_write!(self, db, db.insert_book(book).await)
                                    {
                                        ids.push(id);
                                    }
                                }
                            }
                            Source::Dir(dir, depth) => {
                                if let Ok(books) = books_in_dir(&dir, depth) {
                                    let db = self.db.clone();
                                    futs.push(tokio::spawn(async move {
                                        if let Ok(items) =
                                            db.write().await.insert_books(books.into_iter()).await
                                        {
                                            items
                                        } else {
                                            vec![]
                                        }
                                    }));
                                }
                            }
                            Source::Glob(glob) => {
                                if let Ok(books) = books_globbed(&glob) {
                                    let db = self.db.clone();
                                    futs.push(tokio::spawn(async move {
                                        if let Ok(items) =
                                            db.write().await.insert_books(books.into_iter()).await
                                        {
                                            items
                                        } else {
                                            vec![]
                                        }
                                    }));
                                }
                            }
                        }
                    }

                    // Minimal overhead when # of futures is small, but allows reading & inserting
                    // in parallel when # of sources is large - very close to real read time when
                    // reading many 100s of thousands of books.
                    // (eg. if you're reading an entire file system).
                    // We should provide a mechanism to stream books as they're read, and then
                    // chunk these into a transaction-sized list
                    for fut in futs {
                        if let Ok(items) = fut.await {
                            ids.extend(items);
                        }
                    }

                    AppResponse::Created(ids)
                }
                AppTask::TryMergeAllBooks => {
                    if let Ok(ids) = async_write!(self, db, db.merge_similar().await) {
                        AppResponse::MergeRefresh(ids)
                    } else {
                        AppResponse::Empty
                    }
                }
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
