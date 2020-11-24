use std::fs::OpenOptions;
use std::io::Write;
use std::iter::FromIterator;
use std::time::Duration;
#[cfg(windows)]
use std::{path::PathBuf, process::Command};

use crossterm::event::{poll, read, Event, KeyCode, MouseEvent};

use tui::backend::Backend;
use tui::layout::{Constraint, Direction, Layout, Rect};
use tui::style::{Modifier, Style};
use tui::text::{Span, Text};
use tui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use tui::{Frame, Terminal};

use unicase::UniCase;

use crate::database::{DatabaseError, SelectableDatabase};
use crate::parser::command_parser;
use crate::parser::{parse_args, BookIndex};
use crate::record::book::ColumnIdentifier;
use crate::record::Book;
use crate::ui::settings::{InterfaceStyle, NavigationSettings, Settings, SortSettings};
use crate::ui::user_input::{CommandString, EditState};

// TODO: Add MoveUp / MoveDown for stepping up and down so we don't
//      regenerate everything from scratch.
#[derive(Debug, Clone, Eq, PartialEq)]
enum ColumnUpdate {
    Regenerate,
    AddColumn(UniCase<String>),
    RemoveColumn(UniCase<String>),
    NoUpdate,
}

pub(crate) struct App<D> {
    name: String,
    db: D,

    selected_cols: Vec<UniCase<String>>,
    curr_command: CommandString,
    sort_settings: SortSettings,
    edit: EditState,
    selected_column: usize,
    updated: bool,

    // Navigation
    nav_settings: NavigationSettings,

    // Database
    update_columns: ColumnUpdate,
    column_data: Vec<Vec<String>>,
    style: InterfaceStyle,
    terminal_size: Option<(u16, u16)>,
}

#[derive(Debug)]
pub(crate) enum ApplicationError {
    IoError(std::io::Error),
    TerminalError(crossterm::ErrorKind),
    DatabaseError(DatabaseError),
    NoBookSelected,
    Err(()),
}

impl From<std::io::Error> for ApplicationError {
    fn from(e: std::io::Error) -> Self {
        ApplicationError::IoError(e)
    }
}

impl From<()> for ApplicationError {
    fn from(_: ()) -> Self {
        ApplicationError::Err(())
    }
}

impl From<DatabaseError> for ApplicationError {
    fn from(e: DatabaseError) -> Self {
        match e {
            DatabaseError::NoBookSelected => ApplicationError::NoBookSelected,
            x => ApplicationError::DatabaseError(x),
        }
    }
}

impl From<crossterm::ErrorKind> for ApplicationError {
    fn from(e: crossterm::ErrorKind) -> Self {
        ApplicationError::TerminalError(e)
    }
}

// TODO: Currently unstable.
// impl From<std::option::NoneError> for ApplicationError {
//     fn from(e: std::option::NoneError) -> Self {
//     }
// }

impl<D: SelectableDatabase> App<D> {
    // pub(crate) fn splash(style: InterfaceStyle, terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<App<'a, D>, ApplicationError> {
    //     terminal.draw(|f| {
    //         let vchunks = Layout::default()
    //             .margin(1)
    //             .direction(Direction::Horizontal)
    //             .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
    //             .split(f.size());
    //
    //         let block = Block::default()
    //             .title(format!(" bookshop || Open or create new library "))
    //             .borders(Borders::ALL);
    //
    //         f.render_widget(block, f.size());
    //
    //         let h = (f.size().height - 2)/2;
    //
    //         let hchunks = Layout::default()
    //             .direction(Direction::Horizontal)
    //             .constraints([Constraint::Length(h), Constraint::Length(1), Constraint::Length(1), Constraint::Length(h)])
    //             .split(vchunks[0]);
    //
    //         let open = Span::from("Open existing database");
    //         let close = Span::from("Open new database");
    //
    //         Self::new(file_name, style, D::open(file_name));
    //     })?
    // }
    /// Returns a new database, instantiated with the provided settings and database.
    ///
    /// # Arguments
    ///
    /// * ` name ` - The application instance name. Not to confused with the file name.
    /// * ` settings` - The application settings.
    /// * ` db ` - The database which contains books to be read.
    ///
    /// # Errors
    /// None.
    pub(crate) fn new<S: AsRef<str>>(
        name: S,
        settings: Settings,
        db: D,
    ) -> Result<App<D>, ApplicationError> {
        let selected_cols: Vec<_> = settings
            .columns
            .iter()
            .map(|s| UniCase::new(s.clone()))
            .collect();

        let column_data = (0..selected_cols.len())
            .into_iter()
            .map(|_| vec![])
            .collect();

        Ok(App {
            name: name.as_ref().to_string(),
            db,
            selected_cols,
            curr_command: CommandString::new(),
            sort_settings: settings.sort_settings,
            updated: true,
            update_columns: ColumnUpdate::Regenerate,
            column_data,
            style: settings.interface_style,
            terminal_size: None,
            edit: EditState::default(),
            selected_column: 0,
            nav_settings: settings.navigation_settings,
        })
    }

