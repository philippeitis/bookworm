use std::cmp::Ordering;
use std::collections::HashMap;
use std::str::FromStr;
use std::{fmt, path};

use serde::{Deserialize, Serialize};

use crate::record::{BookError, BookVariant};

/// Identifies the columns a Book provides.
pub(crate) enum ColumnIdentifier {
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
pub(crate) struct RawBook {
    pub(crate) title: Option<String>,
    pub(crate) authors: Option<Vec<String>>,
    pub(crate) series: Option<(String, Option<f32>)>,
    pub(crate) description: Option<String>,
    variants: Option<Vec<BookVariant>>,
    extended_tags: Option<HashMap<String, String>>,
}

impl RawBook {
    /// Generates a book from the file at the specified `file_path`, and assigns the given ID
    /// by default (note that the ID can not be changed).
    /// To match books to their respective parsers, it is assumed that the extension is
    /// the correct file format, case-insensitive.
    /// Metadata will be generated from the file and corresponding parser.
    ///
    /// # Arguments
    /// * ` file_path ` - The path to the file of interest.
    /// * ` id ` - the id to set.
    ///
    /// # Errors
    /// Will return an error if the provided file_path does not lead to a file.
    pub(crate) fn generate_from_file<P>(file_path: P) -> Result<RawBook, BookError>
    where
        P: AsRef<path::Path>,
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

        let mut book = RawBook {
            title: None,
            authors: None,
            series: None,
            variants: Some(variants.clone()),
            extended_tags: None,
            description: None,
        };

        for variant in variants.iter() {
            if book.title.is_none() {
                book.title = variant.local_title.clone();
            }

            if book.authors.is_none() {
                book.authors = variant.additional_authors.clone();
            }

            if book.description.is_none() {
                book.description = variant.description.clone();
            }
        }

        Ok(book)
    }
}

impl RawBook {
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

    pub(crate) fn get_description(&self) -> Option<&String> {
        self.description.as_ref()
    }

    pub(crate) fn get_column_or<S: AsRef<str>>(
        &self,
        column: &ColumnIdentifier,
        default: S,
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
            ColumnIdentifier::Description => {
                if let Some(desc) = &self.description {
                    desc.clone()
                } else {
                    default.as_ref().to_string()
                }
            }
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
            _ => default.as_ref().to_owned(),
        }
    }
}

impl RawBook {
    /// Sets specified columns, to the specified value. Titles will be stored directly,
    /// authors will be stored as a list containing a single author.
    /// ID and Variants can not be modified through set_column.
    /// Series will be parsed to extract an index - strings in the form "series ... [num]"
    /// will be parsed as ("series ...", num).
    ///
    /// All remaining arguments will be stored literally.
    ///
    /// # Arguments
    /// * ` column ` - the column of interest.
    /// * ` value ` - the value to store into the column.
    pub(crate) fn set_column<S: AsRef<str>>(
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
            ColumnIdentifier::ID => Ordering::Equal,
            c => self.get_column_or(c, "").cmp(&other.get_column_or(c, "")),
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

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
/// A raw_book, with an associated ID.
pub(crate) struct Book {
    id: u32,
    raw_book: RawBook,
}

impl Book {
    pub(crate) fn get_authors(&self) -> Option<&[String]> {
        self.raw_book.get_authors()
    }

    pub(crate) fn get_title(&self) -> Option<&str> {
        self.raw_book.get_title()
    }

    #[allow(dead_code)]
    pub(crate) fn get_series(&self) -> Option<&(String, Option<f32>)> {
        self.raw_book.get_series()
    }

    pub(crate) fn get_variants(&self) -> Option<&[BookVariant]> {
        self.raw_book.get_variants()
    }

    pub(crate) fn get_extended_columns(&self) -> Option<&HashMap<String, String>> {
        self.raw_book.get_extended_columns()
    }

    pub(crate) fn get_description(&self) -> Option<&String> {
        self.raw_book.get_description()
    }

    pub(crate) fn get_column_or<S: AsRef<str>>(
        &self,
        column: &ColumnIdentifier,
        default: S,
    ) -> String {
        match column {
            ColumnIdentifier::ID => self.id.to_string(),
            x => self.raw_book.get_column_or(x, default),
        }
    }

    #[allow(dead_code)]
    pub(crate) fn inner(&self) -> &RawBook {
        &self.raw_book
    }
}

impl Book {
    /// Generates a book with the given ID - intended to be used as a placeholder
    /// to allow reducing the run-time of specific operations.
    ///
    /// # Arguments
    /// * ` id ` - the id to assign this book.
    pub(crate) fn with_id(id: u32) -> Book {
        Book {
            id,
            raw_book: RawBook::default(),
        }
    }

    /// Generates a book from the file at the specified `file_path`, and assigns the given ID
    /// by default (note that the ID can not be changed).
    /// To match books to their respective parsers, it is assumed that the extension is
    /// the correct file format, case-insensitive.
    /// Metadata will be generated from the file and corresponding parser.
    ///
    /// # Arguments
    /// * ` file_path ` - The path to the file of interest.
    /// * ` id ` - the id to set.
    ///
    /// # Errors
    /// Will return an error if the provided file_path does not lead to a file.
    pub(crate) fn from_raw_book(raw_book: RawBook, id: u32) -> Book {
        Book { id, raw_book }
    }

    pub(crate) fn get_id(&self) -> u32 {
        self.id
    }
}

impl Book {
    /// Sets specified columns, to the specified value. Titles will be stored directly,
    /// authors will be stored as a list containing a single author.
    /// ID and Variants can not be modified through set_column.
    /// Series will be parsed to extract an index - strings in the form "series ... [num]"
    /// will be parsed as ("series ...", num).
    ///
    /// All remaining arguments will be stored literally.
    ///
    /// # Arguments
    /// * ` column ` - the column of interest.
    /// * ` value ` - the value to store into the column.
    pub(crate) fn set_column<S: AsRef<str>>(
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
    pub(crate) fn cmp_column(&self, other: &Self, column: &ColumnIdentifier) -> Ordering {
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
    pub(crate) fn merge_mut(&mut self, other: Self) {
        self.raw_book.merge_mut(other.raw_book)
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
            assert_eq!(book.get_column_or(&col, ""), expected.to_owned());
        }
    }
}
