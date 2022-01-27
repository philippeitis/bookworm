use std::marker::PhantomData;

use crossterm::event::{Event, KeyCode, MouseEventKind};
use tui::backend::Backend;
use tui::layout::{Constraint, Direction, Layout, Rect};
use tui::style::{Modifier, Style};
use tui::text::Text;
use tui::widgets::Paragraph;
use tui::Frame;

use bookworm_app::app::AppChannel;
use bookworm_database::AppDatabase;

use crate::ui::scrollable_text::ScrollableText;
use crate::ui::widgets::{InputHandler, ResizableWidget};
use crate::{AppView, ApplicationTask, TuiError, UIState};

pub struct HelpWidget<D> {
    pub(crate) text: ScrollableText,
    pub(crate) database: PhantomData<fn(D)>,
}

use async_trait::async_trait;

#[async_trait]
impl<'b, D: AppDatabase + Send + Sync, B: Backend> ResizableWidget<D, B> for HelpWidget<D> {
    async fn prepare_render(&mut self, _state: &mut UIState<D>, chunk: Rect) {
        let vchunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(chunk.height.saturating_sub(1)),
                Constraint::Length(1),
            ])
            .split(chunk);

        self.text
            .refresh_window_height(usize::from(vchunks[0].height));
    }

    fn render_into_frame(&self, f: &mut Frame<B>, _state: &UIState<D>, chunk: Rect) {
        let vchunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(chunk.height - 1), Constraint::Length(1)])
            .split(chunk);

        let paragraph = Paragraph::new(self.text.text())
            .scroll((self.text.offset() as u16, 0))
            .style(Style::default());

        f.render_widget(paragraph, vchunks[0]);
        let text = Text::styled(
            "Press ESC to return",
            Style::default().add_modifier(Modifier::BOLD),
        );

        f.render_widget(Paragraph::new(text), vchunks[1])
    }
}

#[async_trait]
impl<D: AppDatabase + Send + Sync> InputHandler<D> for HelpWidget<D> {
    async fn handle_input(
        &mut self,
        event: Event,
        state: &mut UIState<D>,
        _app: &mut AppChannel<D>,
    ) -> Result<ApplicationTask, TuiError<D::Error>> {
        match event {
            Event::Resize(_, _) => return Ok(ApplicationTask::UpdateUI),
            Event::Mouse(m) => match m.kind {
                MouseEventKind::ScrollDown => {
                    let scroll = state.nav_settings.scroll;
                    if state.nav_settings.inverted {
                        self.text.scroll_up(scroll)
                    } else {
                        self.text.scroll_down(scroll)
                    };
                }
                MouseEventKind::ScrollUp => {
                    let scroll = state.nav_settings.scroll;
                    if state.nav_settings.inverted {
                        self.text.scroll_down(scroll)
                    } else {
                        self.text.scroll_up(scroll)
                    };
                }
                _ => {
                    return Ok(ApplicationTask::DoNothing);
                }
            },
            // TODO: Add text input to look up commands.
            Event::Key(event) => {
                match event.code {
                    KeyCode::Esc => return Ok(ApplicationTask::SwitchView(AppView::Columns)),
                    // Scrolling
                    KeyCode::Up => {
                        self.text.scroll_up(1);
                    }
                    KeyCode::Down => {
                        self.text.scroll_down(1);
                    }
                    KeyCode::PageDown => {
                        self.text.page_down();
                    }
                    KeyCode::PageUp => {
                        self.text.page_up();
                    }
                    KeyCode::Home => {
                        self.text.home();
                    }
                    KeyCode::End => {
                        self.text.end();
                    }
                    _ => return Ok(ApplicationTask::DoNothing),
                }
            }
        }
        Ok(ApplicationTask::UpdateUI)
    }
}
