use std::path::PathBuf;

use glob::glob;

pub(crate) struct AutoCompleter<S> {
    word: String,
    possibilities: GetRing<S>,
}

impl AutoCompleter<PathBuf> {
    /// Returns a new AutoCompleter, which will fill in entries from the path fragment.
    ///
    /// # Arguments
    ///
    /// * ` word ` - The word to provide autofills for.
    pub(crate) fn new<S: AsRef<str>>(word: S) -> Result<Self, ()> {
        let word = word.as_ref().to_string();
        let glob_str = word.clone() + "*";

        if let Ok(paths) = glob(glob_str.as_str()) {
            let mut p: Vec<_> = paths.into_iter().filter_map(Result::ok).collect();
            p.sort();
            Ok(AutoCompleter {
                word,
                possibilities: GetRing::new(p),
            })
        } else {
            Err(())
        }
    }

    /// Returns the next path which is at least as long as the original, or None if none can be
    /// found. If at least one such word exists, this function will always return a value.
    pub(crate) fn get_next_word(&mut self) -> Option<&PathBuf> {
        self.get_next_word_by(|_| true)
    }

    /// Returns the next path which is at least as long as the original, and matches the provided
    /// predicate, or None if none can be found. If at least one such word exists,
    /// this function will always return a value.
    ///
    /// # Arguments
    ///
    /// * ` p ` - A predicate to test the paths.
    pub(crate) fn get_next_word_by(&mut self, p: impl Fn(&PathBuf) -> bool) -> Option<&PathBuf> {
        let word_len = self.word.len();
        self.possibilities
            .get_next_item_by(&|path| path.as_os_str().len() >= word_len && p(path))
    }
}

pub(crate) struct GetRing<S> {
    possibilities: Vec<S>,
    curr_state: usize,
}

impl<S> GetRing<S> {
    pub(crate) fn new(possibilities: Vec<S>) -> Self {
        GetRing {
            possibilities,
            curr_state: 0,
        }
    }

    pub(crate) fn get_next_item_by(&mut self, p: &dyn Fn(&S) -> bool) -> Option<&S> {
        self.curr_state %= self.possibilities.len();
        let init_state = self.curr_state;
        let (p2, p1) = self.possibilities.split_at(init_state);
        for item in p1.iter().chain(p2.iter()) {
            self.curr_state += 1;
            if p(item) {
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
    fn test_get_ring() {
        let mut a = GetRing::new(vec![0u8, 1, 2, 3, 4, 5]);
        assert_eq!(a.get_next_item_by(&|&i| i % 2 == 0), Some(&0));
        assert_eq!(a.get_next_item_by(&|&i| i % 2 == 0), Some(&2));
        assert_eq!(a.get_next_item_by(&|&i| i % 2 == 0), Some(&4));
        assert_eq!(a.get_next_item_by(&|&i| i % 2 == 0), Some(&0));

        assert_eq!(a.get_next_item_by(&|&i| i == 6), None);
        assert_eq!(a.get_next_item_by(&|&i| i == 6), None);
        assert_eq!(a.get_next_item_by(&|&i| i == 6), None);
        assert_eq!(a.get_next_item_by(&|&i| i == 6), None);
        assert_eq!(a.get_next_item_by(&|&i| i == 6), None);

        assert_eq!(a.get_next_item_by(&|&i| i == 0), Some(&0));
        assert_eq!(a.get_next_item_by(&|&i| i == 0), Some(&0));
        assert_eq!(a.get_next_item_by(&|&i| i == 0), Some(&0));
        assert_eq!(a.get_next_item_by(&|&i| i == 0), Some(&0));
        assert_eq!(a.get_next_item_by(&|&i| i == 0), Some(&0));
    }
}
