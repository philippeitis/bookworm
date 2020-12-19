use std::collections::hash_map::RandomState;
use std::collections::HashSet;
use std::ops::{Bound, RangeBounds};
use std::path;

use itertools::Itertools;
use sqlx::migrate::MigrateDatabase;
use sqlx::{Connection, Sqlite, SqliteConnection};
use unicase::UniCase;

use bookstore_records::book::RawBook;
use bookstore_records::Book;

use crate::search::Search;
use crate::{AppDatabase, DatabaseError, IndexableDatabase};

const CREATE_BOOKS: &str = r#"CREATE TABLE `books` (
`book_id` INTEGER PRIMARY KEY,
`title` TEXT DEFAULT NULL,
`series_name` TEXT DEFAULT NULL,
`series_id` NUM DEFAULT NULL
);"#;

// Authors are stored here as well.
const CREATE_EXTENDED_TAGS: &str = r#"CREATE TABLE `extended_tags` (
`tag_name` TEXT,
`tag_value` TEXT,
`book_id` INTEGER NOT NULL,
FOREIGN KEY(book_id) REFERENCES books(book_id)
    ON UPDATE CASCADE
    ON DELETE CASCADE
);"#;

const CREATE_VARIANTS: &str = r#"CREATE TABLE `variants` (
`book_type` TEXT,
`path` TEXT,
`local_title` TEXT DEFAULT NULL,
`identifier` TEXT DEFAULT NULL,
`language` TEXT DEFAULT NULL,
`description` TEXT DEFAULT NULL,
`id` INTEGER DEFAULT NULL,
`book_id` INTEGER NOT NULL,
FOREIGN KEY(book_id) REFERENCES books(book_id)
    ON UPDATE CASCADE
    ON DELETE CASCADE
);"#;

const FETCH_ID: &str = r#"SELECT * FROM {} WHERE book_id = {}";"#;
const FETCH_IDS: &str = r#"SELECT * FROM {} WHERE book_id IN ({}, )";"#;
const DELETE_BOOK: &str = r#"DELETE FROM books WHERE book_id = {}"#;
const DELETE_BOOKS: &str = r#"DELETE FROM books WHERE book_id IN ({},)"#;
const DELETE_ALL: &str = r#"DELETE FROM books"#;
const EDIT_BOOK_BY_ID: &str = r#"UPDATE {} SET {} = {} WHERE book_id = {}"#;
const GET_ALL_COLUMNS: &str = r#"SELECT DISTINCT tag_name FROM extended_tags"#;

// TODO: FIND_MATCHES_* - look at SQLite FTS5.
// TODO: MERGE_SIMILAR?
const FIND_MATCHES_REGEX: &str = r#"SELECT * FROM {} WHERE {} REGEXP {};"#;
const GET_SIZE: &str = r#"SELECT COUNT(*) FROM books;"#;

const FETCH_SINGLE_INDEX: &str = r#"SELECT * FROM {} ORDER BY {} {} LIMIT 1 OFFSET {};"#;
//                               all values         ascending or descending
//                                  |     table   column  |  number of books
//                                  v      v           v  v        v    start index
const FETCH_RANGE_INDEX: &str = r#"SELECT * FROM {} ORDER BY {} {} LIMIT {} OFFSET {};"#;
const FETCH_ALL: &str = r#"SELECT * FROM {} ORDER BY {} {};"#;
const DELETE_BOOK_INDEX: &str = r#"DELETE FROM books ORDER BY {} {} LIMIT 1 OFFSET {}#;
const DELETE_BOOKS_INDEX: &str = r#"DELETE FROM books ORDER BY {} {} LIMIT {} OFFSET {}"#;
const EDIT_BOOK_BY_INDEX: &str = r#"UPDATE {} SET {} = {} WHERE book_id = {} LIMIT 1 OFFSET {}"#;

pub struct SQLiteDatabase {
    backend: SqliteConnection,
    /// All available columns. Case-insensitive.
    cols: HashSet<UniCase<String>>,
    len: usize,
    saved: bool,
}