    /// Updates the required sorting settings if the column changes.
    ///
    /// # Arguments
    ///
    /// * ` word ` - The column to sort the table on.
    /// * ` reverse ` - Whether to reverse the sort.
    fn update_selected_column(&mut self, word: UniCase<String>, reverse: bool) {
        let word = UniCase::new(
            match word.as_str() {
                "author" => "authors",
                x => x,
            }
            .to_string(),
        );

        if self.selected_cols.contains(&word) {
            self.sort_settings.column = word;
            self.sort_settings.is_sorted = false;
            self.sort_settings.reverse = reverse;
            self.update_columns = ColumnUpdate::Regenerate;
        }
    }

    fn log(&self, msg: impl AsRef<str>) -> Result<(), ApplicationError> {
        let mut file = OpenOptions::new()
            .write(true)
            .append(true)
            .create(true)
            .open("log.txt")?;
        writeln!(file, "{}", msg.as_ref())?;
        Ok(())
    }

    /// Updates the table data if a change occurs.
    fn update_column_data(&mut self) -> bool {
        match &self.update_columns {
            ColumnUpdate::Regenerate => {
                self.updated = true;
                self.column_data = (0..self.selected_cols.len())
                    .into_iter()
                    .map(|_| Vec::with_capacity(self.db.cursor().window_size()))
                    .collect();

                if self.db.cursor().window_size() == 0 {
                    self.update_columns = ColumnUpdate::NoUpdate;
                    return false;
                }

                let _ = self.log(format!(
                    "update_column_data(ColumnUpdate::Regenerate): {} {} {:?}",
                    0,
                    self.db.cursor().window_size(),
                    self.db.selected()
                ));

                let cols = self
                    .selected_cols
                    .iter()
                    .map(|col| ColumnIdentifier::from(col.as_str()))
                    .collect::<Vec<_>>();

                for b in self.db.get_books_cursored().unwrap() {
                    for (col, column) in cols.iter().zip(self.column_data.iter_mut()) {
                        column.push(b.get_column_or(&col, ""));
                    }
                }
            }
            ColumnUpdate::AddColumn(word) => {
                self.updated = true;
                if self.db.has_column(&word) && !self.selected_cols.contains(&word) {
                    self.selected_cols.push(word.clone());
                    let column_string = ColumnIdentifier::from(word.as_str());
                    self.column_data.push(
                        self.db
                            .get_books_cursored()
                            .unwrap()
                            .iter()
                            .map(|book| book.get_column_or(&column_string, ""))
                            .collect(),
                    );
                }
            }
            ColumnUpdate::RemoveColumn(word) => {
                self.updated = true;
                let index = self.selected_cols.iter().position(|x| x.eq(&word));
                if let Some(index) = index {
                    self.selected_cols.remove(index);
                    self.column_data.remove(index);
                }
            }
            ColumnUpdate::NoUpdate => {}
        }

        if self.update_columns != ColumnUpdate::NoUpdate {
            self.update_columns = ColumnUpdate::NoUpdate;
            true
        } else {
            false
        }
    }

