use bookstore_records::{book::ColumnIdentifier, Book, BookError};
use indexmap::map::IndexMap;
use regex::{Error as RegexError, Regex};
use sublime_fuzzy::best_match;

use crate::basic_database::IndexableDatabase;
use crate::search::Search;
use crate::{AppDatabase, DatabaseError, PageCursor};

#[derive(Debug)]
pub enum BookViewError {
    Database(DatabaseError),
    NoBookSelected,
    SearchError,
    BookError,
}

impl From<DatabaseError> for BookViewError {
    fn from(e: DatabaseError) -> Self {
        BookViewError::Database(e)
    }
}

impl From<RegexError> for BookViewError {
    fn from(_: RegexError) -> Self {
        BookViewError::SearchError
    }
}

impl From<BookError> for BookViewError {
    fn from(_: BookError) -> Self {
        BookViewError::BookError
    }
}

pub trait BookView<D: AppDatabase> {
    fn new(db: D) -> Self;

    fn get_books_cursored(&self) -> Result<Vec<Book>, BookViewError>;

    fn sort_by_column<S: AsRef<str>>(&mut self, col: S, reverse: bool)
        -> Result<(), DatabaseError>;

    fn get_book(&self, id: u32) -> Result<Book, DatabaseError>;

    fn remove_book(&mut self, id: u32) -> Result<(), DatabaseError>;

    fn get_selected_book(&self) -> Result<Book, BookViewError>;

    fn remove_selected_book(&mut self) -> Result<(), BookViewError>;

    fn edit_selected_book<S0: AsRef<str>, S1: AsRef<str>>(
        &mut self,
        col: S0,
        new_val: S1,
    ) -> Result<(), BookViewError>;

    fn inner(&self) -> &D;

    fn refresh_window_size(&mut self, size: usize) -> bool;

    fn window_size(&self) -> usize;

    fn select(&mut self, item: usize) -> bool;

    fn selected(&self) -> Option<usize>;

    fn deselect(&mut self) -> bool;

    fn read<B>(&self, f: impl Fn(&D) -> B) -> B;

    fn write<B>(&mut self, f: impl Fn(&mut D) -> B) -> B;
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
    fn push_scope(&mut self, search: Search) -> Result<(), BookViewError>;

    fn pop_scope(&mut self) -> bool;
}

pub struct SearchableBookView<D: AppDatabase> {
    scopes: Vec<(PageCursor, IndexMap<u32, Book>)>,
    // The "root" scope.
    root_cursor: PageCursor,
    db: D,
}

impl<D: AppDatabase> SearchableBookView<D> {
    fn active_cursor_mut(&mut self) -> &mut PageCursor {
        match self.scopes.last_mut() {
            None => &mut self.root_cursor,
            Some((cursor, _)) => cursor,
        }
    }
}

impl<D: IndexableDatabase> BookView<D> for SearchableBookView<D> {
    fn new(db: D) -> Self {
        Self {
            scopes: vec![],
            root_cursor: PageCursor::new(0, db.size()),
            db,
        }
    }

