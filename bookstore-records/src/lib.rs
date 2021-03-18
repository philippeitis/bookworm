pub mod book;
mod epub;
pub mod error;
mod mobi;
pub mod series;
pub mod variant;

pub use book::Book;
pub use error::BookError;
pub use variant::BookVariant;
