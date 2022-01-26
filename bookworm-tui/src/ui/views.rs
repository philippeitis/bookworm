use std::collections::{HashMap, VecDeque};
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
                    KeyCode::Enter => if !state.book_view.selected_books().is_empty() {
                        return Ok(ApplicationTask::SwitchView(AppView::Edit));
                    }
                    // if active widget, deactivates
                    KeyCode::Esc => {
                        state.book_view.deselect_all();
                        state.book_view.pop_scope();
                    }
                    KeyCode::Delete => if !state.book_view.selected_books().is_empty() {
                        run_command(app, Command::DeleteSelected, state).await?;
                    }                 // Scrolling
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
    Main(ColumnWidget<D>, Option<BookWidgetWrapper<D>>, CommandWidgetWrapper<D>, [BoundingBox; 3]),
    Edit(EditWidget<D>, CommandWidgetWrapper<D>, [BoundingBox; 2]),
    Help(HelpWidget<D>, BoundingBox),
}


struct WidgetBox<D: AppDatabase + Send + Sync> {
    widgets: WidgetLayout<D>,
    widget_priority: VecDeque<u8>,
}

#[async_trait]
impl<'b, D: AppDatabase + Send + Sync, B: Backend> ResizableWidget<D, B> for WidgetBox<D> {
    // #[tracing::instrument(name = "Preparing ColumnWidgetRender", skip(self, state))]
    async fn prepare_render(&mut self, state: &mut UIState<D>, chunk: Rect) {
        match &mut self.widgets {
            WidgetLayout::Main(columns, books, _, boxes) => {
                let chunk = if let Some(book_widget) = books {
                    let hchunks = Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints([Constraint::Percentage(75), Constraint::Percentage(25)])
                        .split(chunk);
                    ResizableWidget::<D, B>::prepare_render(book_widget, state, hchunks[1]).await;
                    boxes[1] = BoundingBox::new(hchunks[1]);
                    hchunks[0]
                } else {
                    boxes[1] = BoundingBox { top_left: (0, 0), bottom_right: (0, 0) };
                    chunk
                };

                let vchunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(chunk.height.saturating_sub(1)),
                        Constraint::Length(1),
                    ])
                    .split(chunk);

                ResizableWidget::<D, B>::prepare_render(columns, state, vchunks[0]).await;
                boxes[0] = BoundingBox::new(vchunks[0]);
                boxes[2] = BoundingBox::new(vchunks[1]);
            }
            WidgetLayout::Edit(edits, _, boxes) => {
                let vchunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(chunk.height.saturating_sub(1)),
                        Constraint::Length(1),
                    ])
                    .split(chunk);

                ResizableWidget::<D, B>::prepare_render(edits, state, vchunks[0]).await;
                boxes[0] = BoundingBox::new(vchunks[0]);
                boxes[1] = BoundingBox::new(vchunks[1]);
            }
            WidgetLayout::Help(_, b) => {
                *b = BoundingBox::new(chunk);
            }
        }
    }

    fn render_into_frame(&self, f: &mut Frame<B>, state: &UIState<D>, chunk: Rect) {
        match &self.widgets {
            WidgetLayout::Main(columns, books, _, boxes) => {
                let chunk = if let Some(book_widget) = books {
                    let hchunks = Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints([Constraint::Percentage(75), Constraint::Percentage(25)])
                        .split(chunk);
                    book_widget.render_into_frame(f, state, hchunks[1]);
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

                columns.render_into_frame(f, state, vchunks[0]);
                CommandWidgetWrapper { database: PhantomData }.render_into_frame(f, state, vchunks[1]);
            }
            WidgetLayout::Edit(edits, _, boxes) => {
                let vchunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(chunk.height.saturating_sub(1)),
                        Constraint::Length(1),
                    ])
                    .split(chunk);

                edits.render_into_frame(f, state, vchunks[0]);
                CommandWidgetWrapper { database: PhantomData }.render_into_frame(f, state, vchunks[1]);
            }
            WidgetLayout::Help(help, b) => {
                help.render_into_frame(f, state, chunk);
            }
        }
    }
}

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
        let vchunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(chunk.height - 1), Constraint::Length(1)])
            .split(chunk);

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

