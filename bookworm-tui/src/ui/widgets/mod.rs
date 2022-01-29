mod book_widget;
mod border_widget;
mod box_widget;
mod column_widget;
mod command_widget;
mod edit_widget;
mod help_widget;

use crossterm::event::Event;

use tui::backend::Backend;
use tui::layout::Rect;
use tui::style::{Modifier, Style};
use tui::Frame;

use bookworm_app::app::AppChannel;
use bookworm_app::settings::InterfaceStyle;
use bookworm_database::AppDatabase;

use crate::ui::utils::{to_tui, ApplicationTask, TuiStyle};
use crate::{TuiError, UIState};

pub use book_widget::BookWidget;
pub use border_widget::BorderWidget;
pub use box_widget::WidgetBox;
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

#[async_trait]
pub(crate) trait Widget<D: AppDatabase + Send + Sync, B: Backend> {
    /// Resizes the widget before the rendering step. May require changing the UIState - for example,
    /// if more books need to be fetched.
    async fn prepare_render(&mut self, state: &mut UIState<D>, chunk: Rect);

    /// Renders the widget into the frame, using the provided space.
    ///
    /// # Arguments
    ///
    /// * ` f ` - A frame to render into.
    /// * ` chunk ` - A chunk to specify the size of the widget.
    fn render_into_frame(&self, f: &mut Frame<B>, state: &UIState<D>, chunk: Rect);

    /// Processes `event` and may modify the UI state `state`, or the overall application state `app`,
    /// depending on specific event.
    ///
    /// Example: If `event` triggers submission of command which adds books, then `state` may
    /// register new books, and `app` may be updated as new books are added.
    ///
    /// Returns ApplicationTask::DoNothing in the event that the input is not captured.
    async fn handle_input(
        &mut self,
        event: Event,
        state: &mut UIState<D>,
        app: &mut AppChannel<D>,
    ) -> Result<ApplicationTask, TuiError<D::Error>>;
}
