use std::collections::{HashMap, HashSet};
use std::{fs, path};

use rayon::prelude::*;
use rustbreak::{deser::Ron, FileDatabase, RustbreakError};
use serde::{Deserialize, Serialize};

use crate::record::{Book, BookError};

#[derive(Debug)]
pub(crate) enum DatabaseError {
    Io(std::io::Error),
    BookReading(BookError),
    Backend(RustbreakError),
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
        DatabaseError::BookReading(e)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub(crate) struct BookMap {
    max_id: u32,
    books: HashMap<u32, Book>,
}

impl BookMap {
    fn new_id(&mut self) -> u32 {
        let id = self.max_id;
        self.max_id += 1;
        id
    }
}

pub(crate) struct BasicDatabase {
    backend: FileDatabase<BookMap, Ron>,
    // title_author_map: HashMap<(String, String), u32>
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
        &self,
        dir: S,
    ) -> Result<(Vec<u32>, Vec<DatabaseError>), DatabaseError>
    where
        S: AsRef<path::Path>;

    /// Reads the book at the given location into the database, and returns the book's ID.
    ///
    /// # Arguments
    /// * ` file_path ` - The path to the book to be read.
    ///
    /// # Errors
    /// This function will return an error if the database fails,
    /// the file does not exist, or can not be read.
    fn read_book_from_file<S>(&self, file_path: S) -> Result<u32, DatabaseError>
    where
        S: AsRef<path::Path>;

    /// Inserts the given book into the database. If the book does not have an ID, it is given
    /// an ID equal to the largest ID in the database so far, plus one.
    ///
    /// # Arguments
    /// * ` book ` - A book to be stored.
    ///
    /// # Errors
    /// This function will return an error if the database fails.
    fn insert_book(&self, book: Book) -> Result<u32, DatabaseError>;

    /// Removes the book with the given ID. If no book with the given ID exists, no change occurs.
    ///
    /// # Arguments
    /// * ` id ` - The ID of the book to be removed.
    ///
    /// # Errors
    /// This function will return an error if the database fails.
    fn remove_book(&self, id: u32) -> Result<(), DatabaseError>;

    /// Removes all books with the given IDs. If a book with a given ID does not exists, no change
    /// for that particular ID.
    ///
    /// # Arguments
    /// * ` ids ` - The IDs of the book to be removed.
    ///
    /// # Errors
    /// This function will return an error if the database fails.
    fn remove_books(&self, ids: Vec<u32>) -> Result<(), DatabaseError>;

    /// Returns a copy of every book in the database. If reading fails, None is returned.
    fn get_all_books(&self) -> Option<Vec<Book>>;

    /// Finds and returns the book with the given ID. If no book is found, nothing is returned.
    ///
    /// # Arguments
    /// * ` id ` - The ID of the book to be returned.
    fn get_book(&self, id: u32) -> Option<Book>;

    /// Finds and returns the books with the given IDs, and if a particular book is not found,
    /// nothing is returned for that particular book.
    fn get_books(&self, ids: Vec<u32>) -> Vec<Option<Book>>;

    /// Returns a list of columns that exist for at least one book in the database.
    fn get_available_columns(&self) -> Option<Vec<String>>;

    /// Returns a new ID which is larger than all previous IDs.
    fn get_new_id(&self) -> Result<u32, DatabaseError>;

    fn merge_similar(&self) -> Result<Vec<u32>, DatabaseError>;
}

impl AppDatabase for BasicDatabase {
    fn open<S>(file_path: S) -> Result<Self, DatabaseError>
    where
        S: AsRef<path::Path>,
    {
        Ok(BasicDatabase {
            backend: FileDatabase::<BookMap, Ron>::load_from_path_or_default(file_path)?,
            // title_author_map: HashMap::default(),
        })
    }

    fn save(&self) -> Result<(), DatabaseError> {
        Ok(self.backend.save()?)
    }

    // TODO: Add possibility of doing this accross multiple threads.
    fn read_books_from_dir<S>(
        &self,
        dir: S,
    ) -> Result<(Vec<u32>, Vec<DatabaseError>), DatabaseError>
    where
        S: AsRef<path::Path>,
    {
        let mut ids = vec![];
        let mut errs = vec![];
        let results = fs::read_dir(dir)?
            .map(|res| res.map(|e| e.path()))
            .collect::<Result<Vec<_>, std::io::Error>>()?
            .par_iter()
            .map(|path| self.read_book_from_file(path))
            .collect::<Vec<_>>();
        results.into_iter().for_each(|result| match result {
            Ok(id) => ids.push(id),
            Err(e) => errs.push(e),
        });
        Ok((ids, errs))
    }

    fn get_new_id(&self) -> Result<u32, DatabaseError> {
        Ok(self.backend.write(|db| db.new_id())?)
    }

    fn read_book_from_file<S>(&self, file_path: S) -> Result<u32, DatabaseError>
    where
        S: AsRef<path::Path>,
    {
        Ok(self.insert_book(Book::generate_from_file(file_path, self.get_new_id()?)?)?)
    }

    fn insert_book(&self, book: Book) -> Result<u32, DatabaseError> {
        Ok(self.backend.write(|db| {
            let id = book.get_id();
            db.books.insert(id, book);
            id
        })?)
    }

    fn remove_book(&self, id: u32) -> Result<(), DatabaseError> {
        Ok(self.backend.write(|db| {
            db.books.remove(&id);
        })?)
    }

    fn remove_books(&self, ids: Vec<u32>) -> Result<(), DatabaseError> {
        Ok(self.backend.write(|db| {
            for id in ids {
                db.books.remove(&id);
            }
        })?)
    }

    // TODO: Make this return a Vec of references?
    fn get_all_books(&self) -> Option<Vec<Book>> {
        match self.backend.read(|db| -> Result<Option<Vec<Book>>, ()> {
            Ok(Some(db.books.values().cloned().collect()))
        }) {
            Ok(books) => books.unwrap_or(None),
            Err(_) => None,
        }
    }

    fn get_book(&self, id: u32) -> Option<Book> {
        match self.backend.read(|db| match db.books.get(&id) {
            None => Err(()),
            Some(book) => Ok(Some(book.clone())),
        }) {
            Ok(book) => book.unwrap_or(None),
            Err(_) => None,
        }
    }

    fn get_books(&self, ids: Vec<u32>) -> Vec<Option<Book>> {
        ids.iter().map(|&id| self.get_book(id)).collect()
    }

    fn get_available_columns(&self) -> Option<Vec<String>> {
        match self
            .backend
            .read(|db| -> Result<Option<HashSet<String>>, ()> {
                let mut c = HashSet::new();
                c.insert("title".to_string());
                c.insert("authors".to_string());
                c.insert("series".to_string());
                c.insert("id".to_string());
                for book in db.books.values() {
                    if let Some(e) = book.get_extended_columns() {
                        for key in e.keys() {
                            c.insert(key.clone());
                        }
                    }
                }
                Ok(Some(c))
            }) {
            Ok(book) => {
                if let Ok(x) = book {
                    if let Some(hset) = x {
                        return Some(hset.iter().cloned().collect());
                    }
                }
                None
            }
            Err(_) => None,
        }
    }

    fn merge_similar(&self) -> Result<Vec<u32>, DatabaseError> {
        Ok(self.backend.write(|db| {
            let mut ref_map: HashMap<(String, String), u32> = HashMap::new();
            let mut merges = vec![];
            let mut merged = vec![];
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
                merged.push(*b2_id);
            }
            merged
        })?)
    }
}