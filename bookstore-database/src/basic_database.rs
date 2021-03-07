use std::collections::HashSet;
use std::ops::{Bound, RangeBounds};
use std::path;
use std::sync::{Arc, RwLock};

use rustbreak::{deser::Ron, FileDatabase, RustbreakError};
use unicase::UniCase;

use bookstore_records::book::BookID;
use bookstore_records::{book::RawBook, Book, BookError};

use crate::bookmap::BookMap;
use crate::search::{Error as SearchError, Search};

#[derive(Debug)]
pub enum DatabaseError<DBError> {
    Io(std::io::Error),
    Search(SearchError),
    Book(BookError),
    BookNotFound(BookID),
    IndexOutOfBounds(usize),
    Backend(DBError),
}

impl<DBError> From<std::io::Error> for DatabaseError<DBError> {
    fn from(e: std::io::Error) -> Self {
        DatabaseError::Io(e)
    }
}

impl<DBError> From<BookError> for DatabaseError<DBError> {
    fn from(e: BookError) -> Self {
        DatabaseError::Book(e)
    }
}

impl<DBError> From<SearchError> for DatabaseError<DBError> {
    fn from(e: SearchError) -> Self {
        DatabaseError::Search(e)
    }
}

pub trait AppDatabase {
    type Error;
    /// Opens the database at the path if it exists.
    ///
    /// # Arguments
    ///
    /// * ` file_path ` - A path to a database.
    ///
    /// # Errors
    /// This function will return an error if the file can not be found, or the database
    /// is itself invalid.
    fn open<P>(file_path: P) -> Result<Self, DatabaseError<Self::Error>>
    where
        P: AsRef<path::Path>,
        Self: Sized;

    /// Saves the database to its original location.
    ///
    /// # Errors
    /// This function will return an error if the database can not be saved correctly.
    fn save(&mut self) -> Result<(), DatabaseError<Self::Error>>;

    /// Inserts the given book into the database, setting the ID automatically.
    ///
    /// # Arguments
    /// * ` book ` - A book to be stored.
    ///
    /// # Errors
    /// This function will return an error if the database fails.
    fn insert_book(&mut self, book: RawBook) -> Result<BookID, DatabaseError<Self::Error>>;

    /// Stores each book into the database, and returns a Vec of corresponding IDs.
    ///
    /// # Arguments
    /// * ` books ` - One or more books to be stored.
    ///
    /// # Errors
    /// This function will return an error if the database fails
    fn insert_books(
        &mut self,
        books: Vec<RawBook>,
    ) -> Result<Vec<BookID>, DatabaseError<Self::Error>>;

    /// Removes the book with the given ID. If no book with the given ID exists, no change occurs.
    ///
    /// # Arguments
    /// * ` id ` - The ID of the book to be removed.
    ///
    /// # Errors
    /// This function will return an error if the database fails.
    fn remove_book(&mut self, id: BookID) -> Result<(), DatabaseError<Self::Error>>;

    /// Removes all books with the given IDs. If a book with a given ID does not exist, no change
    /// for that particular ID.
    ///
    /// # Arguments
    /// * ` ids ` - An iterator yielding the IDs of the book to be removed.
    ///
    /// # Errors
    /// This function will return an error if the database fails.
    fn remove_books(&mut self, ids: &HashSet<BookID>) -> Result<(), DatabaseError<Self::Error>>;

    fn clear(&mut self) -> Result<(), DatabaseError<Self::Error>>;

    /// Finds and returns the book with the given ID. If no book is found,
    /// a `BookNotFound` error is returned.
    ///
    /// # Arguments
    /// * ` id ` - The ID of the book to be returned.
    ///
    /// # Errors
    /// This function will return an error if the database fails or no book is found
    /// with the given ID.
    fn get_book(&self, id: BookID) -> Result<Arc<RwLock<Book>>, DatabaseError<Self::Error>>;

    /// Finds and returns all books with the given IDs. If a book with a given ID does not exist,
    /// None is returned for that particular ID.
    ///
    /// # Arguments
    /// * ` ids ` - The IDs of the book to be removed.
    ///
    /// # Errors
    /// This function will return an error if the database fails.
    fn get_books<I: IntoIterator<Item = BookID>>(
        &self,
        ids: I,
    ) -> Result<Vec<Option<Arc<RwLock<Book>>>>, DatabaseError<Self::Error>>;

    /// Returns a copy of every book in the database. If a database error occurs while reading,
    /// the error is returned.
    ///
    /// # Errors
    /// This function will return an error if the database fails.
    fn get_all_books(&self) -> Result<Vec<Arc<RwLock<Book>>>, DatabaseError<Self::Error>>;

