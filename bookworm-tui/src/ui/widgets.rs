use std::collections::HashMap;
use std::marker::PhantomData;
use std::path::PathBuf;
use std::sync::Arc;

use crossterm::event::{Event, KeyCode, KeyModifiers, MouseEventKind};

use tui::backend::Backend;
use tui::layout::{Constraint, Direction, Layout, Rect};
use tui::style::{Color, Modifier, Style};
use tui::text::{Span, Text};
use tui::widgets::{Block, Borders, Paragraph};
use tui::Frame;
use unicode_truncate::UnicodeTruncateStr;

use bookworm_app::app::AppChannel;
use bookworm_app::parser::Source;
use bookworm_app::settings::InterfaceStyle;
use bookworm_app::{parse_args, ApplicationError, BookIndex, Command};
use bookworm_database::{AppDatabase, Book, DatabaseError};
use bookworm_input::user_input::InputRecorder;
use bookworm_input::Edit;
use bookworm_records::book::{BookID, ColumnIdentifier, RecordError};

use crate::ui::scrollable_text::{BlindOffset, ScrollableText};
use crate::ui::tui_widgets::{ListItemX, MultiSelectList, MultiSelectListState};
use crate::ui::utils::{
    char_chunks_to_styled_text, copy_from_clipboard, cut_word_to_fit, paste_into_clipboard,
    run_command, split_chunk_into_columns, to_tui, AppView, ApplicationTask, StyleRules, TuiStyle,
};
use crate::{TuiError, UIState};
use async_trait::async_trait;

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

pub(crate) trait Widget<B: Backend> {
    /// Renders the widget into the frame, using the provided space.
    ///
    /// # Arguments
    ///
    /// * ` f ` - A frame to render into.
    /// * ` chunk ` - A chunk to specify the size of the widget.
    fn render_into_frame(&self, f: &mut Frame<B>, chunk: Rect);
}

pub(crate) struct BorderWidget {
    name: String,
    path: PathBuf,
    pub(crate) saved: bool,
}

impl BorderWidget {
    pub(crate) fn new(name: String, path: PathBuf) -> Self {
        BorderWidget {
            name,
            path,
            saved: true,
        }
    }
}

