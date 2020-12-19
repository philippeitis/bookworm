use std::ffi::{OsStr, OsString};
use std::path;
use std::str::FromStr;

use mobi::MobiMetadata;
use serde::{Deserialize, Serialize};

use crate::record::epub::Metadata as EpubMetadata;
use crate::record::{BookError, ISBN};

fn unravel_author(author: &str) -> String {
    if let Some(i) = author.find(',') {
        let (a, b) = author.split_at(i);
        let b = b.trim_start_matches(',').trim_start_matches(' ');
        format!("{} {}", b, a)
    } else {
        author.to_owned()
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub(crate) enum Identifier {
    ISBN(ISBN),
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
/// Enumerates all supported book types.
pub(crate) enum BookType {
    EPUB,
    MOBI,
    // Look at lo-pdf and pdf-extract.
    PDF,
    // TODO: AZW3, DJVU, DOC, RTF, custom extensions?
    Unsupported(OsString),
}

// TODO: Implement timeout to prevent crashing if reading explodes.
impl BookType {
    /// Returns a new `BookType` from the provided string - this should be a file extension.
    fn new<S>(s: S) -> BookType
    where
        S: AsRef<OsStr>,
    {
        let so = s.as_ref();
        if let Some(s) = so.to_str() {
            match s.to_ascii_lowercase().as_str() {
                "epub" => BookType::EPUB,
                "mobi" => BookType::MOBI,
                "pdf" => BookType::PDF,
                _ => BookType::Unsupported(so.to_os_string()),
            }
        } else {
            BookType::Unsupported(so.to_os_string())
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub(crate) struct BookVariant {
    pub(super) book_type: BookType,
    pub(super) path: path::PathBuf,
    pub(super) local_title: Option<String>,
    pub(super) identifier: Option<Identifier>,
    pub(super) language: Option<String>,
    pub(super) additional_authors: Option<Vec<String>>,
    pub(super) translators: Option<Vec<String>>,
    pub(super) description: Option<String>,
    pub(super) id: Option<u32>,
}

impl BookVariant {
    /// Generates a book variant from the file at the specified `file_path`, and fills in
    /// information from the metadata of the book.
    ///
    /// # Arguments
    /// * ` file_path ` - The path to the file of interest.
    ///
    /// # Errors
    /// Will return an error if the provided path does not lead to a file.
    /// Will panic if the title can not be set.
    pub(crate) fn generate_from_file<P>(file_path: P) -> Result<Self, BookError>
    where
        P: AsRef<path::Path>,
    {
        // let file = File::open(file_path.clone()).map_err(|_e| BookError::FileError)?;
        // let data = file.metadata().map_err(|_e| BookError::MetadataError)?;
        let path = file_path.as_ref();

        if !path.is_file() {
            return Err(BookError::FileError);
        }

        let file_name = if let Some(file_name) = path.file_name() {
            file_name.to_owned()
        } else {
            return Err(BookError::FileError);
        };
        let ext = if let Some(ext) = path.extension() {
            ext
        } else {
            return Err(BookError::FileError);
        };
        let book_type = BookType::new(ext);
        let mut book = BookVariant {
            book_type,
            path: path.to_owned(),
            local_title: None,
            identifier: None,
            language: None,
            additional_authors: None,
            translators: None,
            description: None,
            id: None,
        };

        let _ = book.fill_in_metadata();

        if book.local_title.is_none() {
            book.local_title = Some(
                file_name
                    .to_str()
                    .expect("Handle local title strings")
                    .to_string(),
            );
        }

        Ok(book)
    }

    /// Fills in the metadata for book from the internal book type.
    fn fill_in_metadata(&mut self) -> Result<(), BookError> {
        match &self.book_type {
            BookType::EPUB => {
                let metadata = EpubMetadata::open(&self.path)?;

                if self.local_title.is_none() {
                    self.local_title = metadata.title;
                }

                if self.additional_authors.is_none() {
                    if let Some(author) = metadata.author {
                        self.additional_authors = Some(vec![unravel_author(&author)]);
                    }
                }

                if self.language.is_none() {
                    self.language = metadata.language;
                }

                if self.description.is_none() {
                    self.description = metadata.description;
                }

                if self.identifier.is_none() {
                    if let Some(isbn) = metadata.isbn {
                        if let Ok(isbn) = ISBN::from_str(&isbn) {
                            self.identifier = Some(Identifier::ISBN(isbn));
                        }
                    }
                }

                Ok(())
            }
            BookType::MOBI => {
                let doc = MobiMetadata::from_path(&self.path)?;
                if self.additional_authors.is_none() {
                    if let Some(author) = doc.author() {
                        self.additional_authors = Some(vec![unravel_author(&author)]);
                    }
                }

                if self.language.is_none() {
                    self.language = doc.language();
                }

                if self.identifier.is_none() {
                    if let Some(isbn) = doc.isbn() {
                        if let Ok(isbn) = ISBN::from_str(&isbn) {
                            self.identifier = Some(Identifier::ISBN(isbn));
                        }
                    }
                }

                if self.description.is_none() {
                    self.description = doc.description();
                }

                if self.local_title.is_none() {
                    if doc.title().is_none() {
                        self.local_title = Some(doc.name);
                    } else {
                        self.local_title = doc.title();
                    }
                }

                Ok(())
            }
            b => Err(BookError::UnsupportedExtension(b.clone())),
        }
    }

    pub(crate) fn path(&self) -> &path::Path {
        self.path.as_ref()
    }

    pub(crate) fn book_type(&self) -> &BookType {
        &self.book_type
    }
}
