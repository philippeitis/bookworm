pub mod basic_database;
mod bookmap;
pub mod bookview;
#[cfg(feature = "cloud")]
pub mod google_cloud_database;
pub mod paged_cursor;
pub mod search;
#[cfg(feature = "sqlite")]
pub mod sqlite_database;

pub use basic_database::{AppDatabase, BasicDatabase, DatabaseError, IndexableDatabase};
pub use bookview::{BookView, NestedBookView, ScrollableBookView, SearchableBookView};

#[cfg(feature = "cloud")]
pub use google_cloud_database::CloudDatabase;
pub use paged_cursor::PageCursor;

#[cfg(feature = "sqlite")]
pub use sqlite_database::SQLiteDatabase;
