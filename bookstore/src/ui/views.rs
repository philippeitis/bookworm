use std::iter::FromIterator;

use crossterm::event::{Event, KeyCode, MouseEvent};

use tui::backend::Backend;
use tui::layout::{Constraint, Direction, Layout, Rect};
use tui::text::Span;
use tui::widgets::{Block, List, ListItem, ListState};
use tui::Frame;

use crate::app::app::ColumnUpdate;
use crate::app::parse_args;
use crate::app::user_input::EditState;
use crate::app::{App, ApplicationError};
use crate::database::SelectableDatabase;
use crate::ui::terminal_ui::UIState;
use crate::ui::widgets::{BookWidget, CommandWidget, Widget};

pub(crate) enum AppView {
    ColumnView,
    EditView,
}

pub(crate) enum ApplicationTask {
    Quit,
    DoNothing,
    Update,
    SwitchView(AppView),
}

// TODO: when https://github.com/crossterm-rs/crossterm/issues/507 is resolved,
//  use code to allow a Resizable trait for EditWidget and ColumnWidget.

pub(crate) trait ResizableWidget<D, B: Backend> {
    /// Renders the widget into the frame, using the provided space.
    ///
    /// # Arguments
    ///
    /// * ` f ` - A frame to render into.
    /// * ` chunk ` - A chunk to specify the size of the widget.
    fn render_into_frame(&self, app: &mut App<D>, f: &mut Frame<B>, chunk: Rect);
}

pub(crate) trait View<D, B: Backend>: ResizableWidget<D, B> {
    /// Renders the widget into the frame, using the provided space.
    ///
    /// # Arguments
    ///
    /// * ` f ` - A frame to render into.
    /// * ` chunk ` - A chunk to specify the size of the widget.
    fn handle_input(
        &mut self,
        event: Event,
        app_state: &mut App<D>,
    ) -> Result<ApplicationTask, ApplicationError>;

    fn get_owned_state(&mut self) -> UIState;
}

fn cut_word_to_fit(word: &str, max_len: usize) -> ListItem {
    ListItem::new(Span::from(if word.len() > max_len {
        let mut base_word = word.chars().into_iter().collect::<Vec<_>>();
        base_word.truncate(max_len - 3);
        String::from_iter(base_word.iter()) + "..."
    } else {
        word.to_string()
    }))
}

fn split_chunk_into_columns(chunk: Rect, num_cols: u16) -> Vec<Rect> {
    let col_width = chunk.width / num_cols;

    let mut widths = vec![col_width; usize::from(num_cols)];
    let total_w: u16 = widths.iter().sum();
    if total_w != chunk.width {
        widths[0] += chunk.width - total_w;
    }
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints(
            widths
                .into_iter()
                .map(Constraint::Length)
                .collect::<Vec<_>>(),
        )
        .split(chunk)
}

pub(crate) struct ColumnWidget {
    pub(crate) state: UIState,
}

impl<D: SelectableDatabase, B: Backend> ResizableWidget<D, B> for ColumnWidget {
    fn render_into_frame(&self, app: &mut App<D>, f: &mut Frame<B>, chunk: Rect) {
        let chunk = if let Ok(b) = app.selected_item() {
            let hchunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(75), Constraint::Percentage(25)])
                .split(chunk);
            BookWidget::new(&b).render_into_frame(f, hchunks[1]);
            hchunks[0]
        } else {
            chunk
        };

        let vchunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(chunk.height - 1), Constraint::Length(1)])
            .split(chunk);

        let curr_height = usize::from(vchunks[0].height);

        if curr_height != 0 && app.refresh_window_size(curr_height - 1) {
            app.set_column_update(ColumnUpdate::Regenerate);
            app.update_column_data();
        }

        let chunk = vchunks[0];
        let hchunks = split_chunk_into_columns(chunk, app.num_cols() as u16);
        let select_style = self.state.style.select_style();

        for ((title, data), &chunk) in app.header_col_iter().zip(hchunks.iter()) {
            let list = List::new(
                data.iter()
                    .map(|word| cut_word_to_fit(word, usize::from(chunk.width).saturating_sub(3)))
                    .collect::<Vec<_>>(),
            )
            .block(Block::default().title(Span::from(title.to_string())))
            .highlight_style(select_style);

            let mut selected_row = ListState::default();
            selected_row.select(app.selected());
            f.render_stateful_widget(list, chunk, &mut selected_row);
        }

        CommandWidget::new(&self.state.curr_command).render_into_frame(f, vchunks[1]);
    }
}

