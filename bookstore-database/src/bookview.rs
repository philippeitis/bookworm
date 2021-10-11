use std::collections::HashSet;
use std::sync::Arc;

use tokio::sync::RwLock;

use indexmap::map::IndexMap;
use unicase::UniCase;

use bookstore_records::book::{BookID, ColumnIdentifier, RecordError};
use bookstore_records::{Book, ColumnOrder};

use crate::paged_cursor::{PageCursor, RelativeSelection, Selection};
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

struct Scope {
    cursor: PageCursor,
    books: IndexMap<BookID, Arc<Book>>,
}

pub struct BookView<D: AppDatabase> {
    scopes: Vec<Scope>,
    // The "root" scope.
    root_cursor: PageCursor,
    db: Arc<RwLock<D>>,
}

impl<D: AppDatabase> BookView<D> {
    fn active_cursor_mut(&mut self) -> &mut PageCursor {
        match self.scopes.last_mut() {
            None => &mut self.root_cursor,
            Some(scope) => &mut scope.cursor,
        }
    }
}

impl<D: IndexableDatabase + Send + Sync> BookView<D> {
    pub async fn new(db: Arc<RwLock<D>>) -> Self {
        let size = db.read().await.size().await;
        Self {
            scopes: vec![],
            root_cursor: PageCursor::new(size, 0),
            db,
        }
    }

    pub async fn get_books_cursored(&self) -> Result<Vec<Arc<Book>>, BookViewError<D::Error>> {
        match self.scopes.last() {
            None => Ok(self
                .db
                .read()
                .await
                .get_books_indexed(self.root_cursor.window_range())
                .await?),
            Some(scope) => Ok(scope
                .cursor
                .window_range()
                .filter_map(|i| scope.books.get_index(i))
                .map(|(_, b)| b.clone())
                .collect()),
        }
    }

    pub async fn sort_by_columns(
        &mut self,
        cols: &[(ColumnIdentifier, ColumnOrder)],
    ) -> Result<(), DatabaseError<D::Error>> {
        self.scopes
            .iter_mut()
            .for_each(|scope| scope.books.sort_by(|_, a, _, b| b.cmp_columns(a, &cols)));

        Ok(())
    }

    pub async fn get_book(&self, id: BookID) -> Result<Arc<Book>, DatabaseError<D::Error>> {
        self.db.read().await.get_book(id).await
    }

    /// Removes the specified book from the upper scopes. Note that this does not affect the root
    /// scope, which depends on the database, and must be refreshed with a call to `refresh_db_size()`.
    /// Not calling `refresh_db_size()` after deleting from the underlying database is undefined behaviour.
    pub fn remove_book(&mut self, id: BookID) {
        for scope in self.scopes.iter_mut() {
            // Required to maintain sort order.
            if scope.books.shift_remove(&id).is_none() {
                break;
            } else {
                scope.cursor.refresh_height(scope.books.len());
                // if let Some(s) = cursor.selected() {
                //     if s == cursor.window_size() && s != 0 {
                //         cursor.select(s - 1);
                //     }
                // }
            }
        }
    }

    /// Removes the specified books from the upper scopes. Note that this does not affect the root
    /// scope, which depends on the database, and must be refreshed with a call to `refresh_db_size()`
    pub fn remove_books(&mut self, ids: &HashSet<BookID>) {
        for scope in self.scopes.iter_mut() {
            // Required to maintain sort order.
            scope.books.retain(|id, _| !ids.contains(id));
            scope.cursor.refresh_height(scope.books.len());
            match scope.cursor.selected() {
                None => {}
                Some(Selection::Single(s)) => {
                    if *s != 0 {
                        scope.cursor.select_index(*s - 1);
                    }
                }
                Some(Selection::Range(start, _, _)) => {
                    scope
                        .cursor
                        .select_index_and_make_visible(start.saturating_sub(1));
                }
                _ => unimplemented!("Non-continuous selections not currently supported."),
            }
        }
    }

