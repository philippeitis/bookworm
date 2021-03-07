use crate::search::{Error, Search};
use bookstore_records::{
    book::{BookID, ColumnIdentifier, RawBook},
    Book, BookError,
};
use indexmap::map::IndexMap;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::convert::TryFrom;
use std::sync::{Arc, RwLock};
use unicase::UniCase;

macro_rules! book {
    ($book: ident) => {
        $book.as_ref().read().unwrap()
    };
}

macro_rules! book_mut {
    ($book: ident) => {
        $book.as_ref().write().unwrap()
    };
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub(crate) struct BookMap {
    max_id: u64,
    books: IndexMap<BookID, Arc<RwLock<Book>>>,
    cols: HashSet<UniCase<String>>,
}

impl Default for BookMap {
    fn default() -> Self {
        BookMap {
            max_id: 1,
            books: IndexMap::default(),
            cols: ["title", "authors", "series", "id", "description"]
                .iter()
                .map(|s| s.to_string())
                .map(UniCase::new)
                .collect(),
        }
    }
}

impl BookMap {
    /// Return a monotonically increasing ID to use for a new
    /// book.
    ///
    /// # Errors
    /// Will panic if the ID can no longer be correctly increased.
    fn new_id(&mut self) -> BookID {
        let id = self.max_id;
        if self.max_id == u64::MAX {
            panic!(
                "Current ID is at maximum value of {} and can not be increased.",
                u32::MAX
            );
        }
        self.max_id += 1;
        BookID::try_from(id).unwrap()
    }
}

impl BookMap {
    #[allow(dead_code)]
    pub(crate) fn from_values_unchecked(
        books: IndexMap<BookID, Arc<RwLock<Book>>>,
        cols: HashSet<UniCase<String>>,
    ) -> Self {
        BookMap {
            max_id: 1,
            books,
            cols,
        }
    }
    pub(crate) fn insert_book_with_id(&mut self, book: Book) {
        self.books
            .insert(book.get_id(), Arc::new(RwLock::new(book)));
    }

    pub fn insert_raw_book(&mut self, book: RawBook) -> BookID {
        let id = self.new_id();
        self.insert_book_with_id(Book::from_raw_book(id, book));
        id
    }

    pub fn len(&self) -> usize {
        self.books.len()
    }

    pub fn remove_book(&mut self, id: BookID) {
        self.books.shift_remove(&id);
    }

    pub fn remove_book_indexed(&mut self, index: usize) -> bool {
        self.books.shift_remove_index(index).is_some()
    }

    pub fn remove_books(&mut self, ids: &HashSet<BookID>) {
        self.books.retain(|id, _| !ids.contains(id));
    }

    pub fn clear(&mut self) {
        self.books.clear();
    }

    pub fn get_book(&self, id: BookID) -> Option<Arc<RwLock<Book>>> {
        self.books.get(&id).cloned()
    }

    pub fn get_book_indexed(&self, index: usize) -> Option<Arc<RwLock<Book>>> {
        self.books.get_index(index).map(|(_, book)| book.clone())
    }

    pub fn get_all_books(&self) -> Vec<Arc<RwLock<Book>>> {
        self.books.values().cloned().collect()
    }

    pub fn edit_book_with_id<S0: AsRef<str>, S1: AsRef<str>>(
        &mut self,
        id: BookID,
        edits: &[(S0, S1)],
    ) -> Result<bool, BookError> {
        match self.books.get_mut(&id) {
            None => Ok(false),
            Some(book) => {
                for (column, new_value) in edits {
                    book_mut!(book).set_column(&column.as_ref().into(), new_value)?;
                    self.cols.insert(UniCase::new(column.as_ref().to_owned()));
                }
                Ok(true)
            }
        }
    }

    pub fn edit_book_indexed<S0: AsRef<str>, S1: AsRef<str>>(
        &mut self,
        index: usize,
        edits: &[(S0, S1)],
    ) -> Result<bool, BookError> {
        match self.books.get_index_mut(index) {
            None => Ok(false),
            Some((_, book)) => {
                for (column, new_value) in edits {
                    book_mut!(book).set_column(&column.as_ref().into(), new_value)?;
                    self.cols.insert(UniCase::new(column.as_ref().to_owned()));
                }
                Ok(true)
            }
        }
    }

    pub fn merge_similar(&mut self) -> HashSet<BookID> {
        let mut ref_map: HashMap<(String, String), BookID> = HashMap::new();
        let mut merges = vec![];
        for book in self.books.values() {
            let book = book!(book);
            if let Some(title) = book.get_title() {
                if let Some(authors) = book.get_authors() {
                    let a: String = authors.join(", ").to_ascii_lowercase();
                    let val = (title.to_ascii_lowercase(), a);
                    if let Some(id) = ref_map.get(&val) {
                        merges.push((*id, book.get_id()));
                    } else {
                        ref_map.insert(val, book.get_id());
                    }
                }
            }
        }

        let dummy = Arc::new(RwLock::new(Book::dummy()));
        for (b1, b2_id) in merges.iter() {
            // Dummy allows for O(n) time book removal while maintaining sort
            // and getting owned copy of book.
            let b2 = self.books.insert(*b2_id, dummy.clone());
            // b1, b2 always exist: ref_map only stores b1, and any given b2 can only merge into
            // a b1, and never a b2, and a b1 never merges into b2, since b1 comes first.
            if let Some(b1) = self.books.get_mut(b1) {
                if let Some(b2) = b2 {
                    book_mut!(b1).merge_mut(&book!(b2));
                }
            }
        }
        self.books.retain(|_, book| !book!(book).is_dummy());
        merges.into_iter().map(|(_, m)| m).collect()
    }

    pub fn find_matches(&self, searches: &[Search]) -> Result<Vec<Arc<RwLock<Book>>>, Error> {
        let mut results: Vec<_> = self.books.values().cloned().collect();
        for search in searches {
            let matcher = search.clone().into_matcher()?;
            results.retain(|book| matcher.is_match(&book!(book)));
        }
        Ok(results)
    }

    pub fn sort_books_by_col<S: AsRef<str>>(&mut self, col: S, reverse: bool) {
        let col = ColumnIdentifier::from(col);

        // Use some heuristic to sort in parallel when it would offer speedup -
        // parallel threads are slower for small sorts.
        if self.books.len() < 2500 {
            if reverse {
                self.books
                    .sort_by(|_, a, _, b| book!(b).cmp_column(&book!(a), &col))
            } else {
                self.books
                    .sort_by(|_, a, _, b| book!(a).cmp_column(&book!(b), &col))
            }
        } else if reverse {
            self.books
                .par_sort_by(|_, a, _, b| book!(b).cmp_column(&book!(a), &col))
        } else {
            self.books
                .par_sort_by(|_, a, _, b| book!(a).cmp_column(&book!(b), &col))
        }
    }

    pub fn sort_books_by_cols<S: AsRef<str>>(&mut self, cols: &[(S, bool)]) {
        let cols: Vec<_> = cols
            .iter()
            .map(|(c, r)| (ColumnIdentifier::from(c), *r))
            .collect();

        // Use some heuristic to sort in parallel when it would offer speedup -
        // parallel threads are slower for small sorts.
        if self.books.len() < 2500 {
            self.books
                .sort_by(|_, a, _, b| book!(a).cmp_columns(&book!(b), &cols))
        } else {
            self.books
                .par_sort_by(|_, a, _, b| book!(a).cmp_columns(&book!(b), &cols))
        }
    }

    pub fn init_columns(&mut self) {
        let mut c = HashSet::new();

        for &col in &["title", "authors", "series", "id", "description"] {
            c.insert(col.to_owned());
        }

        for book in self.books.values() {
            let book = book!(book);
            if let Some(e) = book.get_extended_columns() {
                for key in e.keys() {
                    if !c.contains(key) {
                        c.insert(key.to_owned());
                    }
                }
            }
        }

        self.cols = c.into_iter().map(UniCase::new).collect();
    }

    pub(crate) fn has_column(&self, col: &UniCase<String>) -> bool {
        self.cols.contains(col)
    }
}
