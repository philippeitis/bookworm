use std::path::PathBuf;

use glob::glob;

#[derive(Clone)]
pub(crate) struct AutoCompleter<S> {
    word: String,
    possibilities: Vec<S>,
    curr_state: usize,
}

impl AutoCompleter<PathBuf> {
    /// Returns a new AutoCompleter, which will fill in entries from the path fragment.
    ///
    /// # Arguments
    ///
    /// * ` word ` - The word to provide autofills for.
    pub(crate) fn new<S: AsRef<str>>(word: S) -> Result<Self, ()> {
        let word = word.as_ref().to_string();
        let mut glob_str = word.clone();
        glob_str.push('*');

        if let Ok(paths) = glob(glob_str.as_str()) {
            let mut p: Vec<_> = paths.into_iter().filter_map(Result::ok).collect();
            p.sort();
            Ok(AutoCompleter {
                word,
                possibilities: p,
                curr_state: 0,
            })
        } else {
            Err(())
        }
    }

    /// Returns the next path which is at least as long as the original, or None if none can be
    /// found. If at least one such word exists, this function will always return a value.
    pub(crate) fn get_next_word(&mut self) -> Option<PathBuf> {
        let init_state = self.curr_state;
        while self.curr_state < self.possibilities.len() {
            let word = &self.possibilities[self.curr_state];
            self.curr_state += 1;
            if word.as_os_str().len() < self.word.len() {
                continue;
            } else {
                return Some(word.clone());
            }
        }
        self.curr_state = 0;
        while self.curr_state < init_state {
            let word = &self.possibilities[self.curr_state];
            self.curr_state += 1;
            if word.as_os_str().len() < self.word.len() {
                continue;
            } else {
                return Some(word.clone());
            }
        }
        None
    }

    /// Returns the next path which is at least as long as the original, and matches the provided
    /// predicate, or None if none can be found. If at least one such word exists,
    /// this function will always return a value.
    ///
    /// # Arguments
    ///
    /// * ` p ` - A predicate to test the paths.
    pub(crate) fn get_next_word_by(&mut self, p: &dyn Fn(&PathBuf) -> bool) -> Option<PathBuf> {
        let init_state = self.curr_state;
        while self.curr_state < self.possibilities.len() {
            let word = &self.possibilities[self.curr_state];
            self.curr_state += 1;
            if word.as_os_str().len() < self.word.len() || !p(word) {
                continue;
            } else {
                return Some(word.clone());
            }
        }
        self.curr_state = 0;
        while self.curr_state < init_state {
            let word = &self.possibilities[self.curr_state];
            self.curr_state += 1;
            if word.as_os_str().len() < self.word.len() || !p(word) {
                continue;
            } else {
                return Some(word.clone());
            }
        }
        None
    }
}