    /// Renders the table, sized according to the chunk.
    ///
    /// # Arguments
    ///
    /// * ` f ` - A frame to render into.
    /// * ` chunk ` - A chunk to specify the visible table size.
    fn render_columns<B: Backend>(&mut self, f: &mut Frame<B>, chunk: Rect) {
        fn cut_word_to_fit(word: &str, max_len: usize) -> String {
            if word.len() > max_len {
                let mut base_word = word.chars().into_iter().collect::<Vec<_>>();
                base_word.truncate(max_len - 3);
                String::from_iter(base_word.iter()) + "..."
            } else {
                word.to_string()
            }
        }

        // fn columns_to_rows<T: Clone>(columns: &Vec<Vec<T>>) -> Vec<Vec<T>> {
        //     let x = columns[0].len();
        //     assert!(columns.iter().all(|v| v.len() == x));
        //     (0..x).map(|i| columns.iter().map(|x| x[i].clone()).collect()).collect()
        // }

        let col_width = chunk.width / self.selected_cols.len() as u16;
        let mut widths: Vec<_> = std::iter::repeat(col_width)
            .take(self.selected_cols.len())
            .collect();
        let total_w: u16 = widths.iter().sum();
        if total_w != chunk.width {
            widths[0] += chunk.width - total_w;
        }
        let hchunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(
                widths
                    .into_iter()
                    .map(Constraint::Length)
                    .collect::<Vec<Constraint>>()
                    .as_ref(),
            )
            .split(chunk);

        // TODO: Remove this assert.
        assert!(
            self.db
                .selected()
                .map(|x| { x <= chunk.height as usize })
                .unwrap_or(true),
            format!("{:?}", self.db.selected())
        );

        let edit_style = Style::default()
            .fg(self.style.edit_fg)
            .bg(self.style.edit_bg);
        let select_style = Style::default()
            .fg(self.style.selected_fg)
            .bg(self.style.selected_bg);

        for (i, ((title, data), &chunk)) in self
            .selected_cols
            .iter()
            .zip(self.column_data.iter())
            .zip(hchunks.iter())
            .enumerate()
        {
            let list = List::new(
                data.iter()
                    .map(|word| {
                        ListItem::new(Span::from(cut_word_to_fit(word, chunk.width as usize - 3)))
                    })
                    .collect::<Vec<_>>(),
            )
            .block(Block::default().title(Span::from(title.to_string())))
            .highlight_style(if self.edit.active && i == self.selected_column {
                edit_style
            } else {
                select_style
            });

            let mut selected_row = ListState::default();
            selected_row.select(self.db.selected());
            f.render_stateful_widget(list, chunk, &mut selected_row);
        }
    }

    /// Renders the command string into the frame, sized according to the chunk.
    ///
    /// # Arguments
    ///
    /// * ` f ` - A frame to render into.
    /// * ` chunk ` - A chunk to specify the command string size.
    fn render_command_prompt<B: Backend>(&mut self, f: &mut Frame<B>, chunk: Rect) {
        let command_widget = if !self.curr_command.is_empty() {
            // TODO: Slow blink looks wrong
            let text = Text::styled(
                self.curr_command.to_string(),
                Style::default().add_modifier(Modifier::BOLD),
            );
            Paragraph::new(text)
        } else {
            Paragraph::new(Text::styled(
                "Enter command or search",
                Style::default().add_modifier(Modifier::BOLD),
            ))
        };
        f.render_widget(command_widget, chunk);
    }

