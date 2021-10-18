#![deny(unused_must_use)]
#![deny(unused_imports)]

pub use bookstore_records::book::Book;
pub use bookview::BookView;
pub use database::{AppDatabase, DatabaseError};
#[cfg(feature = "sqlite")]
pub use sqlite_database::SQLiteDatabase;

mod cache;
pub mod bookview;
pub mod database;
pub mod paginator;
pub mod search;
#[cfg(feature = "sqlite")]
pub mod sqlite_database;
