use std::path::PathBuf;

use crossterm::event::Event;
use tui::backend::Backend;
use tui::layout::Rect;
use tui::widgets::{Block, Borders};
use tui::Frame;

use bookworm_app::app::AppChannel;
use bookworm_database::AppDatabase;

use crate::ui::widgets::Widget;
use crate::{ApplicationTask, TuiError, UIState};

use async_trait::async_trait;

pub struct BorderWidget<D: AppDatabase + Send + Sync, B: Backend> {
    name: String,
    path: PathBuf,
    pub(crate) saved: bool,
    pub(crate) inner: Box<dyn Widget<D, B> + Send + Sync>,
}

impl<D: AppDatabase + Send + Sync, B: Backend> BorderWidget<D, B> {
    pub(crate) fn new(name: String, path: PathBuf, inner: Box<dyn Widget<D, B> + Send + Sync>) -> Self {
        BorderWidget {
            name,
            path,
            saved: true,
            inner,
        }
    }
}

#[async_trait]
impl<D: AppDatabase + Send + Sync, B: Backend> Widget<D, B> for BorderWidget<D, B> {
    async fn prepare_render(&mut self, state: &mut UIState<D>, chunk: Rect) {
        self.inner.prepare_render(
            state,
            Rect::new(
                chunk.x + 1,
                chunk.y + 1,
                chunk.width.saturating_sub(2),
                chunk.height.saturating_sub(2),
            ),
        ).await
    }

    fn render_into_frame(&self, f: &mut Frame<B>, state: &UIState<D>, chunk: Rect) {
        let block = Block::default()
            .title(format!(
                " bookworm || {} || {}{}",
                self.name,
                self.path.display(),
                if self.saved { " " } else { " * " }
            ))
            .borders(Borders::ALL);

        f.render_widget(block, chunk);
        self.inner.render_into_frame(
            f,
            state,
            Rect::new(
                chunk.x + 1,
                chunk.y + 1,
                chunk.width.saturating_sub(2),
                chunk.height.saturating_sub(2),
            ),
        )
    }

    async fn handle_input(
        &mut self,
        event: Event,
        state: &mut UIState<D>,
        app: &mut AppChannel<D>,
    ) -> Result<ApplicationTask, TuiError<D::Error>> {
        self.inner.handle_input(event, state, app).await
    }
}
