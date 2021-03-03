use std::fmt;
use std::path::PathBuf;

use itertools::Itertools;

use crate::autocomplete::AutoCompleteError;
use crate::AutoCompleter;

pub struct EditState {
    pub started_edit: bool,
    pub value: String,
}

impl Default for EditState {
    fn default() -> Self {
        EditState {
            started_edit: false,
            value: String::new(),
        }
    }
}

impl EditState {
    pub fn new<S: AsRef<str>>(value: S) -> Self {
        EditState {
            started_edit: false,
            value: value.as_ref().to_owned(),
        }
    }

    pub fn del(&mut self) {
        if self.started_edit {
            self.value.pop();
        } else {
            self.value.clear();
        }
        self.started_edit = true;
    }

    pub fn push(&mut self, c: char) {
        if !self.started_edit {
            self.value.clear();
        }
        self.started_edit = true;
        self.value.push(c);
    }

    pub fn extend(&mut self, s: &str) {
        if !self.started_edit {
            self.value.clear();
        }
        self.started_edit = true;
        self.value.push_str(s);
    }
}

#[derive(Default)]
pub struct CommandString {
    char_buf: Vec<char>,
    auto_fill: Option<AutoCompleter<PathBuf>>,
    autofilled: Option<String>,
    open_end: bool,
    keep_last: bool,
}

#[derive(Debug)]
pub enum CommandStringError {
    AutoComplete(AutoCompleteError),
}

impl From<AutoCompleteError> for CommandStringError {
    fn from(e: AutoCompleteError) -> Self {
        Self::AutoComplete(e)
    }
}

impl CommandString {
    fn vals_to_string<I: IntoIterator<Item = (bool, std::string::String)>>(values: I) -> String {
        values
            .into_iter()
            .map(|(escaped, raw_str)| {
                if escaped {
                    let mut s = String::with_capacity(2 + raw_str.len());
                    s.push('"');
                    s.push_str(raw_str.as_str());
                    s.push('"');
                    s
                } else {
                    raw_str
                }
            })
            .join(" ")
    }

    /// Creates an empty CommandString.
    pub fn new() -> Self {
        CommandString {
            char_buf: vec![],
            auto_fill: None,
            autofilled: None,
            open_end: true,
            keep_last: false,
        }
    }

    /// Pushes `c` to the end of the working character buffer. If an unwritten autofill exists,
    /// the autofill is also persisted, and `c` is pushed after. The autofill source is reset.
    pub fn push(&mut self, c: char) {
        self.write_back();
        self.auto_fill = None;
        self.char_buf.push(c)
    }

    /// Pops the last character. If an unwritten autofill exists, the autofill is also persisted,
    /// and the last character is popped. The autofill source is reset.
    pub fn pop(&mut self) {
        self.write_back();
        self.auto_fill = None;
        self.char_buf.pop();
        self.open_end = true;
    }

    /// Writes the current autofill to self.
    fn write_back(&mut self) {
        if self.autofilled.is_some() {
            let v = self.get_values_autofilled();
            self.char_buf = CommandString::vals_to_string(v).chars().collect();
            self.autofilled = None;
        }
    }

    /// Clears all internal state, including autofills.
    pub fn clear(&mut self) {
        self.auto_fill = None;
        self.autofilled = None;
        self.char_buf.clear();
    }

    /// Checks if any characters are currently written or can be written.
    pub fn is_empty(&self) -> bool {
        self.char_buf.is_empty() && self.autofilled.is_none()
    }

    pub fn refresh_autofill(&mut self) -> Result<(), CommandStringError> {
        if self.auto_fill.is_some() {
            return Ok(());
        }

        let val = if let Some((escaped, word)) = self.get_values().last() {
            if !escaped && word.starts_with('-') && self.char_buf.last().eq(&Some(&' ')) {
                self.keep_last = true;
                String::new()
            } else {
                self.keep_last = false;
                word
            }
        } else {
            String::new()
        };

        self.auto_fill = Some(AutoCompleter::new(val)?);
        Ok(())
    }