    pub async fn get_selected_books(&self) -> Result<Vec<Arc<Book>>, BookViewError<D::Error>> {
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
            Some(scope) => match scope.cursor.selected() {
                None => Err(BookViewError::NoBookSelected),
                Some(Selection::Single(i)) => Ok(vec![scope.books[*i].clone()]),
                Some(Selection::Range(start, end, _)) => Ok((*start..*end)
                    .filter_map(|i| scope.books.get_index(i))
                    .map(|(_, b)| b.clone())
                    .collect::<Vec<_>>()),
                Some(Selection::Multi(multi, _)) => Ok(multi
                    .iter()
                    .copied()
                    .filter_map(|i| scope.books.get_index(i))
                    .map(|(_, b)| b.clone())
                    .collect::<Vec<_>>()),
            },
        }
    }

    /// Removes the books selected in the last scope, except if the last scope is the database, in
    /// which case, the user must delete the books from the database and manually refresh the root
    /// cursor with a call to `refresh_db_size()`.
    pub async fn remove_selected_books(
        &mut self,
    ) -> Result<HashSet<BookID>, BookViewError<D::Error>> {
        let mut selected_books = HashSet::new();
        for book in self.get_selected_books().await? {
            selected_books.insert(book.id());
        }
        match self.scopes.last_mut() {
            None => self.root_cursor.deselect(),
            Some(scope) => scope.cursor.deselect(),
        };
        self.remove_books(&selected_books);
        Ok(selected_books)
    }

    pub fn refresh_window_size(&mut self, size: usize) -> bool {
        self.scopes
            .iter_mut()
            .map(|scope| &mut scope.cursor)
            .chain(std::iter::once(&mut self.root_cursor))
            .map(|a| a.refresh_window_size(size))
            .fold(false, |a, b| a | b)
    }

    pub async fn clear(&mut self) {
        self.scopes.clear();
        self.refresh_db_size().await;
    }

    pub fn window_size(&self) -> usize {
        self.root_cursor.window_size()
    }

    pub fn selected(&self) -> Option<&Selection> {
        match self.scopes.last() {
            None => self.root_cursor.selected(),
            Some(scope) => scope.cursor.selected(),
        }
    }

    pub fn make_selection_visible(&mut self) -> bool {
        match self.scopes.last_mut() {
            None => self.root_cursor.make_selected_visible(),
            Some(scope) => scope.cursor.make_selected_visible(),
        }
    }

    pub fn relative_selections(&self) -> Option<RelativeSelection> {
        match self.scopes.last() {
            None => self.root_cursor.relative_selections(),
            Some(scope) => scope.cursor.relative_selections(),
        }
    }

    pub fn deselect_all(&mut self) -> bool {
        match self.scopes.last_mut() {
            None => self.root_cursor.deselect(),
            Some(scope) => scope.cursor.deselect(),
        }
    }

    pub async fn refresh_db_size(&mut self) {
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

    pub async fn has_column(&self, col: &UniCase<String>) -> Result<bool, DatabaseError<D::Error>> {
        self.db.read().await.has_column(col).await
    }
}

impl<D: IndexableDatabase + Send + Sync> BookView<D> {
    pub async fn push_scope(&mut self, searches: &[Search]) -> Result<(), BookViewError<D::Error>> {
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

            Some(scope) => {
                let mut results: Vec<_> = scope.books.values().cloned().collect();
                for search in searches {
                    let matcher = search.clone().into_matcher()?;
                    results.retain(|book| matcher.is_match(book));
                }

                results.into_iter().map(|b| (b.id(), b)).collect()
            }
        };

        self.scopes.push(Scope {
            cursor: PageCursor::new(self.root_cursor.window_size(), results.len()),
            books: results,
        });

        Ok(())
    }

    pub fn pop_scope(&mut self) -> bool {
        self.scopes.pop().is_some()
    }
}

impl<D: IndexableDatabase + Send + Sync> BookView<D> {
    pub async fn jump_to(&mut self, searches: &[Search]) -> Result<bool, DatabaseError<D::Error>> {
        let target_book = match self.scopes.last() {
            None => self.db.read().await.find_book_index(searches).await?,
            Some(scope) => {
                let mut results: Vec<_> = scope.books.values().cloned().collect();
                for search in searches {
                    let matcher = search.clone().into_matcher()?;
                    results.retain(|book| matcher.is_match(&book));
                }
                results.first().cloned().map(|b| {
                    scope
                        .books
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

    pub fn top_index(&self) -> usize {
        match self.scopes.last() {
            None => self.root_cursor.top_index(),
            Some(scope) => scope.cursor.top_index(),
        }
    }
    pub fn scroll_up(&mut self, scroll: usize) -> bool {
        self.active_cursor_mut().scroll_up(scroll)
    }

    pub fn scroll_down(&mut self, scroll: usize) -> bool {
        self.active_cursor_mut().scroll_down(scroll)
    }

    pub fn page_up(&mut self) -> bool {
        self.active_cursor_mut().page_up()
    }

    pub fn page_down(&mut self) -> bool {
        self.active_cursor_mut().page_down()
    }

    pub fn home(&mut self) -> bool {
        self.active_cursor_mut().home()
    }

    pub fn end(&mut self) -> bool {
        self.active_cursor_mut().end()
    }

    pub fn up(&mut self) -> bool {
        self.active_cursor_mut().up()
    }

    pub fn down(&mut self) -> bool {
        self.active_cursor_mut().down()
    }

    pub fn select_up(&mut self) -> bool {
        self.active_cursor_mut().select_up(1)
    }

    pub fn select_down(&mut self) -> bool {
        self.active_cursor_mut().select_down(1)
    }

    pub fn select_all(&mut self) -> bool {
        self.active_cursor_mut().select_all()
    }

    pub fn select_page_up(&mut self) -> bool {
        self.active_cursor_mut().select_page_up()
    }

    pub fn select_page_down(&mut self) -> bool {
        self.active_cursor_mut().select_page_down()
    }

    pub fn select_to_start(&mut self) -> bool {
        self.active_cursor_mut().select_to_home()
    }

    pub fn select_to_end(&mut self) -> bool {
        self.active_cursor_mut().select_to_end()
    }
}
