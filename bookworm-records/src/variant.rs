use std::convert::TryFrom;
use std::ffi::{OsStr, OsString};

use isbn2::Isbn;
use mobi::MobiMetadata;
use quick_epub::Metadata as EpubMetadata;
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

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
pub enum BookFormat {
    EPUB,
    MOBI,
    // Look at lo-pdf and pdf-extract.
    PDF,
    None,
    // TODO: AZW3, DJVU, DOC, RTF, custom extensions?
}

impl Default for BookFormat {
    fn default() -> Self {
        BookFormat::None
    }
}

impl TryFrom<&OsStr> for BookFormat {
    type Error = BookError;

    /// Returns a new `BookType` from the provided string - this should be a file extension.
    fn try_from(o_str: &OsStr) -> Result<Self, Self::Error> {
        match o_str.to_str() {
            Some(s) => match s.to_ascii_lowercase().as_str() {
                "epub" => Ok(BookFormat::EPUB),
                "mobi" => Ok(BookFormat::MOBI),
                "pdf" => Ok(BookFormat::PDF),
                _ => Err(BookError::UnsupportedExtension(o_str.to_os_string())),
            },
            None => Err(BookError::UnsupportedExtension(o_str.to_os_string())),
        }
    }
}

impl BookFormat {
    // TODO: Implement timeout to prevent crashing if reading explodes.
    pub(crate) fn metadata_filler<R: std::io::Read + std::io::Seek>(
        &self,
        reader: R,
    ) -> Result<Box<dyn MetadataFiller>, BookError> {
        match self {
            BookFormat::EPUB => Ok(Box::new(
                EpubMetadata::from_read(reader).map_err(|_| BookError::FileError)?,
            )),
            BookFormat::MOBI => Ok(Box::new(
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
