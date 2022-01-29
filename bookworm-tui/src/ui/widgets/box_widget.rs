use crossterm::event::{Event, KeyCode, MouseEventKind};
use tui::backend::Backend;
use tui::layout::Rect;
use tui::Frame;

use bookworm_app::app::AppChannel;
use bookworm_database::AppDatabase;

use crate::ui::layouts::{LayoutGenerator, RectExt};
use crate::ui::widgets::Widget;
use crate::{ApplicationTask, TuiError, UIState};

use async_trait::async_trait;

#[derive(Eq, PartialEq, Copy, Clone)]
enum TabStatus {
    None,
    PastStart,
    PastEnd,
}

pub struct PriorityIndex {
    tab_status: TabStatus,
    index: usize,
    length: usize,
}

impl PriorityIndex {
    pub(crate) fn new(length: usize) -> Self {
        Self {
            tab_status: TabStatus::None,
            index: 0,
            length,
        }
    }

    fn increment(&mut self) -> bool {
        match self.tab_status {
            TabStatus::None => {
                if self.index + 1 == self.length {
                    self.tab_status = TabStatus::PastEnd;
                    true
                } else {
                    self.index += 1;
                    false
                }
            }
            TabStatus::PastEnd | TabStatus::PastStart => {
                self.tab_status = TabStatus::None;
                self.index = 0;
                false
            }
        }
    }

    fn decrement(&mut self) -> bool {
        match self.tab_status {
            TabStatus::None => {
                if self.index == 0 {
                    self.tab_status = TabStatus::PastStart;
                    true
                } else {
                    self.index -= 1;
                    false
                }
            }
            TabStatus::PastStart | TabStatus::PastEnd => {
                self.tab_status = TabStatus::None;
                self.index = self.length.saturating_sub(1);
                false
            }
        }
    }
}

pub struct WidgetBox<D: AppDatabase + Send + Sync, B: Backend> {
    pub(crate) widgets: Vec<Box<dyn Widget<D, B> + Send + Sync>>,
    pub(crate) priority_index: PriorityIndex,
    pub(crate) layout: Box<dyn LayoutGenerator + Send + Sync>,
    pub(crate) bounding_boxes: Vec<Rect>,
}

impl<D: AppDatabase + Send + Sync, B: Backend> WidgetBox<D, B> {
    pub(crate) fn new(
        widgets: Vec<Box<dyn Widget<D, B> + Send + Sync>>,
        layout: Box<dyn LayoutGenerator + Send + Sync>,
    ) -> Self {
        Self {
            priority_index: PriorityIndex::new(widgets.len()),
            widgets,
            layout,
            bounding_boxes: vec![],
        }
    }

    async fn handle_with_prioritized_widget(
        &mut self,
        event: Event,
        state: &mut UIState<D>,
        app: &mut AppChannel<D>,
    ) -> Result<ApplicationTask, TuiError<D::Error>> {
        self.widgets
            .get_mut(self.priority_index.index)
            .expect("priority_index possesses out of bounds indices")
            .handle_input(event, state, app)
            .await
    }
}

// Needs to do the following:
// Need some way to recompute layout - nested widgets
// Need some way to change layout
// Need some way to tab through nested widgets - almost, with nested TAB events, but need to trigger
// tabbing behaviour in parent
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
                if let Some(i) = self
                    .bounding_boxes
                    .iter()
                    .position(|bb| bb.contains(&(m.column, m.row)))
                {
                    // Don't remove focus on scroll event.
                    // TODO: Should be even more specific, with something like
                    //  ApplicationTask::StealKeyboardFocus
                    if ![
                        MouseEventKind::ScrollUp,
                        MouseEventKind::ScrollDown,
                        MouseEventKind::Moved,
                    ]
                    .contains(&m.kind)
                    {
                        self.priority_index.index = i;
                    }

                    return self
                        .widgets
                        .get_mut(i)
                        .expect("Bounding box does not correspond to existing widget")
                        .handle_input(event, state, app)
                        .await;
                }
                return self.handle_with_prioritized_widget(event, state, app).await;
            }
            Event::Key(event) => {
                // Figure out how to handle esc for de-prioritize
                // Figure out default when nothing is capturing
                match self
                    .handle_with_prioritized_widget(Event::Key(event), state, app)
                    .await?
                {
                    ApplicationTask::DoNothing => match event.code {
                        // If active widget isn't capturing tabs,
                        // capture tab and cycle active widgets
                        // TODO: Allow tabbing between nested WidgetBoxes
                        //  seamlessly
                        KeyCode::Tab => {
                            self.priority_index.increment();
                        }
                        KeyCode::BackTab => {
                            self.priority_index.decrement();
                        }
                        _ => {}
                    },
                    v => return Ok(v),
                }
            }
        }
        Ok(ApplicationTask::UpdateUI)
    }
}
