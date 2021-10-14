use std::sync::Arc;

use tokio::sync::RwLock;

use unicase::UniCase;

use bookstore_records::book::{BookID, ColumnIdentifier, RecordError};
use bookstore_records::{Book, ColumnOrder};

use crate::paginator::Paginator;
use crate::search::{Error as SearchError, Search};
use crate::{AppDatabase, DatabaseError};

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

#[derive(Debug)]
pub enum BookViewError<DBError> {
    Database(DatabaseError<DBError>),
    NoBookSelected,
    Search,
    Record,
}

pub enum BookViewIndex {
    ID(BookID),
    Index(usize),
}

impl<DBError> From<DatabaseError<DBError>> for BookViewError<DBError> {
    fn from(e: DatabaseError<DBError>) -> Self {
        BookViewError::Database(e)
    }
}

impl<DBError> From<SearchError> for BookViewError<DBError> {
    fn from(_: SearchError) -> Self {
        BookViewError::Search
    }
}

impl<DBError> From<RecordError> for BookViewError<DBError> {
    fn from(_: RecordError) -> Self {
        BookViewError::Record
    }
}

pub struct BookView<D: AppDatabase> {
    scopes: Vec<Paginator<D>>,
    // The "root" scope.
    root_cursor: Paginator<D>,
    db: Arc<RwLock<D>>,
}

impl<D: AppDatabase> BookView<D> {
    fn active_cursor_mut(&mut self) -> &mut Paginator<D> {
        match self.scopes.last_mut() {
            None => &mut self.root_cursor,
            Some(cursor) => cursor,
        }
    }
}

impl<D: AppDatabase + Send + Sync> BookView<D> {
    pub async fn new(db: Arc<RwLock<D>>) -> Self {
        Self {
            scopes: vec![],
            root_cursor: Paginator::new(db.clone(), 0, vec![].into_boxed_slice()),
            db,
        }
    }

    pub async fn get_books_cursored(&self) -> Result<Vec<Arc<Book>>, BookViewError<D::Error>> {
        match self.scopes.last() {
            None => Ok(self.root_cursor.window().to_vec()),
            Some(cursor) => Ok(cursor.window().to_vec()),
        }
    }

    pub async fn sort_by_columns(
        &mut self,
        cols: &[(ColumnIdentifier, ColumnOrder)],
    ) -> Result<(), DatabaseError<D::Error>> {
        for scope in std::iter::once(&mut self.root_cursor).chain(self.scopes.iter_mut()) {
            // Required to maintain sort order.
            scope.sort_by(&cols).await?;
        }
        Ok(())
    }

    pub async fn get_book(&self, id: BookID) -> Result<Arc<Book>, DatabaseError<D::Error>> {
        self.db.read().await.get_book(id).await
    }

    pub fn get_selected_books(&self) -> Vec<Arc<Book>> {
        match self.scopes.last() {
            None => self.root_cursor.selected(),
            Some(cursor) => cursor.selected(),
        }
    }

    pub async fn refresh_window_size(
        &mut self,
        size: usize,
    ) -> Result<(), BookViewError<D::Error>> {
        for cursor in std::iter::once(&mut self.root_cursor).chain(self.scopes.iter_mut()) {
            cursor.update_window_size(size).await?;
        }
        Ok(())
    }

    pub async fn clear(&mut self) -> Result<(), BookViewError<D::Error>> {
        self.scopes.clear();
        self.root_cursor.refresh().await?;
        Ok(())
    }

    pub fn window_size(&self) -> usize {
        self.root_cursor.window_size()
    }

    pub fn relative_selections(&self) -> Vec<usize> {
        match self.scopes.last() {
            None => self.root_cursor.relative_selections(),
            Some(cursor) => cursor.relative_selections(),
        }
    }

    pub fn deselect_all(&mut self) {
        match self.scopes.last_mut() {
            None => self.root_cursor.deselect(),
            Some(cursor) => cursor.deselect(),
        }
    }

