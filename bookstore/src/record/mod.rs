pub(crate) mod book;
pub(crate) mod epub;
pub(crate) mod error;
#[allow(dead_code)]
pub(crate) mod isbn;
pub(crate) mod variant;

pub(crate) use book::Book;
pub(crate) use error::BookError;
#[allow(unused_imports)]
pub(crate) use isbn::{ISBNError, ISBN};
pub(crate) use variant::BookVariant;