    fn render_book_into_view<B: Backend>(&self, book: &Book, f: &mut Frame<B>, chunk: Rect) {
        let field_exists = Style::default().add_modifier(Modifier::BOLD);
        let field_not_provided = Style::default();

        let mut data = if let Some(t) = &book.title {
            Text::styled(t, field_exists)
        } else {
            Text::styled("No title provided", field_not_provided)
        };
        if let Some(t) = &book.authors {
            let mut s = "By: ".to_string();
            s.push_str(&t.join(", "));
            data.extend(Text::styled(s, field_exists));
        } else {
            data.extend(Text::styled("No author provided", field_not_provided))
        };

        if let Some(columns) = book.get_extended_columns() {
            data.extend(Text::raw("\nTags provided:"));
            for (key, value) in columns.iter() {
                data.extend(Text::styled(
                    [key.as_str(), value.as_str()].join(": "),
                    field_exists,
                ));
            }
        }

        if let Some(variants) = book.get_variants() {
            let mut added_section = false;
            for variant in variants {
                if let Some(paths) = variant.get_paths() {
                    if !added_section {
                        added_section = true;
                        data.extend(Text::raw("\nVariant paths:"));
                    }
                    for (booktype, path) in paths {
                        let s = format!("{:?}: {}", booktype, path.display().to_string());
                        data.extend(Text::styled(s, field_exists));
                    }
                }
            }
        }

        let p = Paragraph::new(data);

        f.render_widget(p, chunk);
    }

