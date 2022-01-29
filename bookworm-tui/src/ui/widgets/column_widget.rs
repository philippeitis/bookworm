use std::marker::PhantomData;

use crossterm::event::{Event, KeyCode, KeyModifiers, MouseEventKind};
use tui::backend::Backend;
use tui::layout::Rect;
use tui::text::Span;
use tui::widgets::Block;
use tui::Frame;

use bookworm_app::app::AppChannel;
use bookworm_app::Command;
use bookworm_database::{AppDatabase, DatabaseError};

use crate::ui::tui_widgets::{MultiSelectList, MultiSelectListState};
use crate::ui::utils::{cut_word_to_fit, split_chunk_into_columns, TuiStyle};
use crate::ui::widgets::Widget;
use crate::{run_command, AppView, ApplicationTask, TuiError, UIState};

use async_trait::async_trait;


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

pub struct ColumnWidget<D> {
    pub(crate) database: PhantomData<fn(D)>,
}

impl<D: AppDatabase + Send + Sync> ColumnWidget<D> {
    async fn scroll_up(&mut self, state: &mut UIState<D>) -> Result<(), DatabaseError<D::Error>> {
        let scroll = state.nav_settings.scroll;
        if state.nav_settings.inverted {
            state.book_view.scroll_down(scroll).await
        } else {
            state.book_view.scroll_up(scroll).await
        }
    }

    async fn scroll_down(&mut self, state: &mut UIState<D>) -> Result<(), DatabaseError<D::Error>> {
        let scroll = state.nav_settings.scroll;
        if state.nav_settings.inverted {
            state.book_view.scroll_up(scroll).await
        } else {
            state.book_view.scroll_down(scroll).await
        }
    }

    async fn page_down(
        &mut self,
        state: &mut UIState<D>,
        modifiers: KeyModifiers,
    ) -> Result<(), DatabaseError<D::Error>> {
        if modifiers.intersects(KeyModifiers::SHIFT) {
            state.book_view.select_page_down().await
        } else {
            state.book_view.page_down().await
        }
    }

    async fn page_up(
        &mut self,
        state: &mut UIState<D>,
        modifiers: KeyModifiers,
    ) -> Result<(), DatabaseError<D::Error>> {
        if modifiers.intersects(KeyModifiers::SHIFT) {
            state.book_view.select_page_up().await
        } else {
            state.book_view.page_up().await
        }
    }

    async fn home(
        &mut self,
        state: &mut UIState<D>,
        modifiers: KeyModifiers,
    ) -> Result<(), DatabaseError<D::Error>> {
        if modifiers.intersects(KeyModifiers::SHIFT) {
            state.book_view.select_to_start().await?;
        } else {
            state.book_view.home().await?;
        }

        Ok(())
    }

    async fn end(
        &mut self,
        state: &mut UIState<D>,
        modifiers: KeyModifiers,
    ) -> Result<(), DatabaseError<D::Error>> {
        if modifiers.intersects(KeyModifiers::SHIFT) {
            state.book_view.select_to_end().await?;
        } else {
            state.book_view.end().await?;
        }

        Ok(())
    }

    async fn select_up(
        &mut self,
        state: &mut UIState<D>,
        modifiers: KeyModifiers,
    ) -> Result<(), DatabaseError<D::Error>> {
        if modifiers.intersects(KeyModifiers::SHIFT) {
            state.book_view.select_up().await?;
        } else {
            state.book_view.up().await?;
        }

        Ok(())
    }

    async fn select_down(
        &mut self,
        state: &mut UIState<D>,
        modifiers: KeyModifiers,
    ) -> Result<(), DatabaseError<D::Error>> {
        if modifiers.intersects(KeyModifiers::SHIFT) {
            state.book_view.select_down().await?;
        } else {
            state.book_view.down().await?;
        }

        Ok(())
    }
}

