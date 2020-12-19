pub mod book;
pub mod error;
pub mod isbn;
pub mod variant;

pub use book::Book;
pub use error::BookError;
pub use isbn::{ISBNError, ISBN};
pub use variant::BookVariant;
