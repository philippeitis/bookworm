use std::cell::{Ref, RefCell};
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::{Arc, RwLock};

use indexmap::map::IndexMap;
use unicase::UniCase;

use bookstore_records::book::BookID;
use bookstore_records::{book::ColumnIdentifier, Book, BookError};

use crate::search::{Error as SearchError, Search};
use crate::{AppDatabase, DatabaseError, IndexableDatabase, PageCursor};

macro_rules! book {
    ($book: ident) => {
        $book.as_ref().read().unwrap()
    };
}

#[derive(Debug)]
pub enum BookViewError<DBError> {
    Database(DatabaseError<DBError>),
    NoBookSelected,
    SearchError,
    BookError,
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
        BookViewError::SearchError
    }
}

impl<DBError> From<BookError> for BookViewError<DBError> {
    fn from(_: BookError) -> Self {
        BookViewError::BookError
    }
}

pub trait BookView<D: AppDatabase> {
    fn new(db: Rc<RefCell<D>>) -> Self;

    fn get_books_cursored(&self) -> Result<Vec<Arc<RwLock<Book>>>, BookViewError<D::Error>>;

    fn sort_by_column<S: AsRef<str>>(
        &mut self,
        col: S,
        reverse: bool,
    ) -> Result<(), DatabaseError<D::Error>>;

    fn sort_by_columns<S: AsRef<str>>(
        &mut self,
        cols: &[(S, bool)],
    ) -> Result<(), DatabaseError<D::Error>>;

    fn get_book(&self, id: BookID) -> Result<Arc<RwLock<Book>>, DatabaseError<D::Error>>;

    fn remove_book(&mut self, id: BookID);

    fn remove_books(&mut self, ids: &HashSet<BookID>);

    fn get_selected_book(&self) -> Result<Arc<RwLock<Book>>, BookViewError<D::Error>>;

    fn remove_selected_book(&mut self) -> Result<BookViewIndex, BookViewError<D::Error>>;

    fn refresh_window_size(&mut self, size: usize) -> bool;

    fn clear(&mut self);

    fn window_size(&self) -> usize;

    fn select(&mut self, item: usize) -> bool;

    fn selected(&self) -> Option<usize>;

    fn deselect(&mut self) -> bool;

    fn refresh_db_size(&mut self);

    fn has_column(&self, col: &UniCase<String>) -> Result<bool, DatabaseError<D::Error>>;
}

pub trait ScrollableBookView<D: AppDatabase>: BookView<D> {
    fn scroll_up(&mut self, scroll: usize) -> bool;

    fn scroll_down(&mut self, scroll: usize) -> bool;

    fn select_up(&mut self) -> bool;

    fn select_down(&mut self) -> bool;

    fn page_up(&mut self) -> bool;

    fn page_down(&mut self) -> bool;

    fn home(&mut self) -> bool;

    fn end(&mut self) -> bool;
}

pub trait NestedBookView<D: AppDatabase>: BookView<D> {
    fn push_scope(&mut self, searches: &[Search]) -> Result<(), BookViewError<D::Error>>;

    fn pop_scope(&mut self) -> bool;
}

pub struct SearchableBookView<D: AppDatabase> {
    scopes: Vec<(PageCursor, IndexMap<BookID, Arc<RwLock<Book>>>)>,
    // The "root" scope.
    root_cursor: PageCursor,
    db: Rc<RefCell<D>>,
}

impl<D: AppDatabase> SearchableBookView<D> {
    fn active_cursor_mut(&mut self) -> &mut PageCursor {
        match self.scopes.last_mut() {
            None => &mut self.root_cursor,
            Some((cursor, _)) => cursor,
        }
    }

    fn db(&self) -> Ref<D> {
        self.db.as_ref().borrow()
    }
}

impl<D: IndexableDatabase> BookView<D> for SearchableBookView<D> {
    fn new(db: Rc<RefCell<D>>) -> Self {
        let size = db.as_ref().borrow().size();
        Self {
            scopes: vec![],
            root_cursor: PageCursor::new(0, size),
            db,
        }
    }

    fn get_books_cursored(&self) -> Result<Vec<Arc<RwLock<Book>>>, BookViewError<D::Error>> {
        match self.scopes.last() {
            None => Ok(self
                .db()
                .get_books_indexed(self.root_cursor.window_range())?),
            Some((cursor, books)) => Ok(cursor
                .window_range()
                .filter_map(|i| books.get_index(i))
                .map(|(_, b)| b.clone())
                .collect()),
        }
    }

    fn sort_by_column<S: AsRef<str>>(
        &mut self,
        col: S,
        reverse: bool,
    ) -> Result<(), DatabaseError<D::Error>> {
        let col = ColumnIdentifier::from(col);
        if reverse {
            self.scopes.iter_mut().for_each(|(_, scope)| {
                scope.sort_by(|_, a, _, b| book!(b).cmp_column(&book!(a), &col))
            });
        } else {
            self.scopes.iter_mut().for_each(|(_, scope)| {
                scope.sort_by(|_, a, _, b| book!(a).cmp_column(&book!(b), &col))
            });
        }

        Ok(())
    }

    fn sort_by_columns<S: AsRef<str>>(
        &mut self,
        cols: &[(S, bool)],
    ) -> Result<(), DatabaseError<D::Error>> {
        let cols: Vec<_> = cols
            .iter()
            .map(|(c, r)| (ColumnIdentifier::from(c), *r))
            .collect();
        self.scopes.iter_mut().for_each(|(_, scope)| {
            scope.sort_by(|_, a, _, b| book!(b).cmp_columns(&book!(a), &cols))
        });

        Ok(())
    }

