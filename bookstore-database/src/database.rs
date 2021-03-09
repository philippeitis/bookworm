use std::collections::HashSet;
use std::ops::RangeBounds;
use std::path;
use std::sync::{Arc, RwLock};

use unicase::UniCase;

use crate::search::{Error as SearchError, Search};

use bookstore_records::book::{BookID, RawBook};
use bookstore_records::{Book, BookError};

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

    fn path(&self) -> &path::Path;
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
    /// This function will return an error if updating the database fails.
    fn edit_book_with_id<S0: AsRef<str>, S1: AsRef<str>>(
        &mut self,
        id: BookID,
        edits: &[(S0, S1)],
    ) -> Result<(), DatabaseError<Self::Error>>;

    /// Merges all books with matching titles and authors (case insensitive), in no
    /// particular order. Books that are merged will not necessarily free IDs no longer in use.
    /// Returns a HashSet containing the IDs of all books that have been merged.
    ///
    /// # Errors
    /// This function will return an error if updating the database fails.
    fn merge_similar(&mut self) -> Result<HashSet<BookID>, DatabaseError<Self::Error>>;

    /// Finds books, using the match to compare the specified column to the search string.
    ///
    /// # Arguments
    /// * ` searches ` - Some number of search queries.
    ///
    /// # Errors
    /// This function will return an error if the database fails.
    fn find_matches(
        &self,
        searches: &[Search],
    ) -> Result<Vec<Arc<RwLock<Book>>>, DatabaseError<Self::Error>>;

    /// Finds the first book to match all criteria specified by searches.
    ///
    /// # Arguments
    /// * ` searches ` - Some number of search queries.
    ///
    /// # Errors
    /// This function will return an error if the database fails.
    fn find_book_index(
        &self,
        searches: &[Search],
    ) -> Result<Option<usize>, DatabaseError<Self::Error>>;

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
    /// This function will return an error if reading the database fails.
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
    /// This function will return an error if reading the database fails or the given index does not
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
    /// This function will return an error if updating the database fails.
    fn remove_book_indexed(&mut self, index: usize) -> Result<(), DatabaseError<Self::Error>>;

    /// Edit the book at the current index, respecting the current ordering.
    ///
    /// # Arguments
    /// * ` index ` - the index of the book to remove
    /// * ` column ` - the column to modify
    /// * ` new_value ` - the value to set the column to.
    ///
    /// # Errors
    /// This function will return an error if updating the database fails.
    fn edit_book_indexed<S0: AsRef<str>, S1: AsRef<str>>(
        &mut self,
        index: usize,
        edits: &[(S0, S1)],
    ) -> Result<(), DatabaseError<Self::Error>>;
}
