use std::collections::HashMap;
use std::marker::PhantomData;

use async_trait::async_trait;
use crossterm::event::{Event, KeyCode, KeyModifiers, MouseEventKind};

use tui::backend::Backend;
use tui::layout::{Constraint, Direction, Layout, Rect};
use tui::style::Color as TColor;
use tui::style::{Modifier, Style};
use tui::text::{Span, Text};
use tui::widgets::{Block, Paragraph};
use tui::Frame;

use unicode_truncate::UnicodeTruncateStr;

#[cfg(feature = "copypaste")]
use clipboard::{ClipboardContext, ClipboardProvider};

use bookworm_app::app::AppChannel;
use bookworm_app::parser::Source;
use bookworm_app::settings::InterfaceStyle;
use bookworm_app::settings::{Color, SortSettings};
use bookworm_app::{parse_args, ApplicationError, BookIndex, Command};
use bookworm_database::paginator::Selection;
use bookworm_database::{AppDatabase, DatabaseError};
use bookworm_input::{user_input::InputRecorder, Edit};
use bookworm_records::book::{BookID, ColumnIdentifier, RecordError};

use crate::ui::help_strings::{help_strings, GENERAL_HELP};
use crate::ui::scrollable_text::ScrollableText;
use crate::ui::terminal_ui::{TuiError, UIState};
use crate::ui::tui_widgets::{ListItemX, MultiSelectList, MultiSelectListState};
use crate::ui::widgets::{
    char_chunks_to_styled_text, BookWidget, CommandWidget, StyleRules, Widget,
};

