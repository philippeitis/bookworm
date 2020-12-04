use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::{fs, path};

use indexmap::IndexMap;
use rayon::prelude::*;
use regex::{Error as RegexError, Regex};
use rustbreak::{deser::Ron, FileDatabase, RustbreakError};
use serde::{Deserialize, Serialize};
use unicase::UniCase;

use crate::record::book::ColumnIdentifier;
use crate::record::{Book, BookError};

#[derive(Debug, Eq, PartialEq)]
pub enum Matching {
    Regex,
    CaseSensitive,
    Default,
}

#[derive(Debug)]
pub(crate) enum DatabaseError {
    Io(std::io::Error),
    RegexError(RegexError),
    Book(BookError),
    Backend(RustbreakError),
    BookNotFound(u32),
    IndexOutOfBounds(usize),
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

impl From<RegexError> for DatabaseError {
    fn from(e: RegexError) -> Self {
        DatabaseError::RegexError(e)
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

    fn allocate_id(&mut self, id: u32) {
        if id > self.max_id {
            self.max_id = id;
        }
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

/// A database which fully implements the functionality of the AppDatabase trait,
/// but does not guarantee that data is successfully written to disk.
pub(crate) struct BasicDatabase {
    backend: FileDatabase<BookMap, Ron>,
    /// All available columns. Case-insensitive.
    cols: HashSet<UniCase<String>>,
    len: usize,
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
    /// Vec of corresponding IDs as well as a Vec of paths and errors which
    /// occurred while trying to read them.
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
    fn edit_book_with_id<S: AsRef<str>, T: AsRef<str>>(
        &mut self,
        id: u32,
        column: S,
        new_value: T,
    ) -> Result<(), DatabaseError>;

    /// Merges all books with matching titles and authors, skipping everything else, with no
    /// particular order. Books that are merged will not free IDs no longer in use.
    ///
    /// # Errors
    /// This function will return an error if the database fails.
    fn merge_similar(&mut self) -> Result<(), DatabaseError>;

    fn find_matches<S: AsRef<str>, T: AsRef<str>>(
        &self,
        matching: Matching,
        column: S,
        search: T,
    ) -> Result<Vec<Book>, DatabaseError>;

    fn sort_books_by_col(&self, col: &str, reverse: bool) -> Result<(), DatabaseError>;

    fn size(&self) -> usize;
}

pub(crate) trait IndexableDatabase: AppDatabase + Sized {
    fn get_books_indexed(&self, indices: Range<usize>) -> Result<Vec<Book>, DatabaseError>;

    fn get_book_indexed(&self, index: usize) -> Result<Book, DatabaseError>;

    fn remove_book_indexed(&mut self, index: usize) -> Result<(), DatabaseError>;

    fn edit_book_indexed<S: AsRef<str>, T: AsRef<str>>(
        &mut self,
        index: usize,
        column: S,
        new_value: T,
    ) -> Result<(), DatabaseError>;
}

impl AppDatabase for BasicDatabase {
    fn open<S>(file_path: S) -> Result<Self, DatabaseError>
    where
        S: AsRef<path::Path>,
    {
        let backend = FileDatabase::<BookMap, Ron>::load_from_path_or_default(file_path)?;
        let (cols, len) = backend.read(|db| {
            let mut c = HashSet::new();

            for &col in &["title", "authors", "series", "id"] {
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

        Ok(BasicDatabase { backend, cols, len })
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
            db.allocate_id(id);
            db.books.insert(id, book);
            (id, db.books.len())
        })?;
        self.len = len;
        Ok(id)
    }

    fn read_book_from_file<S>(&mut self, file_path: S) -> Result<u32, DatabaseError>
    where
        S: AsRef<path::Path>,
    {
        self.insert_book(Book::generate_from_file(file_path, self.get_new_id()?)?)
    }

    fn read_books_from_dir<S>(
        &mut self,
        dir: S,
    ) -> Result<(Vec<u32>, Vec<DatabaseError>), DatabaseError>
    where
        S: AsRef<path::Path>,
    {
        let start = self.get_new_id()?;
        let results = fs::read_dir(dir)?
            .map(|res| res.map(|e| e.path()))
            .collect::<Result<Vec<_>, std::io::Error>>()?
            .par_iter()
            .enumerate()
            .map(|(id, path)| Book::generate_from_file(path, start + (id as u32)))
            .collect::<Vec<_>>();

        let mut ids = vec![];
        let mut errs = vec![];

        self.len = self.backend.write(|db| {
            results.into_iter().for_each(|result| match result {
                Ok(book) => {
                    let id = book.get_id();
                    db.allocate_id(id);
                    db.books.insert(id, book);
                    ids.push(id);
                }
                Err(e) => errs.push(e.into()),
            });
            db.books.len()
        })?;

        Ok((ids, errs))
    }

    fn remove_book(&mut self, id: u32) -> Result<(), DatabaseError> {
        self.len = self.backend.write(|db| {
            db.books.shift_remove(&id);
            db.books.len()
        })?;
        Ok(())
    }

    fn remove_books(&mut self, ids: Vec<u32>) -> Result<(), DatabaseError> {
        self.len = self.backend.write(|db| {
            let ids = ids.iter().collect::<HashSet<_>>();
            db.books.retain(|id, _| !ids.contains(id));
            db.books.len()
        })?;
        Ok(())
    }

    fn clear(&mut self) -> Result<(), DatabaseError> {
        self.len = self.backend.write(|db| {
            db.books.clear();
            db.books.len()
        })?;

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

    fn edit_book_with_id<S: AsRef<str>, T: AsRef<str>>(
        &mut self,
        id: u32,
        column: S,
        new_value: T,
    ) -> Result<(), DatabaseError> {
        self.backend.write(|db| match db.books.get_mut(&id) {
            None => Err(DatabaseError::BookNotFound(id)),
            Some(book) => Ok(book.set_column(&column.as_ref().into(), new_value)?),
        })??;
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

        Ok(())
    }

    fn find_matches<S: AsRef<str>, T: AsRef<str>>(
        &self,
        matching: Matching,
        column: S,
        search: T,
    ) -> Result<Vec<Book>, DatabaseError> {
        let col = ColumnIdentifier::from(column);

        Ok(self
            .backend
            .read(|db| -> Result<Vec<Book>, DatabaseError> {
                let mut results = vec![];
                match matching {
                    Matching::Default => {
                        let insensitive = search.as_ref().to_ascii_lowercase();
                        for (_, book) in db.books.iter() {
                            if insensitive.contains(&book.get_column_or(&col, "")) {
                                results.push(book.clone());
                            }
                        }
                    }
                    Matching::CaseSensitive => {
                        let sensitive = search.as_ref().to_owned();
                        for (_, book) in db.books.iter() {
                            if sensitive.contains(&book.get_column_or(&col, "")) {
                                results.push(book.clone());
                            }
                        }
                    }
                    Matching::Regex => {
                        let matcher = Regex::new(&search.as_ref())?;
                        for (_, book) in db.books.iter() {
                            if matcher.is_match(&book.get_column_or(&col, "")) {
                                results.push(book.clone());
                            }
                        }
                    }
                }
                Ok(results)
            })??)
    }

    fn sort_books_by_col(&self, col: &str, reverse: bool) -> Result<(), DatabaseError> {
        Ok(self.backend.write(|db| {
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
        })?)
    }

    fn size(&self) -> usize {
        self.len
    }
}

impl IndexableDatabase for BasicDatabase {
    fn get_books_indexed(&self, indices: Range<usize>) -> Result<Vec<Book>, DatabaseError> {
        Ok(self.backend.read(|db| {
            indices
                .map(|i| db.books.get_index(i))
                .filter_map(|b| b)
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

        Ok(())
    }

    fn edit_book_indexed<S: AsRef<str>, T: AsRef<str>>(
        &mut self,
        index: usize,
        column: S,
        new_value: T,
    ) -> Result<(), DatabaseError> {
        self.backend.write(|db| {
            if let Some((_, book)) = db.books.get_index_mut(index) {
                book.set_column(&column.as_ref().into(), new_value)?;
                Ok(())
            } else {
                Err(DatabaseError::IndexOutOfBounds(index))
            }
        })??;

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
        let base_cols = ["title", "authors", "id", "series"];
        assert!(db.cols.eq(&base_cols
            .iter()
            .map(|&c| UniCase::new(c.to_owned()))
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
