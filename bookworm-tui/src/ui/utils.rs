#[cfg(feature = "copypaste")]
use clipboard::{ClipboardContext, ClipboardProvider};
use tui::layout::{Constraint, Direction, Layout, Rect};
use tui::style::{Color as TColor, Modifier, Style};
use tui::text::{Span, Spans};
use unicode_truncate::UnicodeTruncateStr;

use bookworm_app::app::AppChannel;
use bookworm_app::settings::{Color, SortSettings};
use bookworm_app::{BookIndex, Command};
use bookworm_database::paginator::Selection;
use bookworm_database::AppDatabase;
use bookworm_input::user_input::CharChunks;

use crate::ui::help_strings::{help_strings, GENERAL_HELP};
use crate::ui::tui_widgets::ListItemX;
use crate::{TuiError, UIState};

pub trait TuiStyle {
    fn edit_style(&self) -> Style;

    fn select_style(&self) -> Style;

    fn cursor_style(&self) -> Style;
}

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

pub fn to_tui(c: Color) -> tui::style::Color {
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
pub fn get_clipboard_provider() -> Option<ClipboardContext> {
    ClipboardProvider::new().ok()
}

#[cfg(feature = "copypaste")]
pub fn paste_into_clipboard(a: &str) {
    if let Some(mut ctx) = get_clipboard_provider() {
        let _ = ctx.set_contents(a.to_owned());
    } else {
        // How should we handle this case?
    }
}

#[cfg(not(feature = "copypaste"))]
pub fn paste_into_clipboard(_a: &str) {}

#[cfg(feature = "copypaste")]
pub fn copy_from_clipboard() -> Option<String> {
    if let Some(mut ctx) = get_clipboard_provider() {
        ctx.get_contents().ok()
    } else {
        // How should we handle this case?
        None
    }
}

#[cfg(not(feature = "copypaste"))]
pub fn copy_from_clipboard() -> Option<String> {
    None
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
pub fn cut_word_to_fit(word: &str, max_width: usize) -> ListItemX {
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
pub fn split_chunk_into_columns(chunk: Rect, num_cols: u16) -> Vec<Rect> {
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

/// Takes the CharChunk and styles it with the provided styling rules.
pub fn char_chunks_to_styled_text(c: CharChunks, styles: StyleRules) -> Spans {
    let mut text = vec![];
    match c {
        CharChunks::Selected(before, inside, after, direction) => {
            text.push(Span::styled(
                before.iter().collect::<String>(),
                styles.default,
            ));
            match direction {
                bookworm_input::user_input::Direction::Left => match inside.split_first() {
                    None => unreachable!(),
                    Some((&cursor, rest)) => {
                        text.push(Span::styled(String::from(cursor), styles.cursor));
                        text.push(Span::styled(
                            rest.iter().collect::<String>(),
                            styles.selected,
                        ));
                        text.push(Span::styled(
                            after.iter().collect::<String>(),
                            styles.default,
                        ));
                    }
                },
                bookworm_input::user_input::Direction::Right => {
                    text.push(Span::styled(
                        inside.iter().collect::<String>(),
                        styles.selected,
                    ));
                    match after.split_first() {
                        None => {
                            text.push(Span::styled(String::from(" "), styles.cursor));
                        }
                        Some((&cursor, rest)) => {
                            text.push(Span::styled(String::from(cursor), styles.cursor));
                            text.push(Span::styled(
                                rest.iter().collect::<String>(),
                                styles.default,
                            ));
                        }
                    }
                }
            }
        }
        CharChunks::Unselected(before, after) => {
            text.push(Span::styled(before, styles.default));
            if after.is_empty() {
                text.push(Span::styled(String::from(" "), styles.cursor));
            } else {
                let (cursor, rest) = after.split_at(1);
                text.push(Span::styled(cursor.to_string(), styles.cursor));
                text.push(Span::styled(rest.to_string(), styles.default));
            }
        }
    }
    Spans::from(text)
}

#[derive(Default, Copy, Clone)]
pub struct StyleRules {
    pub default: Style,
    pub selected: Style,
    pub cursor: Style,
}

impl StyleRules {
    pub fn add_modifier(self, modifier: Modifier) -> Self {
        self.add_default_modifier(modifier)
            .add_cursor_modifier(modifier)
            .add_selected_modifier(modifier)
    }

    pub fn add_cursor_modifier(mut self, modifier: Modifier) -> Self {
        self.cursor = self.cursor.add_modifier(modifier);
        self
    }

    pub fn add_selected_modifier(mut self, modifier: Modifier) -> Self {
        self.selected = self.selected.add_modifier(modifier);
        self
    }

    pub fn add_default_modifier(mut self, modifier: Modifier) -> Self {
        self.default = self.default.add_modifier(modifier);
        self
    }

    pub fn bg(self, color: tui::style::Color) -> Self {
        self.cursor_bg(color).default_bg(color).selected_bg(color)
    }

    pub fn fg(self, color: tui::style::Color) -> Self {
        self.cursor_fg(color).default_fg(color).selected_fg(color)
    }

    pub fn cursor_bg(mut self, color: tui::style::Color) -> Self {
        self.cursor = self.cursor.bg(color);
        self
    }

    pub fn cursor_fg(mut self, color: tui::style::Color) -> Self {
        self.cursor = self.cursor.fg(color);
        self
    }

    pub fn default_bg(mut self, color: tui::style::Color) -> Self {
        self.default = self.default.bg(color);
        self
    }

    pub fn default_fg(mut self, color: tui::style::Color) -> Self {
        self.default = self.default.fg(color);
        self
    }

    pub fn selected_bg(mut self, color: tui::style::Color) -> Self {
        self.selected = self.selected.bg(color);
        self
    }

    pub fn selected_fg(mut self, color: tui::style::Color) -> Self {
        self.selected = self.selected.fg(color);
        self
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
