pub(crate) mod book;
#[allow(dead_code)]
pub(crate) mod isbn;
pub(crate) mod epub;

#[allow(unused_imports)]
pub(crate) use book::{Book, BookError, BookVariant};
#[allow(unused_imports)]
pub(crate) use isbn::{ISBNError, ISBN};
