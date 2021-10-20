use std::collections::{HashMap, HashSet};
use std::convert::{TryFrom, TryInto};
use std::ffi::OsString;
use std::fmt::Formatter;
use std::num::NonZeroU64;
#[cfg(unix)]
use std::os::unix::ffi::{OsStrExt, OsStringExt};
#[cfg(windows)]
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;

use async_trait::async_trait;
use itertools::Itertools;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Sqlite, SqlitePool, Transaction};
use tokio::sync::RwLock;
use unicase::UniCase;

use bookworm_input::Edit;
use bookworm_records::book::{BookID, ColumnIdentifier};
use bookworm_records::series::Series;
use bookworm_records::{Book, BookVariant, Edit as BEdit};

use crate::cache::BookCache;
use crate::paginator::{QueryBuilder, Selection, Variable};
use crate::{AppDatabase, DatabaseError};

// CREATE VIRTUAL TABLE table_fts USING FTS5 (
//     fields,
//     content="items"
// );
// INSERT INTO table_fts (fields)
//     SELECT fields FROM table;
//
// SELECT rowid, * FROM table_fts WHERE table_fts MATCH ?;
// CREATE TRIGGER notes_fts_before_update BEFORE UPDATE ON notes BEGIN
//     DELETE FROM notes_fts WHERE docid=old.rowid;
// END
//
// CREATE TRIGGER notes_fts_before_delete BEFORE DELETE ON notes BEGIN
//     DELETE FROM notes_fts WHERE docid=old.rowid;
// END
//
// CREATE TRIGGER notes_after_update AFTER UPDATE ON notes BEGIN
//     INSERT INTO notes_fts(docid, id, title, body) SELECT rowid, id, title, body FROM notes WHERE is_conflict = 0 AND encryption_applied = 0 AND new.rowid = notes.rowid;
// END
//
// CREATE TRIGGER notes_after_insert AFTER INSERT ON notes BEGIN
//     INSERT INTO notes_fts(docid, id, title, body) SELECT rowid, id, title, body FROM notes WHERE is_conflict = 0 AND encryption_applied = 0 AND new.rowid = notes.rowid;
// END
// TODO: Index for title, named_tags, min of multimap_tag
/// Top level book metadata
const CREATE_BOOKS: &str = r#"CREATE TABLE IF NOT EXISTS `books` (
`book_id` INTEGER NOT NULL PRIMARY KEY,
`title` TEXT DEFAULT NULL,
`series_name` TEXT DEFAULT NULL,
`series_id` REAL DEFAULT NULL
);"#;

/// Tags for books with a particular name and single value
const CREATE_NAMED_TAGS: &str = r#"CREATE TABLE IF NOT EXISTS `named_tags` (
`name` TEXT NOT NULL,
`value` TEXT NOT NULL,
`book_id` INTEGER NOT NULL,
UNIQUE(`name`, `book_id`),
FOREIGN KEY(book_id) REFERENCES books(book_id)
    ON UPDATE CASCADE
    ON DELETE CASCADE
);"#;

/// Tags without associated value
const CREATE_FREE_TAGS: &str = r#"CREATE TABLE IF NOT EXISTS `free_tags` (
`value` TEXT NOT NULL,
`book_id` INTEGER NOT NULL,
UNIQUE(`value`, `book_id`),
FOREIGN KEY(book_id) REFERENCES books(book_id)
    ON UPDATE CASCADE
    ON DELETE CASCADE
);"#;

/// Tags which map to multiple values
const CREATE_MULTIMAP_TAGS: &str = r#"CREATE TABLE IF NOT EXISTS `multimap_tags` (
`name` TEXT NOT NULL,
`value` TEXT NOT NULL,
`book_id` INTEGER NOT NULL,
UNIQUE(`name`, `value`, `book_id`),
FOREIGN KEY(book_id) REFERENCES books(book_id)
    ON UPDATE CASCADE
    ON DELETE CASCADE
);"#;

/// Variant metadata - each book can have multiple variants
const CREATE_VARIANTS: &str = r#"CREATE TABLE IF NOT EXISTS `variants` (
`book_type` TEXT NOT NULL,
`path` BLOB NOT NULL,
`local_title` TEXT DEFAULT NULL,
`identifier` TEXT DEFAULT NULL,
`language` TEXT DEFAULT NULL,
`description` TEXT DEFAULT NULL,
`id` INTEGER DEFAULT NULL,
`hash` BLOB NOT NULL,
`file_size` INTEGER NOT NULL,
`book_id` INTEGER NOT NULL,
FOREIGN KEY(book_id) REFERENCES books(book_id)
    ON UPDATE CASCADE
    ON DELETE CASCADE
);"#;

#[derive(sqlx::FromRow)]
struct BookData {
    book_id: i64,
    title: Option<String>,
    series_name: Option<String>,
    series_id: Option<f32>,
}

#[derive(sqlx::FromRow)]
struct VariantData {
    book_id: i64,
    book_type: String,
    path: Vec<u8>,
    local_title: Option<String>,
    language: Option<String>,
    identifier: Option<String>,
    description: Option<String>,
    id: Option<i64>,
    file_size: i64,
    hash: Vec<u8>,
}

#[derive(sqlx::FromRow)]
struct NamedTagData {
    name: String,
    value: String,
    book_id: i64,
}

#[derive(sqlx::FromRow)]
struct FreeTagData {
    value: String,
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

#[derive(Debug)]
enum DataError {
    NullPrimaryID,
    Serialize(String),
}

impl std::error::Error for DataError {}

impl std::fmt::Display for DataError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            DataError::NullPrimaryID => f.write_str("id was null - SQLite database corrupted?"),
            DataError::Serialize(reason) => f.write_str(reason),
        }
    }
}

impl TryFrom<BookData> for Book {
    type Error = DataError;

    fn try_from(bd: BookData) -> Result<Self, Self::Error> {
        Ok(Book {
            id: if let Ok(id) = NonZeroU64::try_from(bd.book_id as u64) {
                Some(id)
            } else {
                return Err(DataError::NullPrimaryID);
            },
            title: bd.title.clone(),
            series: {
                let series_id = bd.series_id;
                bd.series_name.map(|sn| Series {
                    name: sn,
                    index: series_id,
                })
            },
            ..Default::default()
        })
    }
}

