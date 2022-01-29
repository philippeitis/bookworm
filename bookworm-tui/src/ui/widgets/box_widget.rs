use crossterm::event::{Event, KeyCode, MouseButton, MouseEventKind};
use std::collections::VecDeque;
use tui::backend::Backend;
use tui::layout::Rect;
use tui::Frame;

use bookworm_app::app::AppChannel;
use bookworm_database::AppDatabase;

use crate::ui::views::{LayoutGenerator, RectExt};
use crate::ui::widgets::Widget;
use crate::{ApplicationTask, TuiError, UIState};

use async_trait::async_trait;

pub struct WidgetBox<D: AppDatabase + Send + Sync, B: Backend> {
    pub(crate) widgets: Vec<Box<dyn Widget<D, B> + Send + Sync>>,
    pub(crate) widget_priority: VecDeque<u8>,
    pub(crate) layout: Box<dyn LayoutGenerator + Send + Sync>,
    pub(crate) bounding_boxes: Vec<Rect>,
}

impl<D: AppDatabase + Send + Sync, B: Backend> WidgetBox<D, B> {
    fn first_priority_mut(&mut self) -> Option<&mut Box<dyn Widget<D, B> + Send + Sync>> {
        let ind = *self.widget_priority.front()? as usize;
        self.widgets.get_mut(ind)
    }
}

// Needs to do the following:
// Maintain priority queue for input handling (so that we correctly switch between widgets)
// Queue should consist of WidgetInformation - widget, bounding box,
// Need some way to recompute layout - nested widgets
// Need some way to change layout
// Need some way to tab through nested widgets
#[async_trait]
impl<'b, D: AppDatabase + Send + Sync, B: Backend> Widget<D, B> for WidgetBox<D, B> {
    async fn prepare_render(&mut self, state: &mut UIState<D>, chunk: Rect) {
        self.bounding_boxes = self.layout.layout(chunk);

        for (mchunk, widget) in self.bounding_boxes.iter().zip(self.widgets.iter_mut()) {
            widget.prepare_render(state, *mchunk).await;
        }
    }

    fn render_into_frame(&self, f: &mut Frame<B>, state: &UIState<D>, chunk: Rect) {
        for (mchunk, widget) in self
            .layout
            .layout(chunk)
            .into_iter()
            .zip(self.widgets.iter())
        {
            widget.render_into_frame(f, state, mchunk);
        }
    }

    async fn handle_input(
        &mut self,
        event: Event,
        state: &mut UIState<D>,
        app: &mut AppChannel<D>,
    ) -> Result<ApplicationTask, TuiError<D::Error>> {
        match event {
            Event::Resize(_, _) => return Ok(ApplicationTask::UpdateUI),
            // find hovered widget & notify
            Event::Mouse(m) => {
                if m.kind == MouseEventKind::Down(MouseButton::Left) {
                    if let Some(i) = self
                        .bounding_boxes
                        .iter()
                        .position(|bb| bb.contains(&(m.column, m.row)))
                    {
                        let ind = self
                            .widget_priority
                            .iter()
                            .position(|x| (*x as usize) == i)
                            .unwrap();
                        let val = self.widget_priority.remove(ind).unwrap();
                        self.widget_priority.push_front(val);
                    }
                    if let Some(w) = self.first_priority_mut() {
                        return w.handle_input(event, state, app).await;
                    }
                } else if let Some(w) = self.first_priority_mut() {
                    return w.handle_input(event, state, app).await;
                }
            }
            Event::Key(event) => {
                if let Some(w) = self.first_priority_mut() {
                    // Is w capturing meta-keys?
                    // eg. tab, esc
                    if w.capturing(&Event::Key(event)) {
                        return w.handle_input(Event::Key(event), state, app).await;
                    }
                } else if !self.widgets.is_empty() {
                    // Text input
                    match event.code {
                        // if active widget isn't capturing tabs,
                        // capture tab and cycle active widgets
                        KeyCode::Tab => {
                            // switch to next in vec
                            if let Some(item) = self.widget_priority.pop_front() {
                                self.widget_priority.push_back(item);
                            }
                        }
                        KeyCode::BackTab => {
                            if let Some(item) = self.widget_priority.pop_back() {
                                self.widget_priority.push_front(item);
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        Ok(ApplicationTask::UpdateUI)
    }
}
