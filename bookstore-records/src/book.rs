use std::cmp::Ordering;
use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::series::Series;
use crate::BookVariant;
use crate::ColumnOrder;

pub type BookID = std::num::NonZeroU64;

#[derive(Debug, PartialEq, Eq)]
pub enum RecordError {
    ImmutableColumn,
}

/// Identifies the columns a Book provides. Intended to provide a way to access arbitrary columns,
/// for the sake of bulk operations which access specific columns.
#[derive(Debug, Eq, PartialEq, Clone)]
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

pub fn str_to_series(value: &str) -> Option<(String, Option<f32>)> {
    if value.ends_with(']') {
        // Replace with rsplit_once when stable.
        let mut words = value.rsplitn(2, char::is_whitespace);
        if let Some(id) = words.next() {
            if let Some(series) = words.next() {
                if let Ok(id) = f32::from_str(id.replace(&['[', ']'][..], "").as_str()) {
                    return Some((series.to_owned(), Some(id)));
                }
            }
        }
    }
    Some((value.to_owned(), None))
}

#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Clone, Debug, PartialEq, Default)]
/// Stores the metadata for a specific book, with an associated ID, variants, and fields shared
/// between different variants.
///
/// Variants can be different file formats of the same book, or different realizations of the same
/// book.
pub struct Book {
    pub id: Option<BookID>,
    pub title: Option<String>,
    // TODO: Should authors be a hashset?
    pub authors: Option<Vec<String>>,
    pub series: Option<Series>,
    pub description: Option<String>,
    pub variants: Vec<BookVariant>,
    pub extended_tags: HashMap<String, String>,
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

    pub fn variants(&self) -> &[BookVariant] {
        &self.variants
    }

    pub fn tags(&self) -> &HashMap<String, String> {
        &self.extended_tags
    }

    pub fn description(&self) -> Option<&String> {
        self.description.as_ref()
    }

    pub fn get_column(&self, column: &ColumnIdentifier) -> Option<String> {
        Some(match column {
            ColumnIdentifier::ID => self.id?.to_string(),
            ColumnIdentifier::Title => self.title()?.to_string(),
            ColumnIdentifier::Author => self.authors()?.join(", "),
            ColumnIdentifier::Series => self.series()?.to_string(),
            ColumnIdentifier::Description => self.description.as_ref()?.to_string(),
            ColumnIdentifier::ExtendedTag(x) => self.extended_tags.get(x)?.to_string(),
            _ => return None,
        })
    }

    pub fn push_variant(&mut self, variant: BookVariant) {
        self.variants.push(variant);
    }

    /// Merges self with other, assuming that other is in fact a variant of self.
    /// Missing metadata will be utilized from other, and `self` variants will be extended
    /// to include `other` variants.
    ///
    /// # Arguments
    /// * ` other ` - Another book with more useful metadata, to be merged with self
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

        self.variants.extend_from_slice(&other.variants);
        self.extended_tags.extend(other.extended_tags.clone());
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

    /// Creates a `Book` with the given ID and core metadata.
    pub fn from_variant(id: BookID, mut variant: BookVariant) -> Book {
        variant.id = Some(0);
        Book {
            id: Some(id),
            title: std::mem::take(&mut variant.local_title),
            authors: std::mem::take(&mut variant.additional_authors),
            series: None,
            description: std::mem::take(&mut variant.description),
            extended_tags: std::mem::take(&mut variant.tags),
            variants: vec![variant],
        }
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
    /// Series will be parsed to extract an index - strings in the form "series [num]"
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
            ColumnIdentifier::ID | ColumnIdentifier::Variants => {
                return Err(RecordError::ImmutableColumn);
            }
            ColumnIdentifier::Series => {
                self.series = Series::from_str(&value).ok();
            }
            ColumnIdentifier::ExtendedTag(column) => {
                self.extended_tags
                    .insert(column.to_owned(), value.to_owned());
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
            assert_eq!(book.get_column(&col), Some(expected.to_string()));
        }
    }
}