impl TryFrom<VariantData> for BookVariant {
    type Error = DataError;

    fn try_from(vd: VariantData) -> Result<Self, Self::Error> {
        Ok(BookVariant {
            book_type: ron::from_str(&vd.book_type).map_err(|_| {
                DataError::Serialize(format!("Failed to parse book type: {}", vd.book_type))
            })?,
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
            hash: {
                let len = vd.hash.len();
                vd.hash.try_into().map_err(|_| {
                    DataError::Serialize(format!("Got {} byte hash, expected 32 byte hash", len))
                })?
            },
            file_size: vd.file_size as u64,
            free_tags: HashSet::new(),
            named_tags: HashMap::new(),
        })
    }
}

pub struct SQLiteDatabase {
    connection: SqlitePool,
    cache: Arc<RwLock<BookCache>>,
    path: PathBuf,
}

// TODO: Measure performance of deletion with current changes, and check if issues are still present.
impl SQLiteDatabase {
    #[tracing::instrument(name = "Reading the ids of books matching the query", skip(self))]
    async fn read_book_ids(
        &self,
        query: &str,
        bound_variables: &[Variable],
    ) -> Result<Vec<BookID>, DatabaseError<<Self as AppDatabase>::Error>> {
        #[derive(sqlx::FromRow, Debug)]
        struct SqlxBookId {
            book_id: i64,
        }

        let mut query = sqlx::query_as(&query);
        for value in bound_variables {
            query = match value {
                Variable::Int(i) => query.bind(i),
                Variable::Str(s) => query.bind(s),
            };
        }
        let start = std::time::Instant::now();
        let ids: Vec<SqlxBookId> = query
            .fetch_all(&self.connection)
            .await
            .map_err(DatabaseError::Backend)?;
        let ids: Vec<BookID> = ids
            .into_iter()
            .map(|id| {
                BookID::try_from(id.book_id as u64).expect("book_id is specified to be non-null.")
            })
            .collect();
        let end = std::time::Instant::now();
        tracing::info!("Took {}s to read ids", (end - start).as_secs_f32());
        Ok(ids)
    }

    async fn deduplicate(
        &mut self,
        merges: &[(BookID, BookID)],
    ) -> Result<(), DatabaseError<<Self as AppDatabase>::Error>> {
        enum ConflictResolution {
            Skip,
            KeepOriginal,
            Return,
        }

        enum DetectionStrategy {
            VariantHash,
            AuthorOverlap,
            Title,
        }

        struct MergeConflict {
            book1: Arc<Book>,
            book2: Arc<Book>,
            conflicts: Vec<ColumnIdentifier>,
        }
        let conflict = ConflictResolution::KeepOriginal;
        let detection = DetectionStrategy::VariantHash;

        // let conflicts = vec![];

        let books = self
            .read_selected_books(
                "SELECT book_id
            FROM `variants`
            GROUP BY hash
            HAVING COUNT(*) > 1",
                &[],
            )
            .await?;
        // titles, authors
        // variant by variant: identical hashmaps
        // SELECT hash
        // FROM variants
        // GROUP BY hash
        // HAVING COUNT(*) > 1
        // ORDER BY hash DESC;
        Ok(())
    }