impl<'a, D: SelectableDatabase, B: Backend> View<D, B> for ColumnWidget {
    fn handle_input(
        &mut self,
        event: Event,
        app: &mut App<D>,
    ) -> Result<ApplicationTask, ApplicationError> {
        match event {
            Event::Resize(_, _) => return Ok(ApplicationTask::Update),
            Event::Mouse(m) => match m {
                MouseEvent::ScrollDown(_, _, _) => {
                    if self.state.nav_settings.inverted {
                        app.scroll_up(self.state.nav_settings.scroll);
                    } else {
                        app.scroll_down(self.state.nav_settings.scroll);
                    }
                }
                MouseEvent::ScrollUp(_, _, _) => {
                    if self.state.nav_settings.inverted {
                        app.scroll_down(self.state.nav_settings.scroll);
                    } else {
                        app.scroll_up(self.state.nav_settings.scroll);
                    }
                }
                _ => {
                    return Ok(ApplicationTask::DoNothing);
                }
            },
            Event::Key(event) => {
                // Text input
                match event.code {
                    KeyCode::F(2) => {
                        if app.selected().is_some() {
                            return Ok(ApplicationTask::SwitchView(AppView::EditView));
                        }
                    }
                    KeyCode::Backspace => {
                        self.state.curr_command.pop();
                    }
                    KeyCode::Char(x) => {
                        self.state.curr_command.push(x);
                    }
                    KeyCode::Enter => {
                        let args: Vec<_> = self
                            .state
                            .curr_command
                            .get_values_autofilled()
                            .into_iter()
                            .map(|(_, a)| a)
                            .collect();
                        self.state.curr_command.clear();
                        if !app.run_command(parse_args(&args))? {
                            return Ok(ApplicationTask::Quit);
                        }

                        return Ok(ApplicationTask::DoNothing);
                    }
                    KeyCode::Tab | KeyCode::BackTab => {
                        self.state.curr_command.refresh_autofill()?;
                        let vals = self.state.curr_command.get_values();
                        if let Some(val) = vals.get(0) {
                            if val.1 == "!a" {
                                let dir = if let Some(val) = vals.get(1) {
                                    val.1 == "-d"
                                } else {
                                    false
                                };
                                self.state.curr_command.auto_fill(dir);
                            }
                        };
                    }
                    KeyCode::Esc => {
                        self.state.curr_command.clear();
                        app.deselect();
                    }
                    KeyCode::Delete => {
                        if self.state.curr_command.is_empty() {
                            app.remove_selected_book()?;
                        } else {
                            // TODO: Add code to delete forwards
                            //  (requires implementing cursor logic)
                        }
                    }
                    // Scrolling
                    KeyCode::Up => {
                        app.select_up();
                    }
                    KeyCode::Down => {
                        app.select_down();
                    }
                    KeyCode::PageDown => {
                        app.page_down();
                    }
                    KeyCode::PageUp => {
                        app.page_up();
                    }
                    KeyCode::Home => {
                        app.home();
                    }
                    KeyCode::End => {
                        app.end();
                    }
                    _ => return Ok(ApplicationTask::DoNothing),
                }
            }
        }
        Ok(ApplicationTask::Update)
    }

    fn get_owned_state(&mut self) -> UIState {
        std::mem::take(&mut self.state)
    }
}

pub(crate) struct EditWidget {
    pub(crate) edit: EditState,
    pub(crate) state: UIState,
}

impl<D: SelectableDatabase, B: Backend> ResizableWidget<D, B> for EditWidget {
    fn render_into_frame(&self, app: &mut App<D>, f: &mut Frame<B>, chunk: Rect) {
        let vchunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(chunk.height - 1), Constraint::Length(1)])
            .split(chunk);

        let curr_height = usize::from(vchunks[0].height);
        if curr_height != 0 && app.refresh_window_size(curr_height - 1) {
            app.set_column_update(ColumnUpdate::Regenerate);

            app.update_column_data();
            app.update_value(
                self.state.selected_column,
                self.edit.selected,
                &self.edit.visible(),
            );
        }

        let hchunks = split_chunk_into_columns(chunk, app.num_cols() as u16);

        let edit_style = self.state.style.edit_style();
        let select_style = self.state.style.select_style();

        for (i, ((title, data), &chunk)) in app.header_col_iter().zip(hchunks.iter()).enumerate() {
            let list = List::new(
                data.iter()
                    .map(|word| cut_word_to_fit(word, usize::from(chunk.width).saturating_sub(3)))
                    .collect::<Vec<_>>(),
            )
            .block(Block::default().title(Span::from(title.to_string())))
            .highlight_style(if i == self.state.selected_column {
                edit_style
            } else {
                select_style
            });

            let mut selected_row = ListState::default();
            selected_row.select(app.selected());
            f.render_stateful_widget(list, chunk, &mut selected_row);
        }
        CommandWidget::new(&self.state.curr_command).render_into_frame(f, vchunks[1]);
    }
}

impl<D: SelectableDatabase, B: Backend> View<D, B> for EditWidget {
    fn handle_input(
        &mut self,
        event: Event,
        app: &mut App<D>,
    ) -> Result<ApplicationTask, ApplicationError> {
        match event {
            Event::Resize(_, _) => return Ok(ApplicationTask::Update),
            // TODO: Should this behave more like Excel / Google Sheets:
            // Up / down write and go up and down
            // Enter writes and goes down
            // Left Right write and go left and right
            // F2 makes arrow keys stick in box
            // tab writes and goes to next box.
            Event::Key(event) => {
                match event.code {
                    KeyCode::Backspace => {
                        self.edit.del();
                    }
                    KeyCode::Char(c) => {
                        self.edit.push(c);
                    }
                    KeyCode::Enter => {
                        if self.edit.started_edit {
                            app.edit_selected_book_with_column(
                                self.state.selected_column,
                                &self.edit.new_value,
                            )?;
                        } else {
                            app.update_value(
                                self.state.selected_column,
                                self.edit.selected,
                                &self.edit.orig_value,
                            );
                        }
                        return Ok(ApplicationTask::SwitchView(AppView::ColumnView));
                    }
                    KeyCode::Esc => {
                        app.update_value(
                            self.state.selected_column,
                            self.edit.selected,
                            &self.edit.orig_value,
                        );
                        return Ok(ApplicationTask::SwitchView(AppView::ColumnView));
                    }
                    KeyCode::Delete => {
                        // TODO: Add code to delete forwards
                        //  (requires implementing cursor logic)
                    }
                    KeyCode::Right => {
                        self.edit.edit_orig();
                    }
                    KeyCode::Down => {
                        if self.state.selected_column + 1 < app.num_cols() {
                            if self.edit.started_edit {
                                app.edit_selected_book_with_column(
                                    self.state.selected_column,
                                    &self.edit.new_value,
                                )?;
                            }
                            self.state.selected_column += 1;
                        }
                        self.edit.reset_orig(
                            app.get_value(self.state.selected_column, self.edit.selected)
                                .to_string(),
                        );
                    }
                    KeyCode::Up => {
                        if self.state.selected_column > 0 {
                            if self.edit.started_edit {
                                app.edit_selected_book_with_column(
                                    self.state.selected_column,
                                    &self.edit.new_value,
                                )?;
                            }
                            self.state.selected_column -= 1;
                        }
                        self.edit.reset_orig(
                            app.get_value(self.state.selected_column, self.edit.selected)
                                .to_string(),
                        );
                    }
                    _ => return Ok(ApplicationTask::DoNothing),
                }
            }
            _ => return Ok(ApplicationTask::DoNothing),
        }
        app.update_value(
            self.state.selected_column,
            self.edit.selected,
            &self.edit.visible(),
        );
        Ok(ApplicationTask::Update)
    }

    fn get_owned_state(&mut self) -> UIState {
        std::mem::take(&mut self.state)
    }
}
