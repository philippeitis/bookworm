use std::cell::{Ref, RefCell, RefMut};
use std::ops::DerefMut;
use std::rc::Rc;

use crossterm::event::{Event, KeyCode, KeyModifiers, MouseEventKind};

use tui::backend::Backend;
use tui::layout::{Constraint, Direction, Layout, Rect};
use tui::style::{Modifier, Style};
use tui::text::{Span, Text};
use tui::widgets::{Block, List, ListItem, ListState, Paragraph};
use tui::Frame;

use unicode_truncate::UnicodeTruncateStr;

#[cfg(feature = "copypaste")]
use clipboard::{ClipboardContext, ClipboardProvider};

use bookstore_app::settings::Color;
use bookstore_app::{parse_args, App, ApplicationError, Command};
use bookstore_app::{settings::InterfaceStyle, user_input::EditState};
use bookstore_database::{BookView, IndexableDatabase, NestedBookView, ScrollableBookView};
use bookstore_records::BookError;

use crate::ui::scrollable_text::{BlindOffset, ScrollableText};
use crate::ui::terminal_ui::UIState;
use crate::ui::widgets::{book_to_widget_text, CommandWidget, Widget};

macro_rules! state {
    ($self: ident) => {
        $self.state.as_ref().borrow()
    };
}

macro_rules! state_mut {
    ($self: ident) => {
        $self.state.as_ref().borrow_mut()
    };
}

#[derive(Copy, Clone)]
pub(crate) enum AppView {
    Columns,
    Edit,
    Help,
}

pub(crate) enum ApplicationTask {
    Quit,
    DoNothing,
    SwitchView(AppView),
    UpdateUI,
}

trait TuiStyle {
    fn edit_style(&self) -> Style;

    fn select_style(&self) -> Style;
}

fn to_tui(c: bookstore_app::settings::Color) -> tui::style::Color {
    use tui::style::Color as TColor;
    match c {
        Color::Black => TColor::Black,
        Color::Red => TColor::Red,
        Color::Green => TColor::Green,
        Color::Yellow => TColor::Yellow,
        Color::Blue => TColor::Blue,
        Color::Magenta => TColor::Magenta,
        Color::Cyan => TColor::Cyan,
        Color::Gray => TColor::Gray,
        Color::DarkGray => TColor::DarkGray,
        Color::LightRed => TColor::LightRed,
        Color::LightGreen => TColor::LightGreen,
        Color::LightYellow => TColor::LightYellow,
        Color::LightBlue => TColor::LightBlue,
        Color::LightMagenta => TColor::LightMagenta,
        Color::LightCyan => TColor::LightCyan,
        Color::White => TColor::White,
    }
}

#[cfg(feature = "copypaste")]
fn paste_into_clipboard(a: &str) {
    let mut ctx: ClipboardContext = ClipboardProvider::new().unwrap();
    let _ = ctx.set_contents(a.to_owned());
}

#[cfg(not(feature = "copypaste"))]
fn paste_into_clipboard(_a: &str) {}

#[cfg(feature = "copypaste")]
fn copy_from_clipboard() -> Option<String> {
    let mut ctx: ClipboardContext = ClipboardProvider::new().unwrap();
    ctx.get_contents().ok()
}

#[cfg(not(feature = "copypaste"))]
fn copy_from_clipboard() -> Option<String> {
    None
}

impl TuiStyle for InterfaceStyle {
    fn edit_style(&self) -> Style {
        Style::default()
            .fg(to_tui(self.edit_fg))
            .bg(to_tui(self.edit_bg))
    }

    fn select_style(&self) -> Style {
        Style::default()
            .fg(to_tui(self.selected_fg))
            .bg(to_tui(self.selected_bg))
    }
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
}

/// Takes `word`, and cuts excess letters to ensure that it fits within
/// `max_width` visible characters. If `word` is too long, it will be truncated
/// and have '...' appended to indicate that it has been truncated (if `max_width`
/// is at least 3, otherwise, letters will simply be cut). It will then be returned as a
/// `ListItem`.
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

pub(crate) struct ColumnWidget<D: IndexableDatabase> {
    pub(crate) state: Rc<RefCell<UIState<D>>>,
    pub(crate) offset: BlindOffset,
    pub(crate) book_area: Rect,
}

impl<D: IndexableDatabase> ColumnWidget<D> {
    fn state(&self) -> Ref<UIState<D>> {
        self.state.as_ref().borrow()
    }

