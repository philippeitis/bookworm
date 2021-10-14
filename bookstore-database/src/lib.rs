#![deny(unused_must_use)]
#![deny(unused_imports)]

pub use bookstore_records::book::Book;
pub use bookview::BookView;
pub use database::{AppDatabase, DatabaseError};
#[cfg(feature = "sqlite")]
pub use sqlite_database::SQLiteDatabase;

mod bookmap;
pub mod bookview;
pub mod database;
pub mod paginator;
pub mod search;
#[cfg(feature = "sqlite")]
pub mod sqlite_database;

fn log(s: impl AsRef<str>) {
    use std::io::Write;

    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(true)
        .open("log.txt")
    {
        let _ = f.write_all(s.as_ref().as_bytes());
        let _ = f.write_all(b"\n");
    }
}
