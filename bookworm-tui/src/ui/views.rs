use std::collections::VecDeque;

use tui::layout::Rect;

use bookworm_database::AppDatabase;

use crate::ui::widgets::{BookWidget, ColumnWidget, CommandWidget, EditWidget, HelpWidget};

// struct SelectionState {
//     selected: Option<(usize, HashMap<usize, BookID>)>,
//     style: StyleRules,
// }
//
// impl SelectionState {
//     fn new<D: AppDatabase + Send + Sync>(state: &UIState<D>) -> Self {
//         SelectionState {
//             selected: state.selected().map(|(scol, srows)| {
//                 (
//                     scol,
//                     srows.into_iter().map(|(i, book)| (i, book.id())).collect(),
//                 )
//             }),
//             style: StyleRules {
//                 cursor: state.style.cursor_style(),
//                 selected: state.style.select_style(),
//                 default: state.style.edit_style(),
//             },
//         }
//     }
//
//     fn multiselect(&self) -> MultiSelectListState {
//         let mut selected_rows = MultiSelectListState::default();
//         if let Some((_, srows)) = &self.selected {
//             for key in srows.keys() {
//                 selected_rows.select(*key);
//             }
//         }
//         selected_rows
//     }
//
//     fn render_item<'a>(
//         &self,
//         col: usize,
//         row: usize,
//         width: usize,
//         edit: &'a Option<InputRecorder<BookID>>,
//         word: &'a std::borrow::Cow<'a, str>,
//     ) -> ListItemX<'a> {
//         if let Some((scol, srows)) = &self.selected {
//             match (col == *scol, edit, srows.get(&row)) {
//                 (true, Some(edit), Some(id)) => {
//                     // TODO: Force text around cursor to be visible.
//                     let styled =
//                         char_chunks_to_styled_text(edit.get(id).unwrap().char_chunks(), self.style);
//                     //Span::from(self.edit.value_to_string().unicode_truncate_start(width).0.to_string())
//                     return ListItemX::new(styled);
//                 }
//                 _ => {}
//             }
//         };
//
//         cut_word_to_fit(word, width)
//     }
//
//     fn selected_col(&self) -> Option<usize> {
//         self.selected.as_ref().map(|(scol, _)| *scol)
//     }
// }
struct BoundingBox {
    top_left: (u16, u16),
    bottom_right: (u16, u16),
}

impl BoundingBox {
    fn new(chunk: Rect) -> Self {
        BoundingBox {
            top_left: (chunk.x, chunk.y),
            bottom_right: (chunk.x + chunk.width, chunk.y + chunk.height),
        }
    }

    fn contains(&self, point: &(u16, u16)) -> bool {
        point > &self.top_left && point <= &self.bottom_right
    }
}

enum WidgetLayout<D: AppDatabase + Send + Sync> {
    Main(
        ColumnWidget<D>,
        Option<BookWidget<D>>,
        CommandWidget<D>,
        [BoundingBox; 3],
    ),
    Edit(EditWidget<D>, CommandWidget<D>, [BoundingBox; 2]),
    Help(HelpWidget<D>, BoundingBox),
}

struct WidgetBox<D: AppDatabase + Send + Sync> {
    widgets: WidgetLayout<D>,
    widget_priority: VecDeque<u8>,
}