    // TODO: Make this set an Action so that the handling is external.
    /// Reads and handles user input. On success, returns a bool
    /// indicating whether to continue or not.
    ///
    /// # Arguments
    ///
    /// * ` terminal ` - The current terminal.
    ///
    /// # Errors
    /// This function may error if executing a particular action fails.
    fn get_input(&mut self) -> Result<bool, ApplicationError> {
        loop {
            if poll(Duration::from_millis(500))? {
                if self.edit.active {
                    match read()? {
                        Event::Resize(_, _) => {}
                        Event::Key(event) => {
                            match event.code {
                                KeyCode::Backspace => {
                                    self.edit.del();
                                }
                                KeyCode::Char(c) => {
                                    self.edit.push(c);
                                }
                                KeyCode::Enter => {
                                    self.updated = true;
                                    if !self.edit.started_edit {
                                        self.edit.active = false;
                                        self.column_data[self.selected_column]
                                            [self.edit.selected] = self.edit.orig_value.clone();
                                        return Ok(false);
                                    }
                                    self.db.edit_selected_book(
                                        self.selected_cols[self.selected_column].clone(),
                                        self.edit.new_value.clone(),
                                    )?;
                                    self.edit.active = false;
                                    self.sort_settings.is_sorted = false;
                                    return Ok(false);
                                }
                                KeyCode::Esc => {
                                    self.edit.active = false;
                                    self.updated = true;
                                    self.column_data[self.selected_column][self.edit.selected] =
                                        self.edit.orig_value.clone();
                                    return Ok(false);
                                }
                                KeyCode::Delete => {
                                    // TODO: Add code to delete forwards
                                    //  (requires implementing cursor logic)
                                }
                                KeyCode::Right => {
                                    self.edit.edit_orig();
                                }
                                KeyCode::Down => {
                                    if self.selected_column < self.selected_cols.len() - 1 {
                                        if self.edit.started_edit {
                                            self.db.edit_selected_book(
                                                self.selected_cols[self.selected_column].clone(),
                                                self.edit.new_value.clone(),
                                            )?;
                                        }
                                        self.selected_column += 1;
                                    }
                                    self.edit.reset_orig(
                                        self.column_data[self.selected_column][self.edit.selected]
                                            .clone(),
                                    );
                                }
                                KeyCode::Up => {
                                    if self.selected_column > 0 {
                                        if self.edit.started_edit {
                                            self.db.edit_selected_book(
                                                self.selected_cols[self.selected_column].clone(),
                                                self.edit.new_value.clone(),
                                            )?;
                                        }
                                        self.selected_column -= 1;
                                    }
                                    self.edit.reset_orig(
                                        self.column_data[self.selected_column][self.edit.selected]
                                            .clone(),
                                    );
                                }
                                _ => return Ok(false),
                            }
                        }
                        _ => return Ok(false),
                    }
                    self.column_data[self.selected_column][self.edit.selected] =
                        self.edit.visible().to_string();
                } else {
                    match read()? {
                        Event::Mouse(m) => match m {
                            MouseEvent::ScrollDown(_, _, _) => {
                                let updated = if self.nav_settings.inverted {
                                    self.db.cursor_mut().scroll_up(self.nav_settings.scroll)
                                } else {
                                    self.db.cursor_mut().scroll_down(self.nav_settings.scroll)
                                };
                                if updated {
                                    self.update_columns = ColumnUpdate::Regenerate;
                                }
                            }
                            MouseEvent::ScrollUp(_, _, _) => {
                                let updated = if self.nav_settings.inverted {
                                    self.db.cursor_mut().scroll_down(self.nav_settings.scroll)
                                } else {
                                    self.db.cursor_mut().scroll_up(self.nav_settings.scroll)
                                };
                                if updated {
                                    self.update_columns = ColumnUpdate::Regenerate;
                                }
                            }
                            _ => {
                                return Ok(false);
                            }
                        },
                        Event::Resize(_, _) => {}
                        Event::Key(event) => {
                            // Text input
                            match event.code {
                                KeyCode::F(2) => {
                                    if let Some(x) = self.db.selected() {
                                        self.edit = EditState::new(
                                            &self.column_data[self.selected_column][x],
                                            x,
                                        );
                                    }
                                }
                                KeyCode::Backspace => {
                                    self.curr_command.pop();
                                }
                                KeyCode::Char(x) => {
                                    self.curr_command.push(x);
                                }
                                KeyCode::Enter => {
                                    let args: Vec<_> = self
                                        .curr_command
                                        .get_values_autofilled()
                                        .into_iter()
                                        .map(|(_, a)| a)
                                        .collect();

                                    if !self.run_command(parse_args(&args))? {
                                        return Ok(true);
                                    }
                                    self.curr_command.clear();
                                }
                                KeyCode::Tab | KeyCode::BackTab => {
                                    self.curr_command.refresh_autofill()?;
                                    let vals = self.curr_command.get_values();
                                    if let Some(val) = vals.get(0) {
                                        if val.1 == "!a" {
                                            let dir = if let Some(val) = vals.get(1) {
                                                val.1 == "-d"
                                            } else {
                                                false
                                            };
                                            self.curr_command.auto_fill(dir);
                                        }
                                    };
                                }
                                KeyCode::Esc => {
                                    self.curr_command.clear();
                                    self.db.cursor_mut().select(None);
                                }
                                KeyCode::Delete => {
                                    if self.curr_command.is_empty() {
                                        self.db.remove_selected_book()?;
                                        self.update_columns = ColumnUpdate::Regenerate;
                                    } else {
                                        // TODO: Add code to delete forwards
                                        //  (requires implementing cursor logic)
                                    }
                                }
                                // Scrolling
                                KeyCode::Up => {
                                    if self.db.cursor_mut().select_up() {
                                        self.update_columns = ColumnUpdate::Regenerate;
                                    }
                                }
                                KeyCode::Down => {
                                    if self.db.cursor_mut().select_down() {
                                        self.update_columns = ColumnUpdate::Regenerate;
                                    }
                                }
                                KeyCode::PageDown => {
                                    if self.db.cursor_mut().page_down() {
                                        self.update_columns = ColumnUpdate::Regenerate;
                                    }
                                }
                                KeyCode::PageUp => {
                                    if self.db.cursor_mut().page_up() {
                                        self.update_columns = ColumnUpdate::Regenerate;
                                    }
                                }
                                KeyCode::Home => {
                                    if self.db.cursor_mut().home() {
                                        self.update_columns = ColumnUpdate::Regenerate;
                                    }
                                }
                                KeyCode::End => {
                                    if self.db.cursor_mut().end() {
                                        self.update_columns = ColumnUpdate::Regenerate;
                                    }
                                }
                                _ => return Ok(false),
                            }
                        }
                    }
                }
                break;
            }
        }
        self.updated = true;
        Ok(false)
    }

    #[cfg(windows)]
    /// Returns the first available path amongst the variants of the book, or None if no such
    /// path exists.
    ///
    /// # Arguments
    ///
    /// * ` book ` - The book to find a path for.
    fn get_book_path(book: &Book, index: usize) -> Option<PathBuf> {
        let mut seen = 0;
        if let Some(variants) = book.get_variants() {
            for variant in variants {
                if let Some(paths) = variant.get_paths() {
                    if let Some((_, path)) = paths.get(index - seen) {
                        return Some(path.to_owned());
                    }
                    seen += paths.len();
                }
            }
        }
        None
    }

