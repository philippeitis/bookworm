pub(crate) mod basic_database;
pub(crate) mod bookview;
#[cfg(feature = "cloud")]
pub(crate) mod google_cloud_database;
pub(crate) mod paged_cursor;
pub(crate) mod search;
#[cfg(feature = "sqlite")]
pub(crate) mod sqlite_database;

#[allow(unused_imports)]
pub(crate) use basic_database::{AppDatabase, DatabaseError, IndexableDatabase};
#[allow(unused_imports)]
pub(crate) use bookview::{BookView, NestedBookView, ScrollableBookView, SearchableBookView};

#[cfg(feature = "cloud")]
pub(crate) use google_cloud_database::CloudDatabase;
pub(crate) use paged_cursor::PageCursor;

#[cfg(not(feature = "sqlite"))]
pub(crate) use basic_database::BasicDatabase as Database;
#[cfg(feature = "sqlite")]
pub(crate) use sqlite_database::SQLiteDatabase as Database;
