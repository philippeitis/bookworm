use std::convert::TryFrom;
use std::ffi::{OsStr, OsString};
use std::io::{BufReader, SeekFrom};
use std::io::{Read, Seek};
use std::path;

use isbn2::Isbn;
use mobi::MobiMetadata;
use quick_epub::Metadata as EpubMetadata;
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::BookError;

pub(crate) fn unravel_author(author: &str) -> String {
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
}

impl TryFrom<&OsStr> for BookType {
    type Error = BookError;

    /// Returns a new `BookType` from the provided string - this should be a file extension.
    fn try_from(o_str: &OsStr) -> Result<Self, Self::Error> {
        match o_str.to_str() {
            Some(s) => match s.to_ascii_lowercase().as_str() {
                "epub" => Ok(BookType::EPUB),
                "mobi" => Ok(BookType::MOBI),
                "pdf" => Ok(BookType::PDF),
                _ => Err(BookError::UnsupportedExtension(o_str.to_os_string())),
            },
            None => Err(BookError::UnsupportedExtension(o_str.to_os_string())),
        }
    }
}

impl BookType {
    // TODO: Implement timeout to prevent crashing if reading explodes.
    fn metadata_filler<R: std::io::Read + std::io::Seek>(
        &self,
        reader: R,
    ) -> Result<Box<dyn MetadataFiller>, BookError> {
        match self {
            BookType::EPUB => Ok(Box::new(
                EpubMetadata::from_read(reader).map_err(|_| BookError::FileError)?,
            )),
            BookType::MOBI => Ok(Box::new(
                MobiMetadata::from_read(reader).map_err(|_| BookError::FileError)?,
            )),
            _ => Err(BookError::UnsupportedExtension(OsString::from("PDF"))),
        }
    }
}

pub trait MetadataFiller {
    fn take_title(&mut self, title: &mut Option<String>);

    fn take_description(&mut self, description: &mut Option<String>);

    fn take_language(&mut self, language: &mut Option<String>);

    fn take_identifier(&mut self, identifier: &mut Option<Identifier>);

    fn take_authors(&mut self, authors: &mut Option<Vec<String>>);
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
    pub hash: [u8; 32],
}

impl BookVariant {
    /// Generates a book variant from the file at `file_path`, and fills in details from the
    /// parsed book metadata.
    ///
    /// # Arguments
    /// * ` file_path ` - The path to the file of interest.
    ///
    /// # Errors
    /// Will return an error if the provided path can not be read.
    /// Will panic if the title can not be set.
    pub fn generate_from_file<P>(file_path: P) -> Result<Self, BookError>
    where
        P: AsRef<path::Path>,
    {
        let path = file_path.as_ref();

        let file_name = if let Some(file_name) = path.file_name() {
            file_name
        } else {
            return Err(BookError::FileError);
        };

        let ext = if let Some(ext) = path.extension() {
            ext
        } else {
            return Err(BookError::FileError);
        };

        let book_type = BookType::try_from(ext)?;

        let (reader, hash) = {
            let mut file = std::fs::File::open(&path)?;
            let len = file.metadata()?.len();
            let bytes_to_read = (len as usize).min(4096);

            let mut buf = [0; 4096];
            file.read_exact(&mut buf[..bytes_to_read])?;
            file.seek(SeekFrom::Start(0))?;

            let mut hasher = Sha256::new();
            hasher.update(&buf[..bytes_to_read]);
            let res = hasher.finalize();

            (
                BufReader::with_capacity(len.saturating_sub(4096).min(4096) as usize, file),
                res.into(),
            )
        };

        let mut book = BookVariant {
            book_type,
            path: path.to_owned(),
            hash,
            local_title: None,
            identifier: None,
            language: None,
            additional_authors: None,
            translators: None,
            description: None,
            id: None,
        };

        if let Ok(mut metadata_filler) = book.book_type.metadata_filler(reader) {
            metadata_filler.take_title(&mut book.local_title);
            metadata_filler.take_authors(&mut book.additional_authors);
            metadata_filler.take_description(&mut book.description);
            metadata_filler.take_language(&mut book.language);
            metadata_filler.take_identifier(&mut book.identifier);
        }

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

    pub fn path(&self) -> &path::Path {
        self.path.as_ref()
    }

    pub fn book_type(&self) -> &BookType {
        &self.book_type
    }
}