    /// Returns whether the provided column exists in at least one book in the database.
    ///
    /// # Arguments
    /// * ` col ` - The column to check.
    fn has_column(&self, col: &UniCase<String>) -> Result<bool, DatabaseError<Self::Error>>;

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
    fn edit_book_with_id<S0: AsRef<str>, S1: AsRef<str>>(
        &mut self,
        id: BookID,
        edits: &[(S0, S1)],
    ) -> Result<(), DatabaseError<Self::Error>>;

    /// Merges all books with matching titles and authors, skipping everything else, with no
    /// particular order. Books that are merged will not free IDs no longer in use.
    ///
    /// # Errors
    /// This function will return an error if the database fails.
    fn merge_similar(&mut self) -> Result<HashSet<BookID>, DatabaseError<Self::Error>>;

    /// Finds books, using the match to compare the specified column to the search string.
    ///
    /// # Arguments
    /// * ` matching ` - The matching method
    /// * ` column ` - The column to search over.
    /// * ` search ` - The search query.
    ///
    /// # Errors
    /// This function will return an error if the database fails.
    fn find_matches(
        &self,
        search: &[Search],
    ) -> Result<Vec<Arc<RwLock<Book>>>, DatabaseError<Self::Error>>;

    /// Sorts books by comparing the specified columns and reverses.
    ///
    /// # Arguments
    ///
    /// # Errors
    /// This function will return an error if the database fails.
    fn sort_books_by_cols<S: AsRef<str>>(
        &mut self,
        columns: &[(S, bool)],
    ) -> Result<(), DatabaseError<Self::Error>>;

    /// Returns the number of books stored internally.
    fn size(&self) -> usize;

    /// Returns true if the internal database is persisted to file.
    /// Note that at the moment, any write action will unset the saved state.
    fn saved(&self) -> bool;
}

pub trait IndexableDatabase: AppDatabase + Sized {
    /// Gets the books in self as specified by absolute indices, respecting the current
    /// ordering.
    ///
    /// # Arguments
    /// * ` indices ` - the indices of the books to fetch
    ///
    /// # Errors
    /// This function will return an error if the database fails.
    fn get_books_indexed(
        &self,
        indices: impl RangeBounds<usize>,
    ) -> Result<Vec<Arc<RwLock<Book>>>, DatabaseError<Self::Error>>;

    /// Get the book at the current index, respecting the current ordering.
    ///
    /// # Arguments
    /// * ` index ` - the index of the book to fetch
    ///
    /// # Errors
    /// This function will return an error if the database fails or the given index does not
    /// exist.
    fn get_book_indexed(
        &self,
        index: usize,
    ) -> Result<Arc<RwLock<Book>>, DatabaseError<Self::Error>>;

    /// Remove the book at the current index, respecting the current ordering.
    ///
    /// # Arguments
    /// * ` index ` - the index of the book to remove
    ///
    /// # Errors
    /// This function will return an error if the database fails.
    fn remove_book_indexed(&mut self, index: usize) -> Result<(), DatabaseError<Self::Error>>;

    /// Edit the book at the current index, respecting the current ordering.
    ///
    /// # Arguments
    /// * ` index ` - the index of the book to remove
    /// * ` column ` - the column to modify
    /// * ` new_value ` - the value to set the column to.
    ///
    /// # Errors
    /// This function will return an error if the database fails.
    fn edit_book_indexed<S0: AsRef<str>, S1: AsRef<str>>(
        &mut self,
        index: usize,
        edits: &[(S0, S1)],
    ) -> Result<(), DatabaseError<Self::Error>>;
}

// TODO: Saved currently returns false negatives - eg. sorting when already sorted is considered
//  unsaving, so is editing book with exact same value, etc.

/// A database which fully implements the functionality of the `AppDatabase` trait,
/// but does not guarantee that data is successfully written to disk.
pub struct BasicDatabase {
    backend: FileDatabase<BookMap, Ron>,
    /// All available columns. Case-insensitive.
    len: usize,
    saved: bool,
}

impl AppDatabase for BasicDatabase {
    type Error = RustbreakError;

    fn open<P>(file_path: P) -> Result<Self, DatabaseError<Self::Error>>
    where
        P: AsRef<path::Path>,
    {
        let backend = FileDatabase::<BookMap, Ron>::load_from_path_or_default(file_path)
            .map_err(DatabaseError::Backend)?;
        let len = backend
            .write(|db| {
                db.init_columns();
                db.len()
            })
            .map_err(DatabaseError::Backend)?;

        Ok(BasicDatabase {
            backend,
            len,
            saved: true,
        })
    }

    fn save(&mut self) -> Result<(), DatabaseError<Self::Error>> {
        self.backend.save().map_err(DatabaseError::Backend)?;
        self.saved = true;
        Ok(())
    }