    #[cfg(windows)]
    /// Opens the book in SumatraPDF on Windows.
    /// Other operating systems not currently supported
    ///
    /// # Arguments
    ///
    /// * ` book ` - The book to open.
    ///
    /// # Errors
    /// This function may error if the book's variants do not exist,
    /// or if the command itself fails.
    fn open_book(&self, book: &Book, index: usize) -> Result<(), ApplicationError> {
        if let Some(path) = Self::get_book_path(book, index) {
            Command::new("cmd.exe")
                .args(&["/C", "start", "sumatrapdf"][..])
                .arg(path)
                .spawn()?;
        }
        Ok(())
    }

    #[cfg(windows)]
    /// Opens the book and selects it, in File Explorer on Windows.
    /// Other operating systems not currently supported
    ///
    /// # Arguments
    ///
    /// * ` book ` - The book to open.
    ///
    /// # Errors
    /// This function may error if the book's variants do not exist,
    /// or if the command itself fails.
    fn open_book_in_dir(&self, book: &Book, index: usize) -> Result<(), ApplicationError> {
        // TODO: This doesn't work when run with install due to relative paths.
        if let Some(path) = Self::get_book_path(book, index) {
            let open_book_path = PathBuf::from(".\\src\\open_book.py").canonicalize()?;
            // TODO: Find a way to do this entirely in Rust
            Command::new("python")
                .args(&[
                    open_book_path.display().to_string().as_str(),
                    path.display().to_string().as_str(),
                ])
                .spawn()?;
        }
        Ok(())
    }

    /// Gets the book that selected by the BookIndex,
    /// or None if the particular book does not exist.
    ///
    /// # Arguments
    ///
    /// * ` b ` - A BookIndex which either represents an exact ID, or the selected book.
    fn get_book(&self, b: BookIndex) -> Result<Book, ApplicationError> {
        match b {
            BookIndex::Selected => Ok(self.db.selected_item()?),
            BookIndex::BookID(id) => Ok(self.db.get_book(id)?),
        }
    }

    /// Runs the command currently in the current command string. On success, returns a bool
    /// indicating whether to continue or not.
    ///
    /// # Arguments
    ///
    /// * ` command ` - The command to run.
    pub(crate) fn run_command(
        &mut self,
        command: command_parser::Command,
    ) -> Result<bool, ApplicationError> {
        match command {
            command_parser::Command::DeleteBook(b) => {
                match b {
                    BookIndex::Selected => self.db.remove_selected_book()?,
                    BookIndex::BookID(id) => self.db.remove_book(id)?,
                };
                self.update_columns = ColumnUpdate::Regenerate;
            }
            command_parser::Command::DeleteAll => {
                self.db.clear()?;
                self.column_data.iter_mut().for_each(|c| c.clear());
                self.update_columns = ColumnUpdate::Regenerate;
            }
            command_parser::Command::EditBook(b, field, new_value) => {
                match b {
                    BookIndex::Selected => self.db.edit_selected_book(field, new_value)?,
                    BookIndex::BookID(id) => self.db.edit_book_with_id(id, field, new_value)?,
                };
            }
            command_parser::Command::AddBookFromFile(f) => {
                self.db.read_book_from_file(f)?;
                self.sort_settings.is_sorted = false;
                self.update_columns = ColumnUpdate::Regenerate;
            }
            command_parser::Command::AddBooksFromDir(dir) => {
                // TODO: Handle failed reads.
                self.db.read_books_from_dir(dir)?;
                self.sort_settings.is_sorted = false;
                self.update_columns = ColumnUpdate::Regenerate;
            }
            command_parser::Command::AddColumn(column) => {
                self.update_columns = ColumnUpdate::AddColumn(UniCase::new(column));
            }
            command_parser::Command::RemoveColumn(column) => {
                self.update_columns = ColumnUpdate::RemoveColumn(UniCase::new(column));
            }
            command_parser::Command::SortColumn(column, rev) => {
                self.update_selected_column(UniCase::new(column), rev);
            }
            #[cfg(windows)]
            command_parser::Command::OpenBookInApp(b, index) => {
                if let Ok(b) = self.get_book(b) {
                    self.open_book(&b, index)?;
                }
            }
            #[cfg(windows)]
            command_parser::Command::OpenBookInExplorer(b, index) => {
                if let Ok(b) = self.get_book(b) {
                    self.open_book_in_dir(&b, index)?;
                }
            }
            command_parser::Command::Write => {
                self.db.save()?;
            }
            command_parser::Command::Quit => {
                return Ok(false);
            }
            command_parser::Command::WriteAndQuit => {
                self.db.save()?;
                return Ok(false);
            }
            command_parser::Command::TryMergeAllBooks => {
                self.db.merge_similar()?;
                self.update_columns = ColumnUpdate::Regenerate;
            }
            _ => {
                return Ok(true);
            }
        }
        Ok(true)
    }