    #[tracing::instrument(
        name = "Converting SQLite data to Book records",
        skip(
            raw_books,
            raw_variants,
            raw_named_tags,
            raw_free_tags,
            raw_multimap_tags
        )
    )]
    fn books_from_sql(
        raw_books: Vec<BookData>,
        raw_variants: Vec<VariantData>,
        raw_named_tags: Vec<NamedTagData>,
        raw_free_tags: Vec<FreeTagData>,
        raw_multimap_tags: Vec<NamedTagData>,
    ) -> (Vec<(BookID, Arc<Book>)>, HashSet<UniCase<String>>) {
        let mut books: HashMap<_, _> = raw_books
            .into_iter()
            .filter_map(|book_data: BookData| match Book::try_from(book_data) {
                Ok(book) => Some((book.id(), book)),
                Err(e) => {
                    tracing::error!("Could not transform SQLite record into book: {}", e);
                    None
                }
            })
            .collect();

        for variant in raw_variants.into_iter() {
            let id = NonZeroU64::try_from(variant.book_id as u64).expect("book_id is non-null");
            let variant = match BookVariant::try_from(variant) {
                Ok(variant) => variant,
                Err(e) => {
                    tracing::error!("Could not transform SQLite record into book variant: {}", e);
                    continue;
                }
            };

            if let Some(book) = books.get_mut(&id) {
                book.push_variant(variant);
            } else {
                tracing::error!("Found orphan variant for ID {} while loading books.", id);
            }
        }

        // To get all columns: "SELECT DISTINCT tag_name FROM extended_tags"
        let mut prime_cols = HashSet::new();
        for &col in &["title", "authors", "series", "id", "description"] {
            prime_cols.insert(col.to_owned());
        }

        for tag in raw_named_tags.into_iter() {
            let id = BookID::try_from(tag.book_id as u64).expect("book_id is non-null");
            match books.get_mut(&id) {
                None => {
                    tracing::error!("Found orphan tag for ID {} while loading books.", id);
                }
                Some(book) => {
                    if !prime_cols.contains(&tag.name) {
                        prime_cols.insert(tag.name.clone());
                    }

                    book.extend_column(&ColumnIdentifier::NamedTag(tag.name), tag.value)
                        .expect("Inserting tags is infallible");
                }
            }
        }

        for tag in raw_free_tags.into_iter() {
            let id = BookID::try_from(tag.book_id as u64).expect("book_id is non-null");
            match books.get_mut(&id) {
                None => {
                    tracing::error!("Found orphan tag for ID {} while loading books.", id);
                }
                Some(book) => {
                    book.extend_column(&ColumnIdentifier::Tags, tag.value)
                        .unwrap();
                }
            }
        }

        for tag in raw_multimap_tags.into_iter() {
            let id = BookID::try_from(tag.book_id as u64).expect("book_id is non-null");
            match books.get_mut(&id) {
                None => {
                    tracing::error!("Found orphan tag for ID {} while loading books.", id);
                }
                Some(book) => match tag.name.as_str() {
                    "author" => book
                        .extend_column(&ColumnIdentifier::Author, tag.value)
                        .expect("Extending author is infallible."),
                    name => {
                        if !prime_cols.contains(name) {
                            prime_cols.insert(name.to_string());
                        }
                        book.extend_column(
                            &ColumnIdentifier::MultiMap(name.to_string()),
                            tag.value,
                        )
                        .expect("Extending multimap is infallible");
                    }
                },
            }
        }

        (
            books.into_iter().map(|(a, b)| (a, Arc::new(b))).collect(),
            prime_cols.into_iter().map(UniCase::new).collect(),
        )
    }

    #[tracing::instrument(name = "Loading books from the database", skip(self))]
    /// NOTE: The performance of this has been scrutinized to some degree.
    /// tokio::join_all offers a substantial improvement in performance (>10x), and
    /// scrolling is significantly smoother.
    ///
    /// Converting books is 10x faster than reading them from SQLite:
    /// ```bash
    ///    >>> Took 0.00082799s to read 19 books from SQLite
    ///    >>> Took 0.000071805s to convert books
    /// ```
    ///
    /// In the future, it would be nice to resolve minor hiccups when rapidly scrolling
    async fn read_books_from_sql(
        &self,
        ids: &[BookID],
    ) -> Result<
        (Vec<(BookID, Arc<Book>)>, HashSet<UniCase<String>>),
        DatabaseError<<SQLiteDatabase as AppDatabase>::Error>,
    > {
        if ids.is_empty() {
            return Ok((Default::default(), Default::default()));
        };
        let where_ = format!("IN ({})", ids.iter().map(|id| id.to_string()).join(", "));
        let raw_book_query = format!("SELECT * FROM books WHERE books.book_id {}", where_);
        let raw_variant_query = format!("SELECT * FROM variants WHERE variants.book_id {}", where_);
        let raw_named_tag_query = format!(
            "SELECT * FROM named_tags WHERE named_tags.book_id {}",
            where_
        );
        let raw_free_tag_query =
            format!("SELECT * FROM free_tags WHERE free_tags.book_id {}", where_);
        let raw_multimap_tag_query = format!(
            "SELECT * FROM multimap_tags WHERE multimap_tags.book_id {}",
            where_
        );

        let raw_books = sqlx::query_as(&raw_book_query).fetch_all(&self.connection);
        let raw_variants = sqlx::query_as(&raw_variant_query).fetch_all(&self.connection);
        let raw_named_tags = sqlx::query_as(&raw_named_tag_query).fetch_all(&self.connection);
        let raw_free_tags = sqlx::query_as(&raw_free_tag_query).fetch_all(&self.connection);
        let raw_multimap_tags = sqlx::query_as(&raw_multimap_tag_query).fetch_all(&self.connection);

        tracing::info!("Reading books from database");

        let start = std::time::Instant::now();

        let (raw_books, raw_variants, raw_named_tags, raw_free_tags, raw_multimap_tags) = tokio::join!(
            raw_books,
            raw_variants,
            raw_named_tags,
            raw_free_tags,
            raw_multimap_tags
        );

        tracing::info!("Received results");

        let (raw_books, raw_variants, raw_named_tags, raw_free_tags, raw_multimap_tags) = (
            raw_books.map_err(DatabaseError::Backend)?,
            raw_variants.map_err(DatabaseError::Backend)?,
            raw_named_tags.map_err(DatabaseError::Backend)?,
            raw_free_tags.map_err(DatabaseError::Backend)?,
            raw_multimap_tags.map_err(DatabaseError::Backend)?,
        );

        let end = std::time::Instant::now();
        tracing::info!(
            "Took {}s to read {} books from SQLite",
            (end - start).as_secs_f32(),
            raw_books.len(),
        );
        let start = std::time::Instant::now();

        let (new_books, columns) = SQLiteDatabase::books_from_sql(
            raw_books,
            raw_variants,
            raw_named_tags,
            raw_free_tags,
            raw_multimap_tags,
        );
        let end = std::time::Instant::now();
        tracing::info!("Took {}s to convert books", (end - start).as_secs_f32());

        Ok((new_books, columns))
    }

    async fn load_books(
        &mut self,
    ) -> Result<(), DatabaseError<<SQLiteDatabase as AppDatabase>::Error>> {
        let raw_books =
            sqlx::query_as!(BookData, "SELECT * FROM books").fetch_all(&self.connection);
        let raw_variants =
            sqlx::query_as!(VariantData, "SELECT * FROM variants").fetch_all(&self.connection);
        let raw_named_tags =
            sqlx::query_as!(NamedTagData, "SELECT * FROM named_tags").fetch_all(&self.connection);
        let raw_free_tags =
            sqlx::query_as!(FreeTagData, "SELECT * FROM free_tags").fetch_all(&self.connection);
        let raw_multimap_tags = sqlx::query_as!(NamedTagData, "SELECT * FROM multimap_tags")
            .fetch_all(&self.connection);

        let (raw_books, raw_variants, raw_named_tags, raw_free_tags, raw_multimap_tags) = tokio::join!(
            raw_books,
            raw_variants,
            raw_named_tags,
            raw_free_tags,
            raw_multimap_tags
        );

        let (raw_books, raw_variants, raw_named_tags, raw_free_tags, raw_multimap_tags) = (
            raw_books.map_err(DatabaseError::Backend)?,
            raw_variants.map_err(DatabaseError::Backend)?,
            raw_named_tags.map_err(DatabaseError::Backend)?,
            raw_free_tags.map_err(DatabaseError::Backend)?,
            raw_multimap_tags.map_err(DatabaseError::Backend)?,
        );

        let (books, columns) = SQLiteDatabase::books_from_sql(
            raw_books,
            raw_variants,
            raw_named_tags,
            raw_free_tags,
            raw_multimap_tags,
        );

        self.cache = Arc::new(RwLock::new(BookCache::from_values_unchecked(
            books.into_iter().collect(),
            columns,
        )));
        Ok(())
    }
}

