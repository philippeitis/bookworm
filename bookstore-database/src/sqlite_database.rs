use std::collections::{HashMap, HashSet};
use std::convert::TryFrom;
use std::ffi::OsString;
use std::num::NonZeroU64;
use std::ops::{Bound, RangeBounds};
#[cfg(unix)]
use std::os::unix::ffi::{OsStrExt, OsStringExt};
#[cfg(windows)]
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use futures::executor::block_on;
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{ConnectOptions, Connection, SqliteConnection};

use unicase::UniCase;

use bookstore_records::book::{str_to_series, BookID, ColumnIdentifier, RawBook};
use bookstore_records::{Book, BookVariant};

use crate::bookmap::BookMap;
use crate::search::Search;
use crate::{AppDatabase, DatabaseError, IndexableDatabase};

const CREATE_BOOKS: &str = r#"CREATE TABLE IF NOT EXISTS `books` (
`book_id` INTEGER NOT NULL PRIMARY KEY,
`title` TEXT DEFAULT NULL,
`series_name` TEXT DEFAULT NULL,
`series_id` REAL DEFAULT NULL
);"#;

// Authors are stored here as well.
const CREATE_EXTENDED_TAGS: &str = r#"CREATE TABLE IF NOT EXISTS `extended_tags` (
`tag_name` TEXT NOT NULL,
`tag_value` TEXT NOT NULL,
`book_id` INTEGER NOT NULL,
FOREIGN KEY(book_id) REFERENCES books(book_id)
    ON UPDATE CASCADE
    ON DELETE CASCADE
);"#;

const CREATE_VARIANTS: &str = r#"CREATE TABLE IF NOT EXISTS `variants` (
`book_type` TEXT NOT NULL,
`path` BLOB NOT NULL,
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
            block_on(async {
                sqlx::query!($query_str).execute(&mut $self.backend).await
            }).map_err(DatabaseError::Backend)
        }
    });
    ($self:ident, $query_str:expr, $($args:tt),*) => ({
        {
            block_on(async {
                sqlx::query!($query_str, $($args),*).execute(&mut $self.backend).await
            }).map_err(DatabaseError::Backend)
        }
    })
}

macro_rules! execute_query_str {
    ($self: ident, $query_str: expr) => {{
        block_on(async {
            sqlx::query($query_str.as_ref())
                .execute(&mut $self.backend)
                .await
        })
        .map_err(DatabaseError::Backend)
    }};
}

