#![deny(unused_must_use)]

pub use bookstore_records::book::Book;
pub use bookview::BookView;
pub use database::{AppDatabase, DatabaseError, IndexableDatabase};
#[cfg(feature = "sqlite")]
pub use sqlite_database::SQLiteDatabase;

mod bookmap;
pub mod bookview;
pub mod database;
pub mod paged_cursor;
pub mod paginator;
pub mod search;
#[cfg(feature = "sqlite")]
pub mod sqlite_database;
