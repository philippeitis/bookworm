pub use book::Book;
pub use bookview::{BookView, NestedBookView, ScrollableBookView, SearchableBookView};
pub use database::{AppDatabase, DatabaseError, IndexableDatabase};
pub use paged_cursor::PageCursor;
#[cfg(feature = "sqlite")]
pub use sqlite_database::SQLiteDatabase;

pub mod book;
mod bookmap;
pub mod bookview;
pub mod database;
pub mod paged_cursor;
pub mod search;
#[cfg(feature = "sqlite")]
pub mod sqlite_database;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ColumnOrder {
    Ascending,
    Descending,
}

impl ColumnOrder {
    pub fn as_bool(&self) -> bool {
        match self {
            ColumnOrder::Ascending => false,
            ColumnOrder::Descending => true,
        }
    }

    pub fn from_bool(reversed: bool) -> Self {
        match reversed {
            false => ColumnOrder::Ascending,
            true => ColumnOrder::Descending,
        }
    }
}
