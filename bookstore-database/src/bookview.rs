use std::collections::HashSet;
use std::sync::Arc;

use tokio::sync::RwLock;

use async_trait::async_trait;
use indexmap::map::IndexMap;
use unicase::UniCase;

use bookstore_records::book::{BookID, ColumnIdentifier, RecordError};
use bookstore_records::{Book, ColumnOrder};

use crate::paged_cursor::{PageCursorMultiple, RelativeSelection, Selection};
use crate::search::{Error as SearchError, Search};
use crate::{AppDatabase, DatabaseError, IndexableDatabase};

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

#[async_trait]
pub trait BookView<D: AppDatabase + Send + Sync> {
    async fn new(db: Arc<RwLock<D>>) -> Self;

    async fn get_books_cursored(&self) -> Result<Vec<Arc<Book>>, BookViewError<D::Error>>;

    async fn sort_by_columns(
        &mut self,
        cols: &[(ColumnIdentifier, ColumnOrder)],
    ) -> Result<(), DatabaseError<D::Error>>;

    async fn get_book(&self, id: BookID) -> Result<Arc<Book>, DatabaseError<D::Error>>;

    /// Removes the specified book from the upper scopes. Note that this does not affect the root
    /// scope, which depends on the database, and must be refreshed with a call to `refresh_db_size()`.
    /// Not calling `refresh_db_size()` after deleting from the underlying database is undefined behaviour.
    fn remove_book(&mut self, id: BookID);

    /// Removes the specified books from the upper scopes. Note that this does not affect the root
    /// scope, which depends on the database, and must be refreshed with a call to `refresh_db_size()`
    fn remove_books(&mut self, ids: &HashSet<BookID>);

    async fn get_selected_books(&self) -> Result<Vec<Arc<Book>>, BookViewError<D::Error>>;

    /// Removes the books selected in the last scope, except if the last scope is the database, in
    /// which case, the user must delete the books from the database and manually refresh the root
    /// cursor with a call to `refresh_db_size()`.
    async fn remove_selected_books(&mut self) -> Result<HashSet<BookID>, BookViewError<D::Error>>;

    fn refresh_window_size(&mut self, size: usize) -> bool;

    async fn clear(&mut self);

    fn window_size(&self) -> usize;

    // fn select(&mut self, item: usize) -> bool;

    fn selected(&self) -> Option<&Selection>;

    fn make_selection_visible(&mut self) -> bool;

    fn relative_selections(&self) -> Option<RelativeSelection>;

    fn deselect_all(&mut self) -> bool;

    async fn refresh_db_size(&mut self);

    async fn has_column(&self, col: &UniCase<String>) -> Result<bool, DatabaseError<D::Error>>;
}

#[async_trait]
pub trait ScrollableBookView<D: AppDatabase + Send + Sync>: BookView<D> {
    async fn jump_to(&mut self, searches: &[Search]) -> Result<bool, DatabaseError<D::Error>>;

    fn top_index(&self) -> usize;

    fn scroll_up(&mut self, scroll: usize) -> bool;

    fn scroll_down(&mut self, scroll: usize) -> bool;

    fn page_up(&mut self) -> bool;

    fn page_down(&mut self) -> bool;

    fn home(&mut self) -> bool;

    fn end(&mut self) -> bool;

    fn up(&mut self) -> bool;

    fn down(&mut self) -> bool;

    fn select_up(&mut self) -> bool;

    fn select_down(&mut self) -> bool;

    fn select_page_up(&mut self) -> bool;

    fn select_page_down(&mut self) -> bool;

    fn select_to_start(&mut self) -> bool;

    fn select_to_end(&mut self) -> bool;
}

#[async_trait]
pub trait NestedBookView<D: AppDatabase + Send + Sync>: BookView<D> {
    async fn push_scope(&mut self, searches: &[Search]) -> Result<(), BookViewError<D::Error>>;

    fn pop_scope(&mut self) -> bool;
}

pub struct SearchableBookView<D: AppDatabase> {
    scopes: Vec<(PageCursorMultiple, IndexMap<BookID, Arc<Book>>)>,
    // The "root" scope.
    root_cursor: PageCursorMultiple,
    db: Arc<RwLock<D>>,
}

impl<D: AppDatabase> SearchableBookView<D> {
    fn active_cursor_mut(&mut self) -> &mut PageCursorMultiple {
        match self.scopes.last_mut() {
            None => &mut self.root_cursor,
            Some((cursor, _)) => cursor,
        }
    }
}

#[async_trait]
impl<D: IndexableDatabase + Send + Sync> BookView<D> for SearchableBookView<D> {
    async fn new(db: Arc<RwLock<D>>) -> Self {
        let size = db.read().await.size().await;
        Self {
            scopes: vec![],
            root_cursor: PageCursorMultiple::new(size, 0),
            db,
        }
    }

