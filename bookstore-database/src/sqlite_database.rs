use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::convert::TryFrom;
use std::num::NonZeroU64;
use std::ops::{Bound, DerefMut, RangeBounds};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Arc, RwLock};

use futures::executor::block_on;
use sqlx::migrate::MigrateDatabase;
use sqlx::{Connection, Sqlite, SqliteConnection};
use unicase::UniCase;

use bookstore_records::book::{str_to_series, BookID, ColumnIdentifier, RawBook};
use bookstore_records::{Book, BookVariant};

use crate::bookmap::BookMap;
use crate::search::Search;
use crate::{AppDatabase, DatabaseError, IndexableDatabase};

const CREATE_BOOKS: &str = r#"CREATE TABLE `books` (
`book_id` INTEGER NOT NULL PRIMARY KEY,
`title` TEXT DEFAULT NULL,
`series_name` TEXT DEFAULT NULL,
`series_id` REAL DEFAULT NULL
);"#;

// Authors are stored here as well.
const CREATE_EXTENDED_TAGS: &str = r#"CREATE TABLE `extended_tags` (
`tag_name` TEXT NOT NULL,
`tag_value` TEXT NOT NULL,
`book_id` INTEGER NOT NULL,
FOREIGN KEY(book_id) REFERENCES books(book_id)
    ON UPDATE CASCADE
    ON DELETE CASCADE
);"#;

const CREATE_VARIANTS: &str = r#"CREATE TABLE `variants` (
`book_type` TEXT NOT NULL,
`path` TEXT NOT NULL,
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

macro_rules! execute_query {
    ($self:ident, $query_str:expr) => ({
        {
            let mut mut_backend = $self.backend.borrow_mut();
            block_on(async {
                sqlx::query!($query_str).execute(mut_backend.deref_mut()).await
            }).map_err(DatabaseError::Backend)
        }
    });
    ($self:ident, $query_str:expr, $($args:tt),*) => ({
        {
            let mut mut_backend = $self.backend.borrow_mut();
            block_on(async {
                sqlx::query!($query_str, $($args),*).execute(mut_backend.deref_mut()).await
            }).map_err(DatabaseError::Backend)
        }
    })
}

macro_rules! execute_query_str {
    ($self: ident, $query_str: expr) => {{
        let mut mut_backend = $self.backend.borrow_mut();
        block_on(async {
            sqlx::query($query_str.as_ref())
                .execute(mut_backend.deref_mut())
                .await
        })
        .map_err(DatabaseError::Backend)
    }};
}

macro_rules! run_query_as {
    ($self:ident, $out_struct:path, $query_str:expr) => ({
        {
            let mut mut_backend = $self.backend.borrow_mut();
            block_on(async {
                sqlx::query_as!($out_struct, $query_str).fetch_all(mut_backend.deref_mut()).await
            }).map_err(DatabaseError::Backend)
        }
    });
    ($self:ident, $out_struct:path, $query_str:expr, $($args:tt),*) => ({
        {
            let mut mut_backend = $self.backend.borrow_mut();
            block_on(async {
                sqlx::query!($out_struct, $query_str, $($args),*).fetch_all(mut_backend.deref_mut()).await
            }).map_err(DatabaseError::Backend)
        }
    })
}

macro_rules! book {
    ($book: ident) => {
        $book.as_ref().read().unwrap()
    };
}

struct BookData {
    book_id: i64,
    title: Option<String>,
    series_name: Option<String>,
    series_id: Option<f32>,
}

struct VariantData {
    book_id: i64,
    book_type: String,
    path: String,
    local_title: Option<String>,
    language: Option<String>,
    identifier: Option<String>,
    description: Option<String>,
    id: Option<i64>,
}

struct TagData {
    tag_name: String,
    tag_value: String,
    book_id: i64,
}

impl From<BookData> for Book {
    fn from(bd: BookData) -> Self {
        let rb = RawBook {
            title: bd.title.clone(),
            authors: None,
            series: bd.series_name.as_ref().map(|sn| (sn.clone(), bd.series_id)),
            description: None,
            variants: None,
            extended_tags: None,
        };
        Book::from_raw_book(NonZeroU64::try_from(bd.book_id as u64).unwrap(), rb)
    }
}

impl From<VariantData> for BookVariant {
    fn from(vd: VariantData) -> Self {
        BookVariant {
            book_type: ron::from_str(&vd.book_type).unwrap(),
            path: PathBuf::from_str(&vd.path).unwrap(),
            local_title: vd.local_title,
            identifier: vd
                .identifier
                .as_ref()
                .map(|s| ron::from_str(s).ok())
                .flatten(),
            language: vd.language,
            additional_authors: None,
            translators: None,
            description: vd.description,
            id: vd.id.map(|id| u32::try_from(id).unwrap()),
        }
    }
}