    fn insert_book(&mut self, book: RawBook) -> Result<BookID, DatabaseError<Self::Error>> {
        let (id, len) = self
            .backend
            .write(|db| {
                let id = db.insert_raw_book(book);
                (id, db.len())
            })
            .map_err(DatabaseError::Backend)?;

        self.len = len;
        self.saved = false;

        Ok(id)
    }

    fn insert_books(
        &mut self,
        books: Vec<RawBook>,
    ) -> Result<Vec<BookID>, DatabaseError<Self::Error>> {
        let mut ids = vec![];

        self.len = self
            .backend
            .write(|db| {
                books.into_iter().for_each(|book| {
                    ids.push(db.insert_raw_book(book));
                });
                db.len()
            })
            .map_err(DatabaseError::Backend)?;

        self.saved = false;

        Ok(ids)
    }

    fn remove_book(&mut self, id: BookID) -> Result<(), DatabaseError<Self::Error>> {
        self.len = self
            .backend
            .write(|db| {
                db.remove_book(id);
                db.len()
            })
            .map_err(DatabaseError::Backend)?;

        self.saved = false;

        Ok(())
    }

    fn remove_books(&mut self, ids: &HashSet<BookID>) -> Result<(), DatabaseError<Self::Error>> {
        self.len = self
            .backend
            .write(|db| {
                db.remove_books(ids);
                db.len()
            })
            .map_err(DatabaseError::Backend)?;

        self.saved = false;

        Ok(())
    }

    fn clear(&mut self) -> Result<(), DatabaseError<Self::Error>> {
        self.len = self
            .backend
            .write(|db| {
                db.clear();
                db.len()
            })
            .map_err(DatabaseError::Backend)?;

        self.saved = false;

        Ok(())
    }

    fn get_book(&self, id: BookID) -> Result<Arc<RwLock<Book>>, DatabaseError<Self::Error>> {
        self.backend
            .read(|db| db.get_book(id).ok_or(DatabaseError::BookNotFound(id)))
            .map_err(DatabaseError::Backend)?
    }

    fn get_books<I: IntoIterator<Item = BookID>>(
        &self,
        ids: I,
    ) -> Result<Vec<Option<Arc<RwLock<Book>>>>, DatabaseError<Self::Error>> {
        Ok(self
            .backend
            .read(|db| ids.into_iter().map(|id| db.get_book(id)).collect())
            .map_err(DatabaseError::Backend)?)
    }

    fn get_all_books(&self) -> Result<Vec<Arc<RwLock<Book>>>, DatabaseError<Self::Error>> {
        Ok(self
            .backend
            .read(|db| db.get_all_books())
            .map_err(DatabaseError::Backend)?)
    }

    fn has_column(&self, col: &UniCase<String>) -> Result<bool, DatabaseError<Self::Error>> {
        self.backend
            .read(|db| db.has_column(col))
            .map_err(DatabaseError::Backend)
    }

    fn edit_book_with_id<S0: AsRef<str>, S1: AsRef<str>>(
        &mut self,
        id: BookID,
        edits: &[(S0, S1)],
    ) -> Result<(), DatabaseError<Self::Error>> {
        self.backend
            .write(|db| db.edit_book_with_id(id, edits))
            .map_err(DatabaseError::Backend)??;
        self.saved = false;
        Ok(())
    }

    fn merge_similar(&mut self) -> Result<HashSet<BookID>, DatabaseError<Self::Error>> {
        let (ids, len) = self
            .backend
            .write(|db| {
                let merges = db.merge_similar();
                (merges, db.len())
            })
            .map_err(DatabaseError::Backend)?;
        self.saved = false;
        self.len = len;
        Ok(ids)
    }

    fn find_matches(
        &self,
        searches: &[Search],
    ) -> Result<Vec<Arc<RwLock<Book>>>, DatabaseError<Self::Error>> {
        Ok(self
            .backend
            .read(|db| db.find_matches(searches))
            .map_err(DatabaseError::Backend)??)
    }

    fn sort_books_by_cols<S: AsRef<str>>(
        &mut self,
        columns: &[(S, bool)],
    ) -> Result<(), DatabaseError<Self::Error>> {
        self.backend
            .write(|db| db.sort_books_by_cols(columns))
            .map_err(DatabaseError::Backend)?;

        Ok(())
    }

    fn size(&self) -> usize {
        self.len
    }

    fn saved(&self) -> bool {
        self.saved
    }
}

