use std::cell::{Ref, RefCell, RefMut};
use std::ops::DerefMut;
use std::rc::Rc;
use std::sync::Arc;

use crossterm::event::{Event, KeyCode, KeyModifiers, MouseEventKind};

use tui::backend::Backend;
use tui::layout::{Constraint, Direction, Layout, Rect};
use tui::style::Color as TColor;
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
use bookstore_records::book::{ColumnIdentifier, RecordError};

use crate::ui::scrollable_text::ScrollableText;
use crate::ui::terminal_ui::UIState;
use crate::ui::widgets::{
    char_chunks_to_styled_text, BookWidget, CommandWidget, StyleRules, Widget,
};

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

    fn cursor_style(&self) -> Style;
}

fn to_tui(c: bookstore_app::settings::Color) -> tui::style::Color {
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
fn get_clipboard_provider() -> Option<ClipboardContext> {
    ClipboardProvider::new().ok()
}

#[cfg(feature = "copypaste")]
fn paste_into_clipboard(a: &str) {
    if let Some(mut ctx) = get_clipboard_provider() {
        let _ = ctx.set_contents(a.to_owned());
    } else {
        // How should we handle this case?
    }
}

#[cfg(not(feature = "copypaste"))]
fn paste_into_clipboard(_a: &str) {}

#[cfg(feature = "copypaste")]
fn copy_from_clipboard() -> Option<String> {
    if let Some(mut ctx) = get_clipboard_provider() {
        ctx.get_contents().ok()
    } else {
        // How should we handle this case?
        None
    }
}

#[cfg(not(feature = "copypaste"))]
fn copy_from_clipboard() -> Option<String> {
    None
}

// TODO: Add Find widget that does live searching as user types (but doesn't update if match isn't being changed).
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

    fn cursor_style(&self) -> Style {
        Style::default()
            .fg(to_tui(self.cursor_fg))
            .bg(to_tui(self.cursor_bg))
            .add_modifier(Modifier::SLOW_BLINK)
    }
}

pub(crate) trait ResizableWidget<D: IndexableDatabase, B: Backend> {
    // Prepares to render the app
    fn prepare_render(&mut self, chunk: Rect);

    /// Renders the widget into the frame, using the provided space.
    ///
    /// # Arguments
    ///
    /// * ` f ` - A frame to render into.
    /// * ` chunk ` - A chunk to specify the size of the widget.
    fn render_into_frame(&self, f: &mut Frame<B>, chunk: Rect);
}

pub(crate) trait InputHandler<D: IndexableDatabase> {
    /// Processes the event and modifies the internal state accordingly. May modify app,
    /// depending on specific event.
    fn handle_input(
        &mut self,
        event: Event,
        app: &mut App<D>,
    ) -> Result<ApplicationTask, ApplicationError<D::Error>>;
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
    pub(crate) book_widget: Option<BookWidget>,
    pub(crate) command_widget_selected: bool,
}

impl<D: IndexableDatabase> ColumnWidget<D> {
    fn state(&self) -> Ref<UIState<D>> {
        self.state.as_ref().borrow()
    }

    fn state_mut(&mut self) -> RefMut<UIState<D>> {
        self.state.as_ref().borrow_mut()
    }

    fn refresh_book_widget(&mut self) {
        let book = self.state().book_view.get_selected_book().ok();
        let should_change = match (&book, &self.book_widget) {
            (Some(_), None) => true,
            (None, Some(_)) => true,
            (Some(b), Some(bw)) => !Arc::ptr_eq(b, bw.book()),
            (None, None) => false,
        };
        if should_change {
            self.book_widget = book.map(|book| BookWidget::new(Rect::default(), book));
        };
    }

    fn scroll_up(&mut self) {
        let mut state = self.state_mut();
        let scroll = state.nav_settings.scroll;
        if state.nav_settings.inverted {
            state.modify_bv(|bv| bv.scroll_down(scroll));
        } else {
            state.modify_bv(|bv| bv.scroll_up(scroll));
        }
    }

    fn scroll_down(&mut self) {
        let mut state = self.state_mut();
        let scroll = state.nav_settings.scroll;
        if state.nav_settings.inverted {
            state.modify_bv(|bv| bv.scroll_up(scroll));
        } else {
            state.modify_bv(|bv| bv.scroll_down(scroll));
        }
    }

    fn page_down(&mut self) {
        self.state_mut().book_view.page_down();
    }