// TODO: Should we read everything into in-memory cache so that we can return
//  books synchronously?
//  Eg. Changes mirrored to internal cache, then written into SQLite
//  db via another process?
//  Would push writes to queue:
//  DELETE_ALL should clear queue, since everything will be deleted.
//  DELETE_BOOK_ID should clear anything that overwrites given book, except when
//  an ordering is enforced in previous command.
impl AppDatabase for SQLiteDatabase {
    fn open<P>(file_path: P) -> Result<Self, DatabaseError>
    where
        P: AsRef<path::Path>,
        Self: Sized,
    {
        let path = file_path.as_ref().display().to_string();
        if !Sqlite::database_exists(path.as_str()).await.unwrap() {
            Sqlite::create_database(path.as_str()).await.unwrap();
        }
        let database = SqliteConnection::connect(path.as_str()).await.unwrap();
        Ok(Self {
            backend: database,
            cols: Default::default(),
            len: 0,
            saved: false,
        })
    }

    fn save(&mut self) -> Result<(), DatabaseError> {
        unimplemented!()
    }

    fn insert_book(&mut self, book: RawBook) -> Result<u32, DatabaseError> {
        unimplemented!()
    }

    fn insert_books(&mut self, books: Vec<RawBook>) -> Result<Vec<u32>, DatabaseError> {
        unimplemented!()
    }

    fn remove_book(&mut self, id: u32) -> Result<(), DatabaseError> {
        // let query = format!("DELETE FROM books WHERE book_id = {}", id);
        // let idx = id as i64;
        // let data = sqlx::query!("DELETE FROM books WHERE book_id = ?", idx)
        //     .fetch_all(&mut self.backend)
        //     .await?;
        unimplemented!()
    }

    fn remove_books(&mut self, ids: Vec<u32>) -> Result<(), DatabaseError> {
        // let query = format!("DELETE FROM books WHERE book_id IN ({})", ids.iter().join(", "));
        // let query = sqlx::query(&query).execute(&mut self.backend).await?;
        // let ids = ids.into_iter().map(|id| id as i64).collect::<Vec<_>>();
        // let data = sqlx::query("DELETE FROM books WHERE book_id IN (?)", ids.as_slice());
        unimplemented!()
    }

    fn clear(&mut self) -> Result<(), DatabaseError> {
        // let query = format!("DELETE FROM books");
        // let data = sqlx::query!("DELETE FROM books")
        //     .fetch_all(&mut self.backend)
        //     .await?;
        unimplemented!()
    }

    fn get_book(&self, id: u32) -> Result<Book, DatabaseError> {
        //     let query = format!("SELECT * FROM books WHERE book_id = {}", id);
        // let data = sqlx::query!("SELECT * FROM books WHERE book_id = ?", id)
        //     .fetch_all(&mut self.backend)
        //     .await?;
        unimplemented!()
    }

    fn get_books(&self, ids: Vec<u32>) -> Result<Vec<Option<Book>>, DatabaseError> {
        // let query = format!("SELECT * FROM books WHERE book_id IN ({})", ids.iter().join(", "));
        // let data = sqlx::query!("SELECT * FROM books WHERE book_id IN ?", ids);
        unimplemented!()
    }

    fn get_all_books(&self) -> Result<Vec<Book>, DatabaseError> {
        // let query = format!("SELECT * FROM {} ORDER BY {} {}");
        // let data = sqlx::query!("SELECT * FROM {} ORDER BY ? ?");
        unimplemented!()
    }

    fn get_available_columns(&self) -> &HashSet<UniCase<String>, RandomState> {
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
        // let query = format!("UPDATE {} SET {} = {} WHERE book_id = {}");
        // let data = sqlx::query!("SELECT * FROM {} ORDER BY ? ?");
        unimplemented!()
    }

    fn merge_similar(&mut self) -> Result<(), DatabaseError> {
        unimplemented!()
    }

    fn find_matches(&self, search: Search) -> Result<Vec<Book>, DatabaseError> {
        unimplemented!()
    }

    fn sort_books_by_col(&mut self, col: &str, reverse: bool) -> Result<(), DatabaseError> {}

    fn size(&self) -> usize {
        self.len
    }

    fn saved(&self) -> bool {
        true
    }
}

impl IndexableDatabase for SQLiteDatabase {
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

        let offset = start;
        let len = end.saturating_sub(start);

        unimplemented!()
    }

    fn get_book_indexed(&self, index: usize) -> Result<Book, DatabaseError> {
        unimplemented!()
    }

    fn remove_book_indexed(&mut self, index: usize) -> Result<(), DatabaseError> {
        unimplemented!()
    }

    fn edit_book_indexed<S0: AsRef<str>, S1: AsRef<str>>(
        &mut self,
        index: usize,
        column: S0,
        new_value: S1,
    ) -> Result<(), DatabaseError> {
        unimplemented!()
    }
}