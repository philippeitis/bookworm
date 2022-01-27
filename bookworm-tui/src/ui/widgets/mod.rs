mod book_widget;
mod column_widget;
mod command_widget;
mod edit_widget;
mod help_widget;

use std::path::PathBuf;

use crossterm::event::Event;

use tui::backend::Backend;
use tui::layout::Rect;
use tui::style::{Modifier, Style};
use tui::widgets::{Block, Borders};
use tui::Frame;

use bookworm_app::app::AppChannel;
use bookworm_app::settings::InterfaceStyle;
use bookworm_database::AppDatabase;

use crate::ui::utils::{to_tui, ApplicationTask, TuiStyle};
use crate::{TuiError, UIState};

pub use book_widget::BookWidget;
pub use column_widget::ColumnWidget;
pub use command_widget::CommandWidget;
pub use edit_widget::EditWidget;
pub use help_widget::HelpWidget;

use async_trait::async_trait;

// TODO: Add Find widget that does live searching as user types (but doesn't update if match isn't being changed).
impl TuiStyle for InterfaceStyle {
    fn edit_style(&self) -> Style {
        Style::default()
            .fg(to_tui(self.edit_fg))
            .bg(to_tui(self.edit_bg))
    }

    fn select_style(&self) -> Style {
        Style::default()
            .fg(to_tui(self.selected_fg))
            .bg(to_tui(self.selected_bg))
    }

    fn cursor_style(&self) -> Style {
        Style::default()
            .fg(to_tui(self.cursor_fg))
            .bg(to_tui(self.cursor_bg))
            .add_modifier(Modifier::SLOW_BLINK)
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
                " bookworm || {} || {}{}",
                self.name,
                self.path.display(),
                if self.saved { " " } else { " * " }
            ))
            .borders(Borders::ALL);

        f.render_widget(block, chunk);
    }
}

#[async_trait]
pub(crate) trait ResizableWidget<D: AppDatabase + Send + Sync, B: Backend> {
    // Prepares to render the app
    async fn prepare_render(&mut self, state: &mut UIState<D>, chunk: Rect);

    /// Renders the widget into the frame, using the provided space.
    ///
    /// # Arguments
    ///
    /// * ` f ` - A frame to render into.
    /// * ` chunk ` - A chunk to specify the size of the widget.
    fn render_into_frame(&self, f: &mut Frame<B>, state: &UIState<D>, chunk: Rect);

    /// Returns whether the widget is currently capturing the key event.
    /// Typically returns true, but may return false if "esc" is pressed and nothing
    /// is active, leaving parent to handle it.
    fn capturing(&self, _event: &Event) -> bool {
        true
    }
}

#[async_trait]
pub(crate) trait InputHandler<D: AppDatabase + Send + Sync> {
    /// Processes the event and modifies the internal state accordingly. May modify app,
    /// depending on specific event.
    async fn handle_input(
        &mut self,
        event: Event,
        state: &mut UIState<D>,
        app: &mut AppChannel<D>,
    ) -> Result<ApplicationTask, TuiError<D::Error>>;
}
