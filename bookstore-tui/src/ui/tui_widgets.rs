#![allow(dead_code)]

use std::collections::BTreeSet;
use tui::buffer::Buffer;
use tui::layout::{Corner, Rect};
use tui::style::Style;
use tui::text::Text;
use tui::widgets::{Block, StatefulWidget, Widget};

use unicode_width::UnicodeWidthStr;

#[derive(Debug, Clone)]
pub struct MultiSelectListState {
    offset: usize,
    target: Option<usize>,
    selected: BTreeSet<usize>,
}

impl Default for MultiSelectListState {
    fn default() -> MultiSelectListState {
        MultiSelectListState {
            offset: 0,
            target: None,
            selected: BTreeSet::new(),
        }
    }
}

impl MultiSelectListState {
    pub fn selected(&self) -> &BTreeSet<usize> {
        &self.selected
    }

    pub fn select(&mut self, index: usize) {
        self.selected.insert(index);
    }

    pub fn front_selection(&mut self) -> Option<usize> {
        match self.target {
            None => self.selected.iter().next().cloned(),
            Some(target) => Some(target),
        }
    }

    pub fn deselect(&mut self, index: usize) {
        self.selected.remove(&index);
    }

    pub fn deselect_all(&mut self) {
        self.selected.clear();
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ListItemX<'a> {
    content: Text<'a>,
    style: Style,
}

impl<'a> ListItemX<'a> {
    pub fn new<T>(content: T) -> ListItemX<'a>
    where
        T: Into<Text<'a>>,
    {
        ListItemX {
            content: content.into(),
            style: Style::default(),
        }
    }

    pub fn style(mut self, style: Style) -> ListItemX<'a> {
        self.style = style;
        self
    }

    pub fn height(&self) -> usize {
        self.content.height()
    }
}

#[derive(Debug, Clone)]
pub struct MultiSelectList<'a> {
    block: Option<Block<'a>>,
    items: Vec<ListItemX<'a>>,
    /// Style used as a base style for the widget
    style: Style,
    start_corner: Corner,
    /// Style used to render selected item
    highlight_style: Style,
    /// Symbol in front of the selected item (Shift all items to the right)
    highlight_symbol: Option<&'a str>,
}

impl<'a> MultiSelectList<'a> {
    pub fn new<T>(items: T) -> MultiSelectList<'a>
    where
        T: Into<Vec<ListItemX<'a>>>,
    {
        MultiSelectList {
            block: None,
            style: Style::default(),
            items: items.into(),
            start_corner: Corner::TopLeft,
            highlight_style: Style::default(),
            highlight_symbol: None,
        }
    }

    pub fn block(mut self, block: Block<'a>) -> MultiSelectList<'a> {
        self.block = Some(block);
        self
    }

    pub fn style(mut self, style: Style) -> MultiSelectList<'a> {
        self.style = style;
        self
    }

    pub fn highlight_symbol(mut self, highlight_symbol: &'a str) -> MultiSelectList<'a> {
        self.highlight_symbol = Some(highlight_symbol);
        self
    }

    pub fn highlight_style(mut self, style: Style) -> MultiSelectList<'a> {
        self.highlight_style = style;
        self
    }

    pub fn start_corner(mut self, corner: Corner) -> MultiSelectList<'a> {
        self.start_corner = corner;
        self
    }

    fn get_items_bounds(
        &self,
        selected: Option<usize>,
        offset: usize,
        max_height: usize,
    ) -> (usize, usize) {
        let offset = offset.min(self.items.len().saturating_sub(1));
        let mut start = offset;
        let mut end = offset;
        let mut height = 0;
        for item in self.items.iter().skip(offset) {
            if height + item.height() > max_height {
                break;
            }
            height += item.height();
            end += 1;
        }

        let selected = selected.unwrap_or(0).min(self.items.len() - 1);
        while selected >= end {
            height = height.saturating_add(self.items[end].height());
            end += 1;
            while height > max_height {
                height = height.saturating_sub(self.items[start].height());
                start += 1;
            }
        }
        while selected < start {
            start -= 1;
            height = height.saturating_add(self.items[start].height());
            while height > max_height {
                end -= 1;
                height = height.saturating_sub(self.items[end].height());
            }
        }
        (start, end)
    }
}

impl<'a> StatefulWidget for MultiSelectList<'a> {
    type State = MultiSelectListState;

    fn render(mut self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        buf.set_style(area, self.style);
        let list_area = match self.block.take() {
            Some(b) => {
                let inner_area = b.inner(area);
                b.render(area, buf);
                inner_area
            }
            None => area,
        };

        if list_area.width < 1 || list_area.height < 1 {
            return;
        }

        if self.items.is_empty() {
            return;
        }
        let list_height = list_area.height as usize;

        let (start, end) =
            self.get_items_bounds(state.front_selection(), state.offset, list_height);
        state.offset = start;

        let highlight_symbol = self.highlight_symbol.unwrap_or("");
        let blank_symbol = " ".repeat(highlight_symbol.width());

        let mut current_height = 0;
        let has_selection = !state.selected.is_empty();
        for (i, item) in self
            .items
            .iter_mut()
            .enumerate()
            .skip(state.offset)
            .take(end - start)
        {
            let (x, y) = match self.start_corner {
                Corner::BottomLeft => {
                    current_height += item.height() as u16;
                    (list_area.left(), list_area.bottom() - current_height)
                }
                _ => {
                    let pos = (list_area.left(), list_area.top() + current_height);
                    current_height += item.height() as u16;
                    pos
                }
            };
            let area = Rect {
                x,
                y,
                width: list_area.width,
                height: item.height() as u16,
            };
            let item_style = self.style.patch(item.style);
            buf.set_style(area, item_style);

            let is_selected = state.selected.contains(&i);
            let elem_x = if has_selection {
                let symbol = if is_selected {
                    highlight_symbol
                } else {
                    &blank_symbol
                };
                let (x, _) = buf.set_stringn(x, y, symbol, list_area.width as usize, item_style);
                x
            } else {
                x
            };
            let max_element_width = (list_area.width - (elem_x - x)) as usize;
            for (j, line) in item.content.lines.iter().enumerate() {
                buf.set_spans(elem_x, y + j as u16, line, max_element_width as u16);
            }
            if is_selected {
                buf.set_style(area, self.highlight_style);
            }
        }
    }
}
