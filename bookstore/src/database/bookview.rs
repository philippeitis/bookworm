use crate::database::basic_database::IndexableDatabase;
use crate::database::{AppDatabase, DatabaseError, PageCursor};
use crate::record::Book;

#[derive(Debug)]
pub(crate) enum BookViewError {
    Database(DatabaseError),
    NoBookSelected,
}

impl From<DatabaseError> for BookViewError {
    fn from(e: DatabaseError) -> Self {
        BookViewError::Database(e)
    }
}

pub(crate) trait BookView<D: AppDatabase> {
    fn new(db: D) -> Self;

    fn get_books_cursored(&self) -> Result<Vec<Book>, BookViewError>;

    fn sort_by_column<S: AsRef<str>>(&mut self, col: S, reverse: bool)
        -> Result<(), DatabaseError>;

    fn get_book(&self, id: u32) -> Result<Book, DatabaseError>;

    fn remove_book(&mut self, id: u32) -> Result<(), DatabaseError>;

    fn get_selected_book(&self) -> Result<Book, BookViewError>;

    fn remove_selected_book(&mut self) -> Result<(), BookViewError>;

    fn edit_selected_book<S: AsRef<str>, T: AsRef<str>>(
        &mut self,
        col: S,
        new_val: T,
    ) -> Result<(), BookViewError>;

    fn inner(&self) -> &D;

    fn refresh_window_size(&mut self, size: usize);

    fn window_size(&self) -> usize;

    fn select(&mut self, item: usize) -> bool;

    fn selected(&self) -> Option<usize>;

    fn deselect(&mut self) -> bool;

    fn read<B>(&self, f: impl Fn(&D) -> B) -> B;

    fn write<B>(&mut self, f: impl Fn(&mut D) -> B) -> B;
}

pub(crate) trait ScrollableBookView<D: AppDatabase>: BookView<D> {
    fn scroll_up(&mut self, scroll: usize) -> bool;

    fn scroll_down(&mut self, scroll: usize) -> bool;

    fn select_up(&mut self) -> bool;

    fn select_down(&mut self) -> bool;

    fn page_up(&mut self) -> bool;

    fn page_down(&mut self) -> bool;

    fn home(&mut self) -> bool;

    fn end(&mut self) -> bool;
}

pub(crate) struct BasicBookView<D: AppDatabase> {
    cursor: PageCursor,
    db: D,
}

impl<D: AppDatabase> BasicBookView<D> {
    fn selected_index_as_err(&self) -> Result<usize, BookViewError> {
        match self.cursor.selected_index() {
            None => Err(BookViewError::NoBookSelected),
            Some(x) => Ok(x),
        }
    }
}

impl<D: IndexableDatabase> BookView<D> for BasicBookView<D> {
    fn new(db: D) -> Self {
        Self {
            cursor: PageCursor::new(0, 0),
            db,
        }
    }

    fn get_books_cursored(&self) -> Result<Vec<Book>, BookViewError> {
        Ok(self.db.get_books_indexed(self.cursor.window_range())?)
    }

    fn sort_by_column<S: AsRef<str>>(
        &mut self,
        col: S,
        reverse: bool,
    ) -> Result<(), DatabaseError> {
        self.db.sort_books_by_col(col.as_ref(), reverse)?;
        Ok(())
    }

    fn get_book(&self, id: u32) -> Result<Book, DatabaseError> {
        self.db.get_book(id)
    }

    fn remove_book(&mut self, id: u32) -> Result<(), DatabaseError> {
        self.write(|db| db.remove_book(id))
    }

    fn get_selected_book(&self) -> Result<Book, BookViewError> {
        Ok(self.db.get_book_indexed(self.selected_index_as_err()?)?)
    }

    fn remove_selected_book(&mut self) -> Result<(), BookViewError> {
        let index = self.selected_index_as_err()?;
        Ok(self.write(|db| db.remove_book_indexed(index))?)
    }

    fn edit_selected_book<S: AsRef<str>, T: AsRef<str>>(
        &mut self,
        col: S,
        new_val: T,
    ) -> Result<(), BookViewError> {
        Ok(self
            .db
            .edit_book_indexed(self.selected_index_as_err()?, col, new_val)?)
    }

    fn inner(&self) -> &D {
        &self.db
    }

    fn refresh_window_size(&mut self, size: usize) {
        self.cursor.refresh_window_size(size)
    }

    fn window_size(&self) -> usize {
        self.cursor.window_size()
    }

    fn select(&mut self, item: usize) -> bool {
        self.cursor.select(Some(item))
    }

    fn selected(&self) -> Option<usize> {
        self.cursor.selected()
    }

    fn deselect(&mut self) -> bool {
        self.cursor.select(None)
    }

    fn read<B>(&self, f: impl Fn(&D) -> B) -> B {
        f(&self.db)
    }

    fn write<B>(&mut self, f: impl Fn(&mut D) -> B) -> B {
        let v = f(&mut self.db);
        self.push_cursor_back(self.db.size());
        v
    }
}

impl<D: AppDatabase> BasicBookView<D> {
    fn push_cursor_back(&mut self, len: usize) {
        self.cursor.refresh_height(len);
        if let Some(s) = self.cursor.selected() {
            if s == self.cursor.window_size() && s != 0 {
                self.cursor.select(Some(s - 1));
            }
        };
    }
}

impl<D: IndexableDatabase> ScrollableBookView<D> for BasicBookView<D> {
    fn scroll_up(&mut self, scroll: usize) -> bool {
        self.cursor.scroll_up(scroll)
    }

    fn scroll_down(&mut self, scroll: usize) -> bool {
        self.cursor.scroll_down(scroll)
    }

    fn select_up(&mut self) -> bool {
        self.cursor.select_up()
    }

    fn select_down(&mut self) -> bool {
        self.cursor.select_down()
    }

    fn page_up(&mut self) -> bool {
        self.cursor.page_up()
    }

    fn page_down(&mut self) -> bool {
        self.cursor.page_down()
    }

    fn home(&mut self) -> bool {
        self.cursor.home()
    }

    fn end(&mut self) -> bool {
        self.cursor.end()
    }
}
