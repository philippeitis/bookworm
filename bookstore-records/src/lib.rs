#![cfg_attr(feature = "sha2", feature(bufreader_seek_relative))]
pub mod book;
pub mod error;
pub mod variant;

pub use book::Book;
pub use error::BookError;
pub use variant::BookVariant;
