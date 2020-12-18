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

// TODO: Add line-wrapping.
pub(crate) fn book_to_widget_text(book: &Book, width: usize) -> Text {
    let field_exists = Style::default().add_modifier(Modifier::BOLD);
    let field_not_provided = Style::default();

    // Can the current directory change? Who knows. Definitely not me.
    let prefix = match std::env::current_dir() {
        Ok(d) => d.canonicalize().ok(),
        Err(_) => None,
    };

    let mut data = if let Some(t) = book.get_title() {
        Text::styled(t, field_exists)
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

    if let Some(columns) = book.get_extended_columns() {
        data.extend(Text::raw("\nTags provided:"));
        for (key, value) in columns.iter() {
            data.extend(Text::styled(
                [key.as_str(), value.as_str()].join(": "),
                field_exists,
            ));
        }
    }

    if let Some(variants) = book.get_variants() {
        if !variants.is_empty() {
            data.extend(Text::raw("\nVariant paths:"));
        }
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
