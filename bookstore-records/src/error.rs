use quick_epub::Error as EpubError;

use crate::variant::BookType;

#[derive(Debug, PartialEq, Eq)]
/// Enumerates all potential errors that can occur when using a Book.
pub enum BookError {
    FileError,
    ImmutableColumnError,
    UnsupportedExtension(BookType), //    MetadataError,
}

impl From<std::io::Error> for BookError {
    fn from(_: std::io::Error) -> Self {
        BookError::FileError
    }
}

impl From<EpubError> for BookError {
    fn from(_: EpubError) -> Self {
        BookError::FileError
    }
}