#[tracing::instrument(name="Modifying book", skip(tx, book, edits), fields(id = %book.id()))]
async fn edit_book<'a>(
    mut tx: Transaction<'a, Sqlite>,
    book: &mut Book,
    edits: &[(ColumnIdentifier, Edit)],
) -> Result<Transaction<'a, Sqlite>, DatabaseError<<SQLiteDatabase as AppDatabase>::Error>> {
    let id = book.id();
    let book_id = u64::from(id) as i64;
    for (column, edit) in edits {
        if matches!(column, ColumnIdentifier::ID | ColumnIdentifier::Variants) {
            tracing::error!("Attempted to edit immutable field (one of ID, Variants)");
            continue;
        }

        let edit = match edit {
            Edit::Delete => BEdit::Delete,
            Edit::Replace(s) => BEdit::Replace(s.to_string()),
            Edit::Append(s) => BEdit::Append(s.to_string()),
            Edit::Sequence(seq) => {
                if let Some(col) = book.get_column(column) {
                    BEdit::Replace(seq.apply_to(&col).render())
                } else {
                    continue;
                }
            }
        };

        match &edit {
            BEdit::Delete => {
                match column {
                    ColumnIdentifier::Title => {
                        sqlx::query!("UPDATE books SET title = null WHERE book_id = ?;", book_id)
                    }
                    ColumnIdentifier::Author => sqlx::query!(
                        "DELETE FROM multimap_tags where book_id = ? AND name = 'author';",
                        book_id
                    ),
                    ColumnIdentifier::ID => unreachable!(),
                    ColumnIdentifier::Series => sqlx::query!(
                        "UPDATE books SET series_name = null, series_id = null WHERE book_id = ?;",
                        book_id
                    ),
                    ColumnIdentifier::Variants => unreachable!(),
                    ColumnIdentifier::Description => sqlx::query!(
                        "UPDATE variants SET description = null WHERE book_id = ?;",
                        book_id
                    ),
                    ColumnIdentifier::NamedTag(column) => {
                        sqlx::query!(
                            "DELETE FROM named_tags where book_id = ? and name = ?;",
                            book_id,
                            column
                        )
                        .execute(&mut tx)
                        .await
                        .map_err(DatabaseError::Backend)?;
                        continue;
                    }
                    ColumnIdentifier::ExactTag(tag) => {
                        sqlx::query!(
                            "DELETE FROM free_tags where book_id = ? AND value = ?;",
                            book_id,
                            tag
                        )
                        .execute(&mut tx)
                        .await
                        .map_err(DatabaseError::Backend)?;
                        continue;
                    }
                    ColumnIdentifier::MultiMap(_) | ColumnIdentifier::MultiMapExact(_, _) => {
                        unimplemented!("Deleting multimap tags not supported.")
                    }
                    ColumnIdentifier::Tags => {
                        sqlx::query!("DELETE FROM free_tags where book_id = ?;", book_id,)
                    }
                }
                .execute(&mut tx)
                .await
                .map_err(DatabaseError::Backend)?;
            }
            BEdit::Replace(value) => match column {
                ColumnIdentifier::Title => {
                    sqlx::query!(
                        "UPDATE books SET title = ? WHERE book_id = ?;",
                        value,
                        book_id
                    )
                    .execute(&mut tx)
                    .await
                    .map_err(DatabaseError::Backend)?;
                }
                ColumnIdentifier::Author => {
                    sqlx::query!(
                        "INSERT INTO multimap_tags (name, value, book_id) VALUES('author', ?, ?);",
                        value,
                        book_id
                    )
                    .execute(&mut tx)
                    .await
                    .map_err(DatabaseError::Backend)?;
                }
                ColumnIdentifier::Series => {
                    let series = Series::from_str(value).ok();
                    let (series, series_index) = match series {
                        None => (None, None),
                        Some(Series { name, index }) => (Some(name), index),
                    };
                    sqlx::query!(
                        "UPDATE books SET series_name = ?, series_id = ? WHERE book_id = ?",
                        series,
                        series_index,
                        book_id
                    )
                    .execute(&mut tx)
                    .await
                    .map_err(DatabaseError::Backend)?;
                }
                ColumnIdentifier::ID => unreachable!(),
                ColumnIdentifier::Variants => unreachable!(),
                ColumnIdentifier::Description => {
                    sqlx::query!(
                        "UPDATE variants SET description = ? WHERE book_id = ?",
                        value,
                        book_id
                    )
                    .execute(&mut tx)
                    .await
                    .map_err(DatabaseError::Backend)?;
                }
                ColumnIdentifier::Tags => {
                    sqlx::query!(
                        "INSERT into free_tags (value, book_id) VALUES(?, ?)",
                        value,
                        book_id
                    )
                    .execute(&mut tx)
                    .await
                    .map_err(DatabaseError::Backend)?;
                }
                ColumnIdentifier::NamedTag(column) => {
                    sqlx::query!(
                        "INSERT into named_tags (name, value, book_id) VALUES(?, ?, ?)",
                        column,
                        value,
                        book_id
                    )
                    .execute(&mut tx)
                    .await
                    .map_err(DatabaseError::Backend)?;
                }
                ColumnIdentifier::ExactTag(tag) => {
                    sqlx::query!(
                            "DELETE FROM free_tags WHERE value = ? AND book_id = ?; INSERT into free_tags (value, book_id) VALUES(?, ?)",
                            tag,
                            book_id,
                            value,
                            book_id
                        )
                            .execute(&mut tx)
                            .await
                            .map_err(DatabaseError::Backend)?;
                }
                ColumnIdentifier::MultiMap(_) | ColumnIdentifier::MultiMapExact(_, _) => {
                    unimplemented!("Replacing multimap tags not supported.")
                }
            },
            BEdit::Append(value) => match column {
                ColumnIdentifier::Title => {
                    sqlx::query!(
                        "UPDATE books SET title = title || ? WHERE book_id = ?;",
                        value,
                        book_id
                    )
                    .execute(&mut tx)
                    .await
                    .map_err(DatabaseError::Backend)?;
                }
                ColumnIdentifier::Author => {
                    sqlx::query!(
                        "INSERT INTO multimap_tags (name, value, book_id) VALUES('author', ?, ?);",
                        value,
                        book_id
                    )
                    .execute(&mut tx)
                    .await
                    .map_err(DatabaseError::Backend)?;
                }
                ColumnIdentifier::Series => {
                    unreachable!("book should reject concatenating to series");
                }
                ColumnIdentifier::ID => unreachable!(),
                ColumnIdentifier::Variants => unreachable!(),
                ColumnIdentifier::Description => {
                    sqlx::query!(
                        "UPDATE variants SET description = description || ? WHERE book_id = ?",
                        value,
                        book_id
                    )
                    .execute(&mut tx)
                    .await
                    .map_err(DatabaseError::Backend)?;
                }
                ColumnIdentifier::Tags => {
                    sqlx::query!(
                        "INSERT into free_tags (value, book_id) VALUES(?, ?)",
                        value,
                        book_id
                    )
                    .execute(&mut tx)
                    .await
                    .map_err(DatabaseError::Backend)?;
                }
                ColumnIdentifier::NamedTag(column) => {
                    sqlx::query!(
                            "INSERT OR REPLACE INTO named_tags (name, value, book_id) VALUES(?, \
                            COALESCE((SELECT value from named_tags where name = ? AND book_id = ?), \'\') || ?, ?)",
                            column,
                            column,
                            book_id,
                            value,
                            book_id
                        )
                            .execute(&mut tx)
                            .await
                            .map_err(DatabaseError::Backend)?;
                }
                ColumnIdentifier::ExactTag(tag) => {
                    let new_tag = tag.to_owned() + value;
                    sqlx::query!(
                            "DELETE FROM free_tags where value = ? AND book_id = ?; INSERT into free_tags (value, book_id) VALUES(?, ?)",
                            tag,
                            book_id,
                            new_tag,
                            book_id
                        )
                            .execute(&mut tx)
                            .await
                            .map_err(DatabaseError::Backend)?;
                }
                ColumnIdentifier::MultiMap(_) | ColumnIdentifier::MultiMapExact(_, _) => {
                    unimplemented!("Appending to multimap tags not supported.")
                }
            },
        }

        book.edit_column(column, edit)
            .map_err(DatabaseError::Record)?;
    }
    Ok(tx)
}