#[derive(Clone)]
pub enum AppView {
    Columns,
    Edit,
    Help(String),
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

#[tracing::instrument(name = "Executing user command", skip(app, command, ui_state))]
pub(crate) async fn run_command<D: AppDatabase + Send + Sync>(
    app: &mut AppChannel<D>,
    command: Command,
    ui_state: &mut UIState<D>,
) -> Result<ApplicationTask, TuiError<D::Error>> {
    match command {
        Command::DeleteSelected => {
            app.delete_selected(ui_state.book_view.selected_books().clone())
                .await;
            ui_state.book_view.refresh().await?;
        }
        Command::DeleteMatching(matches) => {
            // TODO: This will be changed to a set of merge conflicts, which the
            //  UI layer will resolve.
            let matches = matches
                .to_vec()
                .into_iter()
                .filter_map(|s| s.into_matcher().ok())
                .collect::<Vec<_>>()
                .into_boxed_slice();
            let _ = app.delete_selected(Selection::All(matches)).await;
            ui_state.book_view.refresh().await?;
        }
        Command::DeleteAll => {
            app.delete_selected(Selection::All(Box::default())).await;
            ui_state.book_view.refresh().await?;
        }
        Command::EditBook(book, edits) => match book {
            BookIndex::Selected => {
                app.edit_selected(ui_state.book_view.selected_books().clone(), edits)
                    .await;
                ui_state.book_view.refresh().await?;
            }
            BookIndex::ID(id) => app.edit_books(vec![id].into_boxed_slice(), edits).await,
        },
        Command::AddBooks(sources) => {
            app.add_books(sources).await;
            ui_state.book_view.refresh().await?;
        }
        Command::UpdateBooks(sources) => {
            app.update_books(sources).await;
            ui_state.book_view.refresh().await?;
        }
        Command::ModifyColumns(columns) => {
            app.modify_columns(columns, &mut ui_state.table_view, &mut ui_state.book_view)
                .await?;
        }
        Command::SortColumns(columns) => {
            tracing::info!("Sorting by {:?}", columns);
            ui_state.book_view.sort_by_columns(&columns).await?;
            ui_state.sort_settings = SortSettings { columns };
        }
        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
        Command::OpenBookIn(book, index, target) => {
            let id = match book {
                BookIndex::Selected => {
                    if let Some(book) = ui_state.book_view.selected_books().front() {
                        book.id()
                    } else {
                        return Ok(ApplicationTask::DoNothing);
                    }
                }
                BookIndex::ID(id) => id,
            };

            app.open_book(id, index, target).await;
        }
        Command::FilterMatches(searches) => {
            ui_state.book_view.push_scope(&searches).await?;
        }
        Command::JumpTo(searches) => {
            ui_state.book_view.jump_to(&searches).await?;
        }
        Command::Write => {
            app.save().await;
        }
        // TODO: A warning pop-up when user is about to exit
        //  with unsaved changes.
        Command::Quit => return Ok(ApplicationTask::Quit),
        Command::WriteAndQuit => {
            app.save().await;
            return Ok(ApplicationTask::Quit);
        }
        Command::TryMergeAllBooks => {
            ui_state.book_view.refresh().await?;
        }
        Command::Help(target) => {
            return Ok(ApplicationTask::SwitchView(AppView::Help(
                help_strings(&target).unwrap_or(GENERAL_HELP).to_string(),
            )));
        }
        Command::GeneralHelp => {
            return Ok(ApplicationTask::SwitchView(AppView::Help(
                GENERAL_HELP.to_string(),
            )));
        }
    }
    Ok(ApplicationTask::DoNothing)
}

fn to_tui(c: bookworm_app::settings::Color) -> tui::style::Color {
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

#[async_trait]
pub(crate) trait ResizableWidget<D: AppDatabase + Send + Sync, B: Backend> {
    // Prepares to render the app
    async fn prepare_render(&mut self, state: &mut UIState<D>, chunk: Rect);

    /// Renders the widget into the frame, using the provided space.
    ///
    /// # Arguments
    ///
    /// * ` f ` - A frame to render into.
    /// * ` chunk ` - A chunk to specify the size of the widget.
    fn render_into_frame(&self, f: &mut Frame<B>, state: &UIState<D>, chunk: Rect);
}

#[async_trait]
pub(crate) trait InputHandler<D: AppDatabase + Send + Sync> {
    /// Processes the event and modifies the internal state accordingly. May modify app,
    /// depending on specific event.
    async fn handle_input(
        &mut self,
        event: Event,
        state: &mut UIState<D>,
        app: &mut AppChannel<D>,
    ) -> Result<ApplicationTask, TuiError<D::Error>>;
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
fn cut_word_to_fit(word: &str, max_width: usize) -> ListItemX {
    // TODO: What should be done if max_width is too small?
    ListItemX::new(Span::from(if word.len() > max_width {
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

pub(crate) struct ColumnWidget<D> {
    pub(crate) book_widget: Option<BookWidget>,
    pub(crate) command_widget_selected: bool,
    pub(crate) database: PhantomData<fn(D)>,
}

impl<D: AppDatabase + Send + Sync> ColumnWidget<D> {
    async fn refresh_book_widget(&mut self, state: &UIState<D>) {
        let books = state.book_view.selected_books();
        self.book_widget = books
            .front()
            .map(|book| BookWidget::new(Rect::default(), book.clone()));
    }

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
        match (
            self.command_widget_selected,
            modifiers.intersects(KeyModifiers::SHIFT),
        ) {
            (false, false) => state.book_view.page_down().await,
            (false, true) => state.book_view.select_page_down().await,
            (true, _) => {
                unimplemented!("Paging down on command widget not supported.");
            }
        }
    }

    async fn page_up(
        &mut self,
        state: &mut UIState<D>,
        modifiers: KeyModifiers,
    ) -> Result<(), DatabaseError<D::Error>> {
        match (
            self.command_widget_selected,
            modifiers.intersects(KeyModifiers::SHIFT),
        ) {
            (false, false) => state.book_view.page_up().await,
            (false, true) => state.book_view.select_page_up().await,
            (true, _) => {
                unimplemented!("Paging up on command widget not supported.");
            }
        }
    }

    async fn home(
        &mut self,
        state: &mut UIState<D>,
        modifiers: KeyModifiers,
    ) -> Result<(), DatabaseError<D::Error>> {
        match (
            self.command_widget_selected,
            modifiers.intersects(KeyModifiers::SHIFT),
        ) {
            (false, false) => {
                state.book_view.home().await?;
            }
            (false, true) => {
                state.book_view.select_to_start().await?;
            }
            (true, false) => {
                state.curr_command.key_up();
            }
            (true, true) => {
                state.curr_command.key_shift_up();
            }
        }
        Ok(())
    }

    async fn end(
        &mut self,
        state: &mut UIState<D>,
        modifiers: KeyModifiers,
    ) -> Result<(), DatabaseError<D::Error>> {
        match (
            self.command_widget_selected,
            modifiers.intersects(KeyModifiers::SHIFT),
        ) {
            (false, false) => {
                state.book_view.end().await?;
            }
            (false, true) => {
                state.book_view.select_to_end().await?;
            }
            (true, false) => {
                state.curr_command.key_down();
            }
            (true, true) => {
                state.curr_command.key_shift_down();
            }
        }
        Ok(())
    }

    async fn select_up(
        &mut self,
        state: &mut UIState<D>,
        modifiers: KeyModifiers,
    ) -> Result<(), DatabaseError<D::Error>> {
        match (
            self.command_widget_selected,
            modifiers.intersects(KeyModifiers::SHIFT),
        ) {
            (false, false) => {
                state.book_view.up().await?;
            }
            (false, true) => {
                state.book_view.select_up().await?;
            }
            (true, false) => {
                state.curr_command.key_up();
            }
            (true, true) => {
                state.curr_command.key_shift_up();
            }
        }
        Ok(())
    }

    async fn select_down(
        &mut self,
        state: &mut UIState<D>,
        modifiers: KeyModifiers,
    ) -> Result<(), DatabaseError<D::Error>> {
        match (
            self.command_widget_selected,
            modifiers.intersects(KeyModifiers::SHIFT),
        ) {
            (false, false) => {
                state.book_view.down().await?;
            }
            (false, true) => {
                state.book_view.select_down().await?;
            }
            (true, false) => {
                state.curr_command.key_down();
            }
            (true, true) => {
                state.curr_command.key_shift_down();
            }
        }
        Ok(())
    }
}

#[async_trait]
impl<'b, D: AppDatabase + Send + Sync, B: Backend> ResizableWidget<D, B> for ColumnWidget<D> {
    // #[tracing::instrument(name = "Preparing ColumnWidgetRender", skip(self, state))]
    async fn prepare_render(&mut self, state: &mut UIState<D>, chunk: Rect) {
        self.refresh_book_widget(state).await;
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
            .constraints([
                Constraint::Length(chunk.height.saturating_sub(1)),
                Constraint::Length(1),
            ])
            .split(chunk);

        // tracing::info!("Preparing to render into chunk with size {:?}", chunk);
        // tracing::info!("Have vertical chunks: {:?}", vchunks);

        // Account for column titles
        let _ = state
            .book_view
            .refresh_window_size(usize::from(vchunks[0].height).saturating_sub(1))
            .await;
    }

    fn render_into_frame(&self, f: &mut Frame<B>, state: &UIState<D>, chunk: Rect) {
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
            .constraints([
                Constraint::Length(chunk.height.saturating_sub(1)),
                Constraint::Length(1),
            ])
            .split(chunk);

        let chunk = vchunks[0];
        let hchunks = split_chunk_into_columns(chunk, state.num_cols() as u16);
        let select_style = state.style.select_style();

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

        CommandWidget::new(&state.curr_command).render_into_frame(f, vchunks[1]);
    }
}

// NOTE: This is the only place where app scrolling takes place.
#[async_trait]
impl<D: AppDatabase + Send + Sync> InputHandler<D> for ColumnWidget<D> {
    async fn handle_input(
        &mut self,
        event: Event,
        state: &mut UIState<D>,
        app: &mut AppChannel<D>,
    ) -> Result<ApplicationTask, TuiError<D::Error>> {
        match event {
            Event::Resize(_, _) => return Ok(ApplicationTask::UpdateUI),
            Event::Mouse(m) => match (m.kind, m.column, m.row) {
                (MouseEventKind::ScrollDown, c, r) => {
                    let inverted = state.nav_settings.inverted;
                    let scroll = state.nav_settings.scroll;
                    if let Some(book_widget) = &mut self.book_widget {
                        if book_widget.contains_point(c, r) {
                            if inverted {
                                book_widget.offset_mut().scroll_up(scroll);
                            } else {
                                book_widget.offset_mut().scroll_down(scroll);
                            }
                        } else {
                            self.scroll_down(state).await?;
                        }
                    } else {
                        self.scroll_down(state).await?;
                    }
                }
                (MouseEventKind::ScrollUp, c, r) => {
                    let inverted = state.nav_settings.inverted;
                    let scroll = state.nav_settings.scroll;
                    if let Some(book_widget) = &mut self.book_widget {
                        if book_widget.contains_point(c, r) {
                            if inverted {
                                book_widget.offset_mut().scroll_down(scroll);
                            } else {
                                book_widget.offset_mut().scroll_up(scroll);
                            }
                        } else {
                            self.scroll_up(state).await?;
                        }
                    } else {
                        self.scroll_up(state).await?;
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
                        if !state.book_view.selected_books().is_empty() {
                            return Ok(ApplicationTask::SwitchView(AppView::Edit));
                        }
                    }
                    KeyCode::Backspace => {
                        state.curr_command.backspace();
                    }
                    KeyCode::Char(x) => {
                        if event.modifiers == KeyModifiers::CONTROL {
                            match (x, cfg!(feature = "copypaste")) {
                                ('v', true) => {
                                    if let Some(s) = copy_from_clipboard() {
                                        for c in s.chars() {
                                            state.curr_command.push(c);
                                        }
                                    }
                                }
                                ('c', true) => {
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
                                    state.curr_command.deselect();
                                    state.book_view.deselect_all();
                                }
                                ('a', _) => {
                                    if !self.command_widget_selected {
                                        state.book_view.select_all().await?;
                                    } else {
                                        state.curr_command.select_all();
                                    }
                                }
                                _ => state.curr_command.push(x),
                            }
                        } else {
                            state.curr_command.push(x);
                        }
                    }
                    KeyCode::Enter => {
                        if state.curr_command.is_empty() {
                            return if !state.book_view.selected_books().is_empty() {
                                Ok(ApplicationTask::SwitchView(AppView::Edit))
                            } else {
                                self.command_widget_selected = true;
                                Ok(ApplicationTask::UpdateUI)
                            };
                        }

                        let args: Vec<_> = state
                            .curr_command
                            .autofilled_values()
                            .into_iter()
                            .map(|(_, a)| a)
                            .collect();

                        state.curr_command.clear();

                        return match parse_args(args) {
                            Ok(command) => match run_command(app, command, state).await? {
                                ApplicationTask::DoNothing => Ok(ApplicationTask::UpdateUI),
                                other => Ok(other),
                            },
                            Err(_) => {
                                // TODO: How should invalid commands be handled?
                                Ok(ApplicationTask::UpdateUI)
                            }
                        };
                    }
                    KeyCode::Tab | KeyCode::BackTab => {
                        let curr_command = &mut state.curr_command;
                        curr_command.refresh_autofill()?;
                        match parse_args(curr_command.get_values().map(|(_, s)| s).collect()) {
                            Ok(command) => match command {
                                Command::AddBooks(sources) | Command::UpdateBooks(sources) => {
                                    match sources.last() {
                                        Some(Source::File(_)) => {
                                            curr_command.auto_fill(false);
                                        }
                                        Some(Source::Dir(_, _)) => {
                                            curr_command.auto_fill(true);
                                        }
                                        _ => {}
                                    }
                                }
                                _ => {}
                            },
                            Err(_) => {}
                        }
                    }
                    KeyCode::Esc => {
                        self.command_widget_selected = false;
                        state.book_view.deselect_all();
                        state.curr_command.clear();
                        state.book_view.pop_scope();
                    }
                    KeyCode::Delete => {
                        if state.curr_command.is_empty() {
                            tracing::info!("Command string is empty, and user has pressed delete");
                            if !state.book_view.selected_books().is_empty() {
                                run_command(app, Command::DeleteSelected, state).await?;
                            }
                        } else {
                            state.curr_command.del();
                        }
                    }
                    // Scrolling
                    KeyCode::Up => self.select_up(state, event.modifiers).await?,
                    KeyCode::Down => self.select_down(state, event.modifiers).await?,
                    KeyCode::PageDown => self.page_down(state, event.modifiers).await?,
                    KeyCode::PageUp => self.page_up(state, event.modifiers).await?,
                    KeyCode::Home => self.home(state, event.modifiers).await?,
                    KeyCode::End => self.end(state, event.modifiers).await?,
                    KeyCode::Right => {
                        if state.book_view.selected_books().is_empty() {
                            self.command_widget_selected = true;
                            if event.modifiers.intersects(KeyModifiers::SHIFT) {
                                state.curr_command.key_shift_right();
                            } else {
                                state.curr_command.key_right();
                            }
                        }
                    }
                    KeyCode::Left => {
                        if state.book_view.selected_books().is_empty() {
                            self.command_widget_selected = true;
                            if event.modifiers.intersects(KeyModifiers::SHIFT) {
                                state.curr_command.key_shift_left();
                            } else {
                                state.curr_command.key_left();
                            }
                        }
                    }
                    _ => return Ok(ApplicationTask::DoNothing),
                }
            }
        }
        Ok(ApplicationTask::UpdateUI)
    }
}

pub(crate) struct EditWidget<D> {
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
impl<'b, D: AppDatabase + Send + Sync, B: Backend> ResizableWidget<D, B> for EditWidget<D> {
    async fn prepare_render(&mut self, state: &mut UIState<D>, chunk: Rect) {
        let vchunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(chunk.height.saturating_sub(1)),
                Constraint::Length(1),
            ])
            .split(chunk);

        // Account for top table row.
        let _ = state
            .book_view
            .refresh_window_size(usize::from(vchunks[0].height).saturating_sub(1))
            .await;
    }

    fn render_into_frame(&self, f: &mut Frame<B>, state: &UIState<D>, chunk: Rect) {
        let vchunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(chunk.height - 1), Constraint::Length(1)])
            .split(chunk);

        let hchunks = split_chunk_into_columns(chunk, state.num_cols() as u16);

        let edit_style = state.style.edit_style();
        let select_style = state.style.select_style();
        let (scol, srows) = state
            .selected()
            .expect("EditWidget should only exist when items are selected");

        let srows: HashMap<_, _> = srows.into_iter().map(|(i, book)| (i, book.id())).collect();
        let style_rules = StyleRules {
            cursor: state.style.cursor_style(),
            selected: select_style,
            default: edit_style,
        };

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
        CommandWidget::new(&state.curr_command).render_into_frame(f, vchunks[1]);
    }
}

#[async_trait]
impl<D: AppDatabase + Send + Sync> InputHandler<D> for EditWidget<D> {
    async fn handle_input(
        &mut self,
        event: Event,
        state: &mut UIState<D>,
        app: &mut AppChannel<D>,
    ) -> Result<ApplicationTask, TuiError<D::Error>> {
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

pub(crate) struct HelpWidget<D> {
    pub(crate) text: ScrollableText,
    pub(crate) database: PhantomData<fn(D)>,
}

#[async_trait]
impl<'b, D: AppDatabase + Send + Sync, B: Backend> ResizableWidget<D, B> for HelpWidget<D> {
    async fn prepare_render(&mut self, _state: &mut UIState<D>, chunk: Rect) {
        let vchunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(chunk.height.saturating_sub(1)),
                Constraint::Length(1),
            ])
            .split(chunk);

        self.text
            .refresh_window_height(usize::from(vchunks[0].height));
    }

    fn render_into_frame(&self, f: &mut Frame<B>, _state: &UIState<D>, chunk: Rect) {
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

#[async_trait]
impl<D: AppDatabase + Send + Sync> InputHandler<D> for HelpWidget<D> {
    async fn handle_input(
        &mut self,
        event: Event,
        state: &mut UIState<D>,
        _app: &mut AppChannel<D>,
    ) -> Result<ApplicationTask, TuiError<D::Error>> {
        match event {
            Event::Resize(_, _) => return Ok(ApplicationTask::UpdateUI),
            Event::Mouse(m) => match m.kind {
                MouseEventKind::ScrollDown => {
                    let scroll = state.nav_settings.scroll;
                    if state.nav_settings.inverted {
                        self.text.scroll_up(scroll)
                    } else {
                        self.text.scroll_down(scroll)
                    };
                }
                MouseEventKind::ScrollUp => {
                    let scroll = state.nav_settings.scroll;
                    if state.nav_settings.inverted {
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