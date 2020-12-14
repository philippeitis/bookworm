use crossterm::event::{Event, KeyCode, MouseEvent};

use tui::backend::Backend;
use tui::layout::{Constraint, Direction, Layout, Rect};
use tui::style::{Modifier, Style};
use tui::text::{Span, Text};
use tui::widgets::{Block, List, ListItem, ListState, Paragraph};
use tui::Frame;

use unicode_truncate::UnicodeTruncateStr;

use crate::app::parse_args;
use crate::app::user_input::EditState;
use crate::app::{App, ApplicationError};
use crate::database::IndexableDatabase;
use crate::ui::scrollable_text::{BlindOffset, ScrollableText};
use crate::ui::terminal_ui::UIState;
use crate::ui::widgets::{book_to_widget_text, CommandWidget, Widget};

#[derive(Copy, Clone)]
pub(crate) enum AppView {
    Columns,
    Edit,
    Help,
}

pub(crate) enum ApplicationTask {
    Quit,
    DoNothing,
    Update,
    SwitchView(AppView),
}

// TODO: when https://github.com/crossterm-rs/crossterm/issues/507 is resolved,
//  use code to allow a Resizable trait for EditWidget and ColumnWidget.
pub(crate) trait ResizableWidget<D: IndexableDatabase, B: Backend> {
    // TODO: Consider AppCommand enum? Could be used to do other things as well?
    fn allocate_chunk(&self, chunk: Rect) -> Option<usize>;

    /// Renders the widget into the frame, using the provided space.
    ///
    /// # Arguments
    ///
    /// * ` f ` - A frame to render into.
    /// * ` chunk ` - A chunk to specify the size of the widget.
    fn render_into_frame(&mut self, app: &App<D>, f: &mut Frame<B>, chunk: Rect);
}

pub(crate) trait InputHandler<D: IndexableDatabase> {
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

    /// Takes the object's UIState to allow use in another
    /// View.
    fn take_state(&mut self) -> UIState;
}

/// Takes `word`, and cuts excess letters to ensure that it fits within
/// `max_width` visible characters. If `word` is too long, it will be truncated
/// and have '...' appended to indicate that it has been truncated (if max_width
/// is at least 3, otherwise, letters will simply be cut). It will then be returned as a ListItem.
///
/// # Arguments
/// * ` word ` - A string reference.
/// * ` max_width ` - The maximum width of word in visible characters.
fn cut_word_to_fit(word: &str, max_width: usize) -> ListItem {
    // TODO: What should be done if max_width is too small?
    ListItem::new(Span::from(if word.len() > max_width {
        if max_width >= 3 {
            let possible_word = word.unicode_truncate(max_width - 3);
            possible_word.0.to_owned() + "..."
        } else {
            word.unicode_truncate(max_width).0.to_owned()
        }
    } else {
        word.to_owned()
    }))
}