impl SQLiteDatabase {
    async fn insert_books_async<I: Iterator<Item = BookVariant> + Send>(
        &mut self,
        books: I,
        transaction_size: usize,
    ) -> Result<Vec<BookID>, <Self as AppDatabase>::Error> {
        let mut book_iter = books.into_iter().peekable();
        let mut ids = Vec::with_capacity({
            let (low, high) = book_iter.size_hint();
            high.unwrap_or(low)
        });

        while book_iter.peek().is_some() {
            let mut tx = self.connection.begin().await?;
            for variant in book_iter.by_ref().take(transaction_size) {
                let variant: BookVariant = variant;
                let title = variant.local_title.as_ref();
                // let (series, series_index) = (None, None);
                // match book.get_series() {
                //     None => (None, None),
                //     Some(series) => (Some(&series.name), series.index.clone()),
                // };

                // let id = sqlx::query!(
                //     "INSERT into books (title, series_name, series_id) VALUES(?, ?, ?)",
                //     title,
                //     series,
                //     series_index
                // )
                // .execute(&mut tx)
                // .await?
                // .last_insert_rowid();

                let id = sqlx::query!("INSERT into books (title) VALUES(?)", title)
                    .execute(&mut tx)
                    .await?
                    .last_insert_rowid();

                let book_type = ron::to_string(variant.book_type())
                    .expect("Serialization of value should never fail.");
                #[cfg(unix)]
                let path = variant.path().as_os_str().as_bytes();
                #[cfg(windows)]
                let path = v16_to_v8(variant.path().as_os_str().encode_wide().collect());
                let local_title = &variant.local_title;
                let identifier = variant
                    .identifier
                    .as_ref()
                    .map(|i| ron::to_string(i).expect("Serialization of value should never fail."));
                let language = &variant.language;
                let description = &variant.description;
                let sub_id = &variant.id;
                let hash = variant.hash.to_vec();
                let file_size = variant.file_size as i64;
                sqlx::query!(
                    "INSERT into variants (book_type, path, local_title, identifier, language, description, id, hash, file_size, book_id) VALUES(?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                    book_type,
                    path,
                    local_title,
                    identifier,
                    language,
                    description,
                    sub_id,
                    hash,
                    file_size,
                    id,
                ).execute(&mut tx).await?;

                for value in variant.free_tags.iter() {
                    sqlx::query!(
                        "INSERT INTO free_tags (value, book_id) VALUES(?, ?);",
                        value,
                        id
                    )
                    .execute(&mut tx)
                    .await?;
                }

                for (name, value) in variant.named_tags.iter() {
                    sqlx::query!(
                        "INSERT INTO named_tags (name, value, book_id) VALUES(?, ?, ?);",
                        name,
                        value,
                        id
                    )
                    .execute(&mut tx)
                    .await?;
                }

                if let Some(authors) = &variant.additional_authors {
                    for author in authors {
                        sqlx::query!("INSERT INTO multimap_tags (name, value, book_id) VALUES(\"author\", ?, ?);", author, id).execute(&mut tx).await?;
                    }
                }

                let id = BookID::try_from(id as u64)
                    .expect("SQLite database should never return NULL ID from primary key.");
                self.cache
                    .write()
                    .await
                    .insert_book(Arc::new(Book::from_variant(id, variant)));

                ids.push(id);
            }
            tx.commit().await?;
        }
        Ok(ids)
    }

