use std::collections::{HashMap, HashSet};
use std::ops::{Bound, RangeBounds};
use std::path;

use indexmap::IndexMap;
use regex::{Error as RegexError, Regex};
use rustbreak::{deser::Ron, FileDatabase, RustbreakError};
use serde::{Deserialize, Serialize};
#[cfg(feature = "sqlite")]
use sqlx::Error as SQLError;
use sublime_fuzzy::best_match;
use unicase::UniCase;

use crate::search::Search;
use bookstore_records::{
    book::{ColumnIdentifier, RawBook},
    Book, BookError,
};

#[derive(Debug)]
pub enum DatabaseError {
    Io(std::io::Error),
    Regex(RegexError),
    Book(BookError),
    BookNotFound(u32),
    IndexOutOfBounds(usize),
    #[cfg(not(feature = "sqlite"))]
    Backend(RustbreakError),
    #[cfg(feature = "sqlite")]
    Backend(SQLError),
}

impl From<std::io::Error> for DatabaseError {
    fn from(e: std::io::Error) -> Self {
        DatabaseError::Io(e)
    }
}

impl From<BookError> for DatabaseError {
    fn from(e: BookError) -> Self {
        DatabaseError::Book(e)
    }
}

impl From<RegexError> for DatabaseError {
    fn from(e: RegexError) -> Self {
        DatabaseError::Regex(e)
    }
}

#[cfg(not(feature = "sqlite"))]
impl From<RustbreakError> for DatabaseError {
    fn from(e: RustbreakError) -> Self {
        DatabaseError::Backend(e)
    }
}

#[cfg(feature = "sqlite")]
impl From<SQLError> for DatabaseError {
    fn from(e: SQLError) -> Self {
        DatabaseError::Backend(e)
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

    /// Return a monotonically increasing ID to use for a new
    /// book.
    ///
    /// # Errors
    /// Will panic if the ID can no longer be correctly increased.
    fn borrow_id(&mut self) -> u32 {
        let id = self.max_id;
        if self.max_id == u32::MAX {
            panic!(format!(
                "Current ID is at maximum value of {} and can not be increased.",
                u32::MAX
            ));
        }
        id + 1
    }
}

/// A database which fully implements the functionality of the `AppDatabase` trait,
/// but does not guarantee that data is successfully written to disk.
pub struct BasicDatabase {
    backend: FileDatabase<BookMap, Ron>,
    /// All available columns. Case-insensitive.
    cols: HashSet<UniCase<String>>,
    len: usize,
    saved: bool,
}

pub trait AppDatabase {
    /// Opens the database at the path if it exists.
    ///
    /// # Arguments
    ///
    /// * ` file_path ` - A path to a database.
    ///
    /// # Errors
    /// This function will return an error if the file can not be found, or the database
    /// is itself invalid.
    fn open<P>(file_path: P) -> Result<Self, DatabaseError>
    where
        P: AsRef<path::Path>,
        Self: Sized;

    /// Saves the database to its original location.
    ///
    /// # Errors
    /// This function will return an error if the database can not be saved correctly.
    fn save(&mut self) -> Result<(), DatabaseError>;

    /// Inserts the given book into the database, setting the ID automatically.
    ///
    /// # Arguments
    /// * ` book ` - A book to be stored.
    ///
    /// # Errors
    /// This function will return an error if the database fails.
    fn insert_book(&mut self, book: RawBook) -> Result<u32, DatabaseError>;

    /// Stores each book into the database, and returns a Vec of corresponding IDs.
    ///
    /// # Arguments
    /// * ` books ` - One or more books to be stored.
    ///
    /// # Errors
    /// This function will return an error if the database fails
    fn insert_books(&mut self, books: Vec<RawBook>) -> Result<Vec<u32>, DatabaseError>;

    /// Removes the book with the given ID. If no book with the given ID exists, no change occurs.
    ///
    /// # Arguments
    /// * ` id ` - The ID of the book to be removed.
    ///
    /// # Errors
    /// This function will return an error if the database fails.
    fn remove_book(&mut self, id: u32) -> Result<(), DatabaseError>;

