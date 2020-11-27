use std::fmt;
use std::path::PathBuf;

use itertools::Itertools;

use crate::ui::AutoCompleter;

pub(crate) struct EditState {
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
            orig_value: "".to_string(),
            new_value: "".to_string(),
        }
    }
}

impl EditState {
    pub(crate) fn new<S: AsRef<str>>(orig_value: S, selected: usize) -> Self {
        EditState {
            selected,
            started_edit: false,
            orig_value: orig_value.as_ref().to_string(),
            new_value: "".to_string(),
        }
    }

    pub(crate) fn del(&mut self) {
        if self.started_edit {
            self.new_value.pop();
        } else {
            self.new_value.clear();
        }
        self.started_edit = true;
    }

    pub(crate) fn push(&mut self, c: char) {
        if !self.started_edit {
            self.new_value.clear();
        }
        self.started_edit = true;
        self.new_value.push(c);
    }

    pub(crate) fn edit_orig(&mut self) {
        if !self.started_edit {
            self.started_edit = true;
            self.new_value = self.orig_value.clone();
        }
    }

    pub(crate) fn reset_orig(&mut self, orig_value: String) {
        self.started_edit = false;
        self.orig_value = orig_value;
        self.new_value.clear();
    }

    pub(crate) fn visible(&self) -> &str {
        if self.started_edit {
            self.new_value.as_str()
        } else {
            self.orig_value.as_str()
        }
    }
}

#[derive(Default)]
pub(crate) struct CommandString {
    char_buf: Vec<char>,
    auto_fill: Option<AutoCompleter<PathBuf>>,
    autofilled: Option<String>,
    open_end: bool,
    keep_last: bool,
}

impl CommandString {
    pub(crate) fn vals_to_string(
        values: &mut dyn Iterator<Item = &(bool, std::string::String)>,
    ) -> String {
        values
            .map(|(escaped, raw_str)| {
                if *escaped {
                    let mut s = '"'.to_string();
                    s.push_str(raw_str.as_str());
                    s.push('"');
                    s
                } else {
                    raw_str.clone()
                }
            })
            .join(" ")
    }

    pub(crate) fn new() -> Self {
        CommandString {
            char_buf: vec![],
            auto_fill: None,
            autofilled: None,
            open_end: true,
            keep_last: false,
        }
    }

    pub(crate) fn push(&mut self, c: char) {
        self.write_back();
        self.auto_fill = None;
        self.char_buf.push(c)
    }

    pub(crate) fn pop(&mut self) {
        self.write_back();
        self.auto_fill = None;
        self.char_buf.pop();
        self.open_end = true;
    }

    pub(crate) fn write_back(&mut self) {
        if self.autofilled.is_some() {
            let v = self.get_values_autofilled();
            self.char_buf = CommandString::vals_to_string(&mut v.iter())
                .chars()
                .collect();
            self.autofilled = None;
        }
    }

    pub(crate) fn clear(&mut self) {
        self.auto_fill = None;
        self.autofilled = None;
        self.char_buf.clear();
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.char_buf.is_empty()
    }

    pub(crate) fn refresh_autofill(&mut self) -> Result<(), ()> {
        if self.auto_fill.is_some() {
            return Ok(());
        }

        let values = self.get_values();

        let val = if let Some(val) = values.last() {
            if !val.0 && val.1.starts_with('-') && self.char_buf.last().eq(&Some(&' ')) {
                self.keep_last = true;
                ""
            } else {
                self.keep_last = false;
                &val.1
            }
        } else {
            ""
        };

        self.auto_fill = Some(AutoCompleter::new(val)?);
        Ok(())
    }

    /// Runs autofill on the last value.
    pub(crate) fn auto_fill(&mut self, dir: bool) {
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

    pub(crate) fn get_values(&self) -> Vec<(bool, String)> {
        let mut values = vec![];
        let mut escaped = false;
        let mut start = 0;
        for (end, &c) in self.char_buf.iter().enumerate() {
            match c {
                ' ' => {
                    if !escaped {
                        if start != end {
                            values.push((escaped, self.char_buf[start..end].iter().collect()));
                            start = end + 1;
                        } else {
                            start += 1;
                        }
                    }
                }
                '"' => {
                    if escaped {
                        values.push((escaped, self.char_buf[start..end].iter().collect()));
                        start = end;
                        escaped = false;
                    } else if start == end {
                        escaped = true;
                        start = end + 1;
                    }
                }
                _ => {}
            }
        }

        if start < self.char_buf.len() {
            values.push((
                escaped,
                self.char_buf[start..self.char_buf.len()].iter().collect(),
            ));
        }
        values
    }

    pub(crate) fn get_values_autofilled(&self) -> Vec<(bool, String)> {
        let mut values = self.get_values();
        if let Some(s) = &self.autofilled {
            if !self.keep_last {
                values.pop();
            }
            values.push((s.contains(' '), s.clone()));
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
