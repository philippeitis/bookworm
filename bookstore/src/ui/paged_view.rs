pub struct PageView<T> {
    top_index: usize,
    window_size: usize,
    selected: Option<usize>,
    data: Vec<T>,
}

// TODO: Make selection relative to values inside, and add select_in_window
impl<T> PageView<T> {
    /// Creates a new PageView, with the window starting at the first element of data,
    /// and no selection active
    pub(crate) fn new(window_size: usize, data: Vec<T>) -> Self {
        PageView {
            top_index: 0,
            window_size,
            selected: None,
            data,
        }
    }

    /// Returns whether the last element of data is in the window.
    pub(crate) fn at_end(&self) -> bool {
        self.window_size >= self.data.len() ||
            self.top_index >= self.data.len() - self.window_size - 1
    }

    /// Returns whether the first element of data is in the window
    pub(crate) fn at_top(&self) -> bool {
        self.top_index == 0
    }

    /// Moves the view down by inc, but does not move the view past the last element
    /// of data if possible.
    pub(crate) fn scroll_down(&mut self, inc: usize) -> bool {
        if self.top_index + inc + self.window_size > self.data.len() {
            let new_val = if self.data.len() <= self.window_size {
                0
            } else {
                self.data.len() - self.window_size
            };
            let c = self.top_index != new_val;
            self.top_index = new_val;
            c
        } else {
            self.top_index += inc;
            true
        }
    }

    /// Moves the view up by inc, but does not move the start past the first index.
    pub(crate) fn scroll_up(&mut self, inc: usize) -> bool {
        if self.top_index <= inc {
            let c = self.top_index != 0;
            self.top_index = 0;
            c
        } else {
            self.top_index -= inc;
            true
        }
    }

    /// Selects the value at index. If index is greater than the window size or data len,
    /// selects the largest index that is still visible.
    pub(crate) fn select(&mut self, index: Option<usize>) -> bool {
        if let Some(ind) = index {
            let ind = {
                let min = self.window_size.min(self.data.len());
                if ind >= min {
                    if min > 0 {
                        Some(min - 1)
                    } else {
                        None
                    }
                } else {
                    Some(ind)
                }
            };
            if self.selected == ind {
                false
            } else {
                self.selected = ind;
                true
            }
        } else {
            if self.selected == index {
                false
            } else {
                self.selected = index;
                true
            }
        }
    }

    /// Returns the selected value.
    pub(crate) fn selected(&self) -> Option<usize> {
        if let Some(ind) = self.selected {
            if self.window_size == 0 {
                Some(0)
            } else {
                Some(ind)
            }
        } else {
            None
        }
    }

    /// Returns the selected value, if one is selected.
    pub(crate) fn selected_item(&self) -> Option<&T> {
        if let Some(ind) = self.selected {
            self.data.get(self.top_index + ind)
        } else {
            None
        }
    }

    /// Moves the selected cursor down if it exists, otherwise, creates a new cursor at the bottom.
    /// If the cursor moves past the end of the visible window, the window is moved down.
    pub(crate) fn select_down(&mut self) -> bool {
        if let Some(s) = self.selected {
            if s < self.window_size - 1 && s < self.data.len() - 1 {
                self.select(Some(s + 1))
            } else {
                self.scroll_down(1)
            }
        } else {
            self.select(Some(self.window_size - 1))
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
            return if self.selected != Some(0) {
                self.selected = Some(0);
                true
            } else {
                false
            };
        }

        self.scroll_up(self.window_size)
    }

    /// Moves the view down by the size of the window, except in the case that moving the page
    /// down would move it past the down, in which case, the view is so that the end of the window
    /// is the end of the data.
    /// If a value is selected and the view is already at the bottom, the selection is also moved
    /// to the bottom. Otherwise, the selected index is unchanged.
    pub(crate) fn page_down(&mut self) -> bool {
        if self.selected.is_some() && self.at_end() {
            return if self.selected != Some(self.window_size - 1) {
                self.selected = Some(self.window_size - 1);
                true
            } else {
                false
            };
        }
        self.scroll_down(self.window_size)
    }

    /// Moves the view to the end of the data. If a selection exists, the selection is moved to
    /// the bottom.
    pub(crate) fn end(&mut self) -> bool {
        let t_change = self.scroll_down(self.data.len());
        let s_change = if self.selected.is_some() {
            let old_selection = self.selected;
            self.select(Some(self.window_size));
            old_selection != self.selected
        } else {
            false
        };
        t_change || s_change
    }