/// Splits `chunk` into `num_cols` columns with widths differing by no more than
/// one, and adding up to the width of `chunk`, except when `num_cols` is 0.
/// If called with sequentially increasing or decreasing values, chunk sizes
/// will never decrease or increase, respectively.
///
/// # Arguments
/// * ` chunk ` - A chunk which the columns will be placed into.
/// * ` num_cols ` - The number of columns to fit.
fn split_chunk_into_columns(chunk: Rect, num_cols: u16) -> Vec<Rect> {
    if num_cols == 0 {
        return vec![];
    }

    let col_width = chunk.width / num_cols;

    let mut widths = vec![col_width; usize::from(num_cols)];
    let total_w: u16 = widths.iter().sum();
    if total_w != chunk.width {
        widths[..usize::from(chunk.width - total_w)]
            .iter_mut()
            .for_each(|w| *w += 1);
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
    pub(crate) had_selected: bool,
    pub(crate) offset: BlindOffset,
    pub(crate) book_area: Rect,
}

impl<'b, D: IndexableDatabase, B: Backend> ResizableWidget<D, B> for ColumnWidget {
    fn allocate_chunk(&self, chunk: Rect) -> Option<usize> {
        let vchunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(chunk.height - 1), Constraint::Length(1)])
            .split(chunk);
        let curr_height = usize::from(vchunks[0].height);
        Some(curr_height.saturating_sub(1))
    }

    fn render_into_frame(&mut self, app: &App<D>, f: &mut Frame<B>, chunk: Rect) {
        let chunk = if let Ok(b) = app.selected_item() {
            self.had_selected = true;
            let hchunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(75), Constraint::Percentage(25)])
                .split(chunk);

            self.book_area = hchunks[1];
            let book_text =
                book_to_widget_text(&b, self.book_area.width.saturating_sub(1) as usize);
            self.offset
                .refresh_window_height(self.book_area.height as usize);
            let offset = self.offset.offset_with_height(book_text.lines.len());
            let p = Paragraph::new(book_text).scroll((offset as u16, 0));
            f.render_widget(p, self.book_area);

            hchunks[0]
        } else {
            self.had_selected = false;
            chunk
        };

        let vchunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(chunk.height - 1), Constraint::Length(1)])
            .split(chunk);

        let chunk = vchunks[0];
        let hchunks = split_chunk_into_columns(chunk, app.num_cols() as u16);
        let select_style = self.state.style.select_style();

        for ((title, data), &chunk) in app.header_col_iter().zip(hchunks.iter()) {
            let width = usize::from(chunk.width).saturating_sub(1);
            let list = List::new(
                data.iter()
                    .map(|word| cut_word_to_fit(word, width))
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

fn inside_rect(rect: Rect, col: u16, row: u16) -> bool {
    col >= rect.x && col < rect.x + rect.width && row >= rect.y && row < rect.y + rect.height
}

impl<D: IndexableDatabase> InputHandler<D> for ColumnWidget {
    fn handle_input(
        &mut self,
        event: Event,
        app: &mut App<D>,
    ) -> Result<ApplicationTask, ApplicationError> {
        match event {
            Event::Resize(_, _) => return Ok(ApplicationTask::Update),
            Event::Mouse(m) => match m {
                MouseEvent::ScrollDown(c, r, _) => {
                    let inverted = self.state.nav_settings.inverted;
                    let scroll = self.state.nav_settings.scroll;
                    if inside_rect(self.book_area, c, r) {
                        if inverted {
                            self.offset.scroll_up(scroll);
                        } else {
                            self.offset.scroll_down(scroll);
                        }
                    } else {
                        if inverted {
                            app.scroll_up(scroll);
                        } else {
                            app.scroll_down(scroll);
                        }
                    }
                }
                MouseEvent::ScrollUp(c, r, _) => {
                    let inverted = self.state.nav_settings.inverted;
                    let scroll = self.state.nav_settings.scroll;
                    if inside_rect(self.book_area, c, r) {
                        if inverted {
                            self.offset.scroll_down(scroll);
                        } else {
                            self.offset.scroll_up(scroll);
                        }
                    } else {
                        if inverted {
                            app.scroll_down(scroll);
                        } else {
                            app.scroll_up(scroll);
                        }
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
                            return Ok(ApplicationTask::SwitchView(AppView::Edit));
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

                        match parse_args(args) {
                            Ok(command) => {
                                if !app.run_command(command)? {
                                    return Ok(ApplicationTask::Quit);
                                }
                                if app.has_help_string() {
                                    return Ok(ApplicationTask::SwitchView(AppView::Help));
                                }
                            }
                            Err(_) => {
                                // TODO: How should invalid commands be handled?
                            }
                        }
                        return Ok(ApplicationTask::Update);
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
                        app.deselect();
                        self.state.curr_command.clear();
                        app.pop_scope();
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

    fn take_state(&mut self) -> UIState {
        std::mem::take(&mut self.state)
    }
}

pub(crate) struct EditWidget {
    pub(crate) edit: EditState,
    pub(crate) state: UIState,
}

impl<'b, D: IndexableDatabase, B: Backend> ResizableWidget<D, B> for EditWidget {
    fn allocate_chunk(&self, chunk: Rect) -> Option<usize> {
        let vchunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(chunk.height - 1), Constraint::Length(1)])
            .split(chunk);

        let curr_height = usize::from(vchunks[0].height);
        Some(curr_height.saturating_sub(1))
    }

    fn render_into_frame(&mut self, app: &App<D>, f: &mut Frame<B>, chunk: Rect) {
        let vchunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(chunk.height - 1), Constraint::Length(1)])
            .split(chunk);

        let hchunks = split_chunk_into_columns(chunk, app.num_cols() as u16);

        let edit_style = self.state.style.edit_style();
        let select_style = self.state.style.select_style();

        // TODO: Make it so that the selected value is visible
        //  at the cursor location.
        for (i, ((title, data), &chunk)) in app.header_col_iter().zip(hchunks.iter()).enumerate() {
            let width = usize::from(chunk.width).saturating_sub(1);
            let data = if i == self.state.selected_column {
                let mut data = data.clone();
                data[self.edit.selected] = self.edit.visible().to_owned();
                data
            } else {
                data.clone()
            };
            let list = List::new(
                data.iter()
                    .map(|word| ListItem::new(Span::from(word.unicode_truncate(width).0)))
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

impl<D: IndexableDatabase> InputHandler<D> for EditWidget {
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
                        return Ok(ApplicationTask::SwitchView(AppView::Columns));
                    }
                    KeyCode::Esc => {
                        app.update_value(
                            self.state.selected_column,
                            self.edit.selected,
                            &self.edit.orig_value,
                        );
                        return Ok(ApplicationTask::SwitchView(AppView::Columns));
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
                            app.get_value(self.state.selected_column, self.edit.selected),
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
                            app.get_value(self.state.selected_column, self.edit.selected),
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

    fn take_state(&mut self) -> UIState {
        std::mem::take(&mut self.state)
    }
}

pub(crate) struct HelpWidget {
    pub(crate) state: UIState,
    pub(crate) text: ScrollableText,
}

impl<'b, D: IndexableDatabase, B: Backend> ResizableWidget<D, B> for HelpWidget {
    fn allocate_chunk(&self, _chunk: Rect) -> Option<usize> {
        None
    }

    fn render_into_frame(&mut self, _app: &App<D>, f: &mut Frame<B>, chunk: Rect) {
        let vchunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(chunk.height - 1), Constraint::Length(1)])
            .split(chunk);

        self.text.refresh_window_height(vchunks[0].height as usize);
        let paragraph = Paragraph::new(self.text.text().clone())
            .scroll((self.text.offset() as u16, 0))
            .style(Style::default());

        f.render_widget(paragraph, vchunks[0]);
        let text = Text::styled(
            "Press ESC to return".to_string(),
            Style::default().add_modifier(Modifier::BOLD),
        );

        f.render_widget(Paragraph::new(text), vchunks[1])
    }
}

impl<D: IndexableDatabase> InputHandler<D> for HelpWidget {
    fn handle_input(
        &mut self,
        event: Event,
        _app: &mut App<D>,
    ) -> Result<ApplicationTask, ApplicationError> {
        match event {
            Event::Resize(_, _) => return Ok(ApplicationTask::Update),
            Event::Mouse(m) => match m {
                MouseEvent::ScrollDown(_, _, _) => {
                    if self.state.nav_settings.inverted {
                        self.text.scroll_up(self.state.nav_settings.scroll)
                    } else {
                        self.text.scroll_down(self.state.nav_settings.scroll)
                    };
                }
                MouseEvent::ScrollUp(_, _, _) => {
                    if self.state.nav_settings.inverted {
                        self.text.scroll_down(self.state.nav_settings.scroll)
                    } else {
                        self.text.scroll_up(self.state.nav_settings.scroll)
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
        Ok(ApplicationTask::Update)
    }

    fn take_state(&mut self) -> UIState {
        std::mem::take(&mut self.state)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_chunk_split() {
        let width = 50;
        let c = Rect::new(0, 0, width, 0);

        assert_eq!(split_chunk_into_columns(c, 0), vec![]);

        for i in 1..width {
            assert_eq!(
                split_chunk_into_columns(c, i)
                    .iter()
                    .map(|r| r.width)
                    .sum::<u16>(),
                width
            );
        }
    }
}