impl IndexableDatabase for BasicDatabase {
    fn get_books_indexed(
        &self,
        indices: impl RangeBounds<usize>,
    ) -> Result<Vec<Arc<RwLock<Book>>>, DatabaseError<Self::Error>> {
        let start = match indices.start_bound() {
            Bound::Included(i) => *i,
            Bound::Excluded(i) => *i + 1,
            Bound::Unbounded => 0,
        }
        .min(self.len.saturating_sub(1));

        let end = match indices.end_bound() {
            Bound::Included(i) => *i + 1,
            Bound::Excluded(i) => *i,
            Bound::Unbounded => usize::MAX,
        }
        .min(self.len);

        Ok(self
            .backend
            .read(|db| {
                (start..end)
                    .filter_map(|i| db.get_book_indexed(i))
                    .collect()
            })
            .map_err(DatabaseError::Backend)?)
    }

    fn get_book_indexed(
        &self,
        index: usize,
    ) -> Result<Arc<RwLock<Book>>, DatabaseError<Self::Error>> {
        self.backend
            .read(|db| {
                db.get_book_indexed(index)
                    .ok_or(DatabaseError::IndexOutOfBounds(index))
            })
            .map_err(DatabaseError::Backend)?
    }

    fn remove_book_indexed(&mut self, index: usize) -> Result<(), DatabaseError<Self::Error>> {
        self.len = self
            .backend
            .write(|db| {
                if db.remove_book_indexed(index) {
                    Ok(db.len())
                } else {
                    Err(DatabaseError::IndexOutOfBounds(index))
                }
            })
            .map_err(DatabaseError::Backend)??;

        self.saved = false;

        Ok(())
    }

    fn edit_book_indexed<S0: AsRef<str>, S1: AsRef<str>>(
        &mut self,
        index: usize,
        edits: &[(S0, S1)],
    ) -> Result<(), DatabaseError<Self::Error>> {
        self.backend
            .write(|db| {
                if db.edit_book_indexed(index, edits)? {
                    Ok(())
                } else {
                    Err(DatabaseError::IndexOutOfBounds(index))
                }
            })
            .map_err(DatabaseError::Backend)??;

        self.saved = false;
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;

    use std::convert::TryFrom;
    use std::ops::Deref;

    use tempfile;

    use bookstore_records::book::ColumnIdentifier;

    macro_rules! book {
        ($book: ident) => {
            $book.as_ref().read().unwrap()
        };
    }

    fn temp_db() -> BasicDatabase {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().join("database.db");
        BasicDatabase::open(path).unwrap()
    }

    #[test]
    fn test_open() {
        let db = temp_db();
        let base_cols = vec!["title", "authors", "id", "series", "description"];
        for col in base_cols.into_iter() {
            assert!(db.has_column(&UniCase::new(String::from(col))).unwrap());
        }
    }

    #[test]
    fn test_adding_book() {
        let mut db = temp_db();

        let book = RawBook::default();
        let id = BookID::try_from(1).unwrap();
        let res = db.insert_book(book.clone());
        assert!(res.is_ok());
        assert_eq!(res.unwrap(), id);
        let fetched = db.get_book(id).unwrap();
        db.remove_book(id).unwrap();
        assert_eq!(book!(fetched).deref().inner(), &book);
    }

    #[test]
    fn test_adding_2_books() {
        let mut db = temp_db();

        let a = ColumnIdentifier::Series;

        let id1 = BookID::try_from(1).unwrap();
        let id2 = BookID::try_from(2).unwrap();

        let mut book0 = Book::from_raw_book(id1, RawBook::default());
        book0.set_column(&a, "hello world [1]").unwrap();
        let mut book1 = Book::from_raw_book(id2, RawBook::default());
        book1.set_column(&a, "hello world [2]").unwrap();

        assert_ne!(book0, book1);

        let res = db.insert_book(book0.inner().to_owned());
        assert!(res.is_ok());
        assert_eq!(res.unwrap(), id1);

        let res = db.insert_book(book1.inner().to_owned());
        assert!(res.is_ok());
        assert_eq!(res.unwrap(), id2);

        let fetched1 = db.get_book(id2);
        assert!(fetched1.is_ok());
        let fetched1 = fetched1.unwrap();
        assert_eq!(book!(fetched1).get_series(), book1.get_series());

        let fetched0 = db.get_book(id1);
        assert!(fetched0.is_ok());
        let fetched0 = fetched0.unwrap();
        assert_eq!(book!(fetched0).get_series(), book0.get_series());

        assert_ne!(book!(fetched0).get_series(), book!(fetched1).get_series());
        assert_ne!(book!(fetched0).get_series(), book1.get_series());
        assert_ne!(book!(fetched1).get_series(), book0.get_series());
    }

    #[test]
    fn test_book_does_not_exist() {
        let db = temp_db();
        for i in 1..1000 {
            let i = BookID::try_from(i).unwrap();
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
