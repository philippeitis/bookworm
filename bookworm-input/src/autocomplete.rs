use std::path::PathBuf;

use glob::{glob, PatternError};

pub struct AutoCompleter<S> {
    word_len: usize,
    candidates: RingFilter<S>,
}

#[derive(Debug)]
pub enum AutoCompleteError {
    Glob(PatternError),
}

impl From<glob::PatternError> for AutoCompleteError {
    fn from(e: PatternError) -> Self {
        Self::Glob(e)
    }
}

impl AutoCompleter<PathBuf> {
    /// Returns a new `AutoCompleter`, which will fill in entries from the path fragment.
    ///
    /// # Arguments
    /// * ` word ` - The word to provide autofills for.
    ///
    /// # Errors
    /// If the glob fails, an error will be returned.
    pub fn new<S: Into<String>>(word: S) -> Result<Self, AutoCompleteError> {
        let word = word.into();
        let word_len = word.len();
        let glob_str = word + "*";

        let mut p: Vec<_> = glob(&glob_str)?
            .into_iter()
            .filter_map(Result::ok)
            .collect();
        p.sort();
        Ok(AutoCompleter {
            word_len,
            candidates: RingFilter::new(p),
        })
    }

    /// Returns the next path which is at least as long as the original,
    /// or None if no such path exists.
    ///
    /// If at least one such path exists, this function will always
    /// return a value.
    pub fn next_word(&mut self) -> Option<&PathBuf> {
        self.next_word_by(|_| true)
    }

    /// Returns the next path which is at least as long as the original
    /// and satisfies the provided predicate, or None if no such path
    /// exists.
    ///
    /// If at least one such path exists, this function will always
    /// return a value.
    ///
    /// # Arguments
    /// * ` predicate ` - A predicate which returns true if the given path should
    ///             be returned, otherwise false.
    pub fn next_word_by<P: FnMut(&PathBuf) -> bool>(
        &mut self,
        mut predicate: P,
    ) -> Option<&PathBuf> {
        let word_len = self.word_len;
        self.candidates
            .next_item_by(|path| path.as_os_str().len() >= word_len && predicate(path))
    }
}

struct RingFilter<S> {
    items: Vec<S>,
    index: usize,
}

impl<S> RingFilter<S> {
    fn new(items: Vec<S>) -> Self {
        RingFilter { items, index: 0 }
    }

    /// Returns the next item which satisfies the predicate, starting from the
    /// the item immediately after the previous item returned (or at the first
    /// item), in order of appearance, or if no item satisfying the predicate is
    /// found after the previous item, the next item is selected from the items
    /// between the first item and the previous value, inclusive.
    ///
    /// If at least one item exists that satisfies the predicate, a value will
    /// always be returned.
    ///
    /// # Arguments
    /// * ` predicate ` - A predicate which returns true if the given item should
    ///             be returned, otherwise false.
    fn next_item_by<P: FnMut(&S) -> bool>(&mut self, mut predicate: P) -> Option<&S> {
        if self.items.is_empty() {
            return None;
        }

        self.index %= self.items.len();
        let (p2, p1) = self.items.split_at(self.index);

        for item in p1.iter().chain(p2.iter()) {
            self.index += 1;
            if predicate(item) {
                return Some(&item);
            }
        }
        None
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_empty_ring_works_ok() {
        let mut a: RingFilter<u8> = RingFilter::new(vec![]);
        assert!(a.next_item_by(|_| true).is_none());
    }

    #[test]
    fn test_get_ring() {
        let mut a = RingFilter::new(vec![0u8, 1, 2, 3, 4, 5]);
        assert_eq!(a.next_item_by(|&i| i % 2 == 0), Some(&0));
        assert_eq!(a.next_item_by(|&i| i % 2 == 0), Some(&2));
        assert_eq!(a.next_item_by(|&i| i % 2 == 0), Some(&4));
        assert_eq!(a.next_item_by(|&i| i % 2 == 0), Some(&0));

        assert_eq!(a.next_item_by(|&i| i == 6), None);
        assert_eq!(a.next_item_by(|&i| i == 6), None);
        assert_eq!(a.next_item_by(|&i| i == 6), None);
        assert_eq!(a.next_item_by(|&i| i == 6), None);
        assert_eq!(a.next_item_by(|&i| i == 6), None);

        assert_eq!(a.next_item_by(|&i| i == 0), Some(&0));
        assert_eq!(a.next_item_by(|&i| i == 0), Some(&0));
        assert_eq!(a.next_item_by(|&i| i == 0), Some(&0));
        assert_eq!(a.next_item_by(|&i| i == 0), Some(&0));
        assert_eq!(a.next_item_by(|&i| i == 0), Some(&0));
    }
}
