use tui::backend::Backend;
use tui::layout::Rect;
use tui::style::{Modifier, Style};
use tui::text::Text;
use tui::widgets::{Block, Borders, Paragraph};
use tui::Frame;

use crate::app::user_input::CommandString;
use crate::record::Book;

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
            let mut s = String::from("By: ");
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
            if !variants.is_empty() {
                data.extend(Text::raw("\nVariant paths:"));
            }
            for variant in variants {
                let s = format!(
                    "{:?}: {}",
                    variant.book_type(),
                    variant.path().display().to_string()
                );
                data.extend(Text::styled(s, field_exists));
            }
        }

        let p = Paragraph::new(data);

        f.render_widget(p, chunk);
    }
}

pub(crate) struct BorderWidget {
    name: String,
    pub(crate) saved: bool,
}

impl BorderWidget {
    pub(crate) fn new(name: String) -> Self {
        BorderWidget { name, saved: true }
    }
}

impl<B: Backend> Widget<B> for BorderWidget {
    fn render_into_frame(&self, f: &mut Frame<B>, chunk: Rect) {
        let block = Block::default()
            .title(format!(
                " bookshop || {}{}",
                self.name,
                if self.saved { " " } else { " * " }
            ))
            .borders(Borders::ALL);

        f.render_widget(block, chunk);
    }
}
