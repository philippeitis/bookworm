use std::fmt::Debug;
use std::sync::Arc;

use tokio::sync::RwLock;

use unicase::UniCase;

use bookstore_records::book::{BookID, ColumnIdentifier, RecordError};
use bookstore_records::{Book, ColumnOrder};

use crate::paginator::{Paginator, Selection};
use crate::search::{Error as SearchError, Search};
use crate::{AppDatabase, DatabaseError};

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

pub struct BookView<D: AppDatabase + 'static> {
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

impl<D: AppDatabase + Send + Sync + 'static> BookView<D> {
    pub async fn new(db: Arc<RwLock<D>>) -> Self {
        Self {
            scopes: vec![],
            root_cursor: Paginator::new(db.clone(), 0, vec![].into_boxed_slice()),
            db,
        }
    }

    pub fn window(&self) -> Vec<Arc<Book>> {
        match self.scopes.last() {
            None => self.root_cursor.window().to_vec(),
            Some(cursor) => cursor.window().to_vec(),
        }
    }

    #[tracing::instrument(name = "Sorting all scopes", skip(self, cols))]
    pub async fn sort_by_columns(
        &mut self,
        cols: &[(ColumnIdentifier, ColumnOrder)],
    ) -> Result<(), DatabaseError<D::Error>> {
        for scope in std::iter::once(&mut self.root_cursor).chain(self.scopes.iter_mut()) {
            scope.sort_by(&cols).await?;
        }
        Ok(())
    }

    pub fn selected_books(&self) -> &Selection {
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

    pub fn window_size(&self) -> usize {
        self.root_cursor.window_size()
    }

    /// Returns the books in the selection with their index, relative to the top
    /// and the book itself
    pub fn relative_selections(&self) -> Vec<(usize, Arc<Book>)> {
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

    #[tracing::instrument(name = "Refreshing paginators", skip(self))]
    pub async fn refresh(&mut self) -> Result<(), DatabaseError<D::Error>> {
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
        self.scopes.push(self.create_paginator(searches));
        Ok(())
    }

    pub fn pop_scope(&mut self) -> bool {
        self.scopes.pop().is_some()
    }

    fn create_paginator(&self, searches: &[Search]) -> Paginator<D> {
        let mut matchers = vec![];
        for scope in &self.scopes {
            matchers.extend(scope.matchers().iter().map(|x| x.box_clone()));
        }
        for item in searches
            .iter()
            .cloned()
            .map(Search::into_matcher)
            .filter_map(Result::ok)
        {
            matchers.push(item);
        }
        Paginator::new(
            self.db.clone(),
            self.root_cursor.window_size(),
            self.root_cursor.sort_rules().to_vec().into_boxed_slice(),
        )
        .bind_match(matchers.into_boxed_slice())
    }

    #[tracing::instrument(name = "Jumping to match target", skip(self, searches))]
    pub async fn jump_to(&mut self, searches: &[Search]) -> Result<bool, DatabaseError<D::Error>> {
        // Create temporary paginator with window size of 1
        // and make discovered book visible.
        let _jump_span = tracing::info_span!("Saving new subscriber details in the database");
        let mut paginator = self.create_paginator(searches);
        paginator.update_window_size(1).await?;
        if let Some(book) = paginator.window().first().cloned() {
            tracing::info!("Found a match with id: {}", book.id());
            match self.scopes.last_mut() {
                None => &mut self.root_cursor,
                Some(cursor) => cursor,
            }
            .make_book_visible(Some(book))
            .await?;
        } else {
            tracing::info!("Did not find a match - no changes occuring.");
        };

        Ok(true)
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
