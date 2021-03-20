use std::convert::TryFrom;
use std::fmt;
use std::num::NonZeroUsize;
use std::ops::Add;
use std::path::PathBuf;

use itertools::Itertools;

use crate::autocomplete::AutoCompleteError;
use crate::AutoCompleter;

pub struct EditState {
    pub started_edit: bool,
    value: CursoredText,
}

impl Default for EditState {
    fn default() -> Self {
        EditState {
            started_edit: false,
            value: CursoredText::default(),
        }
    }
}

impl EditState {
    pub fn new<S: AsRef<str>>(value: S) -> Self {
        let text: Vec<_> = value.as_ref().chars().collect();
        EditState {
            started_edit: false,
            value: CursoredText {
                cursor: text.len(),
                selection: None,
                text,
            },
        }
    }

    pub fn value_to_string(&self) -> String {
        self.value.text.iter().collect()
    }

    pub fn del(&mut self) {
        if self.started_edit {
            self.value.del();
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
        for c in s.chars() {
            self.value.push(c);
        }
    }

    /// Performs backspace
    pub fn backspace(&mut self) {
        if self.started_edit {
            self.value.backspace();
        } else {
            self.value.clear();
        }
        self.started_edit = true;
    }

    pub fn key_up(&mut self) {
        self.started_edit = true;
        self.value.key_up()
    }

    pub fn key_shift_up(&mut self) {
        self.started_edit = true;
        self.value.key_shift_up()
    }

    pub fn key_down(&mut self) {
        self.started_edit = true;
        self.value.key_down()
    }

    pub fn key_shift_down(&mut self) {
        self.started_edit = true;
        self.value.key_shift_down()
    }

    pub fn key_left(&mut self) {
        self.started_edit = true;
        self.value.key_left()
    }

    pub fn key_shift_left(&mut self) {
        self.started_edit = true;
        self.value.key_shift_left()
    }

    pub fn key_right(&mut self) {
        self.started_edit = true;
        self.value.key_right()
    }

    pub fn key_shift_right(&mut self) {
        self.started_edit = true;
        self.value.key_shift_right()
    }

    pub fn clear(&mut self) {
        self.started_edit = true;
        self.value.clear();
    }

    pub fn select_all(&mut self) {
        self.started_edit = true;
        self.value.select_all()
    }

    pub fn deselect(&mut self) {
        self.started_edit = true;
        self.value.deselect();
    }

    pub fn char_chunks(&self) -> CharChunks {
        let ct = &self.value;
        match ct.selection {
            Some((x, Direction::Left)) => {
                let (a, b) = ct.text.split_at(ct.cursor);
                let (b, c) = b.split_at(usize::from(x));
                CharChunks::Selected(a, b, c, Direction::Left)
            }
            Some((x, Direction::Right)) => {
                let midcursor = ct.cursor - usize::from(x);
                let (a, b) = ct.text.split_at(midcursor);
                let (b, c) = b.split_at(usize::from(x));
                CharChunks::Selected(a, b, c, Direction::Right)
            }
            None => {
                let (a, b) = ct.text.split_at(ct.cursor);
                CharChunks::Unselected(a.iter().collect(), b.iter().collect())
            }
        }
    }

    pub fn selected(&self) -> Option<&[char]> {
        match self.char_chunks() {
            CharChunks::Selected(_, inside, ..) => Some(inside),
            CharChunks::Unselected(..) => None,
        }
    }
}

#[derive(Default)]
pub struct CommandString {
    cursored_text: CursoredText,
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
                    s.push_str(raw_str.replace('"', "\\\"").as_str());
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
            cursored_text: CursoredText::default(),
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
        self.cursored_text.push(c)
    }

    /// Performs backspace
    pub fn backspace(&mut self) {
        self.write_back();
        self.auto_fill = None;
        self.open_end = true;
        self.cursored_text.backspace();
    }

    /// Performs backspace
    pub fn del(&mut self) {
        self.write_back();
        self.auto_fill = None;
        self.open_end = true;
        self.cursored_text.del();
    }

    pub fn key_up(&mut self) {
        self.cursored_text.key_up()
    }

    pub fn key_shift_up(&mut self) {
        self.cursored_text.key_shift_up()
    }

    pub fn key_down(&mut self) {
        self.cursored_text.key_down()
    }

    pub fn key_shift_down(&mut self) {
        self.cursored_text.key_shift_down()
    }

    pub fn key_left(&mut self) {
        self.cursored_text.key_left()
    }

    pub fn key_shift_left(&mut self) {
        self.cursored_text.key_shift_left()
    }