// #[async_trait]
// impl<'b, D: AppDatabase + Send + Sync, B: Backend> ResizableWidget<D, B> for WidgetBox<D> {
//     // #[tracing::instrument(name = "Preparing ColumnWidgetRender", skip(self, state))]
//     async fn prepare_render(&mut self, state: &mut UIState<D>, chunk: Rect) {
//         match &mut self.widgets {
//             WidgetLayout::Main(columns, books, _, boxes) => {
//                 let chunk = if let Some(book_widget) = books {
//                     let hchunks = Layout::default()
//                         .direction(Direction::Horizontal)
//                         .constraints([Constraint::Percentage(75), Constraint::Percentage(25)])
//                         .split(chunk);
//                     ResizableWidget::<D, B>::prepare_render(book_widget, state, hchunks[1]).await;
//                     boxes[1] = BoundingBox::new(hchunks[1]);
//                     hchunks[0]
//                 } else {
//                     boxes[1] = BoundingBox { top_left: (0, 0), bottom_right: (0, 0) };
//                     chunk
//                 };
//
//                 let vchunks = Layout::default()
//                     .direction(Direction::Vertical)
//                     .constraints([
//                         Constraint::Length(chunk.height.saturating_sub(1)),
//                         Constraint::Length(1),
//                     ])
//                     .split(chunk);
//
//                 ResizableWidget::<D, B>::prepare_render(columns, state, vchunks[0]).await;
//                 boxes[0] = BoundingBox::new(vchunks[0]);
//                 boxes[2] = BoundingBox::new(vchunks[1]);
//             }
//             WidgetLayout::Edit(edits, _, boxes) => {
//                 let vchunks = Layout::default()
//                     .direction(Direction::Vertical)
//                     .constraints([
//                         Constraint::Length(chunk.height.saturating_sub(1)),
//                         Constraint::Length(1),
//                     ])
//                     .split(chunk);
//
//                 ResizableWidget::<D, B>::prepare_render(edits, state, vchunks[0]).await;
//                 boxes[0] = BoundingBox::new(vchunks[0]);
//                 boxes[1] = BoundingBox::new(vchunks[1]);
//             }
//             WidgetLayout::Help(_, b) => {
//                 *b = BoundingBox::new(chunk);
//             }
//         }
//     }
//
//     fn render_into_frame(&self, f: &mut Frame<B>, state: &UIState<D>, chunk: Rect) {
//         match &self.widgets {
//             WidgetLayout::Main(columns, books, _, boxes) => {
//                 let chunk = if let Some(book_widget) = books {
//                     let hchunks = Layout::default()
//                         .direction(Direction::Horizontal)
//                         .constraints([Constraint::Percentage(75), Constraint::Percentage(25)])
//                         .split(chunk);
//                     book_widget.render_into_frame(f, state, hchunks[1]);
//                     hchunks[0]
//                 } else {
//                     chunk
//                 };
//
//                 let vchunks = Layout::default()
//                     .direction(Direction::Vertical)
//                     .constraints([
//                         Constraint::Length(chunk.height.saturating_sub(1)),
//                         Constraint::Length(1),
//                     ])
//                     .split(chunk);
//
//                 columns.render_into_frame(f, state, vchunks[0]);
//                 CommandWidgetWrapper { database: PhantomData }.render_into_frame(f, state, vchunks[1]);
//             }
//             WidgetLayout::Edit(edits, _, boxes) => {
//                 let vchunks = Layout::default()
//                     .direction(Direction::Vertical)
//                     .constraints([
//                         Constraint::Length(chunk.height.saturating_sub(1)),
//                         Constraint::Length(1),
//                     ])
//                     .split(chunk);
//
//                 edits.render_into_frame(f, state, vchunks[0]);
//                 CommandWidgetWrapper { database: PhantomData }.render_into_frame(f, state, vchunks[1]);
//             }
//             WidgetLayout::Help(help, b) => {
//                 help.render_into_frame(f, state, chunk);
//             }
//         }
//     }
// }
//
// Needs to do the following:
// Maintain priority queue for input handling (so that we correctly switch between widgets)
// Queue should consist of WidgetInformation - widget, bounding box,
// Need some way to recompute layout - nested widgets
// Need some way to change layout
// Need some way to tab through nested widgets
// #[async_trait]
// impl<D: AppDatabase + Send + Sync> InputHandler<D> for WidgetBox<D> {
//     async fn handle_input(
//         &mut self,
//         event: Event,
//         state: &mut UIState<D>,
//         app: &mut AppChannel<D>,
//     ) -> Result<ApplicationTask, TuiError<D::Error>> {
//         match event {
//             Event::Resize(_, _) => return Ok(ApplicationTask::UpdateUI),
//             // find hovered widget & notify
//             Event::Mouse(m) => if m.kind == MouseEventKind::Down(MouseButton::Left) {
//                 if let Some(i) = self.widgets.iter().position(|(_, bb)| bb.contains(&(m.column, m.row))) {
//                     self.widget_priority.swap(0, i);
//                 }
//                 if let Some((w, _)) = self.widgets.front_mut() {
//                     return w.handle_input(event, state, app).await;
//                 }
//             } else if let Some((w, _)) = self.widgets.front_mut() {
//                 return w.handle_input(event, state, app).await;
//             }
//             Event::Key(event) => if let Some((w, _)) = self.widgets.front_mut() {
//                 // Is w capturing meta-keys?
//                 // eg. tab, esc
//                 if w.capturing(&Event::Key(event)) {
//                     return w.handle_input(Event::Key(event), state, app).await;
//                 }
//                 // Text input
//                 match event.code {
//                     // if active widget isn't capturing tabs,
//                     // capture tab and cycle active widgets
//                     KeyCode::Tab => {
//                         // switch to next in vec
//                         if let Some(item) = self.widgets.pop_front() {
//                             self.widgets.push_back(item);
//                         }
//                     }
//                     KeyCode::BackTab => {
//                         if let Some(item) = self.widgets.pop_back() {
//                             self.widgets.push_front(item);
//                         }
//                     }
//                     _ => {}
//                 }
//             }
//         }
//         Ok(ApplicationTask::UpdateUI)
//     }
// }
