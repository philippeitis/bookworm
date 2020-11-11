use std::path::PathBuf;

use glob::glob;

#[derive(Clone)]
pub(crate) struct AutoCompleter<S> {
    word: String,
    possibilities: Vec<S>,
    curr_state: usize,
}

impl AutoCompleter<PathBuf> {
    pub(crate) fn new<S: AsRef<str>>(word: S) -> Result<Self, ()> {
        let word = word.as_ref().to_string();
        let mut glob_str = word.clone();
        glob_str.push('*');
        let mut p: Vec<_> = glob(glob_str.as_str())
            .unwrap()
            .into_iter()
            .map(|s| s.unwrap())
            .collect();
        p.sort();
        Ok(AutoCompleter {
            word,
            possibilities: p,
            curr_state: 0,
        })
    }

    pub(crate) fn get_next_word(&mut self) -> Option<PathBuf> {
        let init_state = self.curr_state;
        while self.curr_state < self.possibilities.len() {
            let word = &self.possibilities[self.curr_state];
            self.curr_state += 1;
            if word.as_os_str().len() <= self.word.len() {
                continue;
            } else {
                return Some(word.clone());
            }
        }
        self.curr_state = 0;
        while self.curr_state < init_state {
            let word = &self.possibilities[self.curr_state];
            self.curr_state += 1;
            if word.as_os_str().len() <= self.word.len() {
                continue;
            } else {
                return Some(word.clone());
            }
        }
        None
    }

    pub(crate) fn get_next_word_by(&mut self, p: &dyn Fn(&PathBuf) -> bool) -> Option<PathBuf> {
        let init_state = self.curr_state;
        while self.curr_state < self.possibilities.len() {
            let word = &self.possibilities[self.curr_state];
            self.curr_state += 1;
            if word.as_os_str().len() <= self.word.len() || !p(word) {
                continue;
            } else {
                return Some(word.clone());
            }
        }
        self.curr_state = 0;
        while self.curr_state < init_state {
            let word = &self.possibilities[self.curr_state];
            self.curr_state += 1;
            if word.as_os_str().len() <= self.word.len() || !p(word) {
                continue;
            } else {
                return Some(word.clone());
            }
        }
        None
    }
}