pub(crate) struct BookWidgetWrapper<D> {
    pub(crate) book_widget: BookWidget,
    pub(crate) database: PhantomData<fn(D)>,
}

#[async_trait]
impl<'b, D: AppDatabase + Send + Sync, B: Backend> ResizableWidget<D, B> for BookWidgetWrapper<D> {
    async fn prepare_render(&mut self, state: &mut UIState<D>, chunk: Rect) {
        // need to push to parent
        let books = state.book_view.selected_books();
        self.book_widget = books
            .front()
            .map(|book| BookWidget::new(Rect::default(), book.clone())).unwrap();

        self.book_widget.set_chunk(chunk);
    }

    fn render_into_frame(&self, f: &mut Frame<B>, _state: &UIState<D>, chunk: Rect) {
        self.book_widget.render_into_frame(f, chunk);
    }
}

#[async_trait]
impl<D: AppDatabase + Send + Sync> InputHandler<D> for BookWidgetWrapper<D> {
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
                        self.book_widget.offset_mut().scroll_up(scroll);
                    } else {
                        self.book_widget.offset_mut().scroll_down(scroll);
                    }
                }
                (MouseEventKind::ScrollUp, c, r) => {
                    let inverted = state.nav_settings.inverted;
                    let scroll = state.nav_settings.scroll;
                    if inverted {
                        self.book_widget.offset_mut().scroll_down(scroll);
                    } else {
                        self.book_widget.offset_mut().scroll_up(scroll);
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

struct CommandWidgetWrapper<D> {
    pub(crate) database: PhantomData<fn(D)>,
}

#[async_trait]
impl<'b, D: AppDatabase + Send + Sync, B: Backend> ResizableWidget<D, B> for CommandWidgetWrapper<D> {
    async fn prepare_render(&mut self, _state: &mut UIState<D>, chunk: Rect) {}

    fn render_into_frame(&self, f: &mut Frame<B>, _state: &UIState<D>, chunk: Rect) {}
}

#[async_trait]
impl<D: AppDatabase + Send + Sync> InputHandler<D> for CommandWidgetWrapper<D> {
    async fn handle_input(
        &mut self,
        event: Event,
        state: &mut UIState<D>,
        app: &mut AppChannel<D>,
    ) -> Result<ApplicationTask, TuiError<D::Error>> {
        match event {
            Event::Resize(_, _) => return Ok(ApplicationTask::UpdateUI),
            Event::Mouse(m) => return Ok(ApplicationTask::DoNothing),
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
                                ('c', true) => if let Some(text) = state.curr_command.selected() {
                                    paste_into_clipboard(&text.iter().collect::<String>());
                                } else {
                                    let string = state.curr_command.to_string();
                                    if !string.is_empty() {
                                        paste_into_clipboard(&string);
                                    }
                                }
                                ('x', true) => if let Some(text) = state.curr_command.selected() {
                                    paste_into_clipboard(&text.iter().collect::<String>());
                                    state.curr_command.del();
                                } else {
                                    let string = state.curr_command.to_string();
                                    if !string.is_empty() {
                                        paste_into_clipboard(&string);
                                    }
                                    state.curr_command.clear();
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
                    KeyCode::Right => if event.modifiers.intersects(KeyModifiers::SHIFT) {
                        state.curr_command.key_shift_right();
                    } else {
                        state.curr_command.key_right();
                    }

                    KeyCode::Left =>
                        if event.modifiers.intersects(KeyModifiers::SHIFT) {
                            state.curr_command.key_shift_left();
                        } else {
                            state.curr_command.key_left();
                        }
                    _ => return Ok(ApplicationTask::DoNothing),
                }
            }
        }
        Ok(ApplicationTask::UpdateUI)
    }
}

impl<D: AppDatabase + Send + Sync> CommandWidgetWrapper<D> {
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