#[async_trait]
impl<'b, D: AppDatabase + Send + Sync, B: Backend> Widget<D, B> for ColumnWidget<D> {
    async fn prepare_render(&mut self, state: &mut UIState<D>, chunk: Rect) {
        // Account for column titles
        let _ = state
            .book_view
            .refresh_window_size(usize::from(chunk.height).saturating_sub(1))
            .await;
    }

    fn render_into_frame(&self, f: &mut Frame<B>, state: &UIState<D>, chunk: Rect) {
        let hchunks = split_chunk_into_columns(chunk, state.num_cols() as u16);

        // let edit_style = state.style.edit_style();
        let select_style = state.style.select_style();

        // let highlighter = SelectionState::new(state);
        // let books = state.book_view.window();
        // for (col, ((title, data), &chunk)) in state
        //     .table_view
        //     .read_columns(&books)
        //     .zip(hchunks.iter())
        //     .enumerate()
        // {
        // for ((title, data), &chunk) in state.table_view.read_columns(&books).zip(hchunks.iter()) {
        //     let width = usize::from(chunk.width).saturating_sub(1);
        //     let column: Vec<_> = data.collect();
        //     let items = column
        //         .iter()
        //         .enumerate()
        //         .map(|(row, word)| highlighter.render_item(col, row, width, &self.edit, word))
        //         .collect::<Vec<_>>();
        //
        //     let mut list = MultiSelectList::new(items)
        //         .block(Block::default().title(Span::from(title.to_string())));
        //
        //     if !self.focused {
        //         list = list.highlight_style(if Some(col) == highlighter.selected_col() {
        //             edit_style
        //         } else {
        //             select_style
        //         });
        //     }
        //
        //     f.render_stateful_widget(list, chunk, &mut highlighter.multiselect());
        // }

        let books = state.book_view.window();
        for ((title, data), &chunk) in state.table_view.read_columns(&books).zip(hchunks.iter()) {
            let width = usize::from(chunk.width).saturating_sub(1);
            let column: Vec<_> = data.collect();
            let list = MultiSelectList::new(
                column
                    .iter()
                    .map(|word| cut_word_to_fit(&word, width))
                    .collect::<Vec<_>>(),
            )
            .block(Block::default().title(Span::from(title.to_string())))
            .highlight_style(select_style);
            let mut selected_row = MultiSelectListState::default();

            if let Some((_, srows)) = state.selected() {
                for (i, _) in srows {
                    selected_row.select(i);
                }
            }

            f.render_stateful_widget(list, chunk, &mut selected_row);
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
            Event::Mouse(m) => match m.kind {
                MouseEventKind::ScrollDown => self.scroll_down(state).await?,
                MouseEventKind::ScrollUp => self.scroll_up(state).await?,
                _ => return Ok(ApplicationTask::DoNothing),
            },
            Event::Key(event) => {
                // Text input
                match event.code {
                    KeyCode::F(2) => {
                        return if !state.book_view.selected_books().is_empty() {
                            // Parent needs to switch this with EditWidget and remove bookwidget
                            Ok(ApplicationTask::SwitchView(AppView::Edit))
                        } else {
                            Ok(ApplicationTask::DoNothing)
                        };
                    }
                    KeyCode::Enter => {
                        return if !state.book_view.selected_books().is_empty() {
                            Ok(ApplicationTask::SwitchView(AppView::Edit))
                        } else {
                            Ok(ApplicationTask::DoNothing)
                        }
                    }
                    // if active widget, deactivates
                    KeyCode::Esc => {
                        state.book_view.deselect_all();
                        state.book_view.pop_scope();
                    }
                    KeyCode::Delete => {
                        return if !state.book_view.selected_books().is_empty() {
                            run_command(app, Command::DeleteSelected, state).await
                        } else {
                            Ok(ApplicationTask::DoNothing)
                        }
                    } // Scrolling
                    KeyCode::Up => self.select_up(state, event.modifiers).await?,
                    KeyCode::Down => self.select_down(state, event.modifiers).await?,
                    KeyCode::PageDown => self.page_down(state, event.modifiers).await?,
                    KeyCode::PageUp => self.page_up(state, event.modifiers).await?,
                    KeyCode::Home => self.home(state, event.modifiers).await?,
                    KeyCode::End => self.end(state, event.modifiers).await?,
                    _ => return Ok(ApplicationTask::DoNothing),
                }
            }
        }
        Ok(ApplicationTask::UpdateUI)
    }
}