    /// Autofills the current input, replacing the last word with an appropriate autofill.
    /// If `dir` is true, fills in a directory path - otherwise, allows any path.
    /// This is for the sake of being able to drill into directories to find books of
    /// interest.
    ///
    /// # Arguments
    /// * ` dir ` - true if filling in a directory, false otherwise.
    pub fn auto_fill(&mut self, dir: bool) {
        self.open_end = false;

        if let Some(af) = self.auto_fill.as_mut() {
            let path = if dir {
                af.get_next_word_by(|x| x.is_dir())
            } else {
                af.get_next_word()
            };

            if let Some(p) = path {
                self.autofilled = Some(p.display().to_string());
            }
        }
    }

    pub fn get_values(&self) -> CommandStringIter {
        CommandStringIter {
            command_string: &self,
            escaped: false,
            start: 0,
            complete: false,
        }
    }

    /// Returns a Vector of tuples (bool, String), where the bool indicates whether
    /// the string needs to be escaped or not, and the string is the content of a
    /// quote escaped string, or is a regular word without whitespace.
    pub fn get_values_autofilled(&self) -> Vec<(bool, String)> {
        let mut values: Vec<_> = self.get_values().collect();
        if let Some(s) = &self.autofilled {
            if !self.keep_last {
                values.pop();
            }
            values.push((s.contains(' '), s.to_owned()));
        }
        values
    }
}

impl fmt::Display for CommandString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.autofilled.is_some() {
            let mut vals = CommandString::vals_to_string(self.get_values_autofilled());
            if self.open_end && vals.ends_with('"') {
                vals.pop();
            }
            write!(f, "{}", vals)
        } else {
            write!(f, "{}", self.char_buf.iter().collect::<String>())
        }
    }
}

pub struct CommandStringIter<'a> {
    command_string: &'a CommandString,
    escaped: bool,
    start: usize,
    complete: bool,
}

impl<'a> CommandStringIter<'a> {
    fn char_buf(&self) -> &[char] {
        &self.command_string.char_buf[self.start..]
    }
}

impl<'a> Iterator for CommandStringIter<'a> {
    type Item = (bool, String);

    fn next(&mut self) -> Option<Self::Item> {
        for (end, &c) in self.command_string.char_buf[self.start..]
            .iter()
            .enumerate()
        {
            match c {
                ' ' => {
                    if !self.escaped {
                        if end == 0 {
                            self.start += 1;
                        } else {
                            let s = {
                                let buf = self.char_buf()[..end].iter().collect();
                                self.start += end + 1;
                                buf
                            };
                            return Some((self.escaped, s));
                        }
                    }
                }
                '"' => {
                    if self.escaped {
                        let s = {
                            let buf = self.char_buf()[..end.saturating_sub(1)].iter().collect();
                            self.start += end;
                            self.escaped = false;
                            buf
                        };

                        return Some((true, s));
                    } else if end == 0 {
                        self.escaped = true;
                        self.start += 1;
                    }
                }
                _ => {}
            }
        }

        if self.complete {
            None
        } else {
            self.complete = true;
            Some((self.escaped, self.char_buf().iter().collect()))
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_command_splits() {
        let samples = vec![
            ("hello there!", vec![(false, "hello"), (false, "there!")]),
            ("\"hello there!", vec![(true, "hello there!")]),
            (
                "\"hello world\" there!",
                vec![(true, "hello world"), (false, "there!")],
            ),
            ("!a -d x", vec![(false, "!a"), (false, "-d"), (false, "x")]),
        ];

        for (word, expected) in samples {
            let mut cs = CommandString::new();
            cs.char_buf = word.chars().collect();
            let results: Vec<_> = cs.get_values().collect();
            let expected: Vec<_> = expected
                .into_iter()
                .map(|(b, s)| (b, s.to_owned()))
                .collect();
            assert_eq!(results, expected);
        }
    }
}
