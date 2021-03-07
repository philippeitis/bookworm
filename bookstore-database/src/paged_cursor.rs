pub struct PageCursor {
    top_index: usize,
    window_size: usize,
    selected: Option<usize>,
    height: usize,
}

trait ReplaceAndEqual {
    /// Replaces `self` with `other`, and returns whether `self` was
    /// equal to `other`.
    fn replace_and_equal(&mut self, other: Self) -> bool;
}

impl<T: PartialEq + Copy> ReplaceAndEqual for T {
    #[inline(always)]
    fn replace_and_equal(&mut self, other: Self) -> bool {
        std::mem::replace(self, other) == other
    }
}

impl PageCursor {
    /// Creates a new Cursor at location 0, with no selection active.
    pub(crate) fn new(window_size: usize, height: usize) -> Self {
        PageCursor {
            top_index: 0,
            window_size,
            selected: None,
            height,
        }
    }

    /// Returns whether the last element of data is in the window.
    pub(crate) fn at_end(&self) -> bool {
        self.window_size >= self.height || self.top_index >= self.height - self.window_size - 1
    }

    /// Returns whether the first element of data is in the window
    pub(crate) fn at_top(&self) -> bool {
        self.top_index == 0
    }

    /// Moves the view down by inc, but does not move the view past the last element
    /// of data if possible.
    pub(crate) fn scroll_down(&mut self, inc: usize) -> bool {
        if self.top_index + inc + self.window_size > self.height {
            let new_val = self.height.saturating_sub(self.window_size);
            !self.top_index.replace_and_equal(new_val)
        } else {
            self.top_index += inc;
            true
        }
    }

    /// Moves the view up by inc, but does not move the start past the first index.
    pub(crate) fn scroll_up(&mut self, inc: usize) -> bool {
        !self
            .top_index
            .replace_and_equal(self.top_index.saturating_sub(inc))
    }

    /// Selects the value at index. If index is greater than the window size or data len,
    /// selects the largest index that is still visible.
    pub(crate) fn select(&mut self, index: Option<usize>) -> bool {
        if let Some(ind) = index {
            let ind = {
                let min = self.window_size.min(self.height);
                if min == 0 {
                    min
                } else if ind >= min {
                    min - 1
                } else {
                    ind
                }
            };
            !self.selected.replace_and_equal(Some(ind))
        } else {
            !self.selected.replace_and_equal(index)
        }
    }

    /// Returns the selected value.
    pub(crate) fn selected(&self) -> Option<usize> {
        Some(self.selected?.min(self.window_size).min(self.height))
    }

    /// Returns the selected value, if one is selected.
    pub(crate) fn selected_index(&self) -> Option<usize> {
        Some(self.selected? + self.top_index)
    }

    /// Moves the selected cursor down if it exists, otherwise, creates a new cursor at the bottom.
    /// If the cursor moves past the end of the visible window, the window is moved down.
    pub(crate) fn select_down(&mut self) -> bool {
        if let Some(s) = self.selected {
            if s + 1 < self.window_size.min(self.height) {
                self.select(Some(s + 1))
            } else {
                self.scroll_down(1)
            }
        } else {
            self.select(self.window_size.checked_sub(1))
        }
    }

    /// Moves the selected cursor up if it exists, otherwise, creates a new cursor at the top.
    /// If the cursor moves past the beginning of the visible window, the window is moved up,
    /// and the selection index is unchanged.
    pub(crate) fn select_up(&mut self) -> bool {
        if let Some(s) = self.selected {
            if s >= 1 {
                self.select(Some(s - 1))
            } else {
                self.scroll_up(1)
            }
        } else {
            self.select(Some(0))
        }
    }

    /// Moves the view up by the size of the window, except in the case that moving the page
    /// up would move it past the beginning, in which case, the view is moved to the start.
    /// If a value is selected and the view is already at the top, the selection is also moved
    /// to the top. Otherwise, the selected index is unchanged.
    pub(crate) fn page_up(&mut self) -> bool {
        if self.selected.is_some() && self.at_top() {
            !self.selected.replace_and_equal(Some(0))
        } else {
            self.scroll_up(self.window_size)
        }
    }