    fn state_mut(&mut self) -> RefMut<UIState<D>> {
        self.state.as_ref().borrow_mut()
    }
}

impl<'b, D: IndexableDatabase, B: Backend> ResizableWidget<D, B> for ColumnWidget<D> {
    fn allocate_chunk(&self, chunk: Rect) -> Option<usize> {
        let vchunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(chunk.height - 1), Constraint::Length(1)])
            .split(chunk);
        let curr_height = usize::from(vchunks[0].height);
        Some(curr_height.saturating_sub(1))
    }

    fn render_into_frame(&mut self, _app: &App<D>, f: &mut Frame<B>, chunk: Rect) {
        let chunk = if let Ok(b) = state!(self).book_view.get_selected_book() {
            let hchunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(75), Constraint::Percentage(25)])
                .split(chunk);

            self.book_area = hchunks[1];
            let b = &b.as_ref().read().unwrap();
            let book_text = book_to_widget_text(b, self.book_area.width.saturating_sub(1) as usize);
            self.offset
                .refresh_window_height(self.book_area.height as usize);
            let offset = self.offset.offset_with_height(book_text.lines.len());
            let p = Paragraph::new(book_text).scroll((offset as u16, 0));
            f.render_widget(p, self.book_area);

            hchunks[0]
        } else {
            chunk
        };

        let vchunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(chunk.height - 1), Constraint::Length(1)])
            .split(chunk);

        let chunk = vchunks[0];
        let hchunks = split_chunk_into_columns(chunk, self.state().num_cols() as u16);
        let select_style = self.state().style.select_style();

        for ((title, data), &chunk) in self
            .state()
            .table_view
            .header_col_iter()
            .zip(hchunks.iter())
        {
            let width = usize::from(chunk.width).saturating_sub(1);
            let list = List::new(
                data.iter()
                    .map(|word| cut_word_to_fit(word, width))
                    .collect::<Vec<_>>(),
            )
            .block(Block::default().title(Span::from(title.to_string())))
            .highlight_style(select_style);
            let mut selected_row = ListState::default();
            selected_row.select(self.state().book_view.selected());
            f.render_stateful_widget(list, chunk, &mut selected_row);
        }

        CommandWidget::new(&self.state().curr_command).render_into_frame(f, vchunks[1]);
    }
}

fn inside_rect(rect: Rect, col: u16, row: u16) -> bool {
    col >= rect.x && col < rect.x + rect.width && row >= rect.y && row < rect.y + rect.height
}