    async fn get_books_cursored(&self) -> Result<Vec<Arc<Book>>, BookViewError<D::Error>> {
        match self.scopes.last() {
            None => Ok(self
                .db
                .read()
                .await
                .get_books_indexed(self.root_cursor.window_range())
                .await?),
            Some((cursor, books)) => Ok(cursor
                .window_range()
                .filter_map(|i| books.get_index(i))
                .map(|(_, b)| b.clone())
                .collect()),
        }
    }

    async fn sort_by_columns(
        &mut self,
        cols: &[(ColumnIdentifier, ColumnOrder)],
    ) -> Result<(), DatabaseError<D::Error>> {
        self.scopes
            .iter_mut()
            .for_each(|(_, scope)| scope.sort_by(|_, a, _, b| b.cmp_columns(a, &cols)));

        Ok(())
    }

    async fn get_book(&self, id: BookID) -> Result<Arc<Book>, DatabaseError<D::Error>> {
        self.db.read().await.get_book(id).await
    }

    fn remove_book(&mut self, id: BookID) {
        for (cursor, map) in self.scopes.iter_mut() {
            // Required to maintain sort order.
            if map.shift_remove(&id).is_none() {
                break;
            } else {
                cursor.refresh_height(map.len());
                // if let Some(s) = cursor.selected() {
                //     if s == cursor.window_size() && s != 0 {
                //         cursor.select(s - 1);
                //     }
                // }
            }
        }
    }

    fn remove_books(&mut self, ids: &HashSet<BookID>) {
        for (cursor, map) in self.scopes.iter_mut() {
            // Required to maintain sort order.
            map.retain(|id, _| !ids.contains(id));
            cursor.refresh_height(map.len());
            match cursor.selected().cloned() {
                None => {}
                Some(Selection::Single(s)) => {
                    if s != 0 {
                        cursor.select_index(s - 1);
                    }
                }
                Some(Selection::Range(start, _, _)) => {
                    cursor.select_index_and_make_visible(start.saturating_sub(1));
                }
                _ => unimplemented!("Non-continuous selections not currently supported."),
            }
        }
    }

    async fn get_selected_books(&self) -> Result<Vec<Arc<Book>>, BookViewError<D::Error>> {
        match self.scopes.last() {
            None => match self.root_cursor.selected() {
                None => Err(BookViewError::NoBookSelected),
                Some(Selection::Single(i)) => {
                    Ok(vec![self.db.read().await.get_book_indexed(*i).await?])
                }
                Some(Selection::Range(start, end, _)) => {
                    Ok(self.db.read().await.get_books_indexed(*start..*end).await?)
                }
                Some(Selection::Multi(multi, _)) => {
                    let mut results = Vec::with_capacity(multi.len());
                    let db = self.db.read().await;
                    for i in multi.iter().copied() {
                        results.push(db.get_book_indexed(i).await?);
                    }
                    Ok(results)
                }
            },
            Some((cursor, books)) => match cursor.selected() {
                None => Err(BookViewError::NoBookSelected),
                Some(Selection::Single(i)) => Ok(vec![books[*i].clone()]),
                Some(Selection::Range(start, end, _)) => Ok((*start..*end)
                    .filter_map(|i| books.get_index(i))
                    .map(|(_, b)| b.clone())
                    .collect::<Vec<_>>()),
                Some(Selection::Multi(multi, _)) => Ok(multi
                    .iter()
                    .copied()
                    .filter_map(|i| books.get_index(i))
                    .map(|(_, b)| b.clone())
                    .collect::<Vec<_>>()),
            },
        }
    }

    async fn remove_selected_books(&mut self) -> Result<HashSet<BookID>, BookViewError<D::Error>> {
        let mut selected_books = HashSet::new();
        for book in self.get_selected_books().await? {
            selected_books.insert(book.id());
        }
        self.remove_books(&selected_books);
        Ok(selected_books)
    }

    fn refresh_window_size(&mut self, size: usize) -> bool {
        self.scopes
            .iter_mut()
            .map(|(a, _)| a)
            .chain(std::iter::once(&mut self.root_cursor))
            .map(|a| a.refresh_window_size(size))
            .fold(false, |a, b| a | b)
    }

    async fn clear(&mut self) {
        self.scopes.clear();
        self.refresh_db_size().await;
    }

    fn window_size(&self) -> usize {
        self.root_cursor.window_size()
    }

    fn selected(&self) -> Option<&Selection> {
        match self.scopes.last() {
            None => self.root_cursor.selected(),
            Some((cursor, _)) => cursor.selected(),
        }
    }

    fn make_selection_visible(&mut self) -> bool {
        match self.scopes.last_mut() {
            None => self.root_cursor.make_selected_visible(),
            Some((cursor, _)) => cursor.make_selected_visible(),
        }
    }

    fn relative_selections(&self) -> Option<RelativeSelection> {
        match self.scopes.last() {
            None => self.root_cursor.relative_selections(),
            Some((cursor, _)) => cursor.relative_selections(),
        }
    }

    fn deselect_all(&mut self) -> bool {
        match self.scopes.last_mut() {
            None => self.root_cursor.deselect(),
            Some((cursor, _)) => cursor.deselect(),
        }
    }

