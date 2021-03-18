use std::ffi::{OsStr, OsString};
use std::path;
use std::str::FromStr;

use isbn2::Isbn;
use mobi::MobiMetadata;
use quick_epub::{IdentifierScheme, Metadata as EpubMetadata};
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::BookError;

fn unravel_author(author: &str) -> String {
    if let Some(i) = author.find(',') {
        let (a, b) = author.split_at(i);
        let b = b.trim_start_matches(',').trim_start_matches(' ');
        format!("{} {}", b, a)
    } else {
        author.to_owned()
    }
}

#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Identifier {
    ISBN(Isbn),
    Unknown(String, String),
}

#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Clone, Debug, PartialEq, Eq)]
/// Enumerates all supported book types.
pub enum BookType {
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
        match so.to_str() {
            Some(s) => match s.to_ascii_lowercase().as_str() {
                "epub" => BookType::EPUB,
                "mobi" => BookType::MOBI,
                "pdf" => BookType::PDF,
                _ => BookType::Unsupported(so.to_os_string()),
            },
            None => BookType::Unsupported(so.to_os_string()),
        }
    }
}

#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Clone, Debug, PartialEq)]
pub struct BookVariant {
    pub book_type: BookType,
    pub path: path::PathBuf,
    pub local_title: Option<String>,
    pub identifier: Option<Identifier>,
    pub language: Option<String>,
    pub additional_authors: Option<Vec<String>>,
    pub translators: Option<Vec<String>>,
    pub description: Option<String>,
    pub id: Option<u32>,
    #[cfg(feature = "hash")]
    pub hash: Option<[u8; 32]>,
}

impl BookVariant {
    /// Generates a book variant from the file at `file_path`, and fills in details from the
    /// parsed book metadata.
    ///
    /// # Arguments
    /// * ` file_path ` - The path to the file of interest.
    ///
    /// # Errors
    /// Will return an error if the provided path does not lead to a file.
    /// Will panic if the title can not be set.
    pub fn generate_from_file<P>(file_path: P) -> Result<Self, BookError>
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
        let book_type = match BookType::new(ext) {
            x @ BookType::Unsupported(_) => return Err(BookError::UnsupportedExtension(x)),
            supported => supported,
        };
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
            #[cfg(feature = "hash")]
            hash: None,
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
        #[cfg(feature = "hash")]
        let mut reader = {
            use sha2::{Digest, Sha256};
            use std::io::Read;

            let file = std::fs::File::open(&self.path)?;
            let len = file.metadata()?.len();
            let bytes_to_read = (len as usize).min(4096);

            let mut reader = std::io::BufReader::with_capacity(bytes_to_read, file);
            let mut buf = [0; 4096];

            reader.read_exact(&mut buf[..bytes_to_read])?;
            reader.seek_relative(-(bytes_to_read as i64))?;

            let mut hasher = Sha256::new();
            hasher.update(&buf[..bytes_to_read]);
            let res = hasher.finalize();
            self.hash = Some(res.into());

            reader
        };
        match &self.book_type {
            BookType::EPUB => {
                #[cfg(feature = "hash")]
                let metadata = EpubMetadata::from_read(&mut reader)?;
                #[cfg(not(feature = "hash"))]
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
                    match metadata.identifier {
                        Some((id, value)) => match id {
                            IdentifierScheme::ISBN => {
                                self.identifier = Isbn::from_str(&value).ok().map(Identifier::ISBN)
                            }
                            IdentifierScheme::Unknown(id) => {
                                self.identifier = Some(Identifier::Unknown(id, value));
                            }
                            x => self.identifier = Some(Identifier::Unknown(x.to_string(), value)),
                        },
                        None => {}
                    }
                }

                Ok(())
            }
            BookType::MOBI => {
                #[cfg(feature = "hash")]
                let doc = MobiMetadata::from_read(&mut reader)?;
                #[cfg(not(feature = "hash"))]
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
                        if let Ok(isbn) = Isbn::from_str(&isbn) {
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

    pub fn path(&self) -> &path::Path {
        self.path.as_ref()
    }

    pub fn book_type(&self) -> &BookType {
        &self.book_type
    }
}