// NOTE: This is the only place where app scrolling takes place.
impl<D: IndexableDatabase> InputHandler<D> for ColumnWidget<D> {
    fn handle_input(
        &mut self,
        event: Event,
        app: &mut App<D>,
    ) -> Result<ApplicationTask, ApplicationError> {
        match event {
            Event::Resize(_, _) => return Ok(ApplicationTask::UpdateUI),
            Event::Mouse(m) => match (m.kind, m.column, m.row) {
                (MouseEventKind::ScrollDown, c, r) => {
                    let inverted = self.state().nav_settings.inverted;
                    let scroll = self.state().nav_settings.scroll;
                    if inside_rect(self.book_area, c, r) {
                        if inverted {
                            self.offset.scroll_up(scroll);
                        } else {
                            self.offset.scroll_down(scroll);
                        }
                    } else {
                        if inverted {
                            self.state_mut().modify_bv(|bv| bv.scroll_up(scroll));
                        } else {
                            self.state_mut().modify_bv(|bv| bv.scroll_down(scroll));
                        }
                    }
                }
                (MouseEventKind::ScrollUp, c, r) => {
                    let inverted = self.state().nav_settings.inverted;
                    let scroll = self.state().nav_settings.scroll;
                    if inside_rect(self.book_area, c, r) {
                        if inverted {
                            self.offset.scroll_down(scroll);
                        } else {
                            self.offset.scroll_up(scroll);
                        }
                    } else {
                        if inverted {
                            self.state_mut().modify_bv(|bv| bv.scroll_down(scroll));
                        } else {
                            self.state_mut().modify_bv(|bv| bv.scroll_up(scroll));
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
                        if self.state().book_view.selected().is_some() {
                            return Ok(ApplicationTask::SwitchView(AppView::Edit));
                        }
                    }
                    KeyCode::Backspace => {
                        self.state_mut().curr_command.pop();
                    }
                    KeyCode::Char(x) => {
                        let mut state = self.state_mut();
                        if cfg!(feature = "copypaste") && event.modifiers == KeyModifiers::CONTROL {
                            if x == 'v' {
                                if let Some(s) = copy_from_clipboard() {
                                    for c in s.chars() {
                                        state.curr_command.push(c);
                                    }
                                }
                            } else {
                                state.curr_command.push(x);
                            }
                        } else {
                            state.curr_command.push(x);
                        }
                    }
                    KeyCode::Enter => {
                        let mut state = self.state_mut();

                        let args: Vec<_> = state
                            .curr_command
                            .get_values_autofilled()
                            .into_iter()
                            .map(|(_, a)| a)
                            .collect();

                        state.curr_command.clear();

                        match parse_args(args) {
                            Ok(command) => {
                                let state_deref = state.deref_mut();
                                let table_view = &mut state_deref.table_view;
                                let book_view = &mut state_deref.book_view;
                                if !app.run_command(command, table_view, book_view)? {
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
                        return Ok(ApplicationTask::UpdateUI);
                    }
                    KeyCode::Tab | KeyCode::BackTab => {
                        let curr_command = &mut self.state_mut().curr_command;
                        curr_command.refresh_autofill()?;
                        match parse_args(curr_command.get_values().map(|(_, s)| s).collect()) {
                            Ok(command) => match command {
                                Command::AddBookFromFile(_) => curr_command.auto_fill(false),
                                Command::AddBooksFromDir(_, _) => curr_command.auto_fill(true),
                                _ => {}
                            },
                            Err(_) => {}
                        }
                    }
                    KeyCode::Esc => {
                        let mut state = self.state_mut();
                        state.modify_bv(|bv| bv.deselect());
                        state.curr_command.clear();
                        state.modify_bv(|bv| bv.pop_scope());
                    }
                    KeyCode::Delete => {
                        if self.state().curr_command.is_empty() {
                            app.remove_selected_book(&mut self.state_mut().book_view)?;
                        } else {
                            // TODO: Add code to delete forwards
                            //  (requires implementing cursor logic)
                        }
                    }
                    // Scrolling
                    KeyCode::Up => {
                        self.state_mut().modify_bv(|bv| bv.select_up());
                    }
                    KeyCode::Down => {
                        self.state_mut().modify_bv(|bv| bv.select_down());
                    }
                    KeyCode::PageDown => {
                        self.state_mut().modify_bv(|bv| bv.page_down());
                    }
                    KeyCode::PageUp => {
                        self.state_mut().modify_bv(|bv| bv.page_up());
                    }
                    KeyCode::Home => {
                        self.state_mut().modify_bv(|bv| bv.home());
                    }
                    KeyCode::End => {
                        self.state_mut().modify_bv(|bv| bv.end());
                    }
                    _ => return Ok(ApplicationTask::DoNothing),
                }
            }
        }
        Ok(ApplicationTask::UpdateUI)
    }
}

pub(crate) struct EditWidget<D: IndexableDatabase> {
    pub(crate) edit: EditState,
    pub(crate) state: Rc<RefCell<UIState<D>>>,
}

impl<D: IndexableDatabase> EditWidget<D> {
    fn state(&self) -> Ref<UIState<D>> {
        self.state.as_ref().borrow()
    }
    fn state_mut(&mut self) -> RefMut<UIState<D>> {
        self.state.as_ref().borrow_mut()
    }

    /// Used to save the edit to the book being modified.
    fn dump_edit(&mut self, app: &mut App<D>) -> Result<(), ApplicationError> {
        if self.edit.started_edit {
            let column = {
                self.state().table_view.selected_cols()[self.state().selected_column].to_owned()
            };
            match app.edit_selected_book(column, &self.edit.value, &mut state_mut!(self).book_view)
            {
                Ok(_) => {}
                // Catch immutable column error and discard changes.
                Err(ApplicationError::Book(BookError::ImmutableColumnError)) => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    /// Used when column has been changed and edit should reflect new column's value.
    fn reset_edit(&mut self) {
        let value = self.state().get_selected_table_value().unwrap().to_owned();
        self.edit = EditState::new(value);
    }
}

impl<'b, D: IndexableDatabase, B: Backend> ResizableWidget<D, B> for EditWidget<D> {
    fn allocate_chunk(&self, chunk: Rect) -> Option<usize> {
        let vchunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(chunk.height - 1), Constraint::Length(1)])
            .split(chunk);

        let curr_height = usize::from(vchunks[0].height);
        Some(curr_height.saturating_sub(1))
    }

    fn render_into_frame(&mut self, _app: &App<D>, f: &mut Frame<B>, chunk: Rect) {
        let vchunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(chunk.height - 1), Constraint::Length(1)])
            .split(chunk);

        let hchunks = split_chunk_into_columns(chunk, self.state().num_cols() as u16);

        let edit_style = self.state().style.edit_style();
        let select_style = self.state().style.select_style();
        let selected = self.state().selected().unwrap();

        for (col, ((title, data), &chunk)) in self
            .state()
            .table_view
            .header_col_iter()
            .zip(hchunks.iter())
            .enumerate()
        {
            let width = usize::from(chunk.width).saturating_sub(1);
            let items = data
                .iter()
                .enumerate()
                .map(|(row, word)| {
                    if selected == (col, row) {
                        ListItem::new(Span::from(self.edit.value.unicode_truncate_start(width).0))
                    } else {
                        ListItem::new(Span::from(word.unicode_truncate(width).0))
                    }
                })
                .collect::<Vec<_>>();

            let list = List::new(items)
                .block(Block::default().title(Span::from(title.to_string())))
                .highlight_style(if col == selected.0 {
                    edit_style
                } else {
                    select_style
                });

            let mut selected_row = ListState::default();
            selected_row.select(Some(selected.1));
            f.render_stateful_widget(list, chunk, &mut selected_row);
        }
        CommandWidget::new(&self.state().curr_command).render_into_frame(f, vchunks[1]);
    }
}

impl<D: IndexableDatabase> InputHandler<D> for EditWidget<D> {
    fn handle_input(
        &mut self,
        event: Event,
        app: &mut App<D>,
    ) -> Result<ApplicationTask, ApplicationError> {
        match event {
            Event::Resize(_, _) => return Ok(ApplicationTask::UpdateUI),
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
                        if cfg!(feature = "copypaste") && event.modifiers == KeyModifiers::CONTROL {
                            if c == 'v' {
                                if let Some(s) = copy_from_clipboard() {
                                    self.edit.extend(&s);
                                }
                            } else if c == 'c' {
                                paste_into_clipboard(&self.edit.value)
                            } else {
                                self.edit.push(c);
                            }
                        } else {
                            self.edit.push(c);
                        }
                    }
                    KeyCode::Enter => {
                        self.dump_edit(app)?;
                        return Ok(ApplicationTask::SwitchView(AppView::Columns));
                    }
                    KeyCode::Esc => {
                        return Ok(ApplicationTask::SwitchView(AppView::Columns));
                    }
                    KeyCode::Delete => {
                        // TODO: Add code to delete forwards
                        //  (requires implementing cursor logic)
                    }
                    KeyCode::Down => {
                        self.dump_edit(app)?;
                        if self.state().selected_column + 1 < self.state().num_cols() {
                            self.state_mut().selected_column += 1;
                            // Only reset edit if changing columns
                            self.reset_edit();
                        }
                    }
                    KeyCode::Up => {
                        self.dump_edit(app)?;
                        if self.state().selected_column > 0 {
                            self.state_mut().selected_column -= 1;
                            self.reset_edit();
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

pub(crate) struct HelpWidget<D: IndexableDatabase> {
    pub(crate) state: Rc<RefCell<UIState<D>>>,
    pub(crate) text: ScrollableText,
}

impl<D: IndexableDatabase> HelpWidget<D> {
    fn state(&self) -> Ref<UIState<D>> {
        self.state.as_ref().borrow()
    }

    #[allow(dead_code)]
    fn state_mut(&mut self) -> RefMut<UIState<D>> {
        self.state.as_ref().borrow_mut()
    }
}

impl<'b, D: IndexableDatabase, B: Backend> ResizableWidget<D, B> for HelpWidget<D> {
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

impl<D: IndexableDatabase> InputHandler<D> for HelpWidget<D> {
    fn handle_input(
        &mut self,
        event: Event,
        _app: &mut App<D>,
    ) -> Result<ApplicationTask, ApplicationError> {
        match event {
            Event::Resize(_, _) => return Ok(ApplicationTask::UpdateUI),
            Event::Mouse(m) => match m.kind {
                MouseEventKind::ScrollDown => {
                    let scroll = self.state().nav_settings.scroll;
                    if self.state().nav_settings.inverted {
                        self.text.scroll_up(scroll)
                    } else {
                        self.text.scroll_down(scroll)
                    };
                }
                MouseEventKind::ScrollUp => {
                    let scroll = self.state().nav_settings.scroll;
                    if self.state().nav_settings.inverted {
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