    pub async fn refresh(&mut self) -> Result<(), DatabaseError<D::Error>> {
        log("refreshing db size.");
        let db_size = self.db.read().await.size().await;
        log(format!("size is {}.", db_size));
        log(format!("selection is {:?}.", self.root_cursor.selected()));
        for cursor in std::iter::once(&mut self.root_cursor).chain(self.scopes.iter_mut()) {
            cursor.refresh().await?;
        }
        Ok(())
    }

    pub async fn has_column(&self, col: &UniCase<String>) -> Result<bool, DatabaseError<D::Error>> {
        self.db.read().await.has_column(col).await
    }
}

impl<D: AppDatabase + Send + Sync> BookView<D> {
    pub async fn push_scope(&mut self, searches: &[Search]) -> Result<(), BookViewError<D::Error>> {
        self.scopes.push(Paginator::new(
            self.db.clone(),
            self.root_cursor.window_size(),
            self.root_cursor.sort_rules().to_vec().into_boxed_slice(),
        ));
        match self.scopes.last() {
            None => {
                // Read from DB until window is full.
                unimplemented!()
            }

            Some(scope) => {
                // Read from paginator until window is full.
                unimplemented!()
            }
        };

        // self.scopes.push(Paginator::new());

        Ok(())
    }

    pub fn pop_scope(&mut self) -> bool {
        self.scopes.pop().is_some()
    }
}

impl<D: AppDatabase + Send + Sync> BookView<D> {
    pub async fn jump_to(&mut self, searches: &[Search]) -> Result<bool, DatabaseError<D::Error>> {
        // Create temporary paginator with window size of 1
        // and make discovered book visible.
        unimplemented!()
    }

    pub async fn scroll_up(&mut self, scroll: usize) -> Result<(), DatabaseError<D::Error>> {
        self.active_cursor_mut().scroll_up(scroll).await
    }

    pub async fn scroll_down(&mut self, scroll: usize) -> Result<(), DatabaseError<D::Error>> {
        self.active_cursor_mut().scroll_down(scroll).await
    }

    pub async fn page_up(&mut self) -> Result<(), DatabaseError<D::Error>> {
        self.active_cursor_mut().page_up().await
    }

    pub async fn page_down(&mut self) -> Result<(), DatabaseError<D::Error>> {
        self.active_cursor_mut().page_down().await
    }

    pub async fn home(&mut self) -> Result<(), DatabaseError<D::Error>> {
        self.active_cursor_mut().home().await
    }

    pub async fn end(&mut self) -> Result<(), DatabaseError<D::Error>> {
        self.active_cursor_mut().end().await
    }

    pub async fn up(&mut self) -> Result<(), DatabaseError<D::Error>> {
        self.active_cursor_mut().up().await
    }

    pub async fn down(&mut self) -> Result<(), DatabaseError<D::Error>> {
        self.active_cursor_mut().down().await
    }

    pub async fn select_up(&mut self) -> Result<(), DatabaseError<D::Error>> {
        self.active_cursor_mut().select_up(1).await
    }

    pub async fn select_down(&mut self) -> Result<(), DatabaseError<D::Error>> {
        self.active_cursor_mut().select_down(1).await
    }

    pub async fn select_all(&mut self) -> Result<(), DatabaseError<D::Error>> {
        self.active_cursor_mut().select_all().await
    }

    pub async fn select_page_up(&mut self) -> Result<(), DatabaseError<D::Error>> {
        self.active_cursor_mut().select_page_up().await
    }

    pub async fn select_page_down(&mut self) -> Result<(), DatabaseError<D::Error>> {
        self.active_cursor_mut().select_page_down().await
    }

    pub async fn select_to_start(&mut self) -> Result<(), DatabaseError<D::Error>> {
        self.active_cursor_mut().select_to_start().await
    }

    pub async fn select_to_end(&mut self) -> Result<(), DatabaseError<D::Error>> {
        self.active_cursor_mut().select_to_end().await
    }
}
