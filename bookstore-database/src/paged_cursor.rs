use std::collections::BTreeSet;
use std::io::Write;

fn log(s: impl AsRef<str>) {
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(true)
        .open("log.txt")
    {
        let _ = f.write_all(s.as_ref().as_bytes());
        let _ = f.write_all(b"\n");
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Selection {
    Single(usize),
    Range(usize, usize, Direction),
    Multi(BTreeSet<usize>, Direction),
}

trait UnwrapSelection {
    fn unwrap_single(&self) -> usize;

    fn unwrap_range(&self) -> (usize, usize, Direction);

    fn unwrap_multi(&self) -> (&BTreeSet<usize>, Direction);
}

impl UnwrapSelection for Option<Selection> {
    fn unwrap_single(&self) -> usize {
        match self {
            Some(Selection::Single(x)) => *x,
            _ => panic!("Called unwrap_single() on invalid type."),
        }
    }

    fn unwrap_range(&self) -> (usize, usize, Direction) {
        match self {
            Some(Selection::Range(x, y, z)) => (*x, *y, *z),
            _ => panic!("Called unwrap_range() on invalid type."),
        }
    }

    fn unwrap_multi(&self) -> (&BTreeSet<usize>, Direction) {
        match self {
            Some(Selection::Multi(x, y)) => (x, *y),
            _ => panic!("Called unwrap_multi() on invalid type."),
        }
    }
}

/// A window, starting at the top index, containing size elements. Ensures that the top index is set
/// so that `size` elements are available, except in the case where height is smaller than size, in
/// which case the top index is 0. This is equivalent to ensuring that top_index <= height.
/// saturating_sub(size).
#[derive(Clone, Debug, PartialEq, Eq)]
struct Window {
    top_index: usize,
    size: usize,
    height: usize,
}
// Has a window, and has selected items
pub struct PageCursorMultiple {
    window: Window,
    selected: Option<Selection>,
    height: usize,
}

impl Window {
    /// Returns whether the last element of data is in the window.
    pub(crate) fn at_end(&self) -> bool {
        self.size >= self.height || self.top_index >= self.height - self.size - 1
    }

    /// Returns whether the first element of data is in the window
    pub(crate) fn at_top(&self) -> bool {
        self.top_index == 0
    }

    /// Moves the view down by inc, but does not move the view past the last element
    /// of data if possible.
    pub(crate) fn scroll_down(&mut self, inc: usize) -> bool {
        if self.top_index + inc + self.size > self.height {
            let new_val = self.height.saturating_sub(self.size);
            !self.top_index.replace_and_equal(new_val)
        } else {
            self.top_index += inc;
            true
        }
    }

    /// Moves the view down by inc, but does not move the view past the last element
    /// of data if possible.
    pub(crate) fn scroll_up(&mut self, inc: usize) -> bool {
        !self
            .top_index
            .replace_and_equal(self.top_index.saturating_sub(inc))
    }

    pub(crate) fn refresh_height(&mut self, height: usize) {
        self.height = height;
        if self.top_index + self.size > self.height {
            self.top_index = self.height.saturating_sub(self.size)
        }
    }

    pub(crate) fn refresh_size(&mut self, size: usize) -> bool {
        if self.size.replace_and_equal(size) {
            return false;
        }

        if self.top_index + self.size > self.height {
            self.top_index = self.height.saturating_sub(self.size)
        }

        true
    }

    pub(crate) fn range(&self) -> std::ops::Range<usize> {
        // min is used in the case that height < self.size
        self.top_index..(self.top_index + self.size).min(self.height)
    }
}

trait ReplaceAndEqual {
    /// Replaces `self` with `other`, and returns whether `self` was
    /// equal to `other`.
    fn replace_and_equal(&mut self, other: Self) -> bool;
}

impl<T: PartialEq> ReplaceAndEqual for T {
    #[inline(always)]
    fn replace_and_equal(&mut self, other: Self) -> bool {
        if other.eq(self) {
            true
        } else {
            *self = other;
            false
        }
    }
}

impl PageCursorMultiple {
    /// Creates a new Cursor at location 0, with no selection active.
    pub(crate) fn new(height: usize, window_size: usize) -> Self {
        PageCursorMultiple {
            window: Window {
                top_index: 0,
                size: window_size,
                height,
            },
            height,
            selected: None,
        }
    }

    pub fn top_index(&self) -> usize {
        self.assert_invariants();
        self.window.top_index
    }

    pub fn at_end(&self) -> bool {
        self.assert_invariants();
        self.window.at_end()
    }

    /// Moves the view down by inc, but does not move the view past the last element
    /// of data if possible.
    pub(crate) fn scroll_down(&mut self, inc: usize) -> bool {
        self.assert_invariants();
        self.window.scroll_down(inc)
    }

    /// Moves the view up by inc, but does not move the start past the first index.
    pub(crate) fn scroll_up(&mut self, inc: usize) -> bool {
        self.assert_invariants();
        self.window.scroll_up(inc)
    }

    /// Selects the index relative to the internal window. If index is greater than the window size or height,
    /// selects the largest index that is still visible.
    pub(crate) fn select_relative(&mut self, index: usize) -> bool {
        self.assert_invariants();
        let ind = {
            // If ind > min, do nothing.

            let ind = if index > self.window.size {
                self.window.size.saturating_sub(1)
            } else {
                index
            } + self.window.top_index;

            if ind > self.height {
                self.height.saturating_sub(1)
            } else {
                ind
            }
        };

        match &mut self.selected {
            x @ None => {
                *x = Some(Selection::Single(ind));
                true
            }
            Some(Selection::Single(old_ind)) => !old_ind.replace_and_equal(ind),
            _ => unimplemented!("Relative selection with multiple selections not supported."),
        }
    }

    /// Selects the value at the index inside the window. If index is greater than the window size or data len,
    /// selects the largest index that is still visible.
    pub(crate) fn deselect(&mut self) -> bool {
        self.assert_invariants();
        std::mem::take(&mut self.selected).is_some()
    }

    /// Selects the value at index, and adjusts the window so that the value is visible.
    pub(crate) fn select_index_and_make_visible(&mut self, index: usize) -> bool {
        self.assert_invariants();
        if self.height == 0 {
            self.selected = None;
            return true;
        }

        assert!(
            index < self.height,
            "Attempted to select index that does not exist."
        );
        let top_index = if index < self.window.size {
            0
        } else if index + self.window.size > self.height {
            self.height - self.window.size
        } else {
            self.selected = Some(Selection::Single(index));
            return true;
        };
        self.selected = Some(Selection::Single(index));
        self.window.top_index = top_index;

        true
    }

    pub(crate) fn select_index(&mut self, index: usize) {
        self.assert_invariants();
        if self.height == 0 {
            self.selected = None;
        } else {
            assert!(
                index < self.height,
                "Attempted to select index that does not exist."
            );
            self.selected = Some(Selection::Single(index));
        }
    }

    /// Returns the selected value.
    pub(crate) fn selected(&self) -> Option<&Selection> {
        self.selected.as_ref()
    }

    /// Moves the selected cursor down if it exists, otherwise, creates a new cursor at the bottom.
    /// If the cursor moves past the end of the visible window, the window is moved down.
    pub(crate) fn down(&mut self) -> bool {
        self.assert_invariants();

        log(format!(
            "pressed down: {:?} {:?}",
            self.selected, self.window
        ));

        if self.height == 0 || self.window.size == 0 {
            log("No selection applied - no items visible.");
            return false;
        }

        match &mut self.selected {
            x @ None => {
                *x = Some(Selection::Single(self.window.top_index));
                true
            }
            x @ Some(Selection::Single(_)) => {
                let s = x.unwrap_single();

                // if selection > min, move down by one.
                // Else, window moves down.

                if s + 1 >= self.height {
                    return false;
                }
                // t..t+s
                if (s + 1).saturating_sub(self.window.top_index) >= self.window.size {
                    self.window.scroll_down(1);
                }

                *x = Some(Selection::Single(s + 1));
                true
            }
            x @ Some(Selection::Range(_, _, _)) => {
                let (_, end, _) = x.unwrap_range();
                let ind = end.saturating_sub(1);
                *x = Some(Selection::Single(ind));
                if ind < self.window.top_index {
                    self.window.top_index = ind;
                    true
                } else if ind.saturating_sub(self.window.top_index) >= self.window.size {
                    self.window.scroll_down(
                        ind.saturating_sub(self.window.top_index) - self.window.size + 1,
                    )
                } else {
                    true
                }
            }
            _ => unimplemented!("Key down not supported with discontinuous selection."),
        }
    }

    pub(crate) fn select_down(&mut self, inc: usize) -> bool {
        self.assert_invariants();
        log(format!(
            "pressed shift down: {:?} {:?} {}",
            self.selected, self.window, inc
        ));

        if self.height == 0 || self.window.size == 0 {
            log("No selection applied - no items visible.");
            return false;
        }

        match &mut self.selected {
            x @ None => {
                *x = Some(Selection::Single(self.window.top_index));
                true
            }
            x @ Some(Selection::Single(_)) => {
                let s = x.unwrap_single();

                if s + 1 >= self.height {
                    return false;
                }

                let new_s = (s + inc).min(self.height.saturating_sub(1));
                if let Some(diff) = new_s
                    .saturating_sub(self.window.top_index)
                    .checked_sub(self.window.size)
                {
                    self.window.scroll_down(diff + 1);
                }

                *x = Some(Selection::Range(s, new_s + 1, Direction::Down));
                true
            }
            x @ Some(Selection::Range(_, _, Direction::Up)) => {
                let (start, end, _) = x.unwrap_range();
                let new_s = (start + inc).min(self.height.saturating_sub(1));

                if new_s >= end {
                    *x = Some(Selection::Range(end - 1, new_s + 1, Direction::Down))
                } else if new_s >= end - 1 {
                    *x = Some(Selection::Single(new_s));
                } else {
                    *x = Some(Selection::Range(new_s, end, Direction::Up));
                }
                log(format!(
                    "shift down with upwards selection: {} {}",
                    (start + 1).saturating_sub(self.window.top_index),
                    self.window.size
                ));
                if let Some(s) = new_s
                    .saturating_sub(self.window.top_index)
                    .checked_sub(self.window.size)
                {
                    self.window.scroll_down(s + 1);
                }
                true
            }
            x @ Some(Selection::Range(_, _, Direction::Down)) => {
                let (start, end, _) = x.unwrap_range();

                if end + 1 > self.height {
                    return false;
                }
                // t..t+s
                let new_end = (end + inc).min(self.height);

                if let Some(s) = new_end
                    .saturating_sub(self.window.top_index)
                    .checked_sub(self.window.size)
                {
                    self.window.scroll_down(s);
                }

                *x = Some(Selection::Range(start, new_end, Direction::Down));
                true
            }
            _ => unimplemented!("Select down not supported with discontinuous selection."),
        }
    }

    /// Moves the selected cursor up if it exists, otherwise, creates a new cursor at the top.
    /// If the cursor moves past the beginning of the visible window, the window is moved up,
    /// and the selection index is unchanged.
    pub(crate) fn up(&mut self) -> bool {
        self.assert_invariants();

        log(format!("pressed up: {:?} {:?}", self.selected, self.window));

        if self.height == 0 || self.window.size == 0 {
            log("No selection applied - no items visible.");
            return false;
        }

        match &mut self.selected {
            x @ None => {
                *x = Some(Selection::Single(self.window.top_index));
                true
            }
            Some(Selection::Single(0)) => false,
            Some(Selection::Single(s)) => {
                if *s > self.window.top_index {
                    *s -= 1;
                    true
                } else if self.window.top_index > 0 {
                    *s -= 1;
                    self.window.top_index -= 1;
                    true
                } else {
                    false
                }
            }
            x @ Some(Selection::Range(_, _, _)) => {
                let (start, _, _) = x.unwrap_range();
                *x = Some(Selection::Single(start));
                if start < self.window.top_index {
                    self.window.top_index = start;
                }

                true
            }
            _ => unimplemented!("Select up not supported with discontinuous selection."),
        }
    }

    fn assert_invariants(&self) {
        // Can not select if nothing exists.
        debug_assert!(
            if self.height == 0 {
                self.selected.is_none()
            } else {
                true
            },
            "Selection with height = 0."
        );
        // Can not select values outside of bounds.
        debug_assert!(
            match &self.selected {
                None => true,
                Some(Selection::Single(x)) => {
                    *x < self.height
                }
                Some(Selection::Range(start, end, _)) => {
                    *start < *end && *end <= self.height
                }
                Some(Selection::Multi(tree, _)) => {
                    if let Some(s) = tree.iter().last() {
                        *s < self.height
                    } else {
                        false
                    }
                }
            },
            "Selection out of bounds."
        );
        // Can not have top index higher than window height, unless window height is 0.
        debug_assert!(
            if self.window.top_index >= self.window.height {
                self.window.top_index == 0
            } else {
                true
            },
            "Top index must be less than height. {} {}",
            self.window.top_index,
            self.window.height
        );
        // Can't scroll past end - unless there aren't enough items to fill screen.
        debug_assert!(
            if self.window.top_index + self.window.size > self.window.height {
                self.window.top_index == 0
            } else {
                true
            },
            "If window isn't full, height must be smaller than window size"
        );
    }
    /// Moves the view up by the size of the window, except in the case that moving the page
    /// up would move it past the beginning, in which case, the view is moved to the start.
    /// If a value is selected and the view is already at the top, the selection is also moved
    /// to the top. Otherwise, the selected index is unchanged.
    pub(crate) fn page_up(&mut self) -> bool {
        self.assert_invariants();
        log(format!(
            "pressed page up: {:?} {:?}",
            self.selected, self.window
        ));

        if self.height == 0 || self.window.size == 0 {
            log("No selection applied - no items visible.");
            return false;
        }

        let start = match &mut self.selected {
            Some(Selection::Single(s)) => {
                return s.replace_and_equal(s.saturating_sub(self.window.size))
                    | self.window.scroll_up(self.window.size);
            }
            Some(Selection::Range(start, _, _)) => *start,
            Some(Selection::Multi(tree, _)) => *tree.iter().next().unwrap(),
            None => {
                return self.window.scroll_up(self.window.size);
            }
        };

        if start < self.top_index() {
            return !self.window.top_index.replace_and_equal(start)
                | !self
                    .selected
                    .replace_and_equal(Some(Selection::Single(start)));
        }

        let replace_select = !self
            .selected
            .replace_and_equal(Some(Selection::Single(start)));
        let scroll = match start
            .saturating_sub(self.top_index())
            .checked_sub(self.window_size())
        {
            Some(ind) => self.window.scroll_up(ind + 1),
            None => false,
        };
        replace_select | scroll
    }

    pub(crate) fn select_up(&mut self, inc: usize) -> bool {
        self.assert_invariants();
        log(format!(
            "pressed shift up: {:?} {:?}",
            self.selected, self.window
        ));

        if self.height == 0 || self.window.size == 0 {
            log("No selection applied - no items visible.");
            return false;
        }

        match &mut self.selected {
            x @ None => {
                *x = Some(Selection::Single(self.window.top_index));
                true
            }
            Some(Selection::Single(0)) => false,
            x @ Some(Selection::Single(_)) => {
                let s = x.unwrap_single();

                if s.saturating_sub(inc) < self.window.top_index {
                    self.window.scroll_up(inc);
                }
                *x = Some(Selection::Range(
                    s.saturating_sub(inc),
                    s + 1,
                    Direction::Up,
                ));
                true
            }
            Some(Selection::Range(0, _, Direction::Up)) => false,
            Some(Selection::Range(start, _, Direction::Up)) => {
                *start = start.saturating_sub(inc);
                if *start < self.window.top_index {
                    self.window.top_index = *start;
                }
                true
            }
            x @ Some(Selection::Range(_, _, _)) => {
                let (start, end, _) = x.unwrap_range();

                let new_end = end.saturating_sub(inc);
                if new_end.saturating_sub(self.window.top_index) > self.window.size {
                    self.window.scroll_up(inc + 1);
                } else if new_end <= self.window.top_index {
                    self.window.top_index = new_end.saturating_sub(1);
                }

                if new_end < start {
                    *x = Some(Selection::Range(
                        new_end.saturating_sub(1),
                        start + 1,
                        Direction::Up,
                    ));
                    true
                } else if new_end <= start + 1 {
                    *x = Some(Selection::Single(start));
                    true
                } else {
                    *x = Some(Selection::Range(start, new_end, Direction::Down));
                    true
                }
            }
            _ => unimplemented!("Select up with discontinuous selections not supported."),
        }
    }

    pub fn select_page_up(&mut self) -> bool {
        self.assert_invariants();
        self.select_up(self.window.size)
    }

    pub fn select_page_down(&mut self) -> bool {
        self.assert_invariants();
        self.select_down(self.window.size)
    }

    pub fn select_to_home(&mut self) -> bool {
        self.assert_invariants();
        self.select_up(self.height)
    }

    pub fn select_to_end(&mut self) -> bool {
        self.assert_invariants();
        self.select_down(self.height)
    }

    /// Moves the view down by the size of the window, except in the case that moving the page
    /// down would move it past the down, in which case, the view is so that the end of the window
    /// is the end of the data.
    /// If a value is selected and the view is already at the bottom, the selection is also moved
    /// to the bottom. Otherwise, the selected index is unchanged.
    pub(crate) fn page_down(&mut self) -> bool {
        self.assert_invariants();
        log(format!(
            "pressed page down: {:?} {:?}",
            self.selected, self.window
        ));

        if self.height == 0 || self.window.size == 0 {
            log("No selection applied - no items visible.");
            return false;
        }

        let end = match &mut self.selected {
            Some(Selection::Single(s)) => {
                return s
                    .replace_and_equal((*s + self.window.size).min(self.height.saturating_sub(1)))
                    | self.window.scroll_down(self.window.size);
            }
            Some(Selection::Range(_, end, _)) => end.saturating_sub(1),
            Some(Selection::Multi(tree, _)) => *tree.iter().next_back().unwrap(),
            None => {
                return self.window.scroll_down(self.window.size);
            }
        };

        if end < self.top_index() {
            return !self.window.top_index.replace_and_equal(end)
                | !self
                    .selected
                    .replace_and_equal(Some(Selection::Single(end)));
        }

        let replace_select = !self
            .selected
            .replace_and_equal(Some(Selection::Single(end)));
        let scroll = match end
            .saturating_sub(self.top_index())
            .checked_sub(self.window_size())
        {
            Some(ind) => self.window.scroll_down(ind + 1),
            None => false,
        };
        replace_select | scroll
    }

    /// Moves the view to the end of the data. If a selection exists, the selection is moved to
    /// the bottom.
    pub(crate) fn end(&mut self) -> bool {
        self.assert_invariants();
        let t_change = self.window.scroll_down(self.height);
        let s_change = match &mut self.selected {
            None => false,
            Some(Selection::Single(s)) => !s.replace_and_equal(self.height.saturating_sub(1)),
            x @ Some(Selection::Range(_, _, _)) => {
                let (_, end, _) = x.unwrap_range();
                let ind = end.saturating_sub(1);
                *x = Some(Selection::Single(ind));
                if ind < self.window.top_index {
                    self.window.top_index = ind;
                    true
                } else if ind.saturating_sub(self.window.top_index) >= self.window.size {
                    self.window.scroll_down(
                        ind.saturating_sub(self.window.top_index) - self.window.size + 1,
                    )
                } else {
                    true
                }
            }
            _ => unimplemented!("end on multiselection not supported."),
        };
        t_change | s_change
    }

    /// Moves the view to the start of the data. If a selection exists, the selection is moved to
    /// the top.
    pub(crate) fn home(&mut self) -> bool {
        self.assert_invariants();
        let t_change = !self.window.top_index.replace_and_equal(0);
        let s_change = if self.selected.is_some() {
            self.select_relative(0)
        } else {
            false
        };
        t_change || s_change
    }

    /// Sets height internally and adjusts the window settings.
    pub(crate) fn refresh_height(&mut self, height: usize) {
        self.assert_invariants();
        self.height = height;
        self.window.refresh_height(height);
        match &mut self.selected {
            None => {}
            x @ Some(Selection::Single(_)) => {
                let ind = x.unwrap_single();
                if ind >= self.height {
                    *x = None;
                }
            }
            x @ Some(Selection::Range(_, _, _)) => {
                let (start, end, dir) = x.unwrap_range();
            }
            _ => unimplemented!(),
        }
        if height == 0 {
            self.selected = None;
        }
    }

    /// Adjusts the window size.
    /// If a selection exists, and the window size increases, the selected item in the data remains
    /// the same, and the start of the window moves up. If the window size decreases, the start of
    /// the window moves down, but the selected value does not change.
    pub(crate) fn refresh_window_size(&mut self, window_size: usize) -> bool {
        self.assert_invariants();
        self.window.refresh_size(window_size)
    }

    /// Gets the window size.
    pub(crate) fn window_size(&self) -> usize {
        self.assert_invariants();
        self.window.size
    }

    /// Return a range of the elements in data that are inside the window.
    pub(crate) fn window_range(&self) -> std::ops::Range<usize> {
        self.assert_invariants();
        self.window.range()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_at_end() {
        let cursor = PageCursorMultiple::new(20, 25);
        assert!(cursor.at_end());
        let mut cursor = PageCursorMultiple::new(40, 25);
        assert!(!cursor.at_end());
        cursor.scroll_down(10);
        assert!(!cursor.at_end());
        cursor.scroll_down(10);
        assert!(cursor.at_end());
    }

    #[test]
    fn cursor_select_up() {
        let mut cursor = PageCursorMultiple::new(20, 25);
        assert!(cursor.select_up(1));
        assert_eq!(cursor.selected, Some(Selection::Single(0)));
        assert!(!cursor.select_up(1));
        assert_eq!(cursor.selected, Some(Selection::Single(0)));
        assert!(!cursor.select_up(5));
        assert_eq!(cursor.selected, Some(Selection::Single(0)));
        cursor.select_down(5);
        assert_eq!(
            cursor.selected,
            Some(Selection::Range(0, 6, Direction::Down))
        );
        cursor.select_up(1);
        assert_eq!(
            cursor.selected,
            Some(Selection::Range(0, 5, Direction::Down))
        );
        cursor.select_up(3);
        assert_eq!(
            cursor.selected,
            Some(Selection::Range(0, 2, Direction::Down))
        );
        cursor.select_up(100);
        assert_eq!(cursor.selected, Some(Selection::Single(0)));

        assert!(cursor.select_down(25));
        assert_eq!(cursor.window.top_index, 0);
        assert!(cursor.down());
        assert_eq!(cursor.window.top_index, 0);
        assert_eq!(cursor.selected, Some(Selection::Single(19)));
        cursor.select_index(10);
        assert!(cursor.select_up(5));
        assert_eq!(
            cursor.selected,
            Some(Selection::Range(5, 11, Direction::Up))
        );
        assert!(cursor.select_down(10));
        assert_eq!(
            cursor.selected,
            Some(Selection::Range(10, 16, Direction::Down))
        );
        assert!(cursor.select_up(15));
        assert_eq!(
            cursor.selected,
            Some(Selection::Range(0, 11, Direction::Up))
        );
    }

    #[test]
    fn test_window_slice() {
        let mut cursor = PageCursorMultiple::new(50, 25);
        assert!(cursor.window_range().eq(0..25));
        assert!(cursor.scroll_down(10));
        assert!(cursor.window_range().eq(10..35));
        cursor.scroll_down(20);
        assert!(cursor.window_range().eq(25..50));
    }

    #[test]
    fn test_scroll() {
        let mut cursor = PageCursorMultiple::new(50, 25);
        assert_eq!(cursor.window_range().len(), 25);
        cursor.scroll_down(10);
        assert_eq!(cursor.window_range().len(), 25);
        cursor.scroll_down(20);
        assert_eq!(cursor.window_range().len(), 25);
        cursor.scroll_down(100);
        assert_eq!(cursor.window_range().len(), 25);
        cursor.scroll_up(10);
        assert_eq!(cursor.window_range().len(), 25);
        cursor.scroll_up(20);
        assert_eq!(cursor.window_range().len(), 25);
        cursor.scroll_up(100);
        assert_eq!(cursor.window_range().len(), 25);
    }

    #[test]
    fn test_selected_never_out_of_bounds() {
        let mut cursor = PageCursorMultiple::new(50, 25);
        cursor.select_relative(15);
        // assert!(cursor.selected().unwrap() < 50);
        // cursor.end();
        // assert!(cursor.selected().unwrap() < 25);
        // cursor.home();
        // assert!(cursor.selected().unwrap() < 25);
        // cursor.scroll_down(40);
        // assert!(cursor.selected().unwrap() < 25);
        // cursor.scroll_down(15);
        // assert!(cursor.selected().unwrap() < 25);
        // cursor.scroll_up(15);
        // assert!(cursor.selected().unwrap() < 25);
        // cursor.scroll_up(40);
        // assert!(cursor.selected().unwrap() < 25);
    }

    #[test]
    fn test_select_up_down_no_select_when_empty() {
        let mut cursor = PageCursorMultiple::new(25, 0);
        assert!(!cursor.select_up(1));
        assert!(!cursor.select_down(1));
        // assert!(!cursor.select(Some(0)));
    }
}