    /// Moves the view to the start of the data. If a selection exists, the selection is moved to
    /// the top.
    pub(crate) fn home(&mut self) -> bool {
        let t_change = if self.top_index == 0 {
            false
        } else {
            self.top_index = 0;
            true
        };
        let s_change = if self.selected.is_some() {
            let old_selection = self.selected;
            self.select(Some(0));
            old_selection != self.selected
        } else {
            false
        };
        t_change || s_change
    }

    /// Returns a reference to the internal data.
    pub(crate) fn data(&self) -> &Vec<T> {
        &self.data
    }

    /// Returns a mutable reference to the internal data.
    pub(crate) fn data_mut(&mut self) -> &mut Vec<T> {
        self.data.as_mut()
    }

    /// Adjusts the start of the window so that it doesn't go past the end of the data, if possible.
    pub(crate) fn refresh(&mut self) {
        if self.top_index + self.window_size > self.data.len() {
            if self.data.len() > self.window_size {
                self.top_index = self.data.len() - self.window_size;
            } else {
                self.top_index = 0;
            }
        }
    }

    #[allow(dead_code)]
    /// Sets data internally and adjusts the window settings.
    pub(crate) fn refresh_data(&mut self, data: Vec<T>) {
        self.data = data;
        self.refresh();
    }

    /// Adjusts the window size.
    /// If a selection exists, and the window size increases, the selected item in the data remains
    /// the same, and the start of the window moves up. If the window size decreases, the start of
    /// the window moves down, but the selected value does not change.
    pub(crate) fn refresh_window_size(&mut self, window_size: usize) {
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
                } else if ind >= self.data.len() {
                    if self.data.len() > 0 {
                        self.select(Some(self.data.len() - 1));
                    } else {
                        self.select(None);
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
    }

    /// Gets the window size.
    pub(crate) fn window_size(&self) -> usize {
        self.window_size
    }

    /// Return a slice of the elements in data that are inside the window.
    pub(crate) fn window_slice(&self) -> &[T] {
        &self.data[self.top_index..(self.top_index + self.window_size).min(self.data.len())]
    }

    // TODO: Add pop / insert / in_window
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_at_end() {
        let view = PageView::new(25, vec![0; 20]);
        assert!(view.at_end());
        let mut view = PageView::new(20, vec![0; 40]);
        assert!(!view.at_end());
        view.scroll_down(10);
        assert!(!view.at_end());
        view.scroll_down(10);
        assert!(view.at_end());
    }

    #[test]
    fn test_window_slice() {
        let vals: Vec<_> = (0..50).into_iter().collect();
        let mut view = PageView::new(25, vals.clone());
        assert!(view.window_slice().eq(&vals[0..25]));
        view.scroll_down(10);
        assert!(view.window_slice().eq(&vals[10..35]));
        view.scroll_down(20);
        assert!(view.window_slice().eq(&vals[25..50]));
    }

    #[test]
    fn test_scroll() {
        let mut view = PageView::new(25, vec![0; 50]);
        assert_eq!(view.window_slice().len(), 25);
        view.scroll_down(10);
        assert_eq!(view.window_slice().len(), 25);
        view.scroll_down(20);
        assert_eq!(view.window_slice().len(), 25);
        view.scroll_down(100);
        assert_eq!(view.window_slice().len(), 25);
        view.scroll_up(10);
        assert_eq!(view.window_slice().len(), 25);
        view.scroll_up(20);
        assert_eq!(view.window_slice().len(), 25);
        view.scroll_up(100);
        assert_eq!(view.window_slice().len(), 25);
    }

    #[test]
    fn test_selected_never_out_of_bounds() {
        let mut view = PageView::new(25, vec![0; 50]);
        view.select(Some(15));
        assert!(view.selected().unwrap() < 25);
        view.end();
        assert!(view.selected().unwrap() < 25);
        view.home();
        assert!(view.selected().unwrap() < 25);
        view.scroll_down(40);
        assert!(view.selected().unwrap() < 25);
        view.scroll_down(15);
        assert!(view.selected().unwrap() < 25);
        view.scroll_up(15);
        assert!(view.selected().unwrap() < 25);
        view.scroll_up(40);
        assert!(view.selected().unwrap() < 25);

    }

}