    /// Runs the application - including handling user inputs and refreshing the output.
    ///
    /// # Arguments
    ///
    /// * ` terminal ` - The terminal to output text to.
    ///
    /// # Errors
    /// This function will return an error if running the program fails for any reason.
    pub(crate) fn run<B: Backend>(
        mut self,
        terminal: &mut Terminal<B>,
    ) -> Result<(), ApplicationError> {
        self.db
            .cursor_mut()
            .refresh_window_size(terminal.size()?.height as usize);

        loop {
            if !self.sort_settings.is_sorted {
                self.db.sort_books_by_col(
                    self.sort_settings.column.as_str(),
                    self.sort_settings.reverse,
                )?;
                self.sort_settings.is_sorted = true;
            }

            self.update_column_data();

            let frame_size = terminal.get_frame().size();
            let size = Some((frame_size.width, frame_size.height));

            if size != self.terminal_size {
                self.terminal_size = size;
                self.updated = true;
            }

            if self.updated {
                terminal.draw(|f| {
                    let hchunks = Layout::default()
                        .margin(1)
                        .direction(Direction::Horizontal)
                        .constraints([Constraint::Percentage(75), Constraint::Percentage(25)])
                        .split(f.size());

                    let vchunks = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([
                            Constraint::Length(f.size().height - 3),
                            Constraint::Length(1),
                        ])
                        .split(hchunks[0]);

                    let curr_height = vchunks[0].height as usize;
                    if curr_height != 0 && self.db.cursor().window_size() != curr_height - 1 {
                        self.db
                            .cursor_mut()
                            .refresh_window_size(vchunks[0].height as usize - 1);
                        self.update_columns = ColumnUpdate::Regenerate;

                        self.update_column_data();
                        if self.edit.active {
                            self.column_data[self.selected_column][self.edit.selected] =
                                self.edit.visible().to_string();
                        }
                    }

                    let block = Block::default()
                        .title(format!(" bookshop || {} ", self.name))
                        .borders(Borders::ALL);
                    f.render_widget(block, f.size());

                    self.render_columns(f, vchunks[0]);
                    self.render_command_prompt(f, vchunks[1]);
                    if let Ok(b) = self.db.selected_item() {
                        self.render_book_into_view(&b, f, hchunks[1]);
                    }
                })?;
                self.updated = false;
            }
            if self.get_input()? {
                terminal.clear()?;
                return Ok(());
            }
        }
    }
}

// TODO:
//  Live search & search by tags - mysql? meillisearch?
//  Cloud sync support (eg. upload database to Google Drive / read from Google Drive)
//  File conversion (mainly using calibre?)
//  Help menu
//  Splash screen
//  New database button / screen
//  Copy books to central directory: -c flag && set dir in settings.toml
//  Duplicate detection - use blake3 to hash first 4kb or something?
//  Add automatic date column?
//  Convert format to media, convert book to something else
//  Infinite undo redo (!u, !r)
//  Pop-up notifications
//  Documentation
//  Testing
