use regex::{Error as RegexError, Regex};
use sublime_fuzzy::best_match;

use bookstore_records::book::ColumnIdentifier;
use bookstore_records::Book;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum SearchMode {
    Regex,
    ExactSubstring,
    Default,
}

#[derive(Debug)]
pub enum Error {
    Regex(RegexError),
}

impl From<RegexError> for Error {
    fn from(e: RegexError) -> Self {
        Error::Regex(e)
    }
}
#[derive(Debug, Eq, PartialEq, Clone)]
pub struct Search {
    pub mode: SearchMode,
    pub column: ColumnIdentifier,
    pub search: String,
}

impl Search {
    pub(crate) fn into_matcher(self) -> Result<Box<dyn Matcher>, Error> {
        Ok(match self.mode {
            SearchMode::Regex => Box::new(RegexMatcher::new(self.column, self.search)?),
            SearchMode::ExactSubstring => {
                Box::new(ExactSubstringMatcher::new(self.column, self.search)?)
            }
            SearchMode::Default => Box::new(DefaultMatcher::new(self.column, self.search)?),
        })
    }
}

pub trait Matcher {
    fn new(column: ColumnIdentifier, search: String) -> Result<Self, Error>
    where
        Self: Sized;

    fn is_match(&self, book: &Book) -> bool;
}

pub struct RegexMatcher {
    column: ColumnIdentifier,
    regex: Regex,
}

impl Matcher for RegexMatcher {
    fn new(column: ColumnIdentifier, search: String) -> Result<Self, Error> {
        Ok(RegexMatcher {
            column,
            regex: Regex::new(&search)?,
        })
    }

    #[inline(always)]
    fn is_match(&self, book: &Book) -> bool {
        self.regex
            .is_match(&book.get_column(&self.column).unwrap_or_else(String::new))
    }
}

pub struct ExactSubstringMatcher {
    column: ColumnIdentifier,
    regex: Regex,
}

impl Matcher for ExactSubstringMatcher {
    fn new(column: ColumnIdentifier, search: String) -> Result<Self, Error> {
        Ok(ExactSubstringMatcher {
            column,
            regex: Regex::new(&regex::escape(&search))?,
        })
    }

    #[inline(always)]
    fn is_match(&self, book: &Book) -> bool {
        self.regex
            .is_match(&book.get_column(&self.column).unwrap_or_else(String::new))
    }
}

pub struct DefaultMatcher {
    column: ColumnIdentifier,
    string: String,
}

impl Matcher for DefaultMatcher {
    fn new(column: ColumnIdentifier, search: String) -> Result<Self, Error> {
        Ok(DefaultMatcher {
            column,
            string: search,
        })
    }

    #[inline(always)]
    fn is_match(&self, book: &Book) -> bool {
        best_match(
            &self.string,
            &book.get_column(&self.column).unwrap_or_else(String::new),
        )
        .is_some()
    }
}
