use std::marker::PhantomData;
use std::sync::Arc;

use crossterm::event::{Event, MouseEventKind};
use tui::backend::Backend;
use tui::layout::Rect;
use tui::style::{Modifier, Style};
use tui::text::Text;
use tui::widgets::Paragraph;
use tui::Frame;

use bookworm_app::app::AppChannel;
use bookworm_database::AppDatabase;
use bookworm_records::Book;

use crate::ui::scrollable_text::BlindOffset;
use crate::ui::widgets::Widget;
use crate::{ApplicationTask, TuiError, UIState};

use async_trait::async_trait;

/// Contains information needed to render a book.
/// Only guaranteed to reflect the current state of the book if no
/// EditCommand occurs - should be regenerated during the prepare_render call
pub struct BookWidget<D> {
    chunk: Rect,
    offset: BlindOffset,
    book: Arc<Book>,
    pub(crate) database: PhantomData<fn(D)>,
}

impl<D> BookWidget<D> {
    pub fn new(chunk: Rect, book: Arc<Book>) -> Self {
        let mut book_widget = BookWidget {
            chunk,
            offset: BlindOffset::new(),
            book,
            database: PhantomData,
        };
        let height = chunk.height as usize;
        book_widget.offset.refresh_window_height(height as usize);
        book_widget
    }

    pub fn set_chunk(&mut self, chunk: Rect) {
        self.chunk = chunk;
        let height = chunk.height as usize;
        self.offset.refresh_window_height(height as usize);
        self.offset
            .fit_offset_in_height(self.to_widget_text().lines.len());
    }

    pub fn to_widget_text(&self) -> Text {
        let width = self.chunk.width as usize;
        let field_exists = Style::default().add_modifier(Modifier::BOLD);
        let field_not_provided = Style::default();
        let prefix = match std::env::current_dir() {
            Ok(d) => d.canonicalize().ok(),
            Err(_) => None,
        };

        let mut data = if let Some(t) = self.book.title() {
            Text::styled(t.to_string(), field_exists)
        } else {
            Text::styled("No title provided", field_not_provided)
        };

        if let Some(a) = self.book.authors() {
            let mut s = String::from("By: ");
            s.push_str(&a.join(", "));
            data.extend(Text::styled(s, field_exists));
        } else {
            data.extend(Text::styled("No author provided", field_not_provided));
        }

        if let Some(d) = self.book.description() {
            data.extend(Text::styled("\n", field_exists));
            // TODO: Make this look nice in the TUI.
            data.extend(Text::raw(html2text::from_read(d.as_bytes(), width)));
        }

        let columns = self.book.tags();
        if !columns.is_empty() {
            data.extend(Text::raw("\nNamed tags provided:"));
            for (key, value) in columns.iter() {
                data.extend(Text::styled(
                    [key.as_str(), value.as_str()].join(": "),
                    field_exists,
                ));
            }
        }

        let free_tags = self.book.free_tags();
        if !free_tags.is_empty() {
            data.extend(Text::raw("\nTags provided:"));
            for value in free_tags.iter() {
                data.extend(Text::styled(value.clone(), field_exists));
            }
        }

        let variants = self.book.variants();
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

    pub(crate) fn offset_mut(&mut self) -> &mut BlindOffset {
        &mut self.offset
    }
}

#[async_trait]
impl<'b, D: AppDatabase + Send + Sync, B: Backend> Widget<D, B> for BookWidget<D> {
    async fn prepare_render(&mut self, state: &mut UIState<D>, chunk: Rect) {
        // need to push to parent
        let books = state.book_view.selected_books();
        *self = books
            .front()
            .map(|book| BookWidget::new(Rect::default(), book.clone()))
            .unwrap();

        self.set_chunk(chunk);
    }

    fn render_into_frame(&self, f: &mut Frame<B>, _state: &UIState<D>, chunk: Rect) {
        let book_text = self.to_widget_text();
        let offset = self.offset.offset();
        let p = Paragraph::new(book_text).scroll((offset as u16, 0));
        f.render_widget(p, chunk);
    }

    async fn handle_input(
        &mut self,
        event: Event,
        state: &mut UIState<D>,
        _app: &mut AppChannel<D>,
    ) -> Result<ApplicationTask, TuiError<D::Error>> {
        match event {
            Event::Mouse(m) => match m.kind {
                MouseEventKind::ScrollDown => {
                    let inverted = state.nav_settings.inverted;
                    let scroll = state.nav_settings.scroll;
                    if inverted {
                        self.offset_mut().scroll_up(scroll);
                    } else {
                        self.offset_mut().scroll_down(scroll);
                    }
                }
                MouseEventKind::ScrollUp => {
                    let inverted = state.nav_settings.inverted;
                    let scroll = state.nav_settings.scroll;
                    if inverted {
                        self.offset_mut().scroll_down(scroll);
                    } else {
                        self.offset_mut().scroll_up(scroll);
                    }
                }
                _ => {
                    return Ok(ApplicationTask::DoNothing);
                }
            },
            _ => {
                return Ok(ApplicationTask::UpdateUI);
            }
        }
        Ok(ApplicationTask::UpdateUI)
    }
}