    #[tracing::instrument(name = "Clearing database", skip(self))]
    async fn clear_db_async(&mut self) -> Result<(), sqlx::Error> {
        let mut tx = self.connection.begin().await?;
        sqlx::query!("DELETE FROM multimap_tags")
            .execute(&mut tx)
            .await?;
        sqlx::query!("DELETE FROM free_tags")
            .execute(&mut tx)
            .await?;
        sqlx::query!("DELETE FROM named_tags")
            .execute(&mut tx)
            .await?;
        sqlx::query!("DELETE FROM variants")
            .execute(&mut tx)
            .await?;
        sqlx::query!("DELETE FROM books").execute(&mut tx).await?;
        // When deleting all books, 100% should do a vacuum
        tx.commit().await?;
        tracing::info!("Initiating VACUUM");
        sqlx::query!("VACUUM").execute(&self.connection).await?;
        tracing::info!("VACUUM complete");
        Ok(())
    }

    async fn remove_books_async<I: Iterator<Item = BookID> + Send>(
        &mut self,
        merges: I,
    ) -> Result<(), sqlx::Error> {
        let mut tx = self.connection.begin().await?;
        let size = {
            let (low, high) = merges.size_hint();
            high.unwrap_or(low)
        };
        sqlx::query(&format!("PRAGMA cache_size = {}", size))
            .execute(&mut tx)
            .await?;
        let s = merges.map(|id| id.to_string()).join(", ");
        sqlx::query(&format!("DELETE FROM books WHERE book_id IN ({})", s))
            .execute(&mut tx)
            .await?;
        sqlx::query!("PRAGMA cache_size = 4096")
            .execute(&mut tx)
            .await?;
        tx.commit().await
    }

