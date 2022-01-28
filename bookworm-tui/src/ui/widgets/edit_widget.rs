use std::collections::HashMap;
use std::marker::PhantomData;

use crossterm::event::{Event, KeyCode, KeyModifiers};
use tui::backend::Backend;
use tui::layout::Rect;
use tui::text::Span;
use tui::widgets::Block;
use tui::Frame;
use unicode_truncate::UnicodeTruncateStr;

use bookworm_app::app::AppChannel;
use bookworm_app::{ApplicationError, BookIndex, Command};
use bookworm_database::AppDatabase;
use bookworm_input::user_input::InputRecorder;
use bookworm_input::Edit;
use bookworm_records::book::{BookID, ColumnIdentifier, RecordError};

use crate::ui::tui_widgets::{ListItemX, MultiSelectList, MultiSelectListState};
use crate::ui::utils::{
    char_chunks_to_styled_text, copy_from_clipboard, split_chunk_into_columns, StyleRules, TuiStyle,
};
use crate::ui::widgets::Widget;
use crate::{run_command, AppView, ApplicationTask, TuiError, UIState};

use async_trait::async_trait;

pub struct EditWidget<D> {
    pub(crate) edit: InputRecorder<BookID>,
    pub(crate) focused: bool,
    pub(crate) database: PhantomData<fn(D)>,
}

