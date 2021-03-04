use std::cmp::Ordering;
use std::collections::HashMap;
use std::str::FromStr;
use std::{fmt, path};

use serde::{Deserialize, Serialize};

use crate::{BookError, BookVariant};

/// Identifies the columns a Book provides.
pub enum ColumnIdentifier {
    Title,
    Author,
    Series,
    ID,
    Variants,
    Description,
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
            "description" => Self::Description,
            _ => Self::ExtendedTag(val.as_ref().to_owned()),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
/// The struct which contains the major fields a book will have, a set of variants,
/// which corresponds to particular file formats of this book (eg. a EPUB and MOBI version),
/// or even differing realizations of the book (eg. a French and English of the same book).
/// Also provides storage for additional tags which are not specified here.
pub struct RawBook {
    pub title: Option<String>,
    pub authors: Option<Vec<String>>,
    pub series: Option<(String, Option<f32>)>,
    pub description: Option<String>,
    variants: Option<Vec<BookVariant>>,
    extended_tags: Option<HashMap<String, String>>,
}

impl RawBook {
    /// Generates a book from the file at the specified `path`.
    /// To match books to their respective parsers, it is assumed that the extension is
    /// the correct file format, case-insensitive.
    /// Metadata will be generated from the file and corresponding parser.
    ///
    /// # Arguments
    /// * ` path ` - The path to the file of interest.
    ///
    /// # Errors
    /// Will return an error if `path` is not a file.
    pub fn generate_from_file<P>(path: P) -> Result<RawBook, BookError>
    where
        P: AsRef<path::Path>,
    {
        let path = {
            let path = path.as_ref();
            match path.canonicalize() {
                Ok(p) => p,
                Err(_) => path.to_path_buf(),
            }
        };

        if !path.is_file() {
            return Err(BookError::FileError);
        }

        let mut book = RawBook {
            title: None,
            authors: None,
            series: None,
            variants: None,
            extended_tags: None,
            description: None,
        };

        if let Ok(mut variant) = BookVariant::generate_from_file(path) {
            variant.id = Some(0);
            if book.title.is_none() {
                book.title = std::mem::take(&mut variant.local_title);
            }

            if book.authors.is_none() {
                book.authors = std::mem::take(&mut variant.additional_authors);
            }

            if book.description.is_none() {
                book.description = std::mem::take(&mut variant.description);
            }
            book.variants = Some(vec![variant]);
        }

        Ok(book)
    }
}

impl RawBook {
    pub fn get_authors(&self) -> Option<&[String]> {
        self.authors.as_deref()
    }

    pub fn get_title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    pub fn get_series(&self) -> Option<&(String, Option<f32>)> {
        self.series.as_ref()
    }

    pub fn get_variants(&self) -> Option<&[BookVariant]> {
        self.variants.as_deref()
    }

    pub fn get_extended_columns(&self) -> Option<&HashMap<String, String>> {
        self.extended_tags.as_ref()
    }

    pub fn get_description(&self) -> Option<&String> {
        self.description.as_ref()
    }

