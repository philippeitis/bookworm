pub(crate) mod basic_database;
pub(crate) mod bookview;
#[cfg(cloud)]
pub(crate) mod google_cloud_database;
pub(crate) mod paged_cursor;
pub(crate) mod search;
#[cfg(sqlite)]
pub(crate) mod sqlite_database;

pub(crate) use basic_database::{AppDatabase, DatabaseError, IndexableDatabase};
pub(crate) use bookview::{BookView, NestedBookView, ScrollableBookView, SearchableBookView};

#[cfg(cloud)]
pub(crate) use google_cloud_database::CloudDatabase;
pub(crate) use paged_cursor::PageCursor;

#[cfg(not(sqllite))]
pub(crate) use basic_database::BasicDatabase;
#[cfg(sqllite)]
pub(crate) use sqlite_database::SqliteDatabase;