impl<B: Backend> Widget<B> for BorderWidget {
    fn render_into_frame(&self, f: &mut Frame<B>, chunk: Rect) {
        let block = Block::default()
            .title(format!(
                " bookworm || {} || {}{}",
                self.name,
                self.path.display(),
                if self.saved { " " } else { " * " }
            ))
            .borders(Borders::ALL);

        f.render_widget(block, chunk);
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

    /// Returns whether the widget is currently capturing the key event.
    /// Typically returns true, but may return false if "esc" is pressed and nothing
    /// is active, leaving parent to handle it.
    fn capturing(&self, _event: &Event) -> bool {
        true
    }
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
impl<'b, D: AppDatabase + Send + Sync, B: Backend> ResizableWidget<D, B> for ColumnWidget<D> {
    // #[tracing::instrument(name = "Preparing ColumnWidgetRender", skip(self, state))]
    async fn prepare_render(&mut self, state: &mut UIState<D>, chunk: Rect) {
        // Account for column titles
        let _ = state
            .book_view
            .refresh_window_size(usize::from(chunk.height).saturating_sub(1))
            .await;
    }

    fn render_into_frame(&self, f: &mut Frame<B>, state: &UIState<D>, chunk: Rect) {
        let hchunks = split_chunk_into_columns(chunk, state.num_cols() as u16);
        //
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
}

// NOTE: This is the only place where app scrolling takes place.
#[async_trait]
impl<D: AppDatabase + Send + Sync> InputHandler<D> for ColumnWidget<D> {
    // tODO: column widget should know what is selected & if in focus
    // should split out logic for command widget and column view
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
                        if !state.book_view.selected_books().is_empty() {
                            // Parent needs to switch this with EditWidget and remove bookwidget
                            return Ok(ApplicationTask::SwitchView(AppView::Edit));
                        }
                    }
                    KeyCode::Enter => {
                        if !state.book_view.selected_books().is_empty() {
                            return Ok(ApplicationTask::SwitchView(AppView::Edit));
                        }
                    }
                    // if active widget, deactivates
                    KeyCode::Esc => {
                        state.book_view.deselect_all();
                        state.book_view.pop_scope();
                    }
                    KeyCode::Delete => {
                        if !state.book_view.selected_books().is_empty() {
                            run_command(app, Command::DeleteSelected, state).await?;
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

/// Contains information needed to render a book.
/// Only guaranteed to reflect the current state of the book if no
/// EditCommand occurs - should be regenerated during the prepare_render call
pub(crate) struct BookWidget<D> {
    chunk: Rect,
    offset: BlindOffset,
    book: Arc<Book>,
    pub(crate) database: PhantomData<fn(D)>,
}

impl<D> BookWidget<D> {
    pub fn new(chunk: Rect, book: Arc<Book>) -> Self {
        let mut book_widget = BookWidget {
            chunk,
            offset: BlindOffset::new(),
            book,
            database: PhantomData,
        };
        let height = chunk.height as usize;
        book_widget.offset.refresh_window_height(height as usize);
        book_widget
    }

    pub fn set_chunk(&mut self, chunk: Rect) {
        self.chunk = chunk;
        let height = chunk.height as usize;
        self.offset.refresh_window_height(height as usize);
        self.offset
            .fit_offset_in_height(self.to_widget_text().lines.len());
    }
    pub fn contains_point(&self, col: u16, row: u16) -> bool {
        let rect = self.chunk;
        col >= rect.x && col < rect.x + rect.width && row >= rect.y && row < rect.y + rect.height
    }

    pub fn to_widget_text(&self) -> Text {
        let width = self.chunk.width as usize;
        let field_exists = Style::default().add_modifier(Modifier::BOLD);
        let field_not_provided = Style::default();
        let prefix = match std::env::current_dir() {
            Ok(d) => d.canonicalize().ok(),
            Err(_) => None,
        };

        let mut data = if let Some(t) = self.book.title() {
            Text::styled(t.to_string(), field_exists)
        } else {
            Text::styled("No title provided", field_not_provided)
        };

        if let Some(a) = self.book.authors() {
            let mut s = String::from("By: ");
            s.push_str(&a.join(", "));
            data.extend(Text::styled(s, field_exists));
        } else {
            data.extend(Text::styled("No author provided", field_not_provided));
        }

        if let Some(d) = self.book.description() {
            data.extend(Text::styled("\n", field_exists));
            // TODO: Make this look nice in the TUI.
            data.extend(Text::raw(html2text::from_read(d.as_bytes(), width)));
        }

        let columns = self.book.tags();
        if !columns.is_empty() {
            data.extend(Text::raw("\nNamed tags provided:"));
            for (key, value) in columns.iter() {
                data.extend(Text::styled(
                    [key.as_str(), value.as_str()].join(": "),
                    field_exists,
                ));
            }
        }

        let free_tags = self.book.free_tags();
        if !free_tags.is_empty() {
            data.extend(Text::raw("\nTags provided:"));
            for value in free_tags.iter() {
                data.extend(Text::styled(value.clone(), field_exists));
            }
        }

        let variants = self.book.variants();
        if !variants.is_empty() {
            data.extend(Text::raw("\nVariant paths:"));
            for variant in variants {
                let s = format!(
                    "{:?}: {}",
                    variant.book_type(),
                    if let Some(p) = prefix.as_ref() {
                        variant
                            .path()
                            .strip_prefix(p)
                            .unwrap_or_else(|_| variant.path())
                    } else {
                        variant.path()
                    }
                    .display()
                );
                data.extend(Text::styled(s, field_exists));
            }
        }

        data
    }

    pub fn offset_mut(&mut self) -> &mut BlindOffset {
        &mut self.offset
    }
}

#[async_trait]
impl<'b, D: AppDatabase + Send + Sync, B: Backend> ResizableWidget<D, B> for BookWidget<D> {
    async fn prepare_render(&mut self, state: &mut UIState<D>, chunk: Rect) {
        // need to push to parent
        let books = state.book_view.selected_books();
        *self = books
            .front()
            .map(|book| BookWidget::new(Rect::default(), book.clone()))
            .unwrap();

        self.set_chunk(chunk);
    }

    fn render_into_frame(&self, f: &mut Frame<B>, _state: &UIState<D>, chunk: Rect) {
        let book_text = self.to_widget_text();
        let offset = self.offset.offset();
        let p = Paragraph::new(book_text).scroll((offset as u16, 0));
        f.render_widget(p, chunk);
    }
}

#[async_trait]
impl<D: AppDatabase + Send + Sync> InputHandler<D> for BookWidget<D> {
    async fn handle_input(
        &mut self,
        event: Event,
        state: &mut UIState<D>,
        _app: &mut AppChannel<D>,
    ) -> Result<ApplicationTask, TuiError<D::Error>> {
        match event {
            Event::Mouse(m) => match (m.kind, m.column, m.row) {
                (MouseEventKind::ScrollDown, c, r) => {
                    let inverted = state.nav_settings.inverted;
                    let scroll = state.nav_settings.scroll;
                    if inverted {
                        self.offset_mut().scroll_up(scroll);
                    } else {
                        self.offset_mut().scroll_down(scroll);
                    }
                }
                (MouseEventKind::ScrollUp, c, r) => {
                    let inverted = state.nav_settings.inverted;
                    let scroll = state.nav_settings.scroll;
                    if inverted {
                        self.offset_mut().scroll_down(scroll);
                    } else {
                        self.offset_mut().scroll_up(scroll);
                    }
                }
                _ => {
                    return Ok(ApplicationTask::DoNothing);
                }
            },
            _ => {
                return Ok(ApplicationTask::UpdateUI);
            }
        }
        Ok(ApplicationTask::UpdateUI)
    }
}

pub struct CommandWidget<D> {
    pub(crate) database: PhantomData<fn(D)>,
}

#[async_trait]
impl<'b, D: AppDatabase + Send + Sync, B: Backend> ResizableWidget<D, B> for CommandWidget<D> {
    async fn prepare_render(&mut self, _state: &mut UIState<D>, chunk: Rect) {}

    fn render_into_frame(&self, f: &mut Frame<B>, state: &UIState<D>, chunk: Rect) {
        let command_widget = if state.curr_command.is_empty() {
            Paragraph::new(Text::styled(
                "Enter command or search",
                Style::default().add_modifier(Modifier::BOLD),
            ))
        } else {
            let styles = StyleRules::default()
                .add_modifier(Modifier::BOLD)
                .cursor_fg(Color::Black)
                .cursor_bg(Color::White)
                .add_cursor_modifier(Modifier::SLOW_BLINK)
                .selected_fg(Color::White)
                .selected_bg(Color::Blue);

            Paragraph::new(char_chunks_to_styled_text(
                state.curr_command.char_chunks(),
                styles,
            ))
        };
        f.render_widget(command_widget, chunk);
    }
}

#[async_trait]
impl<D: AppDatabase + Send + Sync> InputHandler<D> for CommandWidget<D> {
    async fn handle_input(
        &mut self,
        event: Event,
        state: &mut UIState<D>,
        app: &mut AppChannel<D>,
    ) -> Result<ApplicationTask, TuiError<D::Error>> {
        match event {
            Event::Key(event) => {
                // Text input
                match event.code {
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
                                    state.curr_command.deselect();
                                }
                                ('a', _) => state.curr_command.select_all(),
                                _ => state.curr_command.push(x),
                            }
                        } else {
                            state.curr_command.push(x);
                        }
                    }
                    KeyCode::Enter => {
                        // TODO: Below should be part of accept and widgetbox handling
                        // if state.curr_command.is_empty() {
                        //     return if !state.book_view.selected_books().is_empty() {
                        //         Ok(ApplicationTask::SwitchView(AppView::Edit))
                        //     } else {
                        //         self.command_widget_selected = true;
                        //         Ok(ApplicationTask::UpdateUI)
                        //     };
                        // }

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
                    // if active widget isn't capturing tabs,
                    // capture tab and cycle active widgets
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
                    // if active widget, deactivates
                    KeyCode::Esc => state.curr_command.clear(),
                    KeyCode::Delete => state.curr_command.del(),
                    // Scrolling
                    KeyCode::Up => self.select_up(state, event.modifiers).await?,
                    KeyCode::Down => self.select_down(state, event.modifiers).await?,
                    KeyCode::PageDown => self.page_down(state, event.modifiers).await?,
                    KeyCode::PageUp => self.page_up(state, event.modifiers).await?,
                    KeyCode::Home => self.home(state, event.modifiers).await?,
                    KeyCode::End => self.end(state, event.modifiers).await?,
                    KeyCode::Right => {
                        if event.modifiers.intersects(KeyModifiers::SHIFT) {
                            state.curr_command.key_shift_right();
                        } else {
                            state.curr_command.key_right();
                        }
                    }

                    KeyCode::Left => {
                        if event.modifiers.intersects(KeyModifiers::SHIFT) {
                            state.curr_command.key_shift_left();
                        } else {
                            state.curr_command.key_left();
                        }
                    }
                    _ => return Ok(ApplicationTask::DoNothing),
                }
            }
            _ => return Ok(ApplicationTask::UpdateUI),
        }
        Ok(ApplicationTask::UpdateUI)
    }
}

