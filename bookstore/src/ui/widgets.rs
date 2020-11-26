use std::iter::FromIterator;

use unicase::UniCase;

use tui::backend::Backend;
use tui::layout::{Constraint, Direction, Layout, Rect};
use tui::style::{Modifier, Style};
use tui::text::{Span, Text};
use tui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use tui::Frame;

use crate::record::Book;
use crate::ui::settings::InterfaceStyle;
use crate::ui::user_input::CommandString;

pub(crate) trait Widget<B: Backend> {
    /// Renders the widget into the frame, using the provided space.
    ///
    /// # Arguments
    ///
    /// * ` f ` - A frame to render into.
    /// * ` chunk ` - A chunk to specify the size of the widget.
    fn render_into_frame(&self, f: &mut Frame<B>, chunk: Rect);
}

fn cut_word_to_fit(word: &str, max_len: usize) -> ListItem {
    ListItem::new(Span::from(if word.len() > max_len {
        let mut base_word = word.chars().into_iter().collect::<Vec<_>>();
        base_word.truncate(max_len - 3);
        String::from_iter(base_word.iter()) + "..."
    } else {
        word.to_string()
    }))
}

fn split_chunk_into_columns(chunk: Rect, num_cols: u16) -> Vec<Rect> {
    let col_width = chunk.width / num_cols;

    let mut widths = vec![col_width; num_cols as usize];
    let total_w: u16 = widths.iter().sum();
    if total_w != chunk.width {
        widths[0] += chunk.width - total_w;
    }
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints(
            widths
                .into_iter()
                .map(Constraint::Length)
                .collect::<Vec<_>>(),
        )
        .split(chunk)
}

pub(crate) struct ColumnWidget<'a> {
    pub(crate) selected_cols: &'a Vec<UniCase<String>>,
    pub(crate) column_data: &'a Vec<Vec<String>>,
    pub(crate) style: &'a InterfaceStyle,
    pub(crate) selected: Option<usize>,
    // pub(crate) command_string: CommandString,
}

impl<'a, B: Backend> Widget<B> for ColumnWidget<'a> {
    fn render_into_frame(&self, f: &mut Frame<B>, chunk: Rect) {
        let hchunks = split_chunk_into_columns(chunk, self.selected_cols.len() as u16);
        let select_style = self.style.select_style();

        for ((title, data), &chunk) in self
            .selected_cols
            .iter()
            .zip(self.column_data.iter())
            .zip(hchunks.iter())
        {
            let list = List::new(
                data.iter()
                    .map(|word| cut_word_to_fit(word, chunk.width as usize - 3))
                    .collect::<Vec<_>>(),
            )
            .block(Block::default().title(Span::from(title.to_string())))
            .highlight_style(select_style);

            let mut selected_row = ListState::default();
            selected_row.select(self.selected);
            f.render_stateful_widget(list, chunk, &mut selected_row);
        }
    }
}

pub(crate) struct EditWidget<'a> {
    pub(crate) selected_cols: &'a Vec<UniCase<String>>,
    pub(crate) column_data: &'a Vec<Vec<String>>,
    pub(crate) style: &'a InterfaceStyle,
    pub(crate) selected: Option<usize>,
    pub(crate) selected_column: usize,
}

impl<'a, B: Backend> Widget<B> for EditWidget<'a> {
    fn render_into_frame(&self, f: &mut Frame<B>, chunk: Rect) {
        let hchunks = split_chunk_into_columns(chunk, self.selected_cols.len() as u16);

        let edit_style = self.style.edit_style();
        let select_style = self.style.select_style();

        for (i, ((title, data), &chunk)) in self
            .selected_cols
            .iter()
            .zip(self.column_data.iter())
            .zip(hchunks.iter())
            .enumerate()
        {
            let list = List::new(
                data.iter()
                    .map(|word| cut_word_to_fit(word, chunk.width as usize - 3))
                    .collect::<Vec<_>>(),
            )
            .block(Block::default().title(Span::from(title.to_string())))
            .highlight_style(if i == self.selected_column {
                edit_style
            } else {
                select_style
            });

            let mut selected_row = ListState::default();
            selected_row.select(self.selected);
            f.render_stateful_widget(list, chunk, &mut selected_row);
        }
    }
}

pub(crate) struct CommandWidget<'a> {
    command_string: &'a CommandString,
}

impl<'a> CommandWidget<'a> {
    pub(crate) fn new(command_string: &'a CommandString) -> Self {
        CommandWidget { command_string }
    }
}

impl<'a, B: Backend> Widget<B> for CommandWidget<'a> {
    /// Renders the command string into the frame, sized according to the chunk.
    ///
    /// # Arguments
    ///
    /// * ` f ` - A frame to render into.
    /// * ` chunk ` - A chunk to specify the command string size.
    fn render_into_frame(&self, f: &mut Frame<B>, chunk: Rect) {
        let command_widget = if !self.command_string.is_empty() {
            // TODO: Slow blink looks wrong
            let text = Text::styled(
                self.command_string.to_string(),
                Style::default().add_modifier(Modifier::BOLD),
            );
            Paragraph::new(text)
        } else {
            Paragraph::new(Text::styled(
                "Enter command or search",
                Style::default().add_modifier(Modifier::BOLD),
            ))
        };
        f.render_widget(command_widget, chunk);
    }
}

pub(crate) struct BookWidget<'a> {
    book: &'a Book,
}

impl<'a> BookWidget<'a> {
    pub(crate) fn new(book: &'a Book) -> Self {
        BookWidget { book }
    }
}

impl<'a, B: Backend> Widget<B> for BookWidget<'a> {
    fn render_into_frame(&self, f: &mut Frame<B>, chunk: Rect) {
        let field_exists = Style::default().add_modifier(Modifier::BOLD);
        let field_not_provided = Style::default();

        let mut data = if let Some(t) = &self.book.title {
            Text::styled(t, field_exists)
        } else {
            Text::styled("No title provided", field_not_provided)
        };
        if let Some(t) = &self.book.authors {
            let mut s = "By: ".to_string();
            s.push_str(&t.join(", "));
            data.extend(Text::styled(s, field_exists));
        } else {
            data.extend(Text::styled("No author provided", field_not_provided))
        };

        if let Some(columns) = self.book.get_extended_columns() {
            data.extend(Text::raw("\nTags provided:"));
            for (key, value) in columns.iter() {
                data.extend(Text::styled(
                    [key.as_str(), value.as_str()].join(": "),
                    field_exists,
                ));
            }
        }

        if let Some(variants) = self.book.get_variants() {
            let mut added_section = false;
            for variant in variants {
                if let Some(paths) = variant.get_paths() {
                    if !added_section {
                        added_section = true;
                        data.extend(Text::raw("\nVariant paths:"));
                    }
                    for (booktype, path) in paths {
                        let s = format!("{:?}: {}", booktype, path.display().to_string());
                        data.extend(Text::styled(s, field_exists));
                    }
                }
            }
        }

        let p = Paragraph::new(data);

        f.render_widget(p, chunk);
    }
}

pub(crate) struct BorderWidget {
    name: String,
}

impl BorderWidget {
    pub(crate) fn new(name: String) -> Self {
        BorderWidget { name }
    }
}

impl<B: Backend> Widget<B> for BorderWidget {
    fn render_into_frame(&self, f: &mut Frame<B>, chunk: Rect) {
        let block = Block::default()
            .title(format!(" bookshop || {} ", self.name))
            .borders(Borders::ALL);

        f.render_widget(block, chunk);
    }
}
