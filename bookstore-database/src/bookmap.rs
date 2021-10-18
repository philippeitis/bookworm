use std::collections::{HashMap, HashSet};
use std::sync::Arc;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};
use unicase::UniCase;

use bookstore_records::book::{BookID, ColumnIdentifier, RecordError};
use bookstore_records::{Book, Edit};

/// `BookCache` acts as an intermediate caching layer between the backend database
/// and the front-end UI - allowing books that are already in memory to be provided
/// without going through the database.
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Clone, Debug)]
pub(crate) struct BookCache {
    books: HashMap<BookID, Arc<Book>>,
    cols: HashSet<UniCase<String>>,
}

impl Default for BookCache {
    fn default() -> Self {
        BookCache {
            books: HashMap::default(),
            cols: ["title", "authors", "series", "id", "description"]
                .iter()
                .map(|s| s.to_string())
                .map(UniCase::new)
                .collect(),
        }
    }
}

impl BookCache {
    pub(crate) fn from_values_unchecked(
        books: HashMap<BookID, Arc<Book>>,
        cols: HashSet<UniCase<String>>,
    ) -> Self {
        BookCache { books, cols }
    }

    pub(crate) fn insert_book(&mut self, book: Arc<Book>) {
        self.books.insert(book.id(), book);
    }

    pub(crate) fn insert_columns<I: IntoIterator<Item = UniCase<String>>>(&mut self, columns: I) {
        self.cols.extend(columns.into_iter());
    }

    pub fn remove_book(&mut self, id: BookID) {
        self.books.remove(&id);
    }

    pub fn remove_books(&mut self, ids: &HashSet<BookID>) {
        self.books.retain(|id, _| !ids.contains(id));
    }

    pub fn clear(&mut self) {
        self.books.clear();
    }

    pub fn get_book(&self, id: BookID) -> Option<Arc<Book>> {
        self.books.get(&id).cloned()
    }

    pub fn edit_book_with_id(
        &mut self,
        id: BookID,
        edits: &[(ColumnIdentifier, Edit)],
    ) -> Result<bool, RecordError> {
        match self.books.get_mut(&id) {
            None => Ok(false),
            Some(book) => {
                for (column, edit) in edits {
                    Arc::make_mut(book).edit_column(&column, edit)?;
                    match column {
                        ColumnIdentifier::NamedTag(x) => {
                            self.cols.insert(UniCase::new(x.to_owned()));
                        }
                        _ => {}
                    }
                }
                Ok(true)
            }
        }
    }

    /// Merges all books with matching titles and authors (case insensitive), in no
    /// particular order. Books that are merged will not necessarily free IDs no longer in use.
    /// Returns a Vec containing BookID pairs, where the first BookID is merged into, and exists,
    /// and the second BookID was merged from, and deleted.
    pub fn merge_similar_merge_ids(&mut self) -> Vec<(BookID, BookID)> {
        let mut ref_map: HashMap<(String, String), BookID> = HashMap::new();
        let mut merges = vec![];
        for book in self.books.values() {
            if let Some(title) = book.title() {
                if let Some(authors) = book.authors() {
                    let a: String = authors.join(", ").to_ascii_lowercase();
                    let val = (title.to_ascii_lowercase(), a);
                    if let Some(id) = ref_map.get(&val) {
                        merges.push((*id, book.id()));
                    } else {
                        ref_map.insert(val, book.id());
                    }
                }
            }
        }

        let placeholder = Arc::new(Book::placeholder());
        for (b1, b2_id) in merges.iter() {
            // Placeholder allows for O(n) time book removal while maintaining sort
            // and getting owned copy of book.
            let b2 = self.books.insert(*b2_id, placeholder.clone());
            // b1, b2 always exist: ref_map only stores b1, and any given b2 can only merge into
            // a b1, and never a b2, and a b1 never merges into b2, since b1 comes first.
            if let Some(b1) = self.books.get_mut(b1) {
                if let Some(b2) = b2 {
                    Arc::make_mut(b1).merge_mut(&b2);
                }
            }
        }
        self.books.retain(|_, book| book.is_placeholder());
        merges
    }

    pub(crate) fn has_column(&self, col: &UniCase<String>) -> bool {
        self.cols.contains(col)
    }
}
