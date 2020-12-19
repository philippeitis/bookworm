use std::fmt;
use std::path::PathBuf;

use itertools::Itertools;

use crate::AutoCompleter;

pub struct EditState {
    pub selected: usize,
    pub started_edit: bool,
    pub orig_value: String,
    pub new_value: String,
}

impl Default for EditState {
    fn default() -> Self {
        EditState {
            selected: 0,
            started_edit: false,
            orig_value: String::new(),
            new_value: String::new(),
        }
    }
}

impl EditState {
    pub fn new<S: AsRef<str>>(orig_value: S, selected: usize) -> Self {
        EditState {
            selected,
            started_edit: false,
            orig_value: orig_value.as_ref().to_owned(),
            new_value: String::new(),
        }
    }

    pub fn del(&mut self) {
        if self.started_edit {
            self.new_value.pop();
        } else {
            self.new_value.clear();
        }
        self.started_edit = true;
    }

    pub fn push(&mut self, c: char) {
        if !self.started_edit {
            self.new_value.clear();
        }
        self.started_edit = true;
        self.new_value.push(c);
    }

    pub fn edit_orig(&mut self) {
        if !self.started_edit {
            self.started_edit = true;
            self.new_value = self.orig_value.clone();
        }
    }

    pub fn reset_orig<S: AsRef<str>>(&mut self, orig_value: S) {
        self.started_edit = false;
        // TODO: Use .clone_into() when stabilized?
        self.orig_value.clear();
        self.orig_value.push_str(orig_value.as_ref());
        self.new_value.clear();
    }

    pub fn visible(&self) -> &str {
        if self.started_edit {
            self.new_value.as_str()
        } else {
            self.orig_value.as_str()
        }
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

impl CommandString {
    pub fn vals_to_string(
        values: &mut dyn Iterator<Item = &(bool, std::string::String)>,
    ) -> String {
        values
            .map(|(escaped, raw_str)| {
                if *escaped {
                    let mut s = String::with_capacity(2 + raw_str.len());
                    s.push('"');
                    s.push_str(raw_str.as_str());
                    s.push('"');
                    s
                } else {
                    raw_str.to_owned()
                }
            })
            .join(" ")
    }

    pub fn new() -> Self {
        CommandString {
            char_buf: vec![],
            auto_fill: None,
            autofilled: None,
            open_end: true,
            keep_last: false,
        }
    }

    pub fn push(&mut self, c: char) {
        self.write_back();
        self.auto_fill = None;
        self.char_buf.push(c)
    }

    pub fn pop(&mut self) {
        self.write_back();
        self.auto_fill = None;
        self.char_buf.pop();
        self.open_end = true;
    }

    pub fn write_back(&mut self) {
        if self.autofilled.is_some() {
            let v = self.get_values_autofilled();
            self.char_buf = CommandString::vals_to_string(&mut v.iter())
                .chars()
                .collect();
            self.autofilled = None;
        }
    }

    pub fn clear(&mut self) {
        self.auto_fill = None;
        self.autofilled = None;
        self.char_buf.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.char_buf.is_empty()
    }

    pub fn refresh_autofill(&mut self) -> Result<(), ()> {
        if self.auto_fill.is_some() {
            return Ok(());
        }

        let val = if let Some(val) = self.get_values().last() {
            if !val.0 && val.1.starts_with('-') && self.char_buf.last().eq(&Some(&' ')) {
                self.keep_last = true;
                String::new()
            } else {
                self.keep_last = false;
                val.1
            }
        } else {
            String::new()
        };

        self.auto_fill = Some(AutoCompleter::new(val)?);
        Ok(())
    }

    /// Runs autofill on the last value.
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
        // let mut values = vec![];
        // let mut escaped = false;
        // let mut start = 0;
        // for (end, &c) in self.char_buf.iter().enumerate() {
        //     match c {
        //         ' ' => {
        //             if !escaped {
        //                 if start == end {
        //                     start += 1;
        //                 } else {
        //                     values.push((escaped, self.char_buf[start..end].iter().collect()));
        //                     start = end + 1;
        //                 }
        //             }
        //         }
        //         '"' => {
        //             if escaped {
        //                 values.push((escaped, self.char_buf[start..end].iter().collect()));
        //                 start = end;
        //                 escaped = false;
        //             } else if start == end {
        //                 escaped = true;
        //                 start = end + 1;
        //             }
        //         }
        //         _ => {}
        //     }
        // }
        //
        // if start < self.char_buf.len() {
        //     values.push((
        //         escaped,
        //         self.char_buf[start..self.char_buf.len()].iter().collect(),
        //     ));
        // }
        // values
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
            let mut vals = CommandString::vals_to_string(&mut self.get_values_autofilled().iter());
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
                                self.start += end;
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