impl<D: AppDatabase + Send + Sync> EditWidget<D> {
    /// Used to save the edit to the book being modified.
    async fn dump_edit(
        &mut self,
        app: &mut AppChannel<D>,
        state: &mut UIState<D>,
    ) -> Result<(), TuiError<D::Error>> {
        if self.edit.started_edit {
            self.focused = false;
            let column = { state.table_view.selected_cols()[state.selected_column].to_owned() };
            let edits = vec![(
                ColumnIdentifier::from(column),
                Edit::Sequence(self.edit.get_base()),
            )]
            .into_boxed_slice();
            match run_command(app, Command::EditBook(BookIndex::Selected, edits), state).await {
                Ok(_) => {}
                // Catch immutable column error and discard changes.
                Err(TuiError::Application(ApplicationError::Record(
                    RecordError::ImmutableColumn,
                ))) => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    /// Used when column has been changed and edit should reflect new column's value.
    fn reset_edit(&mut self, state: &UIState<D>) {
        self.edit = InputRecorder::default();
        let selected_books = state.book_view.relative_selections();
        let column = state
            .selected_column_values()
            .expect("Selected value should exist when in edit mode.");

        for ((_, book), col) in selected_books.into_iter().zip(column.into_iter()) {
            self.edit.add_cursor(book.id(), &col);
        }
    }
}

#[async_trait]
impl<'b, D: AppDatabase + Send + Sync, B: Backend> Widget<D, B> for EditWidget<D> {
    async fn prepare_render(&mut self, state: &mut UIState<D>, chunk: Rect) {
        // Account for top table row.
        let _ = state
            .book_view
            .refresh_window_size(usize::from(chunk.height).saturating_sub(1))
            .await;
    }

    fn render_into_frame(&self, f: &mut Frame<B>, state: &UIState<D>, chunk: Rect) {
        let hchunks = split_chunk_into_columns(chunk, state.num_cols() as u16);

        let edit_style = state.style.edit_style();
        let select_style = state.style.select_style();
        let style_rules = StyleRules {
            cursor: state.style.cursor_style(),
            selected: select_style,
            default: edit_style,
        };
        let (scol, srows) = state
            .selected()
            .expect("EditWidget should only exist when items are selected");

        let srows: HashMap<_, _> = srows.into_iter().map(|(i, book)| (i, book.id())).collect();

        let books = state.book_view.window();
        for (col, ((title, data), &chunk)) in state
            .table_view
            .read_columns(&books)
            .zip(hchunks.iter())
            .enumerate()
        {
            let width = usize::from(chunk.width).saturating_sub(1);
            let column: Vec<_> = data.collect();
            let items = column
                .iter()
                .enumerate()
                .map(|(row, word)| {
                    match (col == scol, srows.get(&row)) {
                        (true, Some(id)) => {
                            // TODO: Force text around cursor to be visible.
                            let styled = char_chunks_to_styled_text(
                                self.edit.get(id).unwrap().char_chunks(),
                                style_rules,
                            );
                            //Span::from(self.edit.value_to_string().unicode_truncate_start(width).0.to_string())
                            ListItemX::new(styled)
                        }
                        _ => ListItemX::new(Span::from(word.unicode_truncate(width).0)),
                    }
                })
                .collect::<Vec<_>>();

            let mut list = MultiSelectList::new(items)
                .block(Block::default().title(Span::from(title.to_string())));
            if !self.focused {
                list = list.highlight_style(if col == scol {
                    edit_style
                } else {
                    select_style
                });
            }

            let mut selected_row = MultiSelectListState::default();
            for key in srows.keys() {
                selected_row.select(*key);
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
            Event::Key(event) => {
                match event.code {
                    KeyCode::F(2) => {
                        self.focused = true;
                    }
                    KeyCode::Char(c) => {
                        if event.modifiers == KeyModifiers::CONTROL {
                            match (c, cfg!(feature = "copypaste")) {
                                ('v', true) => {
                                    if let Some(s) = copy_from_clipboard() {
                                        self.edit.extend(&s);
                                    }
                                }
                                ('c', true) => {
                                    unimplemented!()
                                    // if let Some(text) = self.edit.selected() {
                                    //     paste_into_clipboard(&text.iter().collect::<String>());
                                    // } else {
                                    //     let string = self.edit.value_to_string();
                                    //     if !string.is_empty() {
                                    //         paste_into_clipboard(&string);
                                    //     }
                                    // }
                                }
                                ('x', true) => {
                                    unimplemented!()
                                    // if let Some(text) = self.edit.selected() {
                                    //     paste_into_clipboard(&text.iter().collect::<String>());
                                    //     self.edit.del();
                                    // } else {
                                    //     let string = self.edit.value_to_string();
                                    //     if !string.is_empty() {
                                    //         paste_into_clipboard(&string);
                                    //     }
                                    //     self.edit.clear();
                                    // }
                                }
                                ('d', _) => {
                                    if self.focused {
                                        self.edit.deselect();
                                        self.focused = false;
                                    } else {
                                        return Ok(ApplicationTask::SwitchView(AppView::Columns));
                                    }
                                }
                                ('a', _) => {
                                    self.edit.select_all();
                                }
                                _ => {
                                    self.edit.push(c);
                                }
                            }
                        } else {
                            self.edit.push(c);
                        }
                    }
                    KeyCode::Tab => {
                        self.dump_edit(app, state).await?;
                        if state.selected_column + 1 < state.num_cols() {
                            state.selected_column += 1;
                            // Only reset edit if changing columns
                            self.reset_edit(state);
                        }
                    }
                    KeyCode::BackTab => {
                        self.dump_edit(app, state).await?;
                        if state.selected_column > 0 {
                            state.selected_column -= 1;
                            // Only reset edit if changing columns
                            self.reset_edit(state);
                        }
                    }

                    KeyCode::Enter => {
                        if !self.focused {
                            self.focused = true;
                        } else {
                            self.dump_edit(app, state).await?;
                            return Ok(ApplicationTask::SwitchView(AppView::Columns));
                        }
                    }
                    KeyCode::Esc => {
                        return Ok(ApplicationTask::SwitchView(AppView::Columns));
                    }
                    KeyCode::Backspace => {
                        self.edit.backspace();
                    }
                    KeyCode::Delete => {
                        self.edit.del();
                    }
                    KeyCode::Down => {
                        if self.focused {
                            if event.modifiers.intersects(KeyModifiers::SHIFT) {
                                self.edit.key_shift_down();
                            } else {
                                self.edit.key_down();
                            }
                        } else {
                            self.dump_edit(app, state).await?;
                            state.book_view.down().await?;
                            self.reset_edit(state);
                        }
                    }
                    KeyCode::Up => {
                        if self.focused {
                            if event.modifiers.intersects(KeyModifiers::SHIFT) {
                                self.edit.key_shift_up();
                            } else {
                                self.edit.key_up();
                            }
                        } else {
                            self.dump_edit(app, state).await?;
                            state.book_view.up().await?;
                            self.reset_edit(state);
                        }
                    }
                    KeyCode::Left => {
                        if self.focused {
                            if event.modifiers.intersects(KeyModifiers::SHIFT) {
                                self.edit.key_shift_left();
                            } else {
                                self.edit.key_left();
                            }
                        } else {
                            self.dump_edit(app, state).await?;
                            if state.selected_column > 0 {
                                state.selected_column -= 1;
                                self.reset_edit(state);
                            }
                        }
                    }
                    KeyCode::Right => {
                        if self.focused {
                            if event.modifiers.intersects(KeyModifiers::SHIFT) {
                                self.edit.key_shift_right();
                            } else {
                                self.edit.key_right();
                            }
                        } else {
                            self.dump_edit(app, state).await?;
                            if state.selected_column + 1 < state.num_cols() {
                                state.selected_column += 1;
                                // Only reset edit if changing columns
                                self.reset_edit(state);
                            }
                        }
                    }

                    _ => return Ok(ApplicationTask::DoNothing),
                }
            }
            _ => return Ok(ApplicationTask::DoNothing),
        }
        Ok(ApplicationTask::UpdateUI)
    }
}