    async fn merge_by_ids(&mut self, merges: &[(BookID, BookID)]) -> Result<(), sqlx::Error> {
        // titles, authors
        // variant by variant: identical hashmaps
        let mut tx = self.connection.begin().await?;
        for (merged_into, merged_from) in merges.iter().cloned() {
            let merged_into = u64::from(merged_into) as i64;
            let merged_from = u64::from(merged_from) as i64;

            sqlx::query!(
                "UPDATE OR IGNORE multimap_tags SET book_id = ? WHERE book_id = ?",
                merged_into,
                merged_from
            )
            .execute(&mut tx)
            .await?;

            // NOTE: Deletes orphan multimap tags.
            sqlx::query!("DELETE FROM multimap_tags WHERE book_id = ?", merged_from)
                .execute(&mut tx)
                .await?;

            sqlx::query!(
                "UPDATE named_tags SET book_id = ? WHERE book_id = ?",
                merged_into,
                merged_from
            )
            .execute(&mut tx)
            .await?;

            sqlx::query!(
                "UPDATE free_tags SET book_id = ? WHERE book_id = ?",
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

    async fn edit_unique(
        &mut self,
        books: &HashMap<BookID, Arc<Book>>,
        edits: &[(ColumnIdentifier, Edit)],
        transaction_size: usize,
    ) -> Result<(), DatabaseError<<Self as AppDatabase>::Error>> {
        let mut tx = self
            .connection
            .begin()
            .await
            .map_err(DatabaseError::Backend)?;

        let mut num_transactions = 0;
        for mut book in books.values().cloned() {
            tx = edit_book(tx, Arc::make_mut(&mut book), edits).await?;
            self.cache.write().await.insert_book(book);
            // Commits transaction after a certain number of steps
            // to avoid data loss.
            if num_transactions == transaction_size {
                num_transactions = 0;
                tx.commit().await.map_err(DatabaseError::Backend)?;
                tx = self
                    .connection
                    .begin()
                    .await
                    .map_err(DatabaseError::Backend)?;
            }
            num_transactions += 1;
        }
        tx.commit().await.map_err(DatabaseError::Backend)?;
        Ok(())
    }
}

#[async_trait]
impl AppDatabase for SQLiteDatabase {
    type Error = sqlx::Error;

    #[tracing::instrument(
        name = "Opening a database from path",
        skip(file_path),
        fields(
            path=%file_path.as_ref().display()
        )
    )]
    async fn open<P>(file_path: P) -> Result<Self, DatabaseError<Self::Error>>
    where
        P: AsRef<Path> + Send + Sync,
        Self: Sized,
    {
        let db_exists = file_path.as_ref().exists();
        if !db_exists {
            if let Some(path) = file_path.as_ref().parent() {
                std::fs::create_dir_all(path)?;
            }
        }
        let database = SqlitePoolOptions::new()
            .connect_with(
                SqliteConnectOptions::new()
                    .filename(&file_path)
                    .create_if_missing(true),
            )
            .await
            .map_err(DatabaseError::Backend)?;

        let db = Self {
            connection: database,
            cache: Default::default(),
            path: file_path.as_ref().to_path_buf(),
        };

        tracing::info!("Creating core tables if they do not exist");
        for query in [
            CREATE_BOOKS,
            CREATE_FREE_TAGS,
            CREATE_NAMED_TAGS,
            CREATE_MULTIMAP_TAGS,
            CREATE_VARIANTS,
        ] {
            sqlx::query(query)
                .execute(&db.connection)
                .await
                .map_err(DatabaseError::Backend)?;
        }

        // TODO: Disable this when doing large writes.
        // NOTE: These indices are absolutely essential for fast scrolling
        tracing::info!("Creating indices over book_id");
        for table in [
            "books",
            "variants",
            "named_tags",
            "free_tags",
            "multimap_tags",
        ] {
            sqlx::query(&format!(
                "CREATE INDEX IF NOT EXISTS {}_ids on {}(book_id);",
                table, table
            ))
            .execute(&db.connection)
            .await
            .map_err(DatabaseError::Backend)?;
        }

        Ok(db)
    }

    fn path(&self) -> &Path {
        self.path.as_path()
    }

    async fn save(&mut self) -> Result<(), DatabaseError<Self::Error>> {
        Ok(())
    }

    async fn insert_book(
        &mut self,
        book: BookVariant,
    ) -> Result<BookID, DatabaseError<Self::Error>> {
        let ids = self
            .insert_books_async(std::iter::once(book), 1)
            .await
            .map_err(DatabaseError::Backend)?;
        Ok(ids[0])
    }

    async fn insert_books<I: Iterator<Item = BookVariant> + Send>(
        &mut self,
        books: I,
    ) -> Result<Vec<BookID>, DatabaseError<Self::Error>> {
        self.insert_books_async(books, 5000)
            .await
            .map_err(DatabaseError::Backend)
    }

    // TODO: In both remove_books and remove_selected,
    //  we should ensure that large deletes don't
    //  cause the DB to remain large.
    async fn remove_books(
        &mut self,
        ids: &HashSet<BookID>,
    ) -> Result<(), DatabaseError<Self::Error>> {
        self.cache.write().await.remove_books(ids);
        self.remove_books_async(ids.iter().cloned())
            .await
            .map_err(DatabaseError::Backend)
    }

    async fn remove_selected(
        &mut self,
        selected: &Selection,
    ) -> Result<(), DatabaseError<Self::Error>> {
        let (query, bound_variables) = match selected {
            Selection::All(matchers) => {
                if matchers.is_empty() {
                    return self.clear().await;
                } else {
                    QueryBuilder::default().join_cols(None, matchers)
                }
            }
            Selection::Partial(books, _) => {
                return self
                    .remove_books(&books.keys().cloned().collect::<HashSet<_>>())
                    .await;
            }
            Selection::Range(start, end, cmp_rules, _, match_rules) => QueryBuilder::default()
                .cmp_rules(cmp_rules)
                .include_id(true)
                .between_books(start, end, match_rules),
            Selection::Empty => {
                return Ok(());
            }
        };
        let books = self
            .read_selected_books(&query, &bound_variables)
            .await?
            .into_iter()
            .map(|x| x.id())
            .collect::<HashSet<_>>();
        self.remove_books(&books).await
    }

    async fn clear(&mut self) -> Result<(), DatabaseError<Self::Error>> {
        self.clear_db_async()
            .await
            .map_err(DatabaseError::Backend)?;
        self.cache.write().await.clear();
        Ok(())
    }

    async fn get_book(&self, id: BookID) -> Result<Arc<Book>, DatabaseError<Self::Error>> {
        // "SELECT * FROM books WHERE book_id = {id}"
        // If this is put in the match, the read lock will cause the process to hang
        self.get_books(&[id])
            .await?
            .remove(&id)
            .ok_or(DatabaseError::BookNotFound(id))
    }

    #[tracing::instrument(name = "Loading book IDs from disk or cache", skip(self))]
    /// Loads the books with the provided ids - either reading from the underlying database,
    /// or from the internal cache, on a book-by-book basis.
    async fn get_books(
        &self,
        ids: &[BookID],
    ) -> Result<HashMap<BookID, Arc<Book>>, DatabaseError<<SQLiteDatabase as AppDatabase>::Error>>
    {
        let mut books: HashMap<BookID, Option<Arc<Book>>> = self
            .cache
            .read()
            .await
            .get_books(ids)
            .into_iter()
            .zip(ids.iter().cloned())
            .map(|(book, id)| (id, book.cloned()))
            .collect();
        tracing::info!("Finishing loading all cached books");
        let ids: Vec<_> = books
            .iter()
            .filter(|(_, book)| book.is_none())
            .map(|(id, _)| *id)
            .collect();
        tracing::info!(
            "{} books were not available in cache - searching in DB",
            ids.len()
        );
        let (new_books, columns) = self.read_books_from_sql(&ids).await?;
        {
            let mut cache = self.cache.write().await;
            for book in new_books.iter().map(|(_, book)| book) {
                cache.insert_book(book.clone());
            }
            cache.insert_columns(columns);
        }
        new_books
            .into_iter()
            .for_each(|(id, book)| drop(books.insert(id, Some(book))));
        Ok(books
            .into_iter()
            .filter_map(|(id, book)| book.map(|book| (id, book)))
            .collect())
    }

    async fn read_selected_books(
        &self,
        query: &str,
        bound_variables: &[Variable],
    ) -> Result<Vec<Arc<Book>>, DatabaseError<Self::Error>> {
        let ids = self.read_book_ids(query, bound_variables).await?;
        let books = self.get_books(&ids).await?;

        Ok(ids
            .iter()
            .map(|id| {
                books
                    .get(id)
                    .expect("failed to load all books from SQLite")
                    .clone()
            })
            .collect())
    }

    async fn edit_book_with_id(
        &mut self,
        id: BookID,
        edits: &[(ColumnIdentifier, Edit)],
    ) -> Result<(), DatabaseError<Self::Error>> {
        let mut tx = self
            .connection
            .begin()
            .await
            .map_err(DatabaseError::Backend)?;

        let mut book = self
            .get_books(&[id])
            .await?
            .remove(&id)
            .ok_or(DatabaseError::BookNotFound(id))?;
        let mut book_ = Arc::make_mut(&mut book);
        let tx = edit_book(tx, book_, edits).await?;
        tx.commit().await.map_err(DatabaseError::Backend)?;
        self.cache.write().await.insert_book(book);
        Ok(())
    }

    async fn edit_selected(
        &mut self,
        selected: &Selection,
        edits: &[(ColumnIdentifier, Edit)],
    ) -> Result<(), DatabaseError<Self::Error>> {
        let (query, bound_variables) = match selected {
            Selection::All(matchers) => QueryBuilder::default().join_cols(None, matchers),
            Selection::Partial(books, _) => {
                self.edit_unique(books, edits, 5000).await?;
                return Ok(());
            }
            Selection::Range(start, end, cmp_rules, _, match_rules) => QueryBuilder::default()
                .cmp_rules(cmp_rules)
                .include_id(true)
                .between_books(start, end, match_rules),
            Selection::Empty => {
                return Ok(());
            }
        };
        let books = self
            .read_selected_books(&query, &bound_variables)
            .await?
            .into_iter()
            .map(|x| (x.id(), x))
            .collect::<HashMap<_, _>>();

        self.edit_unique(&books, edits, 5000).await
    }

    async fn merge_similar(&mut self) -> Result<HashSet<BookID>, DatabaseError<Self::Error>> {
        // SELECT title, book_id FROM books GROUP BY LOWER(title) HAVING COUNT(*) > 1;
        // Then, for authors ??
        // TODO: This isn't a particularly complete solution, and we should
        //  move to a more robust deduplication strategy with the possibility
        //  of user feedback.
        self.load_books().await?;
        let merged = self.cache.write().await.merge_similar_books();
        self.merge_by_ids(&merged)
            .await
            .map_err(DatabaseError::Backend)?;
        let to_remove = merged.into_iter().map(|(_, m)| m).collect();
        self.remove_books(&to_remove).await?;
        Ok(to_remove)
    }

    // TODO: has_column needs to check DB
    async fn has_column(&self, col: &UniCase<String>) -> Result<bool, DatabaseError<Self::Error>> {
        Ok(self.cache.read().await.has_column(col))
    }

    async fn saved(&self) -> bool {
        true
    }

    #[tracing::instrument(name = "Updating books from sources", skip(self, books))]
    async fn update<I: Iterator<Item = BookVariant> + Send>(
        &mut self,
        books: I,
    ) -> Result<(), DatabaseError<Self::Error>> {
        #[derive(sqlx::FromRow)]
        struct VariantMetadata {
            book_id: i64,
            id: Option<i64>,
            file_size: i64,
            hash: Vec<u8>,
        }

        let raw_variants: Vec<VariantMetadata> = sqlx::query_as!(
            VariantMetadata,
            "SELECT book_id, id, file_size, hash FROM variants"
        )
        .fetch_all(&self.connection)
        .await
        .map_err(DatabaseError::Backend)?;

        tracing::info!("Read {} hashes from disk", raw_variants.len());
        let target_hashmap: HashMap<(u64, [u8; 32]), (Option<u32>, BookID)> = raw_variants
            .into_iter()
            .map(|vm| {
                (
                    (vm.file_size as u64, {
                        let len = vm.hash.len();
                        vm.hash
                            .try_into()
                            .map_err(|_| {
                                DataError::Serialize(format!(
                                    "Got {} byte hash, expected 32 byte hash",
                                    len
                                ))
                            })
                            .unwrap()
                    }),
                    (
                        vm.id.map(|id| id as u32),
                        BookID::try_from(vm.book_id as u64).unwrap(),
                    ),
                )
            })
            .collect();

        sqlx::query!(
            "CREATE INDEX IF NOT EXISTS variant_hashes on variants(book_id, id, file_size, hash);"
        )
        .execute(&self.connection)
        .await
        .map_err(DatabaseError::Backend)?;

        tracing::info!("Created index for fast writes");
        let mut tx = self
            .connection
            .begin()
            .await
            .map_err(DatabaseError::Backend)?;
        for variant in books {
            if let Some((id, book_id)) = target_hashmap.get(&(variant.file_size, variant.hash)) {
                #[cfg(unix)]
                let path = variant.path().as_os_str().as_bytes();
                #[cfg(windows)]
                let path = v16_to_v8(variant.path().as_os_str().encode_wide().collect());
                let id = id.map(|id| id as i64);
                let file_size = variant.file_size as i64;
                let hash = variant.hash.to_vec();
                let book_id_ = u64::from(*book_id) as i64;
                tracing::info!(
                    "Found matching variant, setting path to {} for variant with id {:?}, file size {:?}, hash {:?}, and book_id {}",
                    variant.path.display(),
                    id,
                    file_size,
                    hash,
                    book_id_
                );

                let num_rows = if id.is_none() {
                    sqlx::query!(
                        "UPDATE variants SET path = ? WHERE (id is NULL AND file_size = ? AND hash = ? AND book_id = ?)",
                        path,
                        file_size,
                        hash,
                        book_id_
                    )
                } else {
                    sqlx::query!(
                        "UPDATE variants SET path = ? WHERE (id = ? AND file_size = ? AND hash = ? AND book_id = ?)",
                        path,
                        id,
                        file_size,
                        hash,
                        book_id_
                    )
                }
                    .execute(&mut tx)
                    .await
                    .map_err(DatabaseError::Backend)?
                    .rows_affected();
                if num_rows != 1 {
                    tracing::error!("Query should have modified at least one value");
                }

                self.cache
                    .write()
                    .await
                    .remove_books(&std::iter::once(*book_id).collect::<HashSet<_>>());
            }
        }
        tracing::info!("Completing merge, committing transaction");
        tx.commit().await.map_err(DatabaseError::Backend)
    }

    // async fn perform_query(
    //     &mut self,
    //     mut query: Select,
    //     limit: usize,
    // ) -> Result<Vec<Arc<Book>>, DatabaseError<Self::Error>> {
    //     #[derive(sqlx::FromRow, Debug)]
    //     struct SqlxBookId {
    //         book_id: i64,
    //     }
    //
    //     let lookahead_limit = limit * 5;
    //     let query = query.limit(lookahead_limit);
    //     let (query, bound_variables) = Sqlite::build(query);
    //     let mut query = sqlx::query_as(&query);
    //     for value in bound_variables {
    //         match value {
    //             ParameterizedValue::Null => unimplemented!(),
    //             ParameterizedValue::Integer(value) => query.bind(value),
    //             ParameterizedValue::Real(_value) => unimplemented!(),
    //             ParameterizedValue::Text(value) => query.bind(value.as_ref()),
    //             ParameterizedValue::Enum(_) => unimplemented!(),
    //             ParameterizedValue::Boolean(value) => query.bind(value),
    //             ParameterizedValue::Char(value) => query.bind(value.to_string()),
    //         };
    //     }
    //
    //     let ids: Vec<SqlxBookId> = query
    //         .fetch_all(&self.backend)
    //         .await
    //         .map_err(DatabaseError::Backend)?;
    //     let ids: Vec<BookID> = ids
    //         .into_iter()
    //         .map(|id| BookID::try_from(id.book_id as u64).unwrap())
    //         .collect();
    //
    //     let books = self.load_book_ids(&ids).await?;
    //     Ok(ids
    //         .iter()
    //         .map(|id| books.get(id).unwrap().clone())
    //         .collect())
    // }
}
