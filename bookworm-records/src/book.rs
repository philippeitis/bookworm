use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::convert::TryFrom;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::str::FromStr;
use std::{fmt, path};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::series::Series;
use crate::variant::{BookFormat, Identifier};
use crate::Edit;
use crate::{BookError, ColumnOrder};

pub type BookID = std::num::NonZeroU64;

#[derive(Debug, PartialEq, Eq)]
pub enum RecordError {
    ImmutableColumn,
    InextensibleColumn,
}

/// Identifies the columns a Book provides. Intended to provide a way to access arbitrary columns,
/// for the sake of bulk operations which access specific columns.
#[derive(Debug, Eq, PartialEq, Clone)]
pub enum ColumnIdentifier {
    Title,
    Author,
    Series,
    ID,
    Description,
    Tags,
    ExactTag(String),
    MultiMap(String),
    MultiMapExact(String, String),
    NamedTag(String),
}

impl<S: AsRef<str>> From<S> for ColumnIdentifier {
    fn from(val: S) -> Self {
        match val.as_ref().to_ascii_lowercase().as_str() {
            "author" | "authors" => Self::Author,
            "title" => Self::Title,
            "series" => Self::Series,
            "id" => Self::ID,
            "description" => Self::Description,
            "tag" => Self::Tags,
            _ => Self::NamedTag(val.as_ref().to_owned()),
        }
    }
}

impl ColumnIdentifier {
    pub fn into_string(self) -> String {
        match self {
            ColumnIdentifier::Title => "Title",
            ColumnIdentifier::Author => "Author",
            ColumnIdentifier::Series => "Series",
            ColumnIdentifier::ID => "ID",
            ColumnIdentifier::Description => "Description",
            ColumnIdentifier::Tags | ColumnIdentifier::ExactTag(_) => "Tag",
            ColumnIdentifier::NamedTag(t) => return t,
            ColumnIdentifier::MultiMap(t) | ColumnIdentifier::MultiMapExact(t, _) => return t,
        }
        .to_string()
    }
}

#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Clone, Debug, PartialEq, Default)]
/// Stores the metadata for a specific book, with an associated ID, variants, and fields shared
/// between different variants.
///
/// Variants can be different file formats of the same book, or different realizations of the same
/// book.
pub struct Book {
    pub format: BookFormat,
    pub id: Option<BookID>,
    pub title: Option<String>,
    // TODO: Should authors be a hashset?
    pub authors: Option<Vec<String>>,
    pub series: Option<Series>,
    pub description: Option<String>,
    pub free_tags: HashSet<String>,
    pub named_tags: HashMap<String, String>,
    pub path: path::PathBuf,
    pub file_size: u64,
    pub identifier: Option<Identifier>,
    pub language: Option<String>,
    pub translators: Option<Vec<String>>,
    pub hash: [u8; 32],
}

impl Book {
    pub fn authors(&self) -> Option<&[String]> {
        self.authors.as_deref()
    }

    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    pub fn series(&self) -> Option<&Series> {
        self.series.as_ref()
    }

    pub fn tags(&self) -> &HashMap<String, String> {
        &self.named_tags
    }

    pub fn free_tags(&self) -> &HashSet<String> {
        &self.free_tags
    }

    pub fn description(&self) -> Option<&String> {
        self.description.as_ref()
    }

    pub fn get_column(&self, column: &ColumnIdentifier) -> Option<Cow<str>> {
        Some(match column {
            ColumnIdentifier::ID => Cow::Owned(self.id?.to_string()),
            ColumnIdentifier::Title => Cow::Borrowed(self.title()?),
            ColumnIdentifier::Author => Cow::Owned(self.authors()?.join(", ")),
            ColumnIdentifier::Series => Cow::Owned(self.series()?.to_string()),
            ColumnIdentifier::Description => Cow::Borrowed(self.description()?),
            ColumnIdentifier::NamedTag(x) => Cow::Borrowed(self.named_tags.get(x)?),
            _ => return None,
        })
    }

    /// Merges self with other, assuming that other is in fact a variant of self.
    /// Missing metadata will be utilized from other, and `self` variants will be extended
    /// to include `other` variants.
    ///
    /// # Arguments
    /// * ` other ` - Another book with additional metadata, to be merged with self
    pub fn merge_mut(&mut self, other: &Self) {
        if self.title.is_none() {
            self.title = other.title.clone();
        }
        if self.authors.is_none() {
            self.authors = other.authors.clone();
        }
        if self.series.is_none() {
            self.series = other.series.clone();
        }

        self.named_tags.extend(other.named_tags.clone());
    }
}

impl Book {
    /// Generates a placeholder book, which should not be used except as a way to reduce
    /// the run-time of specific operations. Placeholders do not have an ID, and calling
    /// id() on a placeholder will result in a panic.
    pub fn placeholder() -> Book {
        Self::default()
    }