    pub fn key_right(&mut self) {
        self.cursored_text.key_right()
    }

    pub fn key_shift_right(&mut self) {
        self.cursored_text.key_shift_right()
    }

    /// Writes the current autofill to self.
    fn write_back(&mut self) {
        if self.autofilled.is_some() {
            let v = self.autofilled_values();
            self.cursored_text.text = CommandString::vals_to_string(v).chars().collect();
            self.cursored_text.cursor = self.cursored_text.text.len();
            self.cursored_text.selection = None;
            self.autofilled = None;
        }
    }

    /// Clears all internal state, including autofills.
    pub fn clear(&mut self) {
        self.auto_fill = None;
        self.autofilled = None;
        self.cursored_text.clear();
    }

    /// Checks if any characters are currently written or can be written.
    pub fn is_empty(&self) -> bool {
        self.cursored_text.text.is_empty() && self.autofilled.is_none()
    }

    pub fn refresh_autofill(&mut self) -> Result<(), CommandStringError> {
        if self.auto_fill.is_some() {
            return Ok(());
        }

        let val = if let Some((escaped, word)) = self.get_values().last() {
            if !escaped && word.starts_with('-') && self.cursored_text.text.last().eq(&Some(&' ')) {
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
        if !self.cursored_text.can_autocomplete() {
            return;
        }

        self.open_end = false;

        if let Some(af) = self.auto_fill.as_mut() {
            let path = if dir {
                af.next_word_by(|x| x.is_dir())
            } else {
                af.next_word()
            };

            if let Some(p) = path {
                self.autofilled = Some(p.display().to_string());
            }
        }
    }

    pub fn select_all(&mut self) {
        self.cursored_text.select_all()
    }

    pub fn deselect(&mut self) {
        self.cursored_text.deselect();
    }

    pub fn char_chunks(&self) -> CharChunks {
        let ct = &self.cursored_text;
        match ct.selection {
            Some((x, Direction::Left)) => {
                let (a, b) = ct.text.split_at(ct.cursor);
                let (b, c) = b.split_at(usize::from(x));
                CharChunks::Selected(a, b, c, Direction::Left)
            }
            Some((x, Direction::Right)) => {
                let midcursor = ct.cursor - usize::from(x);
                let (a, b) = ct.text.split_at(midcursor);
                let (b, c) = b.split_at(usize::from(x));
                CharChunks::Selected(a, b, c, Direction::Right)
            }
            None => {
                if self.autofilled.is_some() {
                    let v = self.autofilled_values();
                    CharChunks::Unselected(CommandString::vals_to_string(v), String::new())
                } else {
                    let (a, b) = ct.text.split_at(ct.cursor);
                    CharChunks::Unselected(a.iter().collect(), b.iter().collect())
                }
            }
        }
    }

    pub fn selected(&self) -> Option<&[char]> {
        match self.char_chunks() {
            CharChunks::Selected(_, inside, ..) => Some(inside),
            CharChunks::Unselected(..) => None,
        }
    }

    pub fn get_values(&self) -> CommandStringIter {
        CommandStringIter {
            command_string: &self,
            quoted: Quoted::None,
            start: 0,
            complete: false,
            escaped: false,
            was_escaped: false,
            sub_string: String::new(),
        }
    }

    /// Returns a Vector of tuples (bool, String), where the bool indicates whether
    /// the string needs to be escaped or not, and the string is the content of a
    /// quote escaped string, or is a regular word without whitespace.
    pub fn autofilled_values(&self) -> Vec<(bool, String)> {
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
            let mut vals = CommandString::vals_to_string(self.autofilled_values());
            if self.open_end && vals.ends_with('"') {
                vals.pop();
            }
            write!(f, "{}", vals)
        } else {
            write!(f, "{}", self.cursored_text.text.iter().collect::<String>())
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
enum Quoted {
    // "
    Double,
    // '
    Single,
    None,
}

impl From<char> for Quoted {
    fn from(c: char) -> Self {
        match c {
            '"' => Self::Double,
            '\'' => Self::Single,
            _ => Self::None,
        }
    }
}

pub struct CommandStringIter<'a> {
    command_string: &'a CommandString,
    quoted: Quoted,
    start: usize,
    complete: bool,
    was_escaped: bool,
    escaped: bool,
    sub_string: String,
}

// TODO: Make CommandStringIter able to reproduce exact text input?
impl<'a> Iterator for CommandStringIter<'a> {
    type Item = (bool, String);

    fn next(&mut self) -> Option<Self::Item> {
        if self.start >= self.command_string.cursored_text.text.len() {
            return None;
        }

        for (end, &c) in self.command_string.cursored_text.text[self.start..]
            .iter()
            .enumerate()
        {
            if self.escaped {
                self.was_escaped = true;
                self.sub_string.push(c);
                self.escaped = false;
                continue;
            }
            match c {
                ' ' => {
                    if self.quoted == Quoted::None {
                        if end == 0 {
                            self.start += 1;
                        } else {
                            let s = {
                                self.start += end + 1;
                                std::mem::take(&mut self.sub_string)
                            };
                            return Some((std::mem::replace(&mut self.was_escaped, false), s));
                        }
                    } else {
                        self.sub_string.push(c);
                    }
                }
                '"' | '\'' => {
                    let quote = Quoted::from(c);
                    if self.quoted == quote {
                        let s = {
                            self.start += end + 1;
                            self.quoted = Quoted::None;
                            std::mem::take(&mut self.sub_string)
                        };

                        return Some((true, s));
                    } else if end == 0 {
                        self.quoted = quote;
                        self.start += 1;
                    } else {
                        self.sub_string.push(c);
                    }
                }
                '\\' => self.escaped = true,
                c => self.sub_string.push(c),
            }
        }

        if self.complete {
            None
        } else {
            self.complete = true;
            Some((
                self.was_escaped || self.quoted != Quoted::None,
                std::mem::take(&mut self.sub_string),
            ))
        }
    }
}

pub enum CharChunks<'a> {
    Selected(&'a [char], &'a [char], &'a [char], Direction),
    Unselected(String, String),
}

pub enum Direction {
    Left,
    Right,
}

#[derive(Default)]
struct CursoredText {
    cursor: usize,
    selection: Option<(NonZeroUsize, Direction)>,
    text: Vec<char>,
}

impl CursoredText {
    fn select_all(&mut self) {
        self.cursor = self.text.len();
        self.selection = NonZeroUsize::try_from(self.text.len())
            .map(|x| (x, Direction::Right))
            .ok();
    }

    fn deselect(&mut self) {
        self.cursor = self.text.len();
        self.selection = None;
    }

    fn key_up(&mut self) {
        self.cursor = 0;
        self.selection = None;
    }

    fn key_shift_up(&mut self) {
        if self.cursor == 0 {
            return;
        }

        self.selection = match self.selection {
            Some((x, Direction::Left)) => {
                let midcursor = self.cursor + usize::from(x);
                let size = NonZeroUsize::try_from(midcursor).ok();
                size.map(|x| (x, Direction::Left))
            }
            Some((x, Direction::Right)) => {
                let midcursor = self.cursor - usize::from(x);
                let size = NonZeroUsize::try_from(midcursor).ok();
                size.map(|x| (x, Direction::Left))
            }
            None => {
                let size = NonZeroUsize::try_from(self.cursor).ok();
                size.map(|x| (x, Direction::Left))
            }
        };

        self.cursor = 0;
    }

    fn key_down(&mut self) {
        self.cursor = self.text.len();
        self.selection = None;
    }

    fn key_shift_down(&mut self) {
        if self.cursor == self.text.len() {
            return;
        }

        self.selection = match self.selection {
            Some((x, Direction::Left)) => {
                let midcursor = self.cursor + usize::from(x);
                let size = NonZeroUsize::try_from(self.text.len() - midcursor).ok();
                size.map(|x| (x, Direction::Right))
            }
            Some((x, Direction::Right)) => {
                let midcursor = self.cursor - usize::from(x);
                let size = NonZeroUsize::try_from(self.text.len() - midcursor).ok();
                size.map(|x| (x, Direction::Right))
            }
            None => {
                let size = NonZeroUsize::try_from(self.text.len() - self.cursor).ok();
                size.map(|x| (x, Direction::Right))
            }
        };

        self.cursor = self.text.len();
    }

    fn key_left(&mut self) {
        match self.selection {
            Some((_, Direction::Left)) => {}
            Some((x, Direction::Right)) => {
                self.cursor -= usize::from(x);
            }
            None => {
                self.cursor = self.cursor.saturating_sub(1);
            }
        }

        self.selection = None;
    }

    fn key_shift_left(&mut self) {
        if self.cursor == 0 {
            return;
        }

        self.cursor -= 1;

        self.selection = match self.selection {
            Some((x, Direction::Left)) => {
                let size =
                    NonZeroUsize::try_from(1 + usize::from(x)).expect("Overflowed selection");
                Some((size, Direction::Left))
            }
            Some((x, Direction::Right)) => {
                let size = NonZeroUsize::try_from(usize::from(x) - 1).ok();
                size.map(|x| (x, Direction::Right))
            }
            None => {
                let size = NonZeroUsize::try_from(1)
                    .expect("Cosmic rays or other such events have caused this error.");
                Some((size, Direction::Left))
            }
        };
    }

    fn key_right(&mut self) {
        match self.selection {
            Some((x, Direction::Left)) => {
                self.cursor += usize::from(x);
            }
            Some((_, Direction::Right)) => {}
            None => {
                self.cursor = self.cursor.add(1).min(self.text.len());
            }
        }

        self.selection = None;
    }

    fn key_shift_right(&mut self) {
        if self.cursor == self.text.len() {
            return;
        }

        self.cursor += 1;

        self.selection = match self.selection {
            Some((x, Direction::Left)) => {
                let size = NonZeroUsize::try_from(usize::from(x) - 1).ok();
                size.map(|x| (x, Direction::Left))
            }
            Some((x, Direction::Right)) => {
                let size = NonZeroUsize::try_from(1 + usize::from(x))
                    .expect("User should never select >= 2^32 characters");
                Some((size, Direction::Right))
            }
            None => {
                let size = NonZeroUsize::try_from(1)
                    .expect("Cosmic rays or other such events have caused this error.");
                Some((size, Direction::Right))
            }
        };
    }

    fn del(&mut self) {
        match self.selection {
            Some((x, Direction::Left)) => {
                let midcursor = self.cursor + usize::from(x);
                self.text.drain(self.cursor..midcursor);
            }
            Some((x, Direction::Right)) => {
                let midcursor = self.cursor - usize::from(x);
                self.text.drain(midcursor..self.cursor);
                self.cursor = midcursor;
            }
            None => {
                if self.cursor != self.text.len() {
                    self.text.remove(self.cursor);
                }
            }
        };

        self.selection = None;
    }

    fn backspace(&mut self) {
        match self.selection {
            Some((x, Direction::Left)) => {
                let midcursor = self.cursor + usize::from(x);
                self.text.drain(self.cursor..midcursor);
            }
            Some((x, Direction::Right)) => {
                let midcursor = self.cursor - usize::from(x);
                self.text.drain(midcursor..self.cursor);
                self.cursor = midcursor;
            }
            None => {
                if self.cursor != 0 {
                    self.cursor -= 1;
                    self.text.remove(self.cursor);
                }
            }
        };

        self.selection = None;
    }

    fn push(&mut self, c: char) {
        match self.selection {
            Some((x, Direction::Left)) => {
                let midcursor = self.cursor + usize::from(x);
                self.text.drain(self.cursor..midcursor);
                self.text.insert(self.cursor, c);
                self.cursor += 1;
            }
            Some((x, Direction::Right)) => {
                let midcursor = self.cursor - usize::from(x);
                self.text.drain(midcursor..self.cursor);
                self.text.insert(midcursor, c);
                self.cursor = midcursor + 1;
            }
            None => {
                self.text.insert(self.cursor, c);
                self.cursor += 1;
            }
        };

        self.selection = None;
    }

    fn can_autocomplete(&mut self) -> bool {
        self.selection.is_none()
    }

    fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
        self.selection = None;
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
            (
                "!e title \"hello world\"",
                vec![(false, "!e"), (false, "title"), (true, "hello world")],
            ),
            (
                "!e title \"hello world\" field 12345",
                vec![
                    (false, "!e"),
                    (false, "title"),
                    (true, "hello world"),
                    (false, "field"),
                    (false, "12345"),
                ],
            ),
            (
                "a \"b\" c 12",
                vec![(false, "a"), (true, "b"), (false, "c"), (false, "12")],
            ),
            ("a \\b", vec![(false, "a"), (true, "b")]),
            ("a \" aaaa \\\" b", vec![(false, "a"), (true, " aaaa \" b")]),
            (
                "a ' hello ' \" hello \"",
                vec![(false, "a"), (true, " hello "), (true, " hello ")],
            ),
            ("' \" ' \" ' \"", vec![(true, " \" "), (true, " \' ")]),
            ("a\\ b", vec![(true, "a b")]),
        ];

        for (word, expected) in samples {
            let mut cs = CommandString::new();
            cs.cursored_text.text = word.chars().collect();
            let results: Vec<_> = cs.get_values().collect();
            let expected: Vec<_> = expected
                .into_iter()
                .map(|(b, s)| (b, s.to_owned()))
                .collect();
            assert_eq!(results, expected);
        }
    }
}