pub struct SQLiteDatabase {
    backend: RefCell<SqliteConnection>,
    local_cache: BookMap,
}

impl SQLiteDatabase {
    fn load_books(&mut self) -> Result<(), DatabaseError<<SQLiteDatabase as AppDatabase>::Error>> {
        let book_data = run_query_as!(self, BookData, "SELECT * FROM books")?;
        let variant_data = run_query_as!(self, VariantData, "SELECT * FROM variants")?;
        let tag_data = run_query_as!(self, TagData, "SELECT * FROM extended_tags")?;

        let mut books = HashMap::new();
        for book in book_data.into_iter() {
            let book: Book = book.into();
            books.insert(book.get_id(), book);
        }
        for variant in variant_data.into_iter() {
            let id = NonZeroU64::try_from(variant.book_id as u64).unwrap();
            let variant: BookVariant = variant.into();
            if let Some(book) = books.get_mut(&id) {
                book.inner_mut().push_variant(variant);
            } else {
                panic!();
            }
        }

        // To get all columns: "SELECT DISTINCT tag_name FROM extended_tags"
        let mut prime_cols = HashSet::new();
        for &col in &["title", "authors", "series", "id", "description"] {
            prime_cols.insert(col.to_owned());
        }

        for tag in tag_data.into_iter() {
            let id = NonZeroU64::try_from(tag.book_id as u64).unwrap();
            match books.get_mut(&id) {
                None => panic!(),
                Some(book) => {
                    if !prime_cols.contains(&tag.tag_name) {
                        prime_cols.insert(tag.tag_name.clone());
                    }

                    book.set_column(&ColumnIdentifier::from(tag.tag_name), tag.tag_value)
                        .unwrap();
                }
            }
        }

        self.local_cache = BookMap::from_values_unchecked(
            books
                .into_iter()
                .map(|(a, b)| (a, Arc::new(RwLock::new(b))))
                .collect(),
            prime_cols.into_iter().map(UniCase::new).collect(),
        );
        Ok(())
    }
}

// TODO: Should we use a separate process to mirror changes to SQL database?
//  Would push writes to queue:
//  DELETE_ALL should clear queue, since everything will be deleted.
//  DELETE_BOOK_ID should clear anything that overwrites given book, except when
//  an ordering is enforced in previous command.
impl AppDatabase for SQLiteDatabase {
    type Error = sqlx::Error;

    fn open<P>(file_path: P) -> Result<Self, DatabaseError<Self::Error>>
    where
        P: AsRef<Path>,
        Self: Sized,
    {
        let path = file_path.as_ref().display().to_string();

        let db_exists = block_on(async { Sqlite::database_exists(path.as_str()).await })
            .map_err(DatabaseError::Backend)?;
        if !db_exists {
            block_on(async { Sqlite::create_database(path.as_str()).await })
                .map_err(DatabaseError::Backend)?;
        }
        let database = block_on(async { SqliteConnection::connect(path.as_str()).await })
            .map_err(DatabaseError::Backend)?;

        let mut db = Self {
            backend: RefCell::new(database),
            local_cache: BookMap::default(),
        };

        if !db_exists {
            execute_query_str!(db, CREATE_BOOKS)?;
            execute_query_str!(db, CREATE_EXTENDED_TAGS)?;
            execute_query_str!(db, CREATE_VARIANTS)?;
        } else {
            db.load_books()?;
        }
        Ok(db)
    }

    fn save(&mut self) -> Result<(), DatabaseError<Self::Error>> {
        Ok(())
    }