    fn page_up(&mut self) {
        self.state_mut().book_view.page_up();
    }

    fn home(&mut self) {
        self.state_mut().book_view.home();
    }

    fn end(&mut self) {
        self.state_mut().book_view.end();
    }

    fn select_up(&mut self, modifiers: KeyModifiers) {
        if self.command_widget_selected {
            if modifiers.intersects(KeyModifiers::SHIFT) {
                self.state_mut().curr_command.key_shift_up();
            } else {
                self.state_mut().curr_command.key_up();
            }
        } else {
            self.state_mut().book_view.select_up();
        }
    }

    fn select_down(&mut self, modifiers: KeyModifiers) {
        if self.command_widget_selected {
            if modifiers.intersects(KeyModifiers::SHIFT) {
                self.state_mut().curr_command.key_shift_down();
            } else {
                self.state_mut().curr_command.key_down();
            }
        } else {
            self.state_mut().book_view.select_down();
        }
    }
}

impl<'b, D: IndexableDatabase, B: Backend> ResizableWidget<D, B> for ColumnWidget<D> {
    fn prepare_render(&mut self, chunk: Rect) {
        self.refresh_book_widget();
        let chunk = if let Some(book_widget) = &mut self.book_widget {
            let hchunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(75), Constraint::Percentage(25)])
                .split(chunk);
            book_widget.set_chunk(hchunks[1]);
            hchunks[0]
        } else {
            chunk
        };

        let vchunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(chunk.height - 1), Constraint::Length(1)])
            .split(chunk);
        let window_size = usize::from(vchunks[0].height).saturating_sub(1);
        let mut state = self.state_mut();
        state.book_view.refresh_window_size(window_size);
        let _ = state.update_column_data();
    }

    fn render_into_frame(&self, f: &mut Frame<B>, chunk: Rect) {
        let chunk = if let Some(book_widget) = &self.book_widget {
            let hchunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(75), Constraint::Percentage(25)])
                .split(chunk);

            book_widget.render_into_frame(f, hchunks[1]);
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