    /// Returns true if `self` is a placeholder
    pub fn is_placeholder(&self) -> bool {
        self.id.is_none()
    }

    /// Returns the ID, which can be used as an index.
    ///
    /// # Errors
    /// Will panic if `self` was created by placeholder.
    pub fn id(&self) -> BookID {
        self.id.expect("Called get_id on placeholder book.")
    }
}

impl Book {
    /// Sets specified columns, to the specified value. Titles will be stored directly,
    /// authors will be stored as a list containing a single author.
    /// ID and Variants can not be modified through `set_column`.
    /// Series will be parsed to extract an index - strings in the form "series \[num\]"
    /// will be parsed as ("series", num).
    ///
    /// All remaining arguments will be stored literally.
    ///
    /// # Arguments
    /// * ` column ` - the column of interest.
    /// * ` value ` - the value to store into the column.
    pub fn set_column<S: AsRef<str>>(
        &mut self,
        column: &ColumnIdentifier,
        value: S,
    ) -> Result<(), RecordError> {
        let value = value.as_ref();
        match column {
            ColumnIdentifier::Title => {
                self.title = Some(value.to_owned());
            }
            ColumnIdentifier::Description => {
                self.description = Some(value.to_owned());
            }
            ColumnIdentifier::Author => {
                self.authors = Some(vec![value.to_owned()]);
            }
            ColumnIdentifier::ID => {
                return Err(RecordError::ImmutableColumn);
            }
            ColumnIdentifier::Series => {
                self.series = Series::from_str(&value).ok();
            }
            ColumnIdentifier::NamedTag(column) => {
                self.named_tags.insert(column.to_owned(), value.to_owned());
            }
            ColumnIdentifier::Tags => {
                self.free_tags.insert(value.to_owned());
            }
            ColumnIdentifier::ExactTag(tag) => {
                self.free_tags.remove(tag);
                self.free_tags.insert(value.to_owned());
            }
            ColumnIdentifier::MultiMap(_mm) | ColumnIdentifier::MultiMapExact(_mm, _) => {
                unimplemented!("Can not set multimap columns.")
            }
        }
        Ok(())
    }

    pub fn edit_column<E: AsRef<Edit>>(
        &mut self,
        column: &ColumnIdentifier,
        edit: E,
    ) -> Result<(), RecordError> {
        match edit.as_ref() {
            Edit::Delete => self.delete_column(column),
            Edit::Replace(s) => self.set_column(column, s),
            Edit::Append(s) => self.extend_column(column, s),
        }
    }

    pub fn extend_column<S: AsRef<str>>(
        &mut self,
        column: &ColumnIdentifier,
        value: S,
    ) -> Result<(), RecordError> {
        let value = value.as_ref();
        match column {
            ColumnIdentifier::Title => match &mut self.title {
                x @ None => *x = Some(value.to_string()),
                Some(title) => title.push_str(value),
            },
            ColumnIdentifier::Description => match &mut self.description {
                x @ None => *x = Some(value.to_string()),
                Some(description) => description.push_str(value),
            },
            ColumnIdentifier::Author => match &mut self.authors {
                x @ None => *x = Some(vec![value.to_string()]),
                Some(authors) => authors.push(value.to_owned()),
            },
            ColumnIdentifier::ID => {
                return Err(RecordError::ImmutableColumn);
            }
            ColumnIdentifier::Series => {
                return Err(RecordError::InextensibleColumn);
            }
            ColumnIdentifier::NamedTag(column) => {
                self.named_tags
                    .entry(column.to_owned())
                    .or_insert_with(String::new)
                    .push_str(value);
            }
            ColumnIdentifier::Tags => {
                self.free_tags.insert(value.to_owned());
            }
            ColumnIdentifier::ExactTag(tag) => {
                if !self.free_tags.remove(tag) {
                    self.free_tags.insert(value.to_owned());
                } else {
                    self.free_tags.insert(tag.to_owned() + value);
                }
            }
            _ => unimplemented!("Can not extend multimap columns."),
        }
        Ok(())
    }

    pub fn delete_column(&mut self, column: &ColumnIdentifier) -> Result<(), RecordError> {
        match column {
            ColumnIdentifier::Title => self.title = None,
            ColumnIdentifier::Description => self.description = None,
            ColumnIdentifier::Author => self.authors = None,
            ColumnIdentifier::ID => {
                return Err(RecordError::ImmutableColumn);
            }
            ColumnIdentifier::Series => self.series = None,
            ColumnIdentifier::NamedTag(column) => {
                self.named_tags.remove(column);
            }
            ColumnIdentifier::Tags => {
                self.free_tags.clear();
            }
            ColumnIdentifier::ExactTag(t) => {
                self.free_tags.remove(t);
            }
            _ => unimplemented!("Can not delete multimap columns."),
        }
        Ok(())
    }