    fn insert_book(&mut self, book: RawBook) -> Result<BookID, DatabaseError<Self::Error>> {
        let title = book.title.as_ref();
        let (series, series_index) = match book.series.as_ref() {
            None => (None, None),
            Some((series, series_index)) => (Some(series), series_index.clone()),
        };
        let id = execute_query!(
            self,
            "INSERT into books (title, series_name, series_id) VALUES(?, ?, ?)",
            title,
            series,
            series_index
        )?
        .last_insert_rowid();

        if let Some(variants) = book.get_variants() {
            for variant in variants {
                let book_type = ron::to_string(variant.book_type()).unwrap();
                let path = variant.path().display().to_string();
                let local_title = &variant.local_title;
                let identifier = variant
                    .identifier
                    .as_ref()
                    .map(|i| ron::to_string(i).unwrap());
                let language = &variant.language;
                let description = &variant.description;
                let sub_id = &variant.id;
                execute_query!(
                    self,
                    "INSERT into variants (book_type, path, local_title, identifier, language, description, id, book_id) VALUES(?, ?, ?, ?, ?, ?, ?, ?)",
                    book_type,
                    path,
                    local_title,
                    identifier,
                    language,
                    description,
                    sub_id,
                    id
                )?;
            }
        }

        if let Some(tags) = book.get_extended_columns() {
            let mut query =
                String::from("INSERT INTO extended_tags (tag_name, tag_value, book_id) VALUES");
            query
                .extend(tags.iter().map(|(tag_name, tag_value)| {
                    format!("({}, {}, {}),", tag_name, tag_value, id)
                }));
            query.pop();
            query.push(';');
            execute_query_str!(self, query)?;
        }

        let id = BookID::try_from(id as u64).unwrap();
        self.local_cache
            .insert_book_with_id(Book::from_raw_book(id, book));
        Ok(id)
    }

    fn insert_books(
        &mut self,
        books: Vec<RawBook>,
    ) -> Result<Vec<BookID>, DatabaseError<Self::Error>> {
        let ids = Vec::with_capacity(books.len());
        for book in books {
            self.insert_book(book)?;
        }
        Ok(ids)
    }

    fn remove_book(&mut self, id: BookID) -> Result<(), DatabaseError<Self::Error>> {
        // "DELETE FROM books WHERE book_id = {id}"
        let idx = u64::from(id) as i64;
        execute_query!(self, "DELETE FROM books WHERE book_id = ?", idx)?;
        self.local_cache.remove_book(id);
        Ok(())
    }

    fn remove_books(&mut self, ids: &HashSet<BookID>) -> Result<(), DatabaseError<Self::Error>> {
        // "DELETE FROM books WHERE book_id IN ({ids})"
        self.local_cache.remove_books(ids);

        let ids = ids
            .iter()
            .map(|&id| (u64::from(id) as i64).to_string())
            .collect::<Vec<_>>();

        let query = format!("DELETE FROM books WHERE book_id IN ({})", ids.join(", "));
        execute_query_str!(self, query)?;
        Ok(())
    }

    fn clear(&mut self) -> Result<(), DatabaseError<Self::Error>> {
        // "DELETE FROM books"
        execute_query!(self, "DELETE FROM books")?;
        self.local_cache.clear();
        Ok(())
    }

    fn get_book(&self, id: BookID) -> Result<Arc<RwLock<Book>>, DatabaseError<Self::Error>> {
        // "SELECT * FROM books WHERE book_id = {id}"
        self.local_cache
            .get_book(id)
            .ok_or(DatabaseError::BookNotFound(id))
    }

    fn get_books<I: IntoIterator<Item = BookID>>(
        &self,
        ids: I,
    ) -> Result<Vec<Option<Arc<RwLock<Book>>>>, DatabaseError<Self::Error>> {
        // SELECT * FROM {} WHERE book_id IN ({}, );
        Ok(ids
            .into_iter()
            .map(|id| self.local_cache.get_book(id))
            .collect())
    }

    fn get_all_books(&self) -> Result<Vec<Arc<RwLock<Book>>>, DatabaseError<Self::Error>> {
        // "SELECT * FROM {} ORDER BY {} {};"
        Ok(self.local_cache.get_all_books())
    }

    fn has_column(&self, col: &UniCase<String>) -> Result<bool, DatabaseError<Self::Error>> {
        Ok(self.local_cache.has_column(col))
    }

