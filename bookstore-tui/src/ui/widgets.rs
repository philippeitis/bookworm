use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use tui::backend::Backend;
use tui::layout::Rect;
use tui::style::{Color, Modifier, Style};
use tui::text::{Span, Spans, Text};
use tui::widgets::{Block, Borders, Paragraph};
use tui::Frame;

use bookstore_app::user_input::{CharChunks, CommandString, Direction};
use bookstore_records::Book;

use crate::ui::scrollable_text::BlindOffset;

#[derive(Default, Copy, Clone)]
pub struct StyleRules {
    pub default: Style,
    pub selected: Style,
    pub cursor: Style,
}

impl StyleRules {
    pub fn add_modifier(self, modifier: Modifier) -> Self {
        self.add_default_modifier(modifier)
            .add_cursor_modifier(modifier)
            .add_selected_modifier(modifier)
    }

    pub fn add_cursor_modifier(mut self, modifier: Modifier) -> Self {
        self.cursor = self.cursor.add_modifier(modifier);
        self
    }

    pub fn add_selected_modifier(mut self, modifier: Modifier) -> Self {
        self.selected = self.selected.add_modifier(modifier);
        self
    }

    pub fn add_default_modifier(mut self, modifier: Modifier) -> Self {
        self.default = self.default.add_modifier(modifier);
        self
    }

    pub fn bg(self, color: Color) -> Self {
        self.cursor_bg(color).default_bg(color).selected_bg(color)
    }

    pub fn fg(self, color: Color) -> Self {
        self.cursor_fg(color).default_fg(color).selected_fg(color)
    }

    pub fn cursor_bg(mut self, color: Color) -> Self {
        self.cursor = self.cursor.bg(color);
        self
    }

    pub fn cursor_fg(mut self, color: Color) -> Self {
        self.cursor = self.cursor.fg(color);
        self
    }

    pub fn default_bg(mut self, color: Color) -> Self {
        self.default = self.default.bg(color);
        self
    }

    pub fn default_fg(mut self, color: Color) -> Self {
        self.default = self.default.fg(color);
        self
    }

    pub fn selected_bg(mut self, color: Color) -> Self {
        self.selected = self.selected.bg(color);
        self
    }

    pub fn selected_fg(mut self, color: Color) -> Self {
        self.selected = self.selected.fg(color);
        self
    }
}

pub(crate) trait Widget<B: Backend> {
    /// Renders the widget into the frame, using the provided space.
    ///
    /// # Arguments
    ///
    /// * ` f ` - A frame to render into.
    /// * ` chunk ` - A chunk to specify the size of the widget.
    fn render_into_frame(&self, f: &mut Frame<B>, chunk: Rect);
}

pub(crate) struct CommandWidget<'a> {
    command_string: &'a CommandString,
}

impl<'a> CommandWidget<'a> {
    pub(crate) fn new(command_string: &'a CommandString) -> Self {
        CommandWidget { command_string }
    }
}

/// Takes the CharChunk and styles it with the provided styling rules.
pub fn char_chunks_to_styled_text(c: CharChunks, styles: StyleRules) -> Spans {
    let mut text = vec![];
    match c {
        CharChunks::Selected(before, inside, after, direction) => {
            text.push(Span::styled(
                before.iter().collect::<String>(),
                styles.default,
            ));
            match direction {
                Direction::Left => match inside.split_first() {
                    None => unreachable!(),
                    Some((&cursor, rest)) => {
                        text.push(Span::styled(String::from(cursor), styles.cursor));
                        text.push(Span::styled(
                            rest.iter().collect::<String>(),
                            styles.selected,
                        ));
                        text.push(Span::styled(
                            after.iter().collect::<String>(),
                            styles.default,
                        ));
                    }
                },
                Direction::Right => {
                    text.push(Span::styled(
                        inside.iter().collect::<String>(),
                        styles.selected,
                    ));
                    match after.split_first() {
                        None => {
                            text.push(Span::styled(String::from(" "), styles.cursor));
                        }
                        Some((&cursor, rest)) => {
                            text.push(Span::styled(String::from(cursor), styles.cursor));
                            text.push(Span::styled(
                                rest.iter().collect::<String>(),
                                styles.default,
                            ));
                        }
                    }
                }
            }
        }
        CharChunks::Unselected(before, after) => {
            text.push(Span::styled(before, styles.default));
            if after.is_empty() {
                text.push(Span::styled(String::from(" "), styles.cursor));
            } else {
                let (cursor, rest) = after.split_at(1);
                text.push(Span::styled(cursor.to_string(), styles.cursor));
                text.push(Span::styled(rest.to_string(), styles.default));
            }
        }
    }
    Spans::from(text)
}

