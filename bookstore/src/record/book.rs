use std::cmp::Ordering;
use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::io::Error;
use std::str::FromStr;
use std::{fmt, path};

use mobi::MobiMetadata;

use serde::{Deserialize, Serialize};

use crate::record::epub::{EpubError, EpubMetadata};
use crate::record::ISBN;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub(crate) enum Identifier {
    ISBN(ISBN),
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
/// Enumerates all supported book types.
pub(crate) enum BookType {
    EPUB,
    MOBI,
    PDF,
    Unsupported(OsString),
}

#[derive(Debug)]
/// Enumerates all potential errors that can occur when using a Book.
pub(crate) enum BookError {
    FileError,
    ImmutableColumnError,
    UnsupportedExtension(BookType), //    MetadataError,
}

impl From<std::io::Error> for BookError {
    fn from(_: Error) -> Self {
        BookError::FileError
    }
}

impl From<EpubError> for BookError {
    fn from(_: EpubError) -> Self {
        BookError::FileError
    }
}

fn unravel_author(author: &str) -> String {
    if let Some(i) = author.find(',') {
        let (a, b) = author.split_at(i);
        let b = b.trim_start_matches(',').trim_start_matches(' ');
        format!("{} {}", b, a)
    } else {
        author.to_owned()
    }
}

// TODO: Implement timeout to prevent crashing if reading explodes.
impl BookType {
    /// Returns a new BookType from the provided string - this should be a file extension.
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

    /// Fills in the metadata for book, using self to determine which file format file_path is
    /// in.
    fn fill_in_metadata<S>(&self, book: &mut BookVariant, file_path: S) -> Result<(), BookError>
    where
        S: AsRef<path::Path>,
    {
        match self {
            BookType::EPUB => {
                let metadata = EpubMetadata::open(file_path)?;

                if book.local_title.is_none() {
                    book.local_title = metadata.title;
                }

                if book.additional_authors.is_none() {
                    if let Some(author) = metadata.author {
                        book.additional_authors = Some(vec![unravel_author(&author)]);
                    }
                }

                if book.language.is_none() {
                    book.language = metadata.language;
                }

                if book.identifier.is_none() {
                    if let Some(isbn) = metadata.isbn {
                        if let Ok(isbn) = ISBN::from_str(&isbn) {
                            book.identifier = Some(Identifier::ISBN(isbn));
                        }
                    }
                }

                Ok(())
            }
            BookType::MOBI => {
                let doc = MobiMetadata::from_path(&file_path)?;

                if book.local_title.is_none() {
                    book.local_title = doc.title();
                }

                if book.additional_authors.is_none() {
                    if let Some(author) = doc.author() {
                        book.additional_authors = Some(vec![unravel_author(&author)]);
                    }
                }

                if book.language.is_none() {
                    book.language = doc.language();
                }

                if book.identifier.is_none() {
                    if let Some(isbn) = doc.isbn() {
                        if let Ok(isbn) = ISBN::from_str(&isbn) {
                            book.identifier = Some(Identifier::ISBN(isbn));
                        }
                    }
                }

                Ok(())
            }
            b => Err(BookError::UnsupportedExtension(b.clone())),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
/// The struct which contains the major fields a book will have, a set of variants,
/// which corresponds to particular file formats of this book (eg. a EPUB and MOBI version),
/// or even differing realizations of the book (eg. a French and English of the same book).
/// Contains an unique ID, and provides storage for additional tags which are not specified here.
pub(crate) struct Book {
    pub(crate) title: Option<String>,
    pub(crate) authors: Option<Vec<String>>,
    pub(crate) series: Option<(String, Option<f32>)>,
    variants: Option<Vec<BookVariant>>,
    id: u32,
    extended_tags: Option<HashMap<String, String>>,
}

/// Identifies the columns a Book provides.
pub(crate) enum ColumnIdentifier {
    Title,
    Author,
    Series,
    ID,
    Variants,
    ExtendedTag(String),
}

impl<S: AsRef<str>> From<S> for ColumnIdentifier {
    fn from(val: S) -> Self {
        match val.as_ref().to_ascii_lowercase().as_str() {
            "author" | "authors" => Self::Author,
            "title" => Self::Title,
            "series" => Self::Series,
            "id" => Self::ID,
            "variant" | "variants" => Self::Variants,
            _ => Self::ExtendedTag(val.as_ref().to_owned()),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub(crate) struct BookVariant {
    local_title: Option<String>,
    identifier: Option<Identifier>,
    paths: Option<Vec<(BookType, path::PathBuf)>>,
    language: Option<String>,
    additional_authors: Option<Vec<String>>,
    translators: Option<Vec<String>>,
    description: Option<String>,
    id: Option<u32>,
}

impl Book {
    pub(crate) fn get_authors(&self) -> Option<&[String]> {
        self.authors.as_deref()
    }

    pub(crate) fn get_title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    pub(crate) fn get_series(&self) -> Option<&(String, Option<f32>)> {
        self.series.as_ref()
    }

    pub(crate) fn get_variants(&self) -> Option<&[BookVariant]> {
        self.variants.as_deref()
    }

    pub(crate) fn get_extended_columns(&self) -> Option<&HashMap<String, String>> {
        self.extended_tags.as_ref()
    }

    pub(crate) fn get_column_or<T: AsRef<str>>(
        &self,
        column: &ColumnIdentifier,
        default: T,
    ) -> String {
        match column {
            ColumnIdentifier::Title => self
                .get_title()
                .clone()
                .unwrap_or_else(|| default.as_ref())
                .to_owned(),
            ColumnIdentifier::Author => match self.get_authors() {
                None => default.as_ref().to_owned(),
                Some(authors) => authors.join(", "),
            },
            ColumnIdentifier::Series => {
                if let Some((series_name, nth_in_series)) = self.get_series() {
                    if let Some(nth_in_series) = nth_in_series {
                        format!("{} [{}]", series_name, nth_in_series)
                    } else {
                        series_name.clone()
                    }
                } else {
                    default.as_ref().to_owned()
                }
            }
            ColumnIdentifier::ID => self.id.to_string(),
            ColumnIdentifier::ExtendedTag(x) => {
                if let Some(d) = &self.extended_tags {
                    match d.get(x) {
                        None => default.as_ref().to_owned(),
                        Some(s) => s.to_owned(),
                    }
                } else {
                    default.as_ref().to_owned()
                }
            }
            ColumnIdentifier::Variants => default.as_ref().to_owned(),
        }
    }
}

impl BookVariant {
    pub(crate) fn generate_from_file<S>(file_path: S) -> Result<Self, BookError>
    where
        S: AsRef<path::Path>,
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
        let paths = vec![(book_type, path.to_owned())];
        let mut book = BookVariant {
            local_title: None,
            identifier: None,
            paths: Some(paths.clone()),
            language: None,
            additional_authors: None,
            translators: None,
            description: None,
            id: None,
        };
        for (booktype, path) in paths {
            let _ = booktype.fill_in_metadata(&mut book, path);
        }
        if book.local_title == None {
            book.local_title = Some(
                file_name
                    .to_str()
                    .expect("Handle local title strings")
                    .to_string(),
            );
        }
        Ok(book)
    }

    pub(crate) fn get_paths(&self) -> &Option<Vec<(BookType, path::PathBuf)>> {
        &self.paths
    }
}

impl Book {
    pub(crate) fn with_id(id: u32) -> Book {
        Book {
            title: None,
            authors: None,
            series: None,
            variants: None,
            id,
            extended_tags: None,
        }
    }

    pub(crate) fn generate_from_file<S>(file_path: S, id: u32) -> Result<Book, BookError>
    where
        S: AsRef<path::Path>,
    {
        let mut variants = vec![];
        let file_path = {
            let file_path = file_path.as_ref();
            match file_path.canonicalize() {
                Ok(p) => p,
                Err(_) => file_path.to_path_buf(),
            }
        };

        if !file_path.is_file() {
            return Err(BookError::FileError);
        }

        if let Ok(mut variant) = BookVariant::generate_from_file(file_path) {
            variant.id = Some(0);
            variants.push(variant);
        }

        let mut book = Book {
            title: None,
            authors: None,
            series: None,
            variants: Some(variants.clone()),
            id,
            extended_tags: None,
        };

        for variant in variants.iter() {
            if book.title == None {
                if let Some(title) = variant.local_title.clone() {
                    book.title = Some(title);
                }
            }
            if book.authors == None {
                if let Some(authors) = variant.additional_authors.clone() {
                    book.authors = Some(authors);
                }
            }
        }

        Ok(book)
    }

    pub(crate) fn get_id(&self) -> u32 {
        self.id
    }
}

impl Book {
    pub(crate) fn set_column<T: AsRef<str>>(
        &mut self,
        column: &ColumnIdentifier,
        value: T,
    ) -> Result<(), BookError> {
        let value = value.as_ref();
        match column {
            ColumnIdentifier::Title => {
                self.title = Some(value.to_owned());
            }
            ColumnIdentifier::Author => {
                self.authors = Some(vec![value.to_owned()]);
            }
            ColumnIdentifier::ID | ColumnIdentifier::Variants => {
                return Err(BookError::ImmutableColumnError);
            }
            ColumnIdentifier::Series => {
                if value.ends_with(']') {
                    // Replace with rsplit_once when stable.
                    let mut words = value.rsplitn(2, char::is_whitespace);
                    if let Some(id) = words.next() {
                        if let Some(series) = words.next() {
                            if let Ok(id) = f32::from_str(id.replace(&['[', ']'][..], "").as_str())
                            {
                                self.series = Some((series.to_owned(), Some(id)));
                            }
                        }
                    }
                } else {
                    self.series = Some((value.to_owned(), None));
                }
            }
            ColumnIdentifier::ExtendedTag(column) => {
                if let Some(d) = self.extended_tags.as_mut() {
                    d.insert(column.clone(), value.to_owned());
                } else {
                    let mut d = HashMap::new();
                    d.insert(column.clone(), value.to_owned());
                    self.extended_tags = Some(d);
                }
            }
        }
        Ok(())
    }

    pub(crate) fn cmp_column(&self, other: &Self, column: &ColumnIdentifier) -> Ordering {
        // fn cmp_opt<T: std::cmp::Ord>(a: Option<T>, b: Option<T>) -> Ordering {
        //     if a == b {
        //         Ordering::Equal
        //     } else if let Some(a) = a {
        //         if let Some(b) = b {
        //             a.cmp(&b)
        //         } else {
        //             Ordering::Greater
        //         }
        //     } else {
        //         Ordering::Less
        //     }
        // }

        match column {
            ColumnIdentifier::ID => self.get_id().cmp(&other.get_id()),
            ColumnIdentifier::Series => {
                let s_series = self.get_series();
                let o_series = other.get_series();
                if s_series.eq(&o_series) {
                    Ordering::Equal
                } else if let Some((s_st, s_ind)) = s_series {
                    if let Some((o_st, o_ind)) = o_series {
                        if s_st.eq(o_st) {
                            if s_ind == o_ind {
                                Ordering::Equal
                            } else if let Some(s_ind) = s_ind {
                                if let Some(o_ind) = o_ind {
                                    s_ind.partial_cmp(&o_ind).unwrap_or(Ordering::Equal)
                                } else {
                                    Ordering::Greater
                                }
                            } else {
                                Ordering::Less
                            }
                        } else {
                            s_st.cmp(&o_st)
                        }
                    } else {
                        Ordering::Greater
                    }
                } else {
                    Ordering::Less
                }
            }
            c => self.get_column_or(c, "").cmp(&other.get_column_or(c, "")),
        }
    }
}

impl Book {
    pub(crate) fn merge_mut(&mut self, other: Self) {
        if self.title.is_none() {
            self.title = other.title;
        }
        if self.authors.is_none() {
            self.authors = other.authors;
        }
        if self.series.is_none() {
            self.series = other.series;
        }
        match self.variants.as_mut() {
            None => self.variants = other.variants,
            Some(v) => {
                if let Some(vars) = other.variants {
                    v.extend(vars);
                }
            }
        }
        match self.extended_tags.as_mut() {
            None => self.extended_tags = other.extended_tags,
            Some(e) => {
                if let Some(map) = other.extended_tags {
                    e.extend(map);
                }
            }
        }
    }
}

impl fmt::Display for Book {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if let Some(title) = &self.title {
            write!(f, "{}", title)
        } else {
            write!(f, "")
        }
    }
}

// TODO: Add support for various identifiers (eg. DOI, ISBN, raw links)