    fn edit_book_with_id<S0: AsRef<str>, S1: AsRef<str>>(
        &mut self,
        id: BookID,
        column: S0,
        new_value: S1,
    ) -> Result<(), DatabaseError<Self::Error>> {
        // "UPDATE {} SET {} = {} WHERE book_id = {};"
        if !self
            .local_cache
            .edit_book_with_id(id, &column, &new_value)?
        {
            return Err(DatabaseError::BookNotFound(id));
        }
        let idx = u64::from(id) as i64;
        let new_value = new_value.as_ref();
        match column.as_ref().into() {
            ColumnIdentifier::Title => {
                execute_query!(
                    self,
                    "UPDATE books SET title = ? WHERE book_id = ?",
                    new_value,
                    idx
                )?;
            }
            ColumnIdentifier::Author => {
                execute_query!(
                    self,
                    "INSERT INTO extended_tags (tag_name, tag_value, book_id) VALUES('author', ?, ?);",
                    new_value,
                    idx
                )?;
            }
            ColumnIdentifier::Series => {
                let series = str_to_series(new_value);
                let (series, series_index) = match series.as_ref() {
                    None => (None, None),
                    Some((series, series_index)) => (Some(series), series_index.clone()),
                };

                execute_query!(
                    self,
                    "UPDATE books SET series_name = ?, series_id = ? WHERE book_id = ?",
                    series,
                    series_index,
                    idx
                )?;
            }
            ColumnIdentifier::ID => {
                unreachable!(
                    "id is immutable, and this case is reached when local cache is modified"
                );
            }
            ColumnIdentifier::Variants => {
                execute_query!(
                    self,
                    "UPDATE books SET title = ? WHERE book_id = ?",
                    new_value,
                    idx
                )?;
            }
            ColumnIdentifier::Description => {
                execute_query!(
                    self,
                    "UPDATE variants SET description = ? WHERE book_id = ?",
                    new_value,
                    idx
                )?;
            }
            ColumnIdentifier::ExtendedTag(t) => {
                execute_query!(
                    self,
                    "INSERT into extended_tags (tag_name, tag_value, book_id) VALUES(?, ?, ?)",
                    t,
                    new_value,
                    idx
                )?;
            }
        }
        Ok(())
    }

    fn merge_similar(&mut self) -> Result<HashSet<BookID>, DatabaseError<Self::Error>> {
        let merged = self.local_cache.merge_similar();
        self.remove_books(&merged)?;
        Ok(merged)
    }

    fn find_matches(
        &self,
        search: Search,
    ) -> Result<Vec<Arc<RwLock<Book>>>, DatabaseError<Self::Error>> {
        // "SELECT * FROM {} WHERE {} REGEXP {};"
        // FIND_MATCHES_* - look at SQLite FTS5.
        Ok(self.local_cache.find_matches(search)?)
    }

    fn sort_books_by_col<S: AsRef<str>>(
        &mut self,
        col: S,
        reverse: bool,
    ) -> Result<(), DatabaseError<Self::Error>> {
        self.local_cache.sort_books_by_col(col, reverse);
        Ok(())
    }

    fn size(&self) -> usize {
        // "SELECT COUNT(*) FROM books;"
        self.local_cache.len()
    }

    fn saved(&self) -> bool {
        true
    }
}

impl IndexableDatabase for SQLiteDatabase {
    fn get_books_indexed(
        &self,
        indices: impl RangeBounds<usize>,
    ) -> Result<Vec<Arc<RwLock<Book>>>, DatabaseError<Self::Error>> {
        // "SELECT * FROM {} ORDER BY {} {} LIMIT {} OFFSET {};
        let start = match indices.start_bound() {
            Bound::Included(i) => *i,
            Bound::Excluded(i) => *i + 1,
            Bound::Unbounded => 0,
        }
        .min(self.size().saturating_sub(1));

        let end = match indices.end_bound() {
            Bound::Included(i) => *i + 1,
            Bound::Excluded(i) => *i,
            Bound::Unbounded => usize::MAX,
        }
        .min(self.size());

        // let offset = start;
        // let len = end.saturating_sub(start);

        Ok((start..end)
            .filter_map(|i| self.local_cache.get_book_indexed(i))
            .collect())
    }

    fn get_book_indexed(
        &self,
        index: usize,
    ) -> Result<Arc<RwLock<Book>>, DatabaseError<Self::Error>> {
        // "SELECT * FROM {} ORDER BY {} {} LIMIT 1 OFFSET {};"
        self.local_cache
            .get_book_indexed(index)
            .ok_or(DatabaseError::IndexOutOfBounds(index))
    }

    fn remove_book_indexed(&mut self, index: usize) -> Result<(), DatabaseError<Self::Error>> {
        // "DELETE FROM books ORDER BY {} {} LIMIT 1 OFFSET {}"
        // "DELETE FROM books ORDER BY {} {} LIMIT {} OFFSET {}"
        let book = self
            .local_cache
            .get_book_indexed(index)
            .ok_or(DatabaseError::IndexOutOfBounds(index))?;
        let book = book!(book);
        self.remove_book(book.get_id())
    }

    fn edit_book_indexed<S0: AsRef<str>, S1: AsRef<str>>(
        &mut self,
        index: usize,
        column: S0,
        new_value: S1,
    ) -> Result<(), DatabaseError<Self::Error>> {
        // "UPDATE {} SET {} = {} ORDER BY {} {} LIMIT 1 OFFSET {}"
        let book = self
            .local_cache
            .get_book_indexed(index)
            .ok_or(DatabaseError::IndexOutOfBounds(index))?;
        let book = book!(book);
        self.edit_book_with_id(book.get_id(), column, new_value)
    }
}
