use std::borrow::BorrowMut;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use glob::PatternError;
use rayon::prelude::*;

use tokio::sync::mpsc::{Receiver, Sender};
use tokio::sync::RwLock;
use unicase::UniCase;

use bookstore_database::paginator::Selection;
use bookstore_database::{AppDatabase, Book, BookView, DatabaseError};
use bookstore_records::book::{BookID, ColumnIdentifier, RecordError};
use bookstore_records::{BookError, BookVariant, Edit};

use crate::open::open_in_dir;
use crate::parser::{ModifyColumn, Source, Target};
use crate::table_view::TableView;

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

pub enum AppTask {
    Save,
    IsSaved,
    GetDbPath,
    TakeUpdate,
    GetBookView,
    DeleteIds(HashSet<BookID>),
    DeleteSelected(Selection),
    EditBooks(Box<[BookID]>, Box<[(ColumnIdentifier, Edit)]>),
    EditSelection(Selection, Box<[(ColumnIdentifier, Edit)]>),
    AddBooks(Box<[Source]>),
    TryMergeAllBooks,
    #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
    OpenBookIn(BookID, usize, Target),
}

pub enum AppResponse<D: AppDatabase + 'static> {
    IsSaved(bool),
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

    pub async fn delete_selected(&self, selected: Selection) {
        self.send(AppTask::DeleteSelected(selected)).await;
        match self.receive().await.unwrap() {
            AppResponse::Empty => {}
            _ => panic!("Expected AppResponse::Empty response from application"),
        }
    }

    #[must_use]
    pub async fn take_update(&self) -> bool {
        self.send(AppTask::TakeUpdate).await;
        match self.receive().await.unwrap() {
            AppResponse::Updated(result) => result,
            _ => panic!("Expected IsSaved response from application"),
        }
    }

    #[must_use]
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

    #[must_use]
    pub async fn db_path(&self) -> PathBuf {
        self.send(AppTask::GetDbPath).await;
        match self.receive().await.unwrap() {
            AppResponse::DbPath(result) => result,
            _ => panic!("Expected GetDbPath response from application"),
        }
    }

    #[must_use]
    pub async fn new_book_view(&self) -> BookView<D> {
        self.send(AppTask::GetBookView).await;
        match self.receive().await.unwrap() {
            AppResponse::BookView(result) => result,
            _ => panic!("Expected BookView response from application"),
        }
    }

    pub async fn edit_books(&self, books: Box<[BookID]>, edits: Box<[(ColumnIdentifier, Edit)]>) {
        self.send(AppTask::EditBooks(books, edits)).await;
        match self.receive().await.unwrap() {
            AppResponse::Empty => {}
            _ => panic!("Expected Empty response from application"),
        }
    }

    pub async fn edit_selected(
        &self,
        selection: Selection,
        edits: Box<[(ColumnIdentifier, Edit)]>,
    ) {
        self.send(AppTask::EditSelection(selection, edits)).await;
        match self.receive().await.unwrap() {
            AppResponse::Empty => {}
            _ => panic!("Expected Empty response from application"),
        }
    }

    pub async fn add_books(&self, sources: Box<[Source]>) -> Vec<BookID> {
        self.send(AppTask::AddBooks(sources)).await;
        match self.receive().await.unwrap() {
            AppResponse::Created(result) => result,
            _ => panic!("Expected Created response from application"),
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
    #[must_use]
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

    #[must_use]
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
                AppTask::DeleteIds(ids) => {
                    let _ = self.remove_books(&ids).await;
                    AppResponse::Empty
                }
                AppTask::DeleteSelected(selection) => {
                    let _ = self.db.write().await.remove_selected(&selection).await;
                    self.register_update();
                    AppResponse::Empty
                }
                AppTask::EditBooks(books, edits) => {
                    for book in books.to_vec().into_iter() {
                        let _ = self.edit_book_with_id(book, &edits).await;
                    }
                    AppResponse::Empty
                }
                AppTask::EditSelection(selection, edits) => {
                    let _ = self
                        .db
                        .write()
                        .await
                        .edit_selected(&selection, &edits)
                        .await;
                    AppResponse::Empty
                }
                AppTask::AddBooks(sources) => {
                    // TODO: Handle failed reads.
                    // TODO: Provide feedback about duplicated books
                    //  and some method to resolve duplicates, and add flag to
                    //  check if duplicates exist before inserting (enabled by default)
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
                // Add details about strategies (eg. which types of books, what to do on conflict)
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

    #[must_use]
    fn take_update(&mut self) -> bool {
        std::mem::replace(&mut self.updated, false)
    }

    #[must_use]
    async fn saved(&mut self) -> bool {
        self.db.read().await.saved().await
    }
}
