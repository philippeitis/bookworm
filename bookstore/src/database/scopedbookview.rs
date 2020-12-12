use indexmap::IndexMap;
use regex::Regex;
use sublime_fuzzy::best_match;

use crate::database::basic_database::IndexableDatabase;
use crate::database::bookview::BookViewError;
use crate::database::search::Search;
use crate::database::{AppDatabase, BookView, DatabaseError, PageCursor, ScrollableBookView};
use crate::record::book::ColumnIdentifier;
use crate::record::Book;

pub(crate) trait ScopedBookView<D: AppDatabase>: BookView<D> {
    fn push_scope(&mut self, search: Search) -> Result<(), BookViewError>;

    fn pop_scope(&mut self) -> bool;
}

pub(crate) struct SearchedBookView<D: AppDatabase> {
    scopes: Vec<(PageCursor, IndexMap<u32, Book>)>,
    // Effectively root scope.
    root_cursor: PageCursor,
    db: D,
}

impl<'a, D: AppDatabase> SearchedBookView<D> {
    fn active_cursor_mut(&mut self) -> &mut PageCursor {
        match self.scopes.last_mut() {
            None => &mut self.root_cursor,
            Some((cursor, _)) => cursor,
        }
    }
}

impl<D: IndexableDatabase> BookView<D> for SearchedBookView<D> {
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
                .map(|i| books.get_index(i))
                .filter_map(|b| b)
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

        for scope in self.scopes.iter_mut() {
            if reverse {
                scope.1.sort_by(|_, a, _, b| b.cmp_column(a, &col))
            } else {
                scope.1.sort_by(|_, a, _, b| a.cmp_column(b, &col))
            }
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

    fn edit_selected_book<S: AsRef<str>, T: AsRef<str>>(
        &mut self,
        col: S,
        new_val: T,
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
            for (_, map) in self.scopes.iter_mut() {
                if let Some(val) = map.get_mut(&id) {
                    *val = book.clone();
                }
            }
            Ok(self.db.edit_book_with_id(id, col, new_val)?)
        } else {
            Ok(())
        }
    }

    fn inner(&self) -> &D {
        &self.db
    }

    fn refresh_window_size(&mut self, size: usize) {
        self.scopes
            .iter_mut()
            .map(|(a, _)| a)
            .chain(std::iter::once(&mut self.root_cursor))
            .for_each(|a| a.refresh_window_size(size))
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

impl<D: IndexableDatabase> ScopedBookView<D> for SearchedBookView<D> {
    fn push_scope(&mut self, search: Search) -> Result<(), BookViewError> {
        let mut results = IndexMap::new();
        if self.scopes.is_empty() {
            for book in self.db.find_matches(search)?.into_iter() {
                results.insert(book.get_id(), book);
            }

            self.scopes.push((
                PageCursor::new(self.root_cursor.window_size(), results.len()),
                results,
            ));
            return Ok(());
        }

        let (_, books) = self.scopes.last().unwrap();
        match search {
            Search::Regex(column, search) => {
                let col = ColumnIdentifier::from(column);
                let matcher = Regex::new(search.as_str())?;
                for (_, book) in books.iter() {
                    if matcher.is_match(&book.get_column_or(&col, "")) {
                        results.insert(book.get_id(), book.clone());
                    }
                }
            }
            Search::CaseSensitive(column, search) => {
                let col = ColumnIdentifier::from(column);
                for (_, book) in books.iter() {
                    if best_match(&search, &book.get_column_or(&col, "")).is_some() {
                        results.insert(book.get_id(), book.clone());
                    }
                }
            }
            Search::Default(column, search) => {
                let col = ColumnIdentifier::from(column);
                for (_, book) in books.iter() {
                    if best_match(&search, &book.get_column_or(&col, "")).is_some() {
                        results.insert(book.get_id(), book.clone());
                    }
                }
            }
        }

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

impl<D: IndexableDatabase> ScrollableBookView<D> for SearchedBookView<D> {
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
