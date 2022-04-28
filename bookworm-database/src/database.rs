use std::collections::{HashMap, HashSet};
use std::fmt::Debug;
use std::path;
use std::sync::Arc;

use async_trait::async_trait;
use unicase::UniCase;

use bookworm_input::Edit;
use bookworm_records::book::{BookID, ColumnIdentifier, RecordError};
use bookworm_records::Book;

use crate::paginator::{Selection, Variable};

#[derive(Debug)]
pub enum DatabaseError<DBError> {
    Io(std::io::Error),
    Record(RecordError),
    BookNotFound(BookID),
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

#[async_trait]
pub trait AppDatabase {
    type Error: Send + Debug;
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
    async fn insert_book(&mut self, book: Book) -> Result<BookID, DatabaseError<Self::Error>>;

    /// Stores each book into the database, and returns a Vec of corresponding IDs.
    ///
    /// # Arguments
    /// * ` books ` - Some number of books to be stored.
    ///
    /// # Errors
    /// This function will return an error if the books can not be inserted into the database.
    async fn insert_books<I: Iterator<Item = Book> + Send>(
        &mut self,
        books: I,
    ) -> Result<Vec<BookID>, DatabaseError<Self::Error>>;

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

    /// Removes all books contained in the provided selection.
    ///
    /// # Arguments
    /// * ` selection ` - The selection over items to be removed.
    ///
    /// # Errors
    /// This function will return an error if the database fails.
    async fn remove_selected(
        &mut self,
        selection: &Selection,
    ) -> Result<(), DatabaseError<Self::Error>>;

    async fn clear(&mut self) -> Result<(), DatabaseError<Self::Error>>;

    #[must_use]
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

    #[must_use]
    /// Finds and returns the books with the given IDs as entries in a hashmap.
    ///
    /// # Arguments
    /// * ` ids ` - The IDs of the book to be returned.
    ///
    /// # Errors
    /// This function will return an error if the database fails
    async fn get_books(
        &self,
        id: &[BookID],
    ) -> Result<HashMap<BookID, Arc<Book>>, DatabaseError<Self::Error>>;

    async fn read_selected_books(
        &self,
        query: &str,
        bound_variables: &[Variable],
    ) -> Result<Vec<Arc<Book>>, DatabaseError<Self::Error>>;

    /// Finds the book with the given ID, then, for each pair of strings (field, new_value)
    /// in `edits`, set the corresponding field to new_value. If a given field is immutable,
    /// or some other failure occurs, an error will be returned.
    ///
    /// # Arguments
    /// * ` id ` - The ID of the book to be edited.
    /// * ` edits ` - A set of <field, value> pairs to set in the book.
    ///
    /// # Errors
    /// This function will return an error if the database fails, or if any of the provided
    /// edits try to mutate an immutable column
    async fn edit_book_with_id(
        &mut self,
        id: BookID,
        edits: &[(ColumnIdentifier, Edit)],
    ) -> Result<(), DatabaseError<Self::Error>>;

    /// Edits all books which match the provided selection, in no particular order.
    ///
    /// # Arguments
    /// * ` selected ` - a Selection over target items..
    ///
    /// # Errors
    /// This function will return an error if the database fails, or if any of the provided
    /// edits try to mutate an immutable column
    async fn edit_selected(
        &mut self,
        selected: &Selection,
        edits: &[(ColumnIdentifier, Edit)],
    ) -> Result<(), DatabaseError<Self::Error>>;

    /// Merges all books with matching titles and authors (case insensitive), in no
    /// particular order. Books that are merged will not necessarily free IDs no longer in use.
    /// Returns a HashSet containing the IDs of all books that have been merged.
    ///
    /// # Errors
    /// This function will return an error if updating the database fails.
    async fn merge_similar(&mut self) -> Result<HashSet<BookID>, DatabaseError<Self::Error>>;

    async fn update<I: Iterator<Item = Book> + Send>(
        &mut self,
        books: I,
    ) -> Result<(), DatabaseError<Self::Error>>;

    #[must_use]
    /// Returns whether the provided column exists in at least one book in the database.
    ///
    /// # Arguments
    /// * ` col ` - The column to check.
    async fn has_column(&self, col: &UniCase<String>) -> Result<bool, DatabaseError<Self::Error>>;

    #[must_use]
    /// Returns true if the internal database is persisted to file, but does not necessarily indicate
    /// that it has been changed - eg. if a change is immediately undone, the database may still
    /// be marked as unsaved.
    async fn saved(&self) -> bool;
}