macro_rules! run_query_as {
    ($self:ident, $out_struct:path, $query_str:expr) => ({
        {
            block_on(async {
                sqlx::query_as!($out_struct, $query_str).fetch_all(&mut $self.backend).await
            }).map_err(DatabaseError::Backend)
        }
    });
    ($self:ident, $out_struct:path, $query_str:expr, $($args:tt),*) => ({
        {
            block_on(async {
                sqlx::query!($out_struct, $query_str, $($args),*).fetch_all(&mut $self.backend).await
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
    path: Vec<u8>,
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

#[cfg(windows)]
fn v8_to_v16(a: Vec<u8>) -> Vec<u16> {
    a.chunks(2)
        .map(|c| u16::from_be_bytes([c[0], c[1]]))
        .collect()
}

#[cfg(windows)]
fn v16_to_v8(a: Vec<u16>) -> Vec<u8> {
    let mut v = Vec::with_capacity(a.len() * 2);
    for val in a.into_iter() {
        v.extend_from_slice(&val.to_be_bytes());
    }
    v
}

impl From<BookData> for Book {
    fn from(bd: BookData) -> Self {
        let rb = RawBook {
            title: bd.title.clone(),
            series: bd.series_name.as_ref().map(|sn| (sn.clone(), bd.series_id)),
            ..Default::default()
        };
        Book::from_raw_book(
            NonZeroU64::try_from(bd.book_id as u64)
                .expect("SQLite database returned NULL primary ID."),
            rb,
        )
    }
}

impl From<VariantData> for BookVariant {
    fn from(vd: VariantData) -> Self {
        BookVariant {
            book_type: ron::from_str(&vd.book_type).unwrap(),
            #[cfg(unix)]
            path: PathBuf::from(OsString::from_vec(vd.path)),
            #[cfg(windows)]
            path: PathBuf::from(OsString::from_wide(&v8_to_v16(vd.path))),
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
    backend: SqliteConnection,
    local_cache: BookMap,
    path: PathBuf,
}

impl SQLiteDatabase {
    fn load_books(&mut self) -> Result<(), DatabaseError<<SQLiteDatabase as AppDatabase>::Error>> {
        // TODO: Benchmark this for large databases with complex books.
        let book_data = run_query_as!(self, BookData, "SELECT * FROM books")?;
        let variant_data = run_query_as!(self, VariantData, "SELECT * FROM variants")?;
        let tag_data = run_query_as!(self, TagData, "SELECT * FROM extended_tags")?;

        let mut books: HashMap<_, _> = book_data
            .into_iter()
            .map(|book| {
                let book: Book = book.into();
                (book.get_id(), book)
            })
            .collect();

        for variant in variant_data.into_iter() {
            let id = NonZeroU64::try_from(variant.book_id as u64).unwrap();
            let variant: BookVariant = variant.into();
            if let Some(book) = books.get_mut(&id) {
                book.inner_mut().push_variant(variant);
            } else {
                // TODO: Decide what to do here, since schema dictates that variants are deleted with owning book.
                panic!(
                    "SQLite database may be corrupted. Found orphan variant for {}.",
                    id
                );
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
                None => {
                    // TODO: Decide what to do here, since schema dictates that variants are deleted with owning book.
                    panic!(
                        "SQLite database may be corrupted. Found orphan tag for {}.",
                        id
                    );
                }
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

impl SQLiteDatabase {
    async fn insert_book_async(
        &mut self,
        book: RawBook,
    ) -> Result<BookID, <Self as AppDatabase>::Error> {
        let ids = self.insert_books_async(vec![book], 1).await?;
        Ok(ids[0])
    }

    async fn insert_books_async(
        &mut self,
        books: Vec<RawBook>,
        transaction_size: usize,
    ) -> Result<Vec<BookID>, <Self as AppDatabase>::Error> {
        let mut ids = Vec::with_capacity(books.len());
        let len = books.len();
        let mut book_iter = books.into_iter();
        let mut index = 0;
        while index != len {
            let mut tx = self.backend.begin().await?;
            for book in book_iter.by_ref().take(transaction_size) {
                index += 1;
                let title = book.title.as_ref();
                let (series, series_index) = match book.series.as_ref() {
                    None => (None, None),
                    Some((series, series_index)) => (Some(series), series_index.clone()),
                };

                let id = sqlx::query!(
                    "INSERT into books (title, series_name, series_id) VALUES(?, ?, ?)",
                    title,
                    series,
                    series_index
                )
                .execute(&mut tx)
                .await?
                .last_insert_rowid();

                let variants = book.get_variants();
                if !variants.is_empty() {
                    for variant in variants {
                        let book_type = ron::to_string(variant.book_type())
                            .expect("Serialization of value should never fail.");
                        #[cfg(unix)]
                        let path = variant.path().as_os_str().as_bytes();
                        #[cfg(windows)]
                        let path = v16_to_v8(variant.path().as_os_str().encode_wide().collect());
                        let local_title = &variant.local_title;
                        let identifier = variant.identifier.as_ref().map(|i| {
                            ron::to_string(i).expect("Serialization of value should never fail.")
                        });
                        let language = &variant.language;
                        let description = &variant.description;
                        let sub_id = &variant.id;
                        sqlx::query!(
                            "INSERT into variants (book_type, path, local_title, identifier, language, description, id, book_id) VALUES(?, ?, ?, ?, ?, ?, ?, ?)",
                            book_type,
                            path,
                            local_title,
                            identifier,
                            language,
                            description,
                            sub_id,
                            id
                        ).execute(&mut tx).await?;
                    }
                }

                let tags = book.get_extended_columns();
                if !tags.is_empty() {
                    let mut query = String::from(
                        "INSERT INTO extended_tags (tag_name, tag_value, book_id) VALUES",
                    );
                    query.extend(tags.iter().map(|(tag_name, tag_value)| {
                        format!("({}, {}, {}),", tag_name, tag_value, id)
                    }));
                    query.pop();
                    query.push(';');
                    sqlx::query(&query).execute(&mut tx).await?;
                }

                let id = BookID::try_from(id as u64)
                    .expect("SQLite database should never return NULL ID from primary key.");
                self.local_cache
                    .insert_book_with_id(Book::from_raw_book(id, book));

                ids.push(id);
            }
            tx.commit().await?;
        }
        Ok(ids)
    }

    async fn clear_db_async(&mut self) -> Result<(), sqlx::Error> {
        let mut tx = self.backend.begin().await?;
        sqlx::query!("DELETE FROM extended_tags")
            .execute(&mut tx)
            .await?;
        sqlx::query!("DELETE FROM variants")
            .execute(&mut tx)
            .await?;
        sqlx::query!("DELETE FROM books").execute(&mut tx).await?;
        tx.commit().await?;
        Ok(())
    }

    async fn merge_by_ids(&mut self, merges: &[(BookID, BookID)]) -> Result<(), sqlx::Error> {
        let mut tx = self.backend.begin().await?;
        for (merged_into, merged_from) in merges.iter().cloned() {
            let merged_into = u64::from(merged_into) as i64;
            let merged_from = u64::from(merged_from) as i64;

            sqlx::query!(
                "UPDATE extended_tags SET book_id = ? WHERE book_id = ?",
                merged_into,
                merged_from
            )
            .execute(&mut tx)
            .await?;
            sqlx::query!(
                "UPDATE variants SET book_id = ? WHERE book_id = ?",
                merged_into,
                merged_from
            )
            .execute(&mut tx)
            .await?;
        }
        tx.commit().await
    }

    async fn edit_book_by_id_async<S0: AsRef<str>, S1: AsRef<str>>(
        &mut self,
        id: BookID,
        edits: &[(S0, S1)],
    ) -> Result<(), DatabaseError<<Self as AppDatabase>::Error>> {
        if !self.local_cache.edit_book_with_id(id, &edits)? {
            return Err(DatabaseError::BookNotFound(id));
        }
        let mut tx = self.backend.begin().await.map_err(DatabaseError::Backend)?;

        let idx = u64::from(id) as i64;
        for (column, new_value) in edits {
            let new_value = new_value.as_ref();
            match column.as_ref().into() {
                ColumnIdentifier::Title => {
                    sqlx::query!(
                        "UPDATE books SET title = ? WHERE book_id = ?;",
                        new_value,
                        idx
                    )
                    .execute(&mut tx)
                    .await
                    .map_err(DatabaseError::Backend)?;
                }
                ColumnIdentifier::Author => {
                    sqlx::query!(
                    "INSERT INTO extended_tags (tag_name, tag_value, book_id) VALUES('author', ?, ?);",
                    new_value,
                    idx
                ).execute(&mut tx).await.map_err(DatabaseError::Backend)?;
                }
                ColumnIdentifier::Series => {
                    let series = str_to_series(new_value);
                    let (series, series_index) = match series.as_ref() {
                        None => (None, None),
                        Some((series, series_index)) => (Some(series), series_index.clone()),
                    };

                    sqlx::query!(
                        "UPDATE books SET series_name = ?, series_id = ? WHERE book_id = ?",
                        series,
                        series_index,
                        idx
                    )
                    .execute(&mut tx)
                    .await
                    .map_err(DatabaseError::Backend)?;
                }
                ColumnIdentifier::ID => {
                    unreachable!(
                        "id is immutable, and this case is reached when local cache is modified"
                    );
                }
                ColumnIdentifier::Variants => {
                    sqlx::query!(
                        "UPDATE books SET title = ? WHERE book_id = ?",
                        new_value,
                        idx
                    )
                    .execute(&mut tx)
                    .await
                    .map_err(DatabaseError::Backend)?;
                }
                ColumnIdentifier::Description => {
                    sqlx::query!(
                        "UPDATE variants SET description = ? WHERE book_id = ?",
                        new_value,
                        idx
                    )
                    .execute(&mut tx)
                    .await
                    .map_err(DatabaseError::Backend)?;
                }
                ColumnIdentifier::ExtendedTag(t) => {
                    sqlx::query!(
                        "INSERT into extended_tags (tag_name, tag_value, book_id) VALUES(?, ?, ?)",
                        t,
                        new_value,
                        idx
                    )
                    .execute(&mut tx)
                    .await
                    .map_err(DatabaseError::Backend)?;
                }
            }
        }
        tx.commit().await.map_err(DatabaseError::Backend)
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
        let db_exists = file_path.as_ref().exists();
        if !db_exists {
            if let Some(path) = file_path.as_ref().parent() {
                std::fs::create_dir_all(path)?;
            }
        }
        let database = block_on(async {
            SqliteConnectOptions::new()
                .filename(&file_path)
                .create_if_missing(true)
                .connect()
                .await
        })
        .map_err(DatabaseError::Backend)?;

        let mut db = Self {
            backend: database,
            local_cache: BookMap::default(),
            path: file_path.as_ref().to_path_buf(),
        };

        execute_query_str!(db, CREATE_BOOKS)?;
        execute_query_str!(db, CREATE_EXTENDED_TAGS)?;
        execute_query_str!(db, CREATE_VARIANTS)?;
        if db_exists {
            db.load_books()?;
        }
        Ok(db)
    }

    fn path(&self) -> &Path {
        self.path.as_path()
    }

    fn save(&mut self) -> Result<(), DatabaseError<Self::Error>> {
        Ok(())
    }

    fn insert_book(&mut self, book: RawBook) -> Result<BookID, DatabaseError<Self::Error>> {
        block_on(self.insert_book_async(book)).map_err(DatabaseError::Backend)
    }

    fn insert_books(
        &mut self,
        books: Vec<RawBook>,
    ) -> Result<Vec<BookID>, DatabaseError<Self::Error>> {
        block_on(self.insert_books_async(books, 5000)).map_err(DatabaseError::Backend)
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
        // execute_query!(self, "DELETE FROM extended_tags")?;
        // execute_query!(self, "DELETE FROM variants")?;
        // execute_query!(self, "DELETE FROM books")?;
        block_on(self.clear_db_async()).map_err(DatabaseError::Backend)?;
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
        edits: &[(S0, S1)],
    ) -> Result<(), DatabaseError<Self::Error>> {
        // "UPDATE {} SET {} = {} WHERE book_id = {};"
        block_on(self.edit_book_by_id_async(id, edits))
    }

    fn merge_similar(&mut self) -> Result<HashSet<BookID>, DatabaseError<Self::Error>> {
        // SELECT title, book_id FROM books GROUP BY LOWER(title) HAVING COUNT(*) > 1;
        // Then, for authors ??
        let merged = self.local_cache.merge_similar_merge_ids();
        block_on(self.merge_by_ids(&merged)).map_err(DatabaseError::Backend)?;
        let to_remove = merged.into_iter().map(|(_, m)| m).collect();
        self.remove_books(&to_remove)?;
        Ok(to_remove)
    }

    fn find_matches(
        &self,
        searches: &[Search],
    ) -> Result<Vec<Arc<RwLock<Book>>>, DatabaseError<Self::Error>> {
        // "SELECT * FROM {} WHERE {} REGEXP {};"
        // FIND_MATCHES_* - look at SQLite FTS5.
        Ok(self.local_cache.find_matches(searches)?)
    }

    fn find_book_index(
        &self,
        searches: &[Search],
    ) -> Result<Option<usize>, DatabaseError<Self::Error>> {
        Ok(self.local_cache.find_book_index(searches)?)
    }

    fn sort_books_by_cols<S: AsRef<str>>(
        &mut self,
        columns: &[(S, bool)],
    ) -> Result<(), DatabaseError<Self::Error>> {
        self.local_cache.sort_books_by_cols(columns);
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
        // NOTE: The query below would require a paginated search.
        //  Jumping to end is possible with the reverse search pattern,
        //  BUT: for searches, finding all results can be expensive
        // SELECT * FROM books WHERE book_id > last_id ORDER BY {} [} LIMIT {};
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
        // "DELETE FROM books WHERE book_id > last_id ORDER BY {} {} LIMIT 1"
        // NOTE: remove_book_indexed is for removing a selected book -
        // a book can only be selected if it is already loaded - eg. in cache.
        // "DELETE FROM books WHERE book_id = {}"

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
        edits: &[(S0, S1)],
    ) -> Result<(), DatabaseError<Self::Error>> {
        // "UPDATE {} SET {} = {} WHERE book_id > last_id ORDER BY {} {} LIMIT 1"
        // NOTE: edit_book_indexed is for editing a selected book -
        // a book can only be selected if it is already loaded - eg. in cache.

        let book = self
            .local_cache
            .get_book_indexed(index)
            .ok_or(DatabaseError::IndexOutOfBounds(index))?;
        let book = book!(book);
        self.edit_book_with_id(book.get_id(), edits)
    }
}