    /// Removes all books with the given IDs. If a book with a given ID does not exist, no change
    /// for that particular ID.
    ///
    /// # Arguments
    /// * ` ids ` - The IDs of the book to be removed.
    ///
    /// # Errors
    /// This function will return an error if the database fails.
    fn remove_books(&mut self, ids: Vec<u32>) -> Result<(), DatabaseError>;

    fn clear(&mut self) -> Result<(), DatabaseError>;

    /// Finds and returns the book with the given ID. If no book is found,
    /// a `BookNotFound` error is returned.
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
    fn get_available_columns(&self) -> &HashSet<UniCase<String>>;

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
    fn edit_book_with_id<S0: AsRef<str>, S1: AsRef<str>>(
        &mut self,
        id: u32,
        column: S0,
        new_value: S1,
    ) -> Result<(), DatabaseError>;

    /// Merges all books with matching titles and authors, skipping everything else, with no
    /// particular order. Books that are merged will not free IDs no longer in use.
    ///
    /// # Errors
    /// This function will return an error if the database fails.
    fn merge_similar(&mut self) -> Result<(), DatabaseError>;

    /// Finds books, using the match to compare the specified column to the search string.
    ///
    /// # Arguments
    /// * ` matching ` - The matching method
    /// * ` column ` - The column to search over.
    /// * ` search ` - The search query.
    ///
    /// # Errors
    /// This function will return an error if the database fails.
    fn find_matches(&self, search: Search) -> Result<Vec<Book>, DatabaseError>;

    // TODO: push this into bookview?
    /// Sorts books by comparing the specified column.
    ///
    /// # Arguments
    /// * ` col ` - The column of interest.
    /// * ` reverse ` - whether to sort in reverse order.
    ///
    /// # Errors
    /// This function will return an error if the database fails.
    fn sort_books_by_col(&mut self, col: &str, reverse: bool) -> Result<(), DatabaseError>;

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
    ) -> Result<Vec<Book>, DatabaseError>;

    /// Get the book at the current index, respecting the current ordering.
    ///
    /// # Arguments
    /// * ` index ` - the index of the book to fetch
    ///
    /// # Errors
    /// This function will return an error if the database fails or the given index does not
    /// exist.
    fn get_book_indexed(&self, index: usize) -> Result<Book, DatabaseError>;

    /// Remove the book at the current index, respecting the current ordering.
    ///
    /// # Arguments
    /// * ` index ` - the index of the book to remove
    ///
    /// # Errors
    /// This function will return an error if the database fails.
    fn remove_book_indexed(&mut self, index: usize) -> Result<(), DatabaseError>;

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
        column: S0,
        new_value: S1,
    ) -> Result<(), DatabaseError>;
}

// TODO: Saved currently returns false negatives - eg. sorting when already sorted is considered
//  unsaving, so is editing book with exact same value, etc.

impl AppDatabase for BasicDatabase {
    fn open<P>(file_path: P) -> Result<Self, DatabaseError>
    where
        P: AsRef<path::Path>,
    {
        let backend = FileDatabase::<BookMap, Ron>::load_from_path_or_default(file_path)?;
        let (cols, len) = backend.read(|db| {
            let mut c = HashSet::new();

            for &col in &["title", "authors", "series", "id", "description"] {
                c.insert(col);
            }

            for book in db.books.values() {
                if let Some(e) = book.get_extended_columns() {
                    for key in e.keys() {
                        c.insert(key);
                    }
                }
            }

            (
                c.into_iter().map(|c| UniCase::new(c.to_owned())).collect(),
                db.books.len(),
            )
        })?;

        Ok(BasicDatabase {
            backend,
            cols,
            len,
            saved: true,
        })
    }

    fn save(&mut self) -> Result<(), DatabaseError> {
        self.backend.save()?;
        self.saved = true;
        Ok(())
    }

    fn insert_book(&mut self, book: RawBook) -> Result<u32, DatabaseError> {
        let (id, len) = self.backend.write(|db| {
            let id = db.new_id();
            let book = Book::from_raw_book(book, id);
            db.books.insert(id, book);
            (id, db.books.len())
        })?;

        self.len = len;
        self.saved = false;

        Ok(id)
    }

