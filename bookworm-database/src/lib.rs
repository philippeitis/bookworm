#![deny(unused_must_use)]
#![deny(unused_imports)]
#![deny(unused_attributes)]
#![deny(unused_mut)]

pub use bookview::BookView;
pub use bookworm_records::book::Book;
pub use database::{AppDatabase, DatabaseError};
#[cfg(feature = "sqlite")]
pub use sqlite_database::SQLiteDatabase;

pub mod bookview;
mod cache;
pub mod database;
pub mod paginator;
pub mod search;
#[cfg(feature = "sqlite")]
pub mod sqlite_database;