    fn get_books_cursored(&self) -> Result<Vec<Book>, BookViewError> {
        match self.scopes.last() {
            None => Ok(self.db.get_books_indexed(self.root_cursor.window_range())?),
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
    ) -> Result<(), DatabaseError> {
        self.db.sort_books_by_col(col.as_ref(), reverse)?;

        let col = ColumnIdentifier::from(col);

        if reverse {
            self.scopes
                .iter_mut()
                .for_each(|(_, scope)| scope.sort_by(|_, a, _, b| b.cmp_column(a, &col)));
        } else {
            self.scopes
                .iter_mut()
                .for_each(|(_, scope)| scope.sort_by(|_, a, _, b| a.cmp_column(b, &col)));
        }

        Ok(())
    }

    fn get_book(&self, id: u32) -> Result<Book, DatabaseError> {
        self.db.get_book(id)
    }

    fn remove_book(&mut self, id: u32) -> Result<(), DatabaseError> {
        for (cursor, map) in self.scopes.iter_mut() {
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

        self.write(|db| db.remove_book(id))
    }

    fn get_selected_book(&self) -> Result<Book, BookViewError> {
        match self.scopes.last() {
            None => Ok(self.db.get_book_indexed(
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

    fn remove_selected_book(&mut self) -> Result<(), BookViewError> {
        match self.scopes.last() {
            None => match self.root_cursor.selected_index() {
                None => Err(BookViewError::NoBookSelected),
                Some(i) => Ok(self.write(|db| db.remove_book_indexed(i))?),
            },
            Some((cursor, books)) => match cursor.selected_index() {
                None => Err(BookViewError::NoBookSelected),
                Some(i) => match books.get_index(i) {
                    Some((&id, _)) => Ok(self.remove_book(id)?),
                    None => Ok(()),
                },
            },
        }
    }

    fn edit_selected_book<S0: AsRef<str>, S1: AsRef<str>>(
        &mut self,
        col: S0,
        new_val: S1,
    ) -> Result<(), BookViewError> {
        let book = match self.scopes.last() {
            None => {
                let index = self
                    .root_cursor
                    .selected_index()
                    .ok_or(BookViewError::NoBookSelected)?;
                return Ok(self.db.edit_book_indexed(index, col, new_val)?);
            }
            Some((cursor, books)) => {
                let selected = cursor
                    .selected_index()
                    .ok_or(BookViewError::NoBookSelected)?;
                books.get_index(selected)
            }
        };

        if let Some((&id, book)) = book {
            let col_id = ColumnIdentifier::from(&col);
            let mut book = book.clone();
            book.set_column(&col_id, &new_val)?;
            self.scopes.iter_mut().for_each(|(_, map)| {
                if let Some(val) = map.get_mut(&id) {
                    *val = book.clone();
                }
            });
            Ok(self.db.edit_book_with_id(id, col, new_val)?)
        } else {
            Ok(())
        }
    }

    fn inner(&self) -> &D {
        &self.db
    }

    fn refresh_window_size(&mut self, size: usize) -> bool {
        self.scopes
            .iter_mut()
            .map(|(a, _)| a)
            .chain(std::iter::once(&mut self.root_cursor))
            .map(|a| a.refresh_window_size(size))
            .fold(false, |a, b| a | b)
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

    fn read<B>(&self, f: impl Fn(&D) -> B) -> B {
        f(&self.db)
    }

    fn write<B>(&mut self, f: impl Fn(&mut D) -> B) -> B {
        let v = f(&mut self.db);
        self.root_cursor.refresh_height(self.db.size());
        if let Some(s) = self.root_cursor.selected() {
            if s != 0 && (s == self.root_cursor.window_size() || s == self.db.size()) {
                self.root_cursor.select(Some(s - 1));
            }
        };
        v
    }
}

impl<D: IndexableDatabase> NestedBookView<D> for SearchableBookView<D> {
    fn push_scope(&mut self, search: Search) -> Result<(), BookViewError> {
        let results: IndexMap<_, _> = match self.scopes.last() {
            None => self
                .db
                .find_matches(search)?
                .into_iter()
                .map(|book| (book.get_id(), book))
                .collect(),
            Some((_, books)) => match search {
                Search::Regex(column, search) => {
                    let col = ColumnIdentifier::from(column);
                    let matcher = Regex::new(search.as_str())?;
                    books
                        .iter()
                        .filter(|(_, book)| matcher.is_match(&book.get_column_or(&col, "")))
                        .map(|(_, b)| (b.get_id(), b.clone()))
                        .collect()
                }
                Search::CaseSensitive(column, search) => {
                    let col = ColumnIdentifier::from(column);
                    books
                        .iter()
                        .filter(|(_, book)| {
                            best_match(&search, &book.get_column_or(&col, "")).is_some()
                        })
                        .map(|(_, b)| (b.get_id(), b.clone()))
                        .collect()
                }
                Search::Default(column, search) => {
                    let col = ColumnIdentifier::from(column);
                    books
                        .iter()
                        .filter(|(_, book)| {
                            best_match(&search, &book.get_column_or(&col, "")).is_some()
                        })
                        .map(|(_, b)| (b.get_id(), b.clone()))
                        .collect()
                }
            },
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