    /// Compares the specified columns, by default using Rust's native string comparison
    /// - missing fields will be compared as empty strings.
    /// If ID is compared, numerical comparison will occur.
    /// If Series is compared, the series will be compared first, with the index being used
    /// as a tie-breaker.
    ///
    /// # Arguments
    /// * ` other ` - The other book with column to compare.
    /// * ` column ` - the column of interest.
    pub fn cmp_column(&self, other: &Self, column: &ColumnIdentifier) -> Ordering {
        match column {
            ColumnIdentifier::ID => self.id.cmp(&other.id),
            ColumnIdentifier::Series => self.series().cmp(&other.series()),
            ColumnIdentifier::Title => self.title().cmp(&other.title()),
            ColumnIdentifier::Description => self.description.as_ref().cmp(&other.description()),
            ColumnIdentifier::Author => match (&self.authors, &other.authors) {
                // TODO: Better comparison algorithm.
                (None, None) => Ordering::Equal,
                (None, Some(_)) => Ordering::Less,
                (Some(_), None) => Ordering::Greater,
                (Some(self_authors), Some(other_authors)) => {
                    let mut self_iter = self_authors.iter();
                    let mut other_iter = other_authors.iter();
                    let mut res = Ordering::Equal;
                    while res == Ordering::Equal {
                        let auth_a = self_iter.next();
                        let auth_b = other_iter.next();
                        res = auth_a.cmp(&auth_b);

                        // Only need to check one - if equal and one is none, both are none
                        if auth_a.is_none() {
                            break;
                        }
                    }
                    res
                }
            },
            c => self.get_column(c).cmp(&other.get_column(c)),
        }
    }

    pub fn cmp_columns(
        &self,
        other: &Self,
        columns: &[(ColumnIdentifier, ColumnOrder)],
    ) -> Ordering {
        let mut ordering = Ordering::Equal;
        for (column, column_order) in columns {
            ordering = self.cmp_column(other, column);

            if *column_order == ColumnOrder::Descending {
                ordering = ordering.reverse();
            }

            if ordering != Ordering::Equal {
                return ordering;
            }
        }
        ordering
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

#[cfg(test)]
mod test {
    use super::*;
    use std::convert::TryFrom;

    #[test]
    fn test_setting_columns() {
        let id = BookID::try_from(1).unwrap();
        let mut book = Book::default();
        book.id = Some(id);

        let test_sets = [
            ("title", "hello", Ok(()), "hello"),
            ("authors", "world", Ok(()), "world"),
            ("id", "5", Err(RecordError::ImmutableColumn), "1"),
            ("series", "hello world", Ok(()), "hello world"),
            ("series", "hello world [1.2]", Ok(()), "hello world [1.2]"),
            ("random_tag", "random value", Ok(()), "random value"),
        ];

        for (col, new_value, result, expected) in &test_sets {
            let col = ColumnIdentifier::from(col);
            match result {
                Ok(_) => assert!(book.set_column(&col, new_value).is_ok()),
                Err(err) => {
                    let e = book.set_column(&col, new_value);
                    assert!(e.is_err());
                    let e = e.unwrap_err();
                    assert_eq!(&e, err);
                }
            }
            assert_eq!(
                book.get_column(&col).map(Cow::into_owned),
                Some(expected.to_string())
            );
        }
    }
}

impl Book {
    /// Generates a book from the file at `file_path`, and fills in details from the
    /// parsed book metadata.
    ///
    /// # Arguments
    /// * ` file_path ` - The path to the file of interest.
    ///
    /// # Errors
    /// Will return an error if the provided path can not be read.
    /// Will panic if the title can not be set.
    pub fn from_path<P>(file_path: P) -> Result<Self, BookError>
    where
        P: Into<path::PathBuf>,
    {
        let path = file_path.into();

        let ext = if let Some(ext) = path.extension() {
            ext
        } else {
            return Err(BookError::FileError);
        };

        let book_type = BookFormat::try_from(ext)?;

        let (reader, hash, file_size) = {
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
                BufReader::with_capacity(bytes_to_read, file),
                res.into(),
                len,
            )
        };

        let mut book = Book {
            format: book_type,
            path,
            hash,
            file_size,
            title: None,
            identifier: None,
            language: None,
            authors: None,
            translators: None,
            description: None,
            id: None,
            free_tags: HashSet::new(),
            named_tags: HashMap::new(),
            series: None,
        };

        if let Ok(mut metadata_filler) = book.format.metadata_filler(reader) {
            metadata_filler.take_title(&mut book.title);
            metadata_filler.take_authors(&mut book.authors);
            metadata_filler.take_description(&mut book.description);
            metadata_filler.take_language(&mut book.language);
            metadata_filler.take_identifier(&mut book.identifier);
        }

        if book.title.is_none() {
            let file_name = if let Some(file_name) = book.path.file_name() {
                file_name
            } else {
                return Err(BookError::FileError);
            };

            book.title = Some(
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

    pub fn format(&self) -> &BookFormat {
        &self.format
    }
}
