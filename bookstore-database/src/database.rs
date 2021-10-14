use std::collections::HashSet;
use std::path;
use std::sync::Arc;

use async_trait::async_trait;
use unicase::UniCase;

use bookstore_records::book::{BookID, ColumnIdentifier, RecordError};
use bookstore_records::{Book, BookVariant, Edit};

use crate::paginator::Variable;
use crate::search::{Error as SearchError, Search};

#[derive(Debug)]
pub enum DatabaseError<DBError> {
    Io(std::io::Error),
    Search(SearchError),
    Record(RecordError),
    BookNotFound(BookID),
    IndexOutOfBounds(usize),
    Backend(DBError),
}

impl<DBError> From<std::io::Error> for DatabaseError<DBError> {
    fn from(e: std::io::Error) -> Self {
        DatabaseError::Io(e)
    }
}

impl<DBError> From<RecordError> for DatabaseError<DBError> {
    fn from(e: RecordError) -> Self {
        DatabaseError::Record(e)
    }
}

impl<DBError> From<SearchError> for DatabaseError<DBError> {
    fn from(e: SearchError) -> Self {
        DatabaseError::Search(e)
    }
}

#[async_trait]
pub trait AppDatabase {
    type Error: Send;
    /// Opens the database at the path if it exists.
    ///
    /// # Arguments
    ///
    /// * ` file_path ` - A path to a database.
    ///
    /// # Errors
    /// This function will return an error if the file points to an invalid database.
    async fn open<P>(file_path: P) -> Result<Self, DatabaseError<Self::Error>>
    where
        P: AsRef<path::Path> + Send + Sync,
        Self: Sized;

    fn path(&self) -> &path::Path;

    /// Saves the database to its original location.
    ///
    /// # Errors
    /// This function will return an error if the database can not be saved correctly.
    async fn save(&mut self) -> Result<(), DatabaseError<Self::Error>>;

    /// Inserts the given book into the database, setting the ID automatically. The ID set
    /// will be returned, and calling other `AppDatabase` methods which take `BookID` with the
    /// given ID will perform functions on, or return the same book.
    ///
    /// # Arguments
    /// * ` book ` - A book to be stored.
    ///
    /// # Errors
    /// This function will return an error if the database fails.
    async fn insert_book(
        &mut self,
        book: BookVariant,
    ) -> Result<BookID, DatabaseError<Self::Error>>;

    /// Stores each book into the database, and returns a Vec of corresponding IDs.
    ///
    /// # Arguments
    /// * ` books ` - Some number of books to be stored.
    ///
    /// # Errors
    /// This function will return an error if the books can not be inserted into the database.
    async fn insert_books<I: Iterator<Item = BookVariant> + Send>(
        &mut self,
        books: I,
    ) -> Result<Vec<BookID>, DatabaseError<Self::Error>>;

    /// Removes the book with the given ID. If no book with the given ID exists, no change occurs.
    ///
    /// # Arguments
    /// * ` id ` - The ID of the book to be removed.
    ///
    /// # Errors
    /// This function will return an error if the database fails.
    async fn remove_book(&mut self, id: BookID) -> Result<(), DatabaseError<Self::Error>>;

    /// Removes all books with the given IDs. If a book with a given ID does not exist, or an ID
    /// is repeated, no changes will occur for that particular ID.
    ///
    /// # Arguments
    /// * ` ids ` - A HashSet containing the IDs of the book to be removed.
    ///
    /// # Errors
    /// This function will return an error if the database fails.
    async fn remove_books(
        &mut self,
        ids: &HashSet<BookID>,
    ) -> Result<(), DatabaseError<Self::Error>>;

    async fn clear(&mut self) -> Result<(), DatabaseError<Self::Error>>;

    /// Finds and returns the book with the given ID. If no book is found,
    /// a `BookNotFound` error is returned.
    ///
    /// # Arguments
    /// * ` id ` - The ID of the book to be returned.
    ///
    /// # Errors
    /// This function will return an error if the database fails or no book is found
    /// with the given ID.
    async fn get_book(&self, id: BookID) -> Result<Arc<Book>, DatabaseError<Self::Error>>;

    /// Finds and returns all books with the given IDs. If a book with a given ID does not exist,
    /// None is returned for that particular ID.
    ///
    /// # Arguments
    /// * ` ids ` - An iterator yielding the IDs of the books to be returned.
    ///
    /// # Errors
    /// This function will return an error if the database fails.
    async fn get_books<I: Iterator<Item = BookID> + Send>(
        &self,
        ids: I,
    ) -> Result<Vec<Option<Arc<Book>>>, DatabaseError<Self::Error>>;

    /// Returns a reference to every book in the database. If a database error occurs while reading,
    /// the error is returned.
    ///
    /// # Errors
    /// This function will return an error if the database fails.
    async fn get_all_books(&self) -> Result<Vec<Arc<Book>>, DatabaseError<Self::Error>>;

    /// Returns whether the provided column exists in at least one book in the database.
    ///
    /// # Arguments
    /// * ` col ` - The column to check.
    async fn has_column(&self, col: &UniCase<String>) -> Result<bool, DatabaseError<Self::Error>>;

    /// Finds the book with the given ID, then, for each pair of strings (field, new_value)
    /// in `edits`, set the corresponding field to new_value. If a given field is immutable,
    /// or some other failure occurs, an error will be returned.
    ///
    /// # Arguments
    /// * ` id ` - The ID of the book to be edited.
    /// * ` edits ` - A set of <field, value> pairs to set in the book.
    ///
    /// # Errors
    /// This function will return an error if updating the database fails, or a field can not
    /// be set.
    async fn edit_book_with_id(
        &mut self,
        id: BookID,
        edits: &[(ColumnIdentifier, Edit)],
    ) -> Result<(), DatabaseError<Self::Error>>;

    /// Merges all books with matching titles and authors (case insensitive), in no
    /// particular order. Books that are merged will not necessarily free IDs no longer in use.
    /// Returns a HashSet containing the IDs of all books that have been merged.
    ///
    /// # Errors
    /// This function will return an error if updating the database fails.
    async fn merge_similar(&mut self) -> Result<HashSet<BookID>, DatabaseError<Self::Error>>;

    /// Finds all books, which satisfy all provided `Search` items in `searches`, and returns them
    /// in a Vec<>.
    ///
    /// # Arguments
    /// * ` searches ` - Some number of search queries.
    ///
    /// # Errors
    /// This function will return an error if the database fails, or if a member of `searches`
    /// is malformed.
    async fn find_matches(
        &self,
        searches: &[Search],
    ) -> Result<Vec<Arc<Book>>, DatabaseError<Self::Error>>;

    /// Returns the number of books stored internally.
    async fn size(&self) -> usize;

    /// Returns true if the internal database is persisted to file, but does not necessarily indicate
    /// that it has been changed - eg. if a change is immediately undone, the database may still
    /// be marked as unsaved.
    async fn saved(&self) -> bool;

    async fn update<I: Iterator<Item = BookVariant> + Send>(
        &mut self,
        books: I,
    ) -> Result<Vec<BookID>, DatabaseError<Self::Error>>;

    async fn perform_query(
        &mut self,
        query: &str,
        bound_variables: &[Variable],
        limit: usize,
    ) -> Result<Vec<Arc<Book>>, DatabaseError<Self::Error>>;
}