impl<D: AppDatabase + Send + Sync> CommandWidget<D> {
    async fn page_down(
        &mut self,
        state: &mut UIState<D>,
        modifiers: KeyModifiers,
    ) -> Result<(), DatabaseError<D::Error>> {
        unimplemented!("Paging down on command widget not supported.");
    }

    async fn page_up(
        &mut self,
        state: &mut UIState<D>,
        modifiers: KeyModifiers,
    ) -> Result<(), DatabaseError<D::Error>> {
        unimplemented!("Paging up on command widget not supported.");
    }

    async fn home(
        &mut self,
        state: &mut UIState<D>,
        modifiers: KeyModifiers,
    ) -> Result<(), DatabaseError<D::Error>> {
        if modifiers.intersects(KeyModifiers::SHIFT) {
            state.curr_command.key_shift_up();
        } else {
            state.curr_command.key_up();
        }
        Ok(())
    }

    async fn end(
        &mut self,
        state: &mut UIState<D>,
        modifiers: KeyModifiers,
    ) -> Result<(), DatabaseError<D::Error>> {
        if modifiers.intersects(KeyModifiers::SHIFT) {
            state.curr_command.key_shift_down();
        } else {
            state.curr_command.key_down();
        }
        Ok(())
    }

    async fn select_up(
        &mut self,
        state: &mut UIState<D>,
        modifiers: KeyModifiers,
    ) -> Result<(), DatabaseError<D::Error>> {
        if modifiers.intersects(KeyModifiers::SHIFT) {
            state.curr_command.key_shift_up();
        } else {
            state.curr_command.key_up();
        }
        Ok(())
    }

    async fn select_down(
        &mut self,
        state: &mut UIState<D>,
        modifiers: KeyModifiers,
    ) -> Result<(), DatabaseError<D::Error>> {
        if modifiers.intersects(KeyModifiers::SHIFT) {
            state.curr_command.key_shift_down();
        } else {
            state.curr_command.key_down();
        }
        Ok(())
    }
}