// NOTE: This is the only place where app scrolling takes place.
impl<D: IndexableDatabase> InputHandler<D> for ColumnWidget<D> {
    fn handle_input(
        &mut self,
        event: Event,
        app: &mut App<D>,
    ) -> Result<ApplicationTask, ApplicationError<D::Error>> {
        match event {
            Event::Resize(_, _) => return Ok(ApplicationTask::UpdateUI),
            Event::Mouse(m) => match (m.kind, m.column, m.row) {
                (MouseEventKind::ScrollDown, c, r) => {
                    let inverted = self.state().nav_settings.inverted;
                    let scroll = self.state().nav_settings.scroll;
                    if let Some(book_widget) = &mut self.book_widget {
                        if book_widget.contains_point(c, r) {
                            if inverted {
                                book_widget.offset_mut().scroll_up(scroll);
                            } else {
                                book_widget.offset_mut().scroll_down(scroll);
                            }
                        } else {
                            self.scroll_down();
                        }
                    } else {
                        self.scroll_down();
                    }
                }
                (MouseEventKind::ScrollUp, c, r) => {
                    let inverted = self.state().nav_settings.inverted;
                    let scroll = self.state().nav_settings.scroll;
                    if let Some(book_widget) = &mut self.book_widget {
                        if book_widget.contains_point(c, r) {
                            if inverted {
                                book_widget.offset_mut().scroll_down(scroll);
                            } else {
                                book_widget.offset_mut().scroll_up(scroll);
                            }
                        } else {
                            self.scroll_up();
                        }
                    } else {
                        self.scroll_up();
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
                        self.state_mut().curr_command.backspace();
                    }
                    KeyCode::Char(x) => {
                        if event.modifiers == KeyModifiers::CONTROL {
                            match (x, cfg!(feature = "copypaste")) {
                                ('v', true) => {
                                    let mut state = self.state_mut();
                                    if let Some(s) = copy_from_clipboard() {
                                        for c in s.chars() {
                                            state.curr_command.push(c);
                                        }
                                    }
                                }
                                ('c', true) => {
                                    let state = self.state();
                                    if let Some(text) = state.curr_command.selected() {
                                        paste_into_clipboard(&text.iter().collect::<String>());
                                    } else {
                                        let string = state.curr_command.to_string();
                                        if !string.is_empty() {
                                            paste_into_clipboard(&string);
                                        }
                                    }
                                }
                                ('x', true) => {
                                    let mut state = self.state_mut();
                                    if let Some(text) = state.curr_command.selected() {
                                        paste_into_clipboard(&text.iter().collect::<String>());
                                        state.curr_command.del();
                                    } else {
                                        let string = state.curr_command.to_string();
                                        if !string.is_empty() {
                                            paste_into_clipboard(&string);
                                        }
                                        state.curr_command.clear();
                                    }
                                }
                                ('d', _) => {
                                    self.command_widget_selected = false;
                                    let mut state = self.state_mut();
                                    state.curr_command.deselect();
                                    state.book_view.deselect();
                                }
                                ('a', _) => {
                                    self.state_mut().curr_command.select_all();
                                }
                                _ => self.state_mut().curr_command.push(x),
                            }
                        } else {
                            self.state_mut().curr_command.push(x);
                        }
                    }
                    KeyCode::Enter => {
                        if self.state().curr_command.is_empty() {
                            if self.state().book_view.selected().is_some() {
                                return Ok(ApplicationTask::SwitchView(AppView::Edit));
                            } else {
                                self.command_widget_selected = true;
                                return Ok(ApplicationTask::UpdateUI);
                            }
                        }

                        let mut state = self.state_mut();

                        let args: Vec<_> = state
                            .curr_command
                            .autofilled_values()
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
                        self.command_widget_selected = false;
                        let mut state = self.state_mut();
                        state.modify_bv(|bv| bv.deselect());
                        state.curr_command.clear();
                        state.modify_bv(|bv| bv.pop_scope());
                    }
                    KeyCode::Delete => {
                        let no_command = { self.state().curr_command.is_empty() };
                        if no_command {
                            app.remove_selected_book(&mut self.state_mut().book_view)?;
                        } else {
                            self.state_mut().curr_command.del();
                        }
                    }
                    // Scrolling
                    KeyCode::Up => self.select_up(event.modifiers),
                    KeyCode::Down => self.select_down(event.modifiers),
                    KeyCode::PageDown => self.page_down(),
                    KeyCode::PageUp => self.page_up(),
                    KeyCode::Home => self.home(),
                    KeyCode::End => self.end(),
                    KeyCode::Right => {
                        self.command_widget_selected = true;
                        if event.modifiers.intersects(KeyModifiers::SHIFT) {
                            self.state_mut().curr_command.key_shift_right();
                        } else {
                            self.state_mut().curr_command.key_right();
                        }
                    }
                    KeyCode::Left => {
                        self.command_widget_selected = true;
                        if event.modifiers.intersects(KeyModifiers::SHIFT) {
                            self.state_mut().curr_command.key_shift_left();
                        } else {
                            self.state_mut().curr_command.key_left();
                        }
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
    pub(crate) focused: bool,
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
    fn dump_edit(&mut self, app: &mut App<D>) -> Result<(), ApplicationError<D::Error>> {
        if self.edit.started_edit {
            self.focused = false;
            let column = {
                self.state().table_view.selected_cols()[self.state().selected_column].to_owned()
            };
            match app.edit_selected_book(
                &[(ColumnIdentifier::from(column), &self.edit.value_to_string())],
                &mut state_mut!(self).book_view,
            ) {
                Ok(_) => {}
                // Catch immutable column error and discard changes.
                Err(ApplicationError::Record(RecordError::ImmutableColumn)) => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    /// Used when column has been changed and edit should reflect new column's value.
    fn reset_edit(&mut self) {
        let value = self
            .state()
            .selected_table_value()
            .expect("Selected value should exist when in edit mode.")
            .to_owned();
        self.edit = EditState::new(value);
    }
}

impl<'b, D: IndexableDatabase, B: Backend> ResizableWidget<D, B> for EditWidget<D> {
    fn prepare_render(&mut self, chunk: Rect) {
        let vchunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(chunk.height - 1), Constraint::Length(1)])
            .split(chunk);

        let window = usize::from(vchunks[0].height).saturating_sub(1);
        let mut state = self.state_mut();
        state.book_view.refresh_window_size(window);
        let _ = state.update_column_data();
    }

    fn render_into_frame(&self, f: &mut Frame<B>, chunk: Rect) {
        let vchunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(chunk.height - 1), Constraint::Length(1)])
            .split(chunk);

        let state = self.state();
        let hchunks = split_chunk_into_columns(chunk, state.num_cols() as u16);

        let edit_style = state.style.edit_style();
        let select_style = state.style.select_style();
        let selected = state
            .selected()
            .expect("EditWidget should only exist when items are selected");

        let style_rules = StyleRules {
            cursor: state.style.cursor_style(),
            selected: select_style,
            default: edit_style,
        };

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
                        // TODO: Force text around cursor to be visible.
                        let styled =
                            char_chunks_to_styled_text(self.edit.char_chunks(), style_rules);
                        //Span::from(self.edit.value_to_string().unicode_truncate_start(width).0.to_string())
                        ListItem::new(styled)
                    } else {
                        ListItem::new(Span::from(word.unicode_truncate(width).0))
                    }
                })
                .collect::<Vec<_>>();

            let mut list =
                List::new(items).block(Block::default().title(Span::from(title.to_string())));
            if !self.focused {
                list = list.highlight_style(if col == selected.0 {
                    edit_style
                } else {
                    select_style
                });
            }

            let mut selected_row = ListState::default();
            selected_row.select(Some(selected.1));
            f.render_stateful_widget(list, chunk, &mut selected_row);
        }
        CommandWidget::new(&state.curr_command).render_into_frame(f, vchunks[1]);
    }
}

