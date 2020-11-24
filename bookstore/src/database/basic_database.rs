use std::collections::{HashMap, HashSet};
use std::{fs, path};

use indexmap::IndexMap;
use rustbreak::{deser::Ron, FileDatabase, RustbreakError};
use serde::{Deserialize, Serialize};
use unicase::UniCase;

use crate::database::PageCursor;
use crate::record::book::ColumnIdentifier;
use crate::record::{Book, BookError};

#[derive(Debug)]
pub(crate) enum DatabaseError {
    Io(std::io::Error),
    Book(BookError),
    Backend(RustbreakError),
    BookNotFound(u32),
    NoBookSelected,
}

impl From<std::io::Error> for DatabaseError {
    fn from(e: std::io::Error) -> Self {
        DatabaseError::Io(e)
    }
}

impl From<RustbreakError> for DatabaseError {
    fn from(e: RustbreakError) -> Self {
        DatabaseError::Backend(e)
    }
}

impl From<BookError> for DatabaseError {
    fn from(e: BookError) -> Self {
        DatabaseError::Book(e)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub(crate) struct BookMap {
    max_id: u32,
    books: IndexMap<u32, Book>,
}

impl BookMap {
    /// Return a monotonically increasing ID to use for a new
    /// book.
    ///
    /// # Errors
    /// Will panic if the ID can no longer be correctly increased.
    fn new_id(&mut self) -> u32 {
        let id = self.max_id;
        if self.max_id == u32::MAX {
            panic!(format!(
                "Current ID is at maximum value of {} and can not be increased.",
                u32::MAX
            ));
        }
        self.max_id += 1;
        id
    }
}

/// A database which fully implements the functionality of the AppDatabase trait,
/// but does not guarantee that data is successfully written to disk.
pub(crate) struct BasicDatabase {
    backend: FileDatabase<BookMap, Ron>,
    /// All available columns. Case-insensitive.
    cols: HashSet<UniCase<String>>,
    cursor: PageCursor,
}

pub(crate) trait AppDatabase {
    /// Opens the database at the path if it exists.
    ///
    /// # Arguments
    ///
    /// * ` file_path ` - A path to a database.
    ///
    /// # Errors
    /// This function will return an error if the file can not be found, or the database
    /// is itself invalid.
    fn open<S>(file_path: S) -> Result<Self, DatabaseError>
    where
        S: AsRef<path::Path>,
        Self: Sized;

    /// Saves the database to its original location.
    ///
    /// # Errors
    /// This function will return an error if the database can not be saved correctly.
    fn save(&self) -> Result<(), DatabaseError>;

    /// Returns a new ID which is larger than all previous IDs.
    fn get_new_id(&self) -> Result<u32, DatabaseError>;

    /// Inserts the given book into the database. If the book does not have an ID, it is given
    /// an ID equal to the largest ID in the database so far, plus one.
    ///
    /// # Arguments
    /// * ` book ` - A book to be stored.
    ///
    /// # Errors
    /// This function will return an error if the database fails.
    fn insert_book(&mut self, book: Book) -> Result<u32, DatabaseError>;

    /// Reads the book at the given location into the database, and returns the book's ID.
    ///
    /// # Arguments
    /// * ` file_path ` - The path to the book to be read.
    ///
    /// # Errors
    /// This function will return an error if the database fails,
    /// the file does not exist, or can not be read.
    fn read_book_from_file<S>(&mut self, file_path: S) -> Result<u32, DatabaseError>
    where
        S: AsRef<path::Path>;

    /// Reads each book in the directory into the database, and returns a
    /// Vec of corresponding IDs as well as a Vec of paths and errors which occured while trying to
    /// read them.
    ///
    /// # Arguments
    /// * ` dir ` - A path to directories containing books to load.
    ///
    /// # Errors
    /// This function will return an error if the database fails,
    /// or the directory does not exist.
    fn read_books_from_dir<S>(
        &mut self,
        dir: S,
    ) -> Result<(Vec<u32>, Vec<DatabaseError>), DatabaseError>
    where
        S: AsRef<path::Path>;

    /// Removes the book with the given ID. If no book with the given ID exists, no change occurs.
    ///
    /// # Arguments
    /// * ` id ` - The ID of the book to be removed.
    ///
    /// # Errors
    /// This function will return an error if the database fails.
    fn remove_book(&mut self, id: u32) -> Result<(), DatabaseError>;

    /// Removes all books with the given IDs. If a book with a given ID does not exists, no change
    /// for that particular ID.
    ///
    /// # Arguments
    /// * ` ids ` - The IDs of the book to be removed.
    ///
    /// # Errors
    /// This function will return an error if the database fails.
    fn remove_books(&mut self, ids: Vec<u32>) -> Result<(), DatabaseError>;

    fn clear(&mut self) -> Result<(), DatabaseError>;
    /// Finds and returns the book with the given ID. If no book is found, a BookNotFound error is
    /// returned.
    ///
    /// # Arguments
    /// * ` id ` - The ID of the book to be returned.
    ///
    /// # Errors
    /// This function will return an error if the database fails or no book is found
    /// with the given ID.
    fn get_book(&self, id: u32) -> Result<Book, DatabaseError>;

    /// Finds and returns all books with the given IDs. If a book with a given ID does not exist,
    /// None is returned for that particular ID.
    ///
    /// # Arguments
    /// * ` ids ` - The IDs of the book to be removed.
    ///
    /// # Errors
    /// This function will return an error if the database fails.
    fn get_books(&self, ids: Vec<u32>) -> Result<Vec<Option<Book>>, DatabaseError>;

    /// Returns a copy of every book in the database. If a database error occurs while reading,
    /// the error is returned.
    ///
    /// # Errors
    /// This function will return an error if the database fails.
    fn get_all_books(&self) -> Result<Vec<Book>, DatabaseError>;

    /// Returns a list of columns that exist for at least one book in the database.
    fn get_available_columns(&self) -> Result<HashSet<UniCase<String>>, DatabaseError>;

    /// Returns whether the provided column exists in at least one book in the database.
    ///
    /// # Arguments
    /// * ` col ` - The column to check.
    fn has_column(&self, col: &UniCase<String>) -> bool;

    /// Finds the book with the given ID, then sets the given value for the book to `new_value`.
    /// If all steps are successful, returns a copy of the book to use, otherwise returning
    /// the appropriate error.
    ///
    /// # Arguments
    /// * ` id ` - The ID of the book to be edited.
    /// * ` column ` - The field in the book to modify.
    /// * ` new_value ` - The value to set the field to.
    ///
    /// # Errors
    /// This function will return an error if the database fails.
    fn edit_book_with_id<S: AsRef<str>, T: AsRef<str>>(
        &mut self,
        id: u32,
        column: S,
        new_value: T,
    ) -> Result<Book, DatabaseError>;

    /// Merges all books with matching titles and authors, skipping everything else, with no
    /// particular order. Books that are merged will not free IDs no longer in use.
    ///
    /// # Errors
    /// This function will return an error if the database fails.
    fn merge_similar(&mut self) -> Result<(), DatabaseError>;

    fn sort_books_by_col(&self, col: &str, reverse: bool) -> Result<(), DatabaseError>;

    fn cursor(&self) -> &PageCursor;

    fn cursor_mut(&mut self) -> &mut PageCursor;

    fn get_books_cursored(&self) -> Result<Vec<Book>, DatabaseError>;

    fn selected(&self) -> Option<usize>;

    fn selected_item(&self) -> Result<Book, DatabaseError>;
}

impl AppDatabase for BasicDatabase {
    fn open<S>(file_path: S) -> Result<Self, DatabaseError>
    where
        S: AsRef<path::Path>,
    {
        let backend = FileDatabase::<BookMap, Ron>::load_from_path_or_default(file_path)?;
        let (cols, cursor) = backend.read(|db| {
            let mut c = HashSet::new();
            c.insert(UniCase::new("title".to_string()));
            c.insert(UniCase::new("authors".to_string()));
            c.insert(UniCase::new("series".to_string()));
            c.insert(UniCase::new("id".to_string()));
            for book in db.books.values() {
                if let Some(e) = book.get_extended_columns() {
                    for key in e.keys() {
                        // TODO: Profile this for large database.
                        c.insert(UniCase::new(key.clone()));
                    }
                }
            }
            (c, PageCursor::new(0, db.books.len()))
        })?;

        Ok(BasicDatabase {
            backend,
            cols,
            cursor,
        })
    }

    fn save(&self) -> Result<(), DatabaseError> {
        Ok(self.backend.save()?)
    }

    fn get_new_id(&self) -> Result<u32, DatabaseError> {
        Ok(self.backend.write(|db| db.new_id())?)
    }

    fn insert_book(&mut self, book: Book) -> Result<u32, DatabaseError> {
        let (id, len) = self.backend.write(|db| {
            let id = book.get_id();
            db.books.insert(id, book);
            (id, db.books.len())
        })?;

        self.cursor.refresh_height(len);
        Ok(id)
    }

    fn read_book_from_file<S>(&mut self, file_path: S) -> Result<u32, DatabaseError>
    where
        S: AsRef<path::Path>,
    {
        Ok(self.insert_book(Book::generate_from_file(file_path, self.get_new_id()?)?)?)
    }

    fn read_books_from_dir<S>(
        &mut self,
        dir: S,
    ) -> Result<(Vec<u32>, Vec<DatabaseError>), DatabaseError>
    where
        S: AsRef<path::Path>,
    {
        let mut ids = vec![];
        let mut errs = vec![];
        // TODO: Make this parallel again.
        fs::read_dir(dir)?
            .map(|res| res.map(|e| e.path()))
            .collect::<Result<Vec<_>, std::io::Error>>()?
            .iter()
            .for_each(|path| match self.read_book_from_file(path) {
                Ok(id) => ids.push(id),
                Err(e) => errs.push(e),
            });
        Ok((ids, errs))
    }

    fn remove_book(&mut self, id: u32) -> Result<(), DatabaseError> {
        let len = self.backend.write(|db| {
            db.books.remove(&id);
            db.books.len()
        })?;
        self.cursor.refresh_height(len);
        if let Some(s) = self.cursor.selected() {
            if s == self.cursor.window_size() && s != 0 {
                self.cursor.select(Some(s - 1));
            }
        };
        Ok(())
    }

    fn remove_books(&mut self, ids: Vec<u32>) -> Result<(), DatabaseError> {
        let len = self.backend.write(|db| {
            for id in ids {
                db.books.remove(&id);
            }
            db.books.len()
        })?;
        self.cursor.refresh_height(len);
        Ok(())
    }

    fn clear(&mut self) -> Result<(), DatabaseError> {
        let len = self.backend.write(|db| {
            db.books.clear();
            db.books.len()
        })?;

        self.cursor.refresh_height(len);
        Ok(())
    }

    fn get_book(&self, id: u32) -> Result<Book, DatabaseError> {
        self.backend.read(|db| match db.books.get(&id) {
            None => Err(DatabaseError::BookNotFound(id)),
            Some(book) => Ok(book.clone()),
        })?
    }

    fn get_books(&self, ids: Vec<u32>) -> Result<Vec<Option<Book>>, DatabaseError> {
        Ok(self
            .backend
            .read(|db| ids.iter().map(|id| db.books.get(id).cloned()).collect())?)
    }

    // TODO: Make this return a Vec of references?
    fn get_all_books(&self) -> Result<Vec<Book>, DatabaseError> {
        Ok(self
            .backend
            .read(|db| db.books.values().cloned().collect())?)
    }

    fn get_available_columns(&self) -> Result<HashSet<UniCase<String>>, DatabaseError> {
        Ok(self.backend.read(|db| {
            let mut c = HashSet::new();
            c.insert(UniCase::new("title".to_string()));
            c.insert(UniCase::new("authors".to_string()));
            c.insert(UniCase::new("series".to_string()));
            c.insert(UniCase::new("id".to_string()));
            for book in db.books.values() {
                if let Some(e) = book.get_extended_columns() {
                    for key in e.keys() {
                        // TODO: Profile this for large database.
                        c.insert(UniCase::new(key.clone()));
                    }
                }
            }
            c
        })?)
    }

    fn has_column(&self, col: &UniCase<String>) -> bool {
        self.cols.contains(col)
    }

    fn edit_book_with_id<S: AsRef<str>, T: AsRef<str>>(
        &mut self,
        id: u32,
        column: S,
        new_value: T,
    ) -> Result<Book, DatabaseError> {
        let book = self.backend.write(|db| match db.books.get_mut(&id) {
            None => Err(DatabaseError::BookNotFound(id)),
            Some(book) => {
                book.set_column(&column.as_ref().into(), new_value)?;
                Ok(book.clone())
            }
        })??;
        self.cols.insert(UniCase::new(column.as_ref().to_string()));
        Ok(book)
    }

    fn merge_similar(&mut self) -> Result<(), DatabaseError> {
        let len = self.backend.write(|db| {
            let mut ref_map: HashMap<(String, String), u32> = HashMap::new();
            let mut merges = vec![];
            for book in db.books.values() {
                if let Some(title) = book.get_title() {
                    if let Some(authors) = book.get_authors() {
                        let a: String = authors.join(", ");
                        let val = (title.to_string(), a);
                        if let Some(id) = ref_map.get(&val) {
                            merges.push((*id, book.get_id()));
                        } else {
                            ref_map.insert(val, book.get_id());
                        }
                    }
                }
            }

            for (b1, b2_id) in merges.iter() {
                let b2 = db.books.remove(b2_id);
                if let Some(b1) = db.books.get_mut(b1) {
                    if let Some(b2) = b2 {
                        b1.merge_mut(b2);
                    }
                }
            }

            db.books.len()
        })?;

        self.cursor.refresh_height(len);

        Ok(())
    }

    fn sort_books_by_col(&self, col: &str, reverse: bool) -> Result<(), DatabaseError> {
        Ok(self.backend.write(|db| {
            let col = ColumnIdentifier::from(col);

            if db.books.len() < 2500 {
                if reverse {
                    db.books.sort_by(|_, a, _, b| b.cmp_column(a, &col))
                } else {
                    db.books.sort_by(|_, a, _, b| a.cmp_column(b, &col))
                }
            } else if reverse {
                db.books.par_sort_by(|_, a, _, b| b.cmp_column(a, &col))
            } else {
                db.books.par_sort_by(|_, a, _, b| a.cmp_column(b, &col))
            }
        })?)
    }

    fn cursor(&self) -> &PageCursor {
        &self.cursor
    }

    fn cursor_mut(&mut self) -> &mut PageCursor {
        &mut self.cursor
    }

    fn selected(&self) -> Option<usize> {
        self.cursor.selected()
    }

    fn selected_item(&self) -> Result<Book, DatabaseError> {
        self.backend.read(|db| match self.cursor.selected_index() {
            None => Err(DatabaseError::NoBookSelected),
            Some(i) => {
                if let Some(b) = db.books.get_index(i) {
                    Ok(b.1.clone())
                } else {
                    Err(DatabaseError::NoBookSelected)
                }
            }
        })?
    }

    fn get_books_cursored(&self) -> Result<Vec<Book>, DatabaseError> {
        Ok(self.backend.read(|db| {
            self.cursor
                .window_range()
                .map(|i| db.books.get_index(i))
                .filter_map(|b| b)
                .map(|b| b.1.clone())
                .collect()
        })?)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use tempdir;

    fn temp_db() -> BasicDatabase {
        let temp_dir = tempdir::TempDir::new("not_a_real").unwrap();
        let path = temp_dir.path().join("database.db");
        BasicDatabase::open(path).unwrap()
    }

    #[test]
    fn test_open() {
        let db = temp_db();
        let base_cols = ["title", "authors", "id", "series"];
        assert!(db.cols.eq(&base_cols
            .iter()
            .map(|c| UniCase::new(c.to_string()))
            .collect()));
    }

    #[test]
    fn test_adding_book() {
        let mut db = temp_db();

        let id = db.get_new_id();
        assert!(id.is_ok());
        let id = id.unwrap();
        assert_eq!(id, 0);

        let book = Book::with_id(id);
        let res = db.insert_book(book.clone());
        assert!(res.is_ok());
        assert_eq!(res.unwrap(), id);
        let fetched = db.get_book(id);
        assert!(fetched.is_ok());
        assert_eq!(fetched.unwrap(), book);
    }

    #[test]
    fn test_adding_2_books() {
        let mut db = temp_db();

        let id0 = db.get_new_id();
        assert!(id0.is_ok());
        let id0 = id0.unwrap();
        assert_eq!(id0, 0);
        let book0 = Book::with_id(id0);

        let id1 = db.get_new_id();
        assert!(id1.is_ok());
        let id1 = id1.unwrap();
        assert_eq!(id1, 1);
        let book1 = Book::with_id(id1);

        assert_ne!(book0, book1);

        let res = db.insert_book(book1.clone());
        assert!(res.is_ok());
        assert_eq!(res.unwrap(), id1);

        let res = db.insert_book(book0.clone());
        assert!(res.is_ok());
        assert_eq!(res.unwrap(), id0);

        let fetched1 = db.get_book(id1);
        assert!(fetched1.is_ok());
        let fetched1 = fetched1.unwrap();
        assert_eq!(fetched1, book1);

        let fetched0 = db.get_book(id0);
        assert!(fetched0.is_ok());
        let fetched0 = fetched0.unwrap();
        assert_eq!(fetched0, book0);

        assert_ne!(fetched0, fetched1);
        assert_ne!(fetched0, book1);
        assert_ne!(fetched1, book0);
    }

    #[test]
    fn test_book_does_not_exist() {
        let db = temp_db();
        for i in 0..1000 {
            let get_book = db.get_book(i);
            assert!(get_book.is_err());
            match get_book.unwrap_err() {
                DatabaseError::BookNotFound(id) => {
                    assert_eq!(i, id);
                }
                _ => panic!("Expected BookNotFoundError"),
            }
        }
    }
}