    /// Moves the view down by the size of the window, except in the case that moving the page
    /// down would move it past the down, in which case, the view is so that the end of the window
    /// is the end of the data.
    /// If a value is selected and the view is already at the bottom, the selection is also moved
    /// to the bottom. Otherwise, the selected index is unchanged.
    pub(crate) fn page_down(&mut self) -> bool {
        if self.selected.is_some() && self.at_end() {
            !self
                .selected
                .replace_and_equal(Some(self.window_size.saturating_sub(1)))
        } else {
            self.scroll_down(self.window_size)
        }
    }

    /// Moves the view to the end of the data. If a selection exists, the selection is moved to
    /// the bottom.
    pub(crate) fn end(&mut self) -> bool {
        let t_change = self.scroll_down(self.height);
        let s_change = if self.selected.is_some() {
            self.select(Some(self.window_size))
        } else {
            false
        };
        t_change || s_change
    }

    /// Moves the view to the start of the data. If a selection exists, the selection is moved to
    /// the top.
    pub(crate) fn home(&mut self) -> bool {
        let t_change = !self.top_index.replace_and_equal(0);
        let s_change = if self.selected.is_some() {
            !self.selected.replace_and_equal(Some(0))
        } else {
            false
        };
        t_change || s_change
    }

    /// Adjusts the start of the window so that it doesn't go past the end of the data, if possible.
    pub(crate) fn refresh(&mut self) {
        if self.top_index + self.window_size > self.height {
            self.top_index = self.height.saturating_sub(self.window_size)
        }
    }

    /// Sets height internally and adjusts the window settings.
    pub(crate) fn refresh_height(&mut self, height: usize) {
        self.height = height;
        self.refresh();
    }

    /// Adjusts the window size.
    /// If a selection exists, and the window size increases, the selected item in the data remains
    /// the same, and the start of the window moves up. If the window size decreases, the start of
    /// the window moves down, but the selected value does not change.
    pub(crate) fn refresh_window_size(&mut self, window_size: usize) -> bool {
        let old_size = self.window_size;
        self.window_size = window_size;
        if let Some(ind) = self.selected {
            if window_size < old_size {
                if ind + 1 >= window_size {
                    let diff = ind + 1 - window_size;
                    self.top_index += diff;
                    self.select(if window_size == 0 {
                        None
                    } else {
                        Some(window_size - 1)
                    });
                } else if ind >= self.height {
                    if self.height == 0 {
                        self.select(None);
                    } else {
                        self.select(Some(self.height - 1));
                    }
                }
            } else if window_size >= old_size {
                if self.top_index < window_size - old_size {
                    self.select(Some(ind + self.top_index));
                    self.top_index = 0;
                } else {
                    let diff = window_size - old_size;
                    self.top_index -= diff;
                    self.select(Some(ind + diff));
                }
            }
        }
        self.refresh();
        old_size != self.window_size
    }

    /// Gets the window size.
    pub(crate) fn window_size(&self) -> usize {
        self.window_size
    }

    /// Return a range of the elements in data that are inside the window.
    pub(crate) fn window_range(&self) -> std::ops::Range<usize> {
        self.top_index..(self.top_index + self.window_size).min(self.height)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_at_end() {
        let cursor = PageCursor::new(25, 20);
        assert!(cursor.at_end());
        let mut cursor = PageCursor::new(20, 40);
        assert!(!cursor.at_end());
        cursor.scroll_down(10);
        assert!(!cursor.at_end());
        cursor.scroll_down(10);
        assert!(cursor.at_end());
    }

    #[test]
    fn test_window_slice() {
        let mut cursor = PageCursor::new(25, 50);
        assert!(cursor.window_range().eq(0..25));
        cursor.scroll_down(10);
        assert!(cursor.window_range().eq(10..35));
        cursor.scroll_down(20);
        assert!(cursor.window_range().eq(25..50));
    }

    #[test]
    fn test_scroll() {
        let mut cursor = PageCursor::new(25, 50);
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
        let mut cursor = PageCursor::new(25, 50);
        cursor.select(Some(15));
        assert!(cursor.selected().unwrap() < 25);
        cursor.end();
        assert!(cursor.selected().unwrap() < 25);
        cursor.home();
        assert!(cursor.selected().unwrap() < 25);
        cursor.scroll_down(40);
        assert!(cursor.selected().unwrap() < 25);
        cursor.scroll_down(15);
        assert!(cursor.selected().unwrap() < 25);
        cursor.scroll_up(15);
        assert!(cursor.selected().unwrap() < 25);
        cursor.scroll_up(40);
        assert!(cursor.selected().unwrap() < 25);
    }
}