    async fn refresh_db_size(&mut self) {
        log("refreshing db size.");
        let db_size = self.db.read().await.size().await;
        log(format!("size is {}.", db_size));
        log(format!("selection is {:?}.", self.root_cursor.selected()));
        self.root_cursor.refresh_height(db_size);
        match self.root_cursor.selected().cloned() {
            None => {}
            Some(Selection::Single(s)) => {
                if self.root_cursor.at_end() {
                    if s > db_size {
                        self.root_cursor.select_index(db_size.saturating_sub(1));
                    } else {
                        self.root_cursor.select_index(s.saturating_sub(1));
                    }
                }
            }
            Some(Selection::Range(start, end, _)) => {
                // TODO: select start index, relatively speaking.
                if start >= db_size {
                    self.root_cursor
                        .select_index_and_make_visible(db_size.saturating_sub(1));
                } else {
                    self.root_cursor.select_index_and_make_visible(start);
                }
            }
            _ => unimplemented!("Non-continuous selections not currently supported."),
        }
        log(format!("selection is {:?}.", self.root_cursor.selected()));

        // if let Some(s) = self.root_cursor.selected() {
        //     if s != 0 && (s == self.root_cursor.window_size() || s == db_size) {
        //         self.root_cursor.select_single(s - 1);
        //     }
        // };
    }

    async fn has_column(&self, col: &UniCase<String>) -> Result<bool, DatabaseError<D::Error>> {
        self.db.read().await.has_column(col).await
    }
}

#[async_trait]
impl<D: IndexableDatabase + Send + Sync> NestedBookView<D> for SearchableBookView<D> {
    async fn push_scope(&mut self, searches: &[Search]) -> Result<(), BookViewError<D::Error>> {
        let results: IndexMap<_, _> = match self.scopes.last() {
            None => self
                .db
                .read()
                .await
                .find_matches(searches)
                .await?
                .into_iter()
                .map(|book| (book.id(), book))
                .collect(),

            Some((_, books)) => {
                let mut results: Vec<_> = books.values().cloned().collect();
                for search in searches {
                    let matcher = search.clone().into_matcher()?;
                    results.retain(|book| matcher.is_match(book));
                }

                results.into_iter().map(|b| (b.id(), b)).collect()
            }
        };

        self.scopes.push((
            PageCursorMultiple::new(self.root_cursor.window_size(), results.len()),
            results,
        ));

        Ok(())
    }

    fn pop_scope(&mut self) -> bool {
        self.scopes.pop().is_some()
    }
}

#[async_trait]
impl<D: IndexableDatabase + Send + Sync> ScrollableBookView<D> for SearchableBookView<D> {
    async fn jump_to(&mut self, searches: &[Search]) -> Result<bool, DatabaseError<D::Error>> {
        let target_book = match self.scopes.last() {
            None => self.db.read().await.find_book_index(searches).await?,
            Some((_, books)) => {
                let mut results: Vec<_> = books.values().cloned().collect();
                for search in searches {
                    let matcher = search.clone().into_matcher()?;
                    results.retain(|book| matcher.is_match(&book));
                }
                results.first().cloned().map(|b| {
                    books
                        .get_index_of(&b.id())
                        .expect("Reference to existing book was invalidated during search.")
                })
            }
        };

        Ok(if let Some(index) = target_book {
            self.active_cursor_mut()
                .select_index_and_make_visible(index)
        } else {
            false
        })
    }

    fn top_index(&self) -> usize {
        match self.scopes.last() {
            None => self.root_cursor.top_index(),
            Some((cursor, _)) => cursor.top_index(),
        }
    }
    fn scroll_up(&mut self, scroll: usize) -> bool {
        self.active_cursor_mut().scroll_up(scroll)
    }

    fn scroll_down(&mut self, scroll: usize) -> bool {
        self.active_cursor_mut().scroll_down(scroll)
    }

    fn page_up(&mut self) -> bool {
        self.active_cursor_mut().page_up()
    }

    fn page_down(&mut self) -> bool {
        self.active_cursor_mut().page_down()
    }

    fn home(&mut self) -> bool {
        self.active_cursor_mut().home()
    }

    fn end(&mut self) -> bool {
        self.active_cursor_mut().end()
    }

    fn up(&mut self) -> bool {
        self.active_cursor_mut().up()
    }

    fn down(&mut self) -> bool {
        self.active_cursor_mut().down()
    }

    fn select_up(&mut self) -> bool {
        self.active_cursor_mut().select_up(1)
    }

    fn select_down(&mut self) -> bool {
        self.active_cursor_mut().select_down(1)
    }

    fn select_page_up(&mut self) -> bool {
        self.active_cursor_mut().select_page_up()
    }

    fn select_page_down(&mut self) -> bool {
        self.active_cursor_mut().select_page_down()
    }

    fn select_to_start(&mut self) -> bool {
        self.active_cursor_mut().select_to_home()
    }

    fn select_to_end(&mut self) -> bool {
        self.active_cursor_mut().select_to_end()
    }
}