    pub fn get_column(&self, column: &ColumnIdentifier) -> Option<String> {
        Some(match column {
            ColumnIdentifier::Title => self.get_title()?.to_string(),
            ColumnIdentifier::Author => self.get_authors()?.join(", "),
            ColumnIdentifier::Series => {
                let (series_name, nth_in_series) = self.get_series()?;
                if let Some(nth_in_series) = nth_in_series {
                    format!("{} [{}]", series_name, nth_in_series)
                } else {
                    series_name.clone()
                }
            }
            ColumnIdentifier::Description => self.description.as_ref()?.to_string(),
            ColumnIdentifier::ExtendedTag(x) => self.extended_tags.as_ref()?.get(x)?.to_string(),
            _ => return None,
        })
    }
}

impl RawBook {
    /// Sets specified columns, to the specified value. Titles will be stored directly,
    /// authors will be stored as a list containing a single author.
    /// ID and Variants can not be modified through `set_column`.
    /// Series will be parsed to extract an index - strings in the form "series ... [num]"
    /// will be parsed as ("series ...", num).
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
    ) -> Result<(), BookError> {
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
                    d.insert(column.to_owned(), value.to_owned());
                } else {
                    let mut d = HashMap::new();
                    d.insert(column.to_owned(), value.to_owned());
                    self.extended_tags = Some(d);
                }
            }
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
            ColumnIdentifier::Series => {
                let s_series = self.get_series();
                let o_series = other.get_series();
                if s_series.eq(&o_series) {
                    Ordering::Equal
                } else if let Some((s_st, s_ind)) = s_series {
                    if let Some((o_st, o_ind)) = o_series {
                        if s_st.eq(o_st) {
                            match s_ind.partial_cmp(o_ind) {
                                Some(o) => o,
                                None => match s_ind.map(|f| f.is_nan()) {
                                    Some(true) => match o_ind.map(|f| f.is_nan()) {
                                        Some(true) => Ordering::Equal,
                                        Some(false) => Ordering::Less,
                                        None => {
                                            unreachable!("Neither s_ind nor o_ind can be None.")
                                        }
                                    },
                                    Some(false) => Ordering::Greater,
                                    None => unreachable!("Neither s_ind nor o_ind can be None."),
                                },
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
            ColumnIdentifier::ID => Ordering::Equal,
            ColumnIdentifier::Title => self.get_title().cmp(&other.get_title()),
            c => self.get_column(c).cmp(&other.get_column(c)),
        }
    }
}

impl RawBook {
    /// Merges self with other, assuming that other is in fact a variant of self.
    /// Missing metadata will be utilized from other, and `self` variants will be extended
    /// to include `other` variants.
    ///
    /// # Arguments
    /// * ` other ` - Another book with more useful metadata, to be merged with self
    pub(crate) fn merge_mut(&mut self, other: &Self) {
        if self.title.is_none() {
            self.title = other.title.clone();
        }
        if self.authors.is_none() {
            self.authors = other.authors.clone();
        }
        if self.series.is_none() {
            self.series = other.series.clone();
        }

        if let Some(other_variants) = &other.variants {
            match self.variants.as_mut() {
                None => self.variants = Some(other_variants.clone()),
                Some(variants) => variants.extend_from_slice(other_variants),
            }
        }

        if let Some(other_tags) = other.extended_tags.clone() {
            match self.extended_tags.as_mut() {
                None => self.extended_tags = Some(other_tags),
                Some(tags) => tags.extend(other_tags),
            }
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
/// A `RawBook`, and associated ID.
pub struct Book {
    id: u32,
    raw_book: RawBook,
}

impl Book {
    pub fn get_authors(&self) -> Option<&[String]> {
        self.raw_book.get_authors()
    }

    pub fn get_title(&self) -> Option<&str> {
        self.raw_book.get_title()
    }

    #[allow(dead_code)]
    pub fn get_series(&self) -> Option<&(String, Option<f32>)> {
        self.raw_book.get_series()
    }

    pub fn get_variants(&self) -> Option<&[BookVariant]> {
        self.raw_book.get_variants()
    }

    pub fn get_extended_columns(&self) -> Option<&HashMap<String, String>> {
        self.raw_book.get_extended_columns()
    }

    pub fn get_description(&self) -> Option<&String> {
        self.raw_book.get_description()
    }

    pub fn get_column(&self, column: &ColumnIdentifier) -> Option<String> {
        match column {
            ColumnIdentifier::ID => Some(self.id.to_string()),
            x => self.raw_book.get_column(x),
        }
    }

    #[allow(dead_code)]
    pub fn inner(&self) -> &RawBook {
        &self.raw_book
    }
}

impl Book {
    /// Generates a book with the given ID - intended to be used as a placeholder
    /// to allow reducing the run-time of specific operations.
    ///
    /// # Arguments
    /// * ` id ` - the id to assign this book.
    pub fn with_id(id: u32) -> Book {
        Book {
            id,
            raw_book: RawBook::default(),
        }
    }

    /// Creates a `Book` with the given ID and core metadata.
    pub fn from_raw_book(id: u32, raw_book: RawBook) -> Book {
        Book { id, raw_book }
    }

    pub fn get_id(&self) -> u32 {
        self.id
    }
}

impl Book {
    /// Sets specified columns, to the specified value. Titles will be stored directly,
    /// authors will be stored as a list containing a single author.
    /// ID and Variants can not be modified through `set_column`.
    /// Series will be parsed to extract an index - strings in the form "series ... [num]"
    /// will be parsed as ("series ...", num).
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
    ) -> Result<(), BookError> {
        self.raw_book.set_column(column, value)
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
            ColumnIdentifier::ID => self.get_id().cmp(&other.get_id()),
            col => self.raw_book.cmp_column(&other.raw_book, col),
        }
    }
}

impl Book {
    /// Merges self with other, assuming that other is in fact a variant of self.
    /// Missing metadata will be utilized from other, and `self` variants will be extended
    /// to include `other` variants.
    ///
    /// # Arguments
    /// * ` other ` - Another book with more useful metadata, to be merged with self
    pub fn merge_mut(&mut self, other: &Self) {
        self.raw_book.merge_mut(&other.raw_book)
    }
}

impl fmt::Display for Book {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if let Some(title) = &self.raw_book.title {
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

    #[test]
    fn test_setting_columns() {
        let mut book = Book::with_id(0);
        let test_sets = [
            ("title", "hello", Ok(()), "hello"),
            ("authors", "world", Ok(()), "world"),
            ("id", "5", Err(BookError::ImmutableColumnError), "0"),
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
            assert_eq!(book.get_column(&col), Some(expected.to_string()));
        }
    }
}