impl<'a, B: Backend> Widget<B> for CommandWidget<'a> {
    /// Renders the command string into the frame, sized according to the chunk.
    ///
    /// # Arguments
    ///
    /// * ` f ` - A frame to render into.
    /// * ` chunk ` - A chunk to specify the command string size.
    fn render_into_frame(&self, f: &mut Frame<B>, chunk: Rect) {
        let command_widget = if self.command_string.is_empty() {
            Paragraph::new(Text::styled(
                "Enter command or search",
                Style::default().add_modifier(Modifier::BOLD),
            ))
        } else {
            let styles = StyleRules::default()
                .add_modifier(Modifier::BOLD)
                .cursor_fg(Color::Black)
                .cursor_bg(Color::White)
                .add_cursor_modifier(Modifier::SLOW_BLINK)
                .selected_fg(Color::White)
                .selected_bg(Color::Blue);

            Paragraph::new(char_chunks_to_styled_text(
                self.command_string.char_chunks(),
                styles,
            ))
        };
        f.render_widget(command_widget, chunk);
    }
}
pub(crate) struct BookWidget {
    chunk: Rect,
    offset: BlindOffset,
    book: Arc<RwLock<Book>>,
}

impl BookWidget {
    pub fn new(chunk: Rect, book: Arc<RwLock<Book>>) -> Self {
        let mut book_widget = BookWidget {
            chunk,
            offset: BlindOffset::new(),
            book,
        };
        let height = chunk.height as usize;
        book_widget.offset.refresh_window_height(height as usize);
        book_widget
    }

    pub fn book(&self) -> &Arc<RwLock<Book>> {
        &self.book
    }

    pub fn set_chunk(&mut self, chunk: Rect) {
        self.chunk = chunk;
        let height = chunk.height as usize;
        self.offset.refresh_window_height(height as usize);
        self.offset
            .fit_offset_in_height(self.to_widget_text().lines.len());
    }
    pub fn contains_point(&self, col: u16, row: u16) -> bool {
        let rect = self.chunk;
        col >= rect.x && col < rect.x + rect.width && row >= rect.y && row < rect.y + rect.height
    }

    pub fn to_widget_text(&self) -> Text {
        let width = self.chunk.width as usize;
        let field_exists = Style::default().add_modifier(Modifier::BOLD);
        let field_not_provided = Style::default();
        let book = self
            .book
            .as_ref()
            .read()
            .expect("Failed to acquire read-only lock on book.");
        // Can the current directory change? Who knows. Definitely not me.
        let prefix = match std::env::current_dir() {
            Ok(d) => d.canonicalize().ok(),
            Err(_) => None,
        };

        let mut data = if let Some(t) = book.get_title() {
            Text::styled(t.to_string(), field_exists)
        } else {
            Text::styled("No title provided", field_not_provided)
        };

        if let Some(a) = book.get_authors() {
            let mut s = String::from("By: ");
            s.push_str(&a.join(", "));
            data.extend(Text::styled(s, field_exists));
        } else {
            data.extend(Text::styled("No author provided", field_not_provided));
        }

        if let Some(d) = book.get_description() {
            data.extend(Text::styled("\n", field_exists));
            // TODO: Make this look nice in the TUI.
            data.extend(Text::raw(html2text::from_read(d.as_bytes(), width)));
        }

        let columns = book.get_extended_columns();
        if !columns.is_empty() {
            data.extend(Text::raw("\nTags provided:"));
            for (key, value) in columns.iter() {
                data.extend(Text::styled(
                    [key.as_str(), value.as_str()].join(": "),
                    field_exists,
                ));
            }
        }

        let variants = book.get_variants();
        if !variants.is_empty() {
            data.extend(Text::raw("\nVariant paths:"));
            for variant in variants {
                let s = format!(
                    "{:?}: {}",
                    variant.book_type(),
                    if let Some(p) = prefix.as_ref() {
                        variant
                            .path()
                            .strip_prefix(p)
                            .unwrap_or_else(|_| variant.path())
                    } else {
                        variant.path()
                    }
                    .display()
                );
                data.extend(Text::styled(s, field_exists));
            }
        }

        data
    }

    pub fn offset_mut(&mut self) -> &mut BlindOffset {
        &mut self.offset
    }
}

impl<B: Backend> Widget<B> for BookWidget {
    fn render_into_frame(&self, f: &mut Frame<B>, chunk: Rect) {
        let book_text = self.to_widget_text();
        let offset = self.offset.offset();
        let p = Paragraph::new(book_text).scroll((offset as u16, 0));
        f.render_widget(p, chunk);
    }
}

pub(crate) struct BorderWidget {
    name: String,
    path: PathBuf,
    pub(crate) saved: bool,
}

impl BorderWidget {
    pub(crate) fn new(name: String, path: PathBuf) -> Self {
        BorderWidget {
            name,
            path,
            saved: true,
        }
    }
}

impl<B: Backend> Widget<B> for BorderWidget {
    fn render_into_frame(&self, f: &mut Frame<B>, chunk: Rect) {
        let block = Block::default()
            .title(format!(
                " bookstore || {} || {}{}",
                self.name,
                self.path.display(),
                if self.saved { " " } else { " * " }
            ))
            .borders(Borders::ALL);

        f.render_widget(block, chunk);
    }
}
