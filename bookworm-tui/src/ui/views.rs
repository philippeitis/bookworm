use tui::layout::{Constraint, Direction, Layout, Rect};

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

pub trait RectExt {
    fn contains(&self, point: &(u16, u16)) -> bool;
}

impl RectExt for Rect {
    fn contains(&self, point: &(u16, u16)) -> bool {
        point > &(self.x, self.y) && point <= &(self.x + self.width, self.y + self.height)
    }
}

pub trait LayoutGenerator {
    fn layout(&self, chunk: Rect) -> Vec<Rect>;
}

pub struct EditLayout {}

impl LayoutGenerator for EditLayout {
    fn layout(&self, chunk: Rect) -> Vec<Rect> {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(chunk.height.saturating_sub(1)),
                Constraint::Length(1),
            ])
            .split(chunk)
    }
}

pub struct ColumnBookLayout {}

impl LayoutGenerator for ColumnBookLayout {
    fn layout(&self, chunk: Rect) -> Vec<Rect> {
        let hchunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(75), Constraint::Percentage(25)])
            .split(chunk);

        let mut vchunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(hchunks[0].height.saturating_sub(1)),
                Constraint::Length(1),
            ])
            .split(chunk);

        vchunks.push(hchunks[1]);
        vchunks
    }
}
