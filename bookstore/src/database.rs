use std::collections::{HashMap, HashSet};

use rustbreak::{deser::Ron, FileDatabase, RustbreakError};
use serde::{Deserialize, Serialize};

use std::fs;

use crate::book::{Book, BookError};
use std::path;

#[derive(Debug)]
pub(crate) enum DatabaseError {
    IoError(std::io::Error),
    BookReadingError(BookError),
    BackendError(RustbreakError),
}

impl From<std::io::Error> for DatabaseError {
    fn from(e: std::io::Error) -> Self {
        DatabaseError::IoError(e)
    }
}

impl From<RustbreakError> for DatabaseError {
    fn from(e: RustbreakError) -> Self {
        DatabaseError::BackendError(e)
    }
}

impl From<BookError> for DatabaseError {
    fn from(e: BookError) -> Self {
        DatabaseError::BookReadingError(e)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub(crate) struct BookMap {
    books: HashMap<u32, Book>,
}

pub(crate) struct BasicDatabase {
    backend: FileDatabase<BookMap, Ron>,
}

pub(crate) trait AppDatabase {
    fn open<S>(file_path: S) -> Result<Self, DatabaseError>
    where
        S: AsRef<path::Path>,
        Self: Sized;

    fn save(&self) -> Result<(), DatabaseError>;

    fn read_books_from_dir<S>(&self, dir: S) -> Result<Vec<u32>, DatabaseError>
    where
        S: AsRef<path::Path>;

    fn read_book_from_file<S>(&self, file_path: S) -> Result<u32, DatabaseError>
    where
        S: AsRef<path::Path>;

    fn insert_book(&self, book: Book) -> Result<u32, DatabaseError>;

    fn remove_book(&self, id: u32) -> Result<(), DatabaseError>;

    fn get_all_books(&self) -> Option<Vec<Book>>;

    fn get_book(&self, book_id: u32) -> Option<Book>;

    fn get_books(&self, book_ids: Vec<u32>) -> Vec<Option<Book>>;

    fn get_available_columns(&self) -> Option<Vec<String>>;
}

impl AppDatabase for BasicDatabase {
    fn open<S>(file_path: S) -> Result<Self, DatabaseError>
    where
        S: AsRef<path::Path>,
    {
        Ok(BasicDatabase {
            backend: FileDatabase::<BookMap, Ron>::load_from_path_or_default(file_path)?,
        })
    }

    fn save(&self) -> Result<(), DatabaseError> {
        Ok(self.backend.save()?)
    }

    fn read_books_from_dir<S>(&self, dir: S) -> Result<Vec<u32>, DatabaseError>
    where
        S: AsRef<path::Path>,
    {
        fs::read_dir(dir)?
            .map(|res| res.map(|e| e.path()))
            .collect::<Result<Vec<_>, std::io::Error>>()?
            .iter()
            .map(|path| self.read_book_from_file(path))
            .collect::<Result<Vec<_>, _>>()
    }

    fn read_book_from_file<S>(&self, file_path: S) -> Result<u32, DatabaseError>
    where
        S: AsRef<path::Path>,
    {
        match self.insert_book(Book::generate_from_file(file_path)?) {
            Ok(id) => Ok(id),
            Err(e) => Err(DatabaseError::from(e)),
        }
    }

    fn insert_book(&self, book: Book) -> Result<u32, DatabaseError> {
        Ok(self.backend.write(|db| {
            if let Some(id) = book.get_id() {
                db.books.insert(id, book);
                id
            } else {
                let index = db.books.keys().max().unwrap_or(&0) + 1;
                let mut book = book.clone();
                book.set_id(index);
                db.books.insert(index, book);
                index
            }
        })?)
    }

    fn remove_book(&self, id: u32) -> Result<(), DatabaseError> {
        Ok(self.backend.write(|db| {
            db.books.remove(&id);
        })?)
    }

    fn get_all_books(&self) -> Option<Vec<Book>> {
        match self.backend.read(|db| -> Result<Option<Vec<Book>>, ()> {
            Ok(Some(
                db.books.values().into_iter().map(|b| b.clone()).collect(),
            ))
        }) {
            Ok(book) => book.unwrap_or(None),
            Err(_) => None,
        }
    }

    fn get_book(&self, book_id: u32) -> Option<Book> {
        match self.backend.read(|db| match db.books.get(&book_id) {
            None => Err(()),
            Some(book) => Ok(Some(book.clone())),
        }) {
            Ok(book) => book.unwrap_or(None),
            Err(_) => None,
        }
    }

    fn get_books(&self, book_ids: Vec<u32>) -> Vec<Option<Book>> {
        book_ids.iter().map(|&id| self.get_book(id)).collect()
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
                        return Some(hset.iter().map(|col| col.clone()).collect());
                    }
                }
                None
            }
            Err(_) => None,
        }
    }
}
