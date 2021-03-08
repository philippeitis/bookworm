pub mod basic_database;
mod bookmap;
pub mod bookview;
pub mod database;
pub mod paged_cursor;
pub mod search;
#[cfg(feature = "sqlite")]
pub mod sqlite_database;

pub use bookview::{BookView, NestedBookView, ScrollableBookView, SearchableBookView};
pub use database::{AppDatabase, DatabaseError, IndexableDatabase};

pub use paged_cursor::PageCursor;

#[cfg(feature = "rustbreak")]
pub use basic_database::BasicDatabase;
#[cfg(feature = "sqlite")]
pub use sqlite_database::SQLiteDatabase;