    fn insert_books(&mut self, books: Vec<RawBook>) -> Result<Vec<u32>, DatabaseError> {
        let mut ids = vec![];

        self.len = self.backend.write(|db| {
            books.into_iter().for_each(|book| {
                let id = db.new_id();
                let book = Book::from_raw_book(book, id);
                db.books.insert(id, book);
                ids.push(id);
            });
            db.books.len()
        })?;

        self.saved = false;

        Ok(ids)
    }

    fn remove_book(&mut self, id: u32) -> Result<(), DatabaseError> {
        self.len = self.backend.write(|db| {
            db.books.shift_remove(&id);
            db.books.len()
        })?;

        self.saved = false;

        Ok(())
    }

    fn remove_books(&mut self, ids: Vec<u32>) -> Result<(), DatabaseError> {
        self.len = self.backend.write(|db| {
            let ids = ids.iter().collect::<HashSet<_>>();
            db.books.retain(|id, _| !ids.contains(id));
            db.books.len()
        })?;

        self.saved = false;

        Ok(())
    }

    fn clear(&mut self) -> Result<(), DatabaseError> {
        self.len = self.backend.write(|db| {
            db.books.clear();
            db.books.len()
        })?;

        self.saved = false;

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

    fn get_available_columns(&self) -> &HashSet<UniCase<String>> {
        &self.cols
    }

    fn has_column(&self, col: &UniCase<String>) -> bool {
        self.cols.contains(col)
    }

    fn edit_book_with_id<S0: AsRef<str>, S1: AsRef<str>>(
        &mut self,
        id: u32,
        column: S0,
        new_value: S1,
    ) -> Result<(), DatabaseError> {
        self.backend.write(|db| match db.books.get_mut(&id) {
            None => Err(DatabaseError::BookNotFound(id)),
            Some(book) => Ok(book.set_column(&column.as_ref().into(), new_value)?),
        })??;
        self.saved = false;
        self.cols.insert(UniCase::new(column.as_ref().to_owned()));
        Ok(())
    }

    fn merge_similar(&mut self) -> Result<(), DatabaseError> {
        self.len = self.backend.write(|db| {
            let mut ref_map: HashMap<(String, String), u32> = HashMap::new();
            let mut merges = vec![];
            let dummy_id = db.borrow_id();
            for book in db.books.values() {
                if let Some(title) = book.get_title() {
                    if let Some(authors) = book.get_authors() {
                        let a: String = authors.join(", ").to_ascii_lowercase();
                        let val = (title.to_ascii_lowercase(), a);
                        if let Some(id) = ref_map.get(&val) {
                            merges.push((*id, book.get_id()));
                        } else {
                            ref_map.insert(val, book.get_id());
                        }
                    }
                }
            }

            for (b1, b2_id) in merges.iter() {
                // Dummy allows for O(n) time book removal while maintaining sort
                // and getting owned copy of book.
                let dummy = Book::with_id(dummy_id);
                let b2 = db.books.insert(*b2_id, dummy);
                if let Some(b1) = db.books.get_mut(b1) {
                    if let Some(b2) = b2 {
                        b1.merge_mut(b2);
                    }
                }
            }
            db.books.retain(|_, book| book.get_id() != dummy_id);
            db.books.len()
        })?;
        self.saved = false;
        Ok(())
    }

    fn find_matches(&self, search: Search) -> Result<Vec<Book>, DatabaseError> {
        Ok(self
            .backend
            .read(|db| -> Result<Vec<Book>, DatabaseError> {
                let mut results = vec![];
                match search {
                    Search::Regex(column, search) => {
                        let col = ColumnIdentifier::from(column);
                        let matcher = Regex::new(search.as_str())?;
                        for (_, book) in db.books.iter() {
                            if matcher.is_match(&book.get_column_or(&col, "")) {
                                results.push(book.clone());
                            }
                        }
                    }
                    Search::CaseSensitive(column, search) => {
                        let col = ColumnIdentifier::from(column);
                        for (_, book) in db.books.iter() {
                            if best_match(&search, &book.get_column_or(&col, "")).is_some() {
                                results.push(book.clone());
                            }
                        }
                    }
                    Search::Default(column, search) => {
                        let col = ColumnIdentifier::from(column);
                        let insensitive = search.to_ascii_lowercase();
                        for (_, book) in db.books.iter() {
                            if best_match(&insensitive, &book.get_column_or(&col, "")).is_some() {
                                results.push(book.clone());
                            }
                        }
                    }
                }
                Ok(results)
            })??)
    }

    fn sort_books_by_col(&mut self, col: &str, reverse: bool) -> Result<(), DatabaseError> {
        self.backend.write(|db| {
            let col = ColumnIdentifier::from(col);

            // Use some heuristic to sort in parallel when it would offer speedup -
            // parallel threads are slower for small sorts.
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
        })?;

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
    ) -> Result<Vec<Book>, DatabaseError> {
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

        Ok(self.backend.read(|db| {
            (start..end)
                .filter_map(|i| db.books.get_index(i))
                .map(|b| b.1.clone())
                .collect()
        })?)
    }

    fn get_book_indexed(&self, index: usize) -> Result<Book, DatabaseError> {
        self.backend.read(|db| {
            if let Some(b) = db.books.get_index(index) {
                Ok(b.1.clone())
            } else {
                Err(DatabaseError::IndexOutOfBounds(index))
            }
        })?
    }

    fn remove_book_indexed(&mut self, index: usize) -> Result<(), DatabaseError> {
        self.len = self.backend.write(|db| {
            let id = if let Some((id, _)) = db.books.get_index(index) {
                Some(*id)
            } else {
                None
            };
            if let Some(id) = id {
                db.books.shift_remove(&id);
            }
            db.books.len()
        })?;

        self.saved = false;

        Ok(())
    }

    fn edit_book_indexed<S0: AsRef<str>, S1: AsRef<str>>(
        &mut self,
        index: usize,
        column: S0,
        new_value: S1,
    ) -> Result<(), DatabaseError> {
        self.backend.write(|db| {
            if let Some((_, book)) = db.books.get_index_mut(index) {
                book.set_column(&column.as_ref().into(), new_value)?;
                Ok(())
            } else {
                Err(DatabaseError::IndexOutOfBounds(index))
            }
        })??;

        self.saved = false;
        self.cols.insert(UniCase::new(column.as_ref().to_owned()));
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use tempfile;

    fn temp_db() -> BasicDatabase {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().join("database.db");
        BasicDatabase::open(path).unwrap()
    }

    #[test]
    fn test_open() {
        let db = temp_db();
        let base_cols = ["title", "authors", "id", "series", "description"];
        assert_eq!(
            db.cols,
            base_cols
                .iter()
                .map(|&c| UniCase::new(c.to_owned()))
                .collect()
        );
    }

    #[test]
    fn test_adding_book() {
        let mut db = temp_db();

        let book = RawBook::default();
        let res = db.insert_book(book.clone());
        assert!(res.is_ok());
        assert_eq!(res.unwrap(), 0);
        let fetched = db.get_book(0);
        assert!(fetched.is_ok());
        assert_eq!(fetched.unwrap().inner().to_owned(), book);
    }

    #[test]
    fn test_adding_2_books() {
        let mut db = temp_db();

        let a = ColumnIdentifier::Series;
        let mut book0 = Book::with_id(0);
        book0.set_column(&a, "hello world [1]").unwrap();
        let mut book1 = Book::with_id(1);
        book1.set_column(&a, "hello world [2]").unwrap();

        assert_ne!(book0, book1);

        let res = db.insert_book(book0.inner().to_owned());
        assert!(res.is_ok());
        assert_eq!(res.unwrap(), 0);

        let res = db.insert_book(book1.inner().to_owned());
        assert!(res.is_ok());
        assert_eq!(res.unwrap(), 1);

        let fetched1 = db.get_book(1);
        assert!(fetched1.is_ok());
        let fetched1 = fetched1.unwrap();
        assert_eq!(fetched1, book1);

        let fetched0 = db.get_book(0);
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