impl<D: IndexableDatabase> InputHandler<D> for EditWidget<D> {
    fn handle_input(
        &mut self,
        event: Event,
        app: &mut App<D>,
    ) -> Result<ApplicationTask, ApplicationError<D::Error>> {
        match event {
            Event::Resize(_, _) => return Ok(ApplicationTask::UpdateUI),
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
                                    if let Some(text) = self.edit.selected() {
                                        paste_into_clipboard(&text.iter().collect::<String>());
                                    } else {
                                        let string = self.edit.value_to_string();
                                        if !string.is_empty() {
                                            paste_into_clipboard(&string);
                                        }
                                    }
                                }
                                ('x', true) => {
                                    if let Some(text) = self.edit.selected() {
                                        paste_into_clipboard(&text.iter().collect::<String>());
                                        self.edit.del();
                                    } else {
                                        let string = self.edit.value_to_string();
                                        if !string.is_empty() {
                                            paste_into_clipboard(&string);
                                        }
                                        self.edit.clear();
                                    }
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
                        self.dump_edit(app)?;
                        if self.state().selected_column + 1 < self.state().num_cols() {
                            self.state_mut().selected_column += 1;
                            // Only reset edit if changing columns
                            self.reset_edit();
                        }
                    }
                    KeyCode::BackTab => {
                        self.dump_edit(app)?;
                        if self.state().selected_column > 0 {
                            self.state_mut().selected_column -= 1;
                            // Only reset edit if changing columns
                            self.reset_edit();
                        }
                    }

                    KeyCode::Enter => {
                        if !self.focused {
                            self.focused = true;
                        } else {
                            self.dump_edit(app)?;
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
                            self.dump_edit(app)?;
                            if self.state_mut().book_view.select_down() {
                                self.reset_edit();
                            }
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
                            self.dump_edit(app)?;
                            if self.state_mut().book_view.select_up() {
                                self.reset_edit();
                            }
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
                            self.dump_edit(app)?;
                            if self.state().selected_column > 0 {
                                self.state_mut().selected_column -= 1;
                                self.reset_edit();
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
                            self.dump_edit(app)?;
                            if self.state().selected_column + 1 < self.state().num_cols() {
                                self.state_mut().selected_column += 1;
                                // Only reset edit if changing columns
                                self.reset_edit();
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
    fn prepare_render(&mut self, chunk: Rect) {
        let vchunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(chunk.height - 1), Constraint::Length(1)])
            .split(chunk);

        self.text.refresh_window_height(vchunks[0].height as usize);
    }

    fn render_into_frame(&self, f: &mut Frame<B>, chunk: Rect) {
        let vchunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(chunk.height - 1), Constraint::Length(1)])
            .split(chunk);

        let paragraph = Paragraph::new(self.text.text())
            .scroll((self.text.offset() as u16, 0))
            .style(Style::default());

        f.render_widget(paragraph, vchunks[0]);
        let text = Text::styled(
            "Press ESC to return",
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
    ) -> Result<ApplicationTask, ApplicationError<D::Error>> {
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