    fn get_book(&self, id: BookID) -> Result<Arc<RwLock<Book>>, DatabaseError<D::Error>> {
        self.db().get_book(id)
    }

    fn remove_book(&mut self, id: BookID) {
        for (cursor, map) in self.scopes.iter_mut() {
            // Required to maintain sort order.
            if map.shift_remove(&id).is_none() {
                break;
            } else {
                cursor.refresh_height(map.len());
                if let Some(s) = cursor.selected() {
                    if s == cursor.window_size() && s != 0 {
                        cursor.select(Some(s - 1));
                    }
                }
            }
        }
    }

    fn remove_books(&mut self, ids: &HashSet<BookID>) {
        for (cursor, map) in self.scopes.iter_mut() {
            // Required to maintain sort order.
            map.retain(|id, _| !ids.contains(id));
            cursor.refresh_height(map.len());
            if let Some(s) = cursor.selected() {
                if s == cursor.window_size() && s != 0 {
                    cursor.select(Some(s - 1));
                }
            }
        }
    }

    fn get_selected_book(&self) -> Result<Arc<RwLock<Book>>, BookViewError<D::Error>> {
        match self.scopes.last() {
            None => Ok(self.db().get_book_indexed(
                self.root_cursor
                    .selected_index()
                    .ok_or(BookViewError::NoBookSelected)?,
            )?),
            Some((cursor, books)) => Ok(books[cursor
                .selected_index()
                .ok_or(BookViewError::NoBookSelected)?]
            .clone()),
        }
    }

    fn remove_selected_book(&mut self) -> Result<BookViewIndex, BookViewError<D::Error>> {
        match self.scopes.last() {
            None => match self.root_cursor.selected_index() {
                None => Err(BookViewError::NoBookSelected),
                Some(i) => Ok(BookViewIndex::Index(i)),
            },
            Some((cursor, books)) => match cursor.selected_index() {
                None => Err(BookViewError::NoBookSelected),
                Some(i) => match books.get_index(i) {
                    Some((&id, _)) => {
                        self.remove_book(id);
                        Ok(BookViewIndex::ID(id))
                    }
                    None => Err(BookViewError::NoBookSelected),
                },
            },
        }
    }

    fn refresh_window_size(&mut self, size: usize) -> bool {
        self.scopes
            .iter_mut()
            .map(|(a, _)| a)
            .chain(std::iter::once(&mut self.root_cursor))
            .map(|a| a.refresh_window_size(size))
            .fold(false, |a, b| a | b)
    }

    fn clear(&mut self) {
        self.scopes.clear();
        self.root_cursor.refresh_height(0);
    }

    fn window_size(&self) -> usize {
        self.root_cursor.window_size()
    }

    fn select(&mut self, item: usize) -> bool {
        match self.scopes.last_mut() {
            None => self.root_cursor.select(Some(item)),
            Some((cursor, _)) => cursor.select(Some(item)),
        }
    }

    fn selected(&self) -> Option<usize> {
        match self.scopes.last() {
            None => self.root_cursor.selected(),
            Some((cursor, _)) => cursor.selected(),
        }
    }

    fn deselect(&mut self) -> bool {
        match self.scopes.last_mut() {
            None => self.root_cursor.select(None),
            Some((cursor, _)) => cursor.select(None),
        }
    }

    fn refresh_db_size(&mut self) {
        let db_size = self.db().size();
        self.root_cursor.refresh_height(db_size);
        if let Some(s) = self.root_cursor.selected() {
            if s != 0 && (s == self.root_cursor.window_size() || s == db_size) {
                self.root_cursor.select(Some(s - 1));
            }
        };
    }

    fn has_column(&self, col: &UniCase<String>) -> Result<bool, DatabaseError<D::Error>> {
        self.db().has_column(col)
    }
}

impl<D: IndexableDatabase> NestedBookView<D> for SearchableBookView<D> {
    fn push_scope(&mut self, searches: &[Search]) -> Result<(), BookViewError<D::Error>> {
        let results: IndexMap<_, _> = match self.scopes.last() {
            None => self
                .db()
                .find_matches(searches)?
                .into_iter()
                .map(|book| {
                    let id = book!(book).get_id();
                    (id, book)
                })
                .collect(),

            Some((_, books)) => {
                let mut results: Vec<_> = books.values().cloned().collect();
                for search in searches {
                    let matcher = search.clone().into_matcher()?;
                    results.retain(|book| matcher.is_match(&book!(book)));
                }

                results
                    .into_iter()
                    .map(|b| (book!(b).get_id(), b.clone()))
                    .collect()
            }
        };

        self.scopes.push((
            PageCursor::new(self.root_cursor.window_size(), results.len()),
            results,
        ));

        Ok(())
    }

    fn pop_scope(&mut self) -> bool {
        self.scopes.pop().is_some()
    }
}

impl<D: IndexableDatabase> ScrollableBookView<D> for SearchableBookView<D> {
    fn scroll_up(&mut self, scroll: usize) -> bool {
        self.active_cursor_mut().scroll_up(scroll)
    }

    fn scroll_down(&mut self, scroll: usize) -> bool {
        self.active_cursor_mut().scroll_down(scroll)
    }

    fn select_up(&mut self) -> bool {
        self.active_cursor_mut().select_up()
    }

    fn select_down(&mut self) -> bool {
        self.active_cursor_mut().select_down()
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
}
