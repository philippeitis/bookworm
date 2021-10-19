enum Edit {
    /// Moves cursor to left by usize
    MoveLeft(usize),
    /// Moves cursor to right by usize
    MoveRight(usize),
    /// Deletes backwards - to do a Del,
    /// do MoveRight followed by Delete
    Delete(usize),
    /// Inserts one or more characters
    Insert(Vec<char>),
}

enum EditStart {
    DeleteAll,
    FromStart,
    FromEnd,
}

/// The `UserEdit` struct is intended to provide functionality for editing an arbitrarily large
/// number of items without
struct UserEdit {
    edits: Vec<Edit>,
    start: EditStart,
}

impl Default for UserEdit {
    fn default() -> Self {
        Self {
            edits: vec![],
            start: EditStart::FromEnd,
        }
    }
}

impl UserEdit {
    /// Applies the internal sequence of commands to the target
    fn apply(&self, target: &str) -> String {
        let (mut chars, mut ind): (Vec<_>, _) = match self.start {
            EditStart::DeleteAll => (vec![], 0),
            EditStart::FromStart => (target.chars().collect(), 0),
            EditStart::FromEnd => {
                let chars = target.chars().collect::<Vec<_>>();
                let char_len = chars.len();
                (chars, char_len)
            }
        };

        for edit in &self.edits {
            match edit {
                Edit::MoveLeft(i) => {
                    ind = ind.saturating_sub(*i);
                }
                Edit::MoveRight(i) => {
                    ind = ind.saturating_add(*i).min(chars.len());
                }
                Edit::Delete(count) => {
                    ind = ind.saturating_add(*count).min(chars.len());
                    chars.drain(ind..ind + count);
                }
                Edit::Insert(new_chars) => {
                    chars.splice(ind..ind, new_chars.iter().cloned());
                }
            }
        }

        chars.into_iter().collect()
    }

    // TODO: Is this actually a good idea with utf-8 codepoints?
    /// Returns the SQL which will transform a string into the target
    /// Deletion will include wrapping SUBSTR excluding the deleted portion
    /// Insert will be SUBSTR || text || SUBSTR
    /// EditStart with DeleteAll is simply a text replacement -
    ///     using COLUMN = ...;
    /// other variants will adjust the cursor appropriately
    /// SQLite's substring allows negative indices.
    fn sql(&self) -> (String, Vec<String>) {
        match self.start {
            EditStart::DeleteAll => {
                return ("?".to_string(), vec![self.apply("")]);
            }
            EditStart::FromStart => {
                let mut edit_str = String::new();
                let mut binds: Vec<String> = vec![];
                let mut ind = 0usize;
                for edit in &self.edits {
                    match edit {
                        Edit::MoveLeft(i) => {
                            ind = ind.saturating_sub(*i);
                        }
                        Edit::MoveRight(i) => {
                            ind = ind.saturating_add(*i);
                        }
                        Edit::Delete(count) => {
                            ind = ind.saturating_sub(*count);
                            let before_substr = format!("SUBSTR({}, 1, {})", edit_str, ind + 1);
                            let after_substr = format!("SUBSTR({}, {})", edit_str, ind + 1);
                            edit_str = format!("{} || {}", before_substr, after_substr);
                        }
                        Edit::Insert(_) => {
                            unimplemented!()
                            // substr before, middle, substr after
                        }
                    }
                }
                //
                unimplemented!()
            }
            EditStart::FromEnd => {
                unimplemented!()
            }
        };
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::edit::Edit::*;

    #[test]
    fn test_edit() {
        let mut edit = UserEdit {
            edits: vec![MoveLeft(5), MoveRight(5), Insert(vec!['a', 'b', 'c'])],
            start: EditStart::FromEnd,
        };
        assert_eq!(edit.apply("hello").as_str(), "helloabc");
        edit.start = EditStart::FromStart;
        assert_eq!(edit.apply("hello").as_str(), "helloabc");
        edit.start = EditStart::DeleteAll;
        assert_eq!(edit.apply("hello").as_str(), "abc");
    }
}
