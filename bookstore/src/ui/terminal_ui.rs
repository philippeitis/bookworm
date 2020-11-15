use std::fs::OpenOptions;
use std::io::Write;
use std::iter::FromIterator;
use std::path::PathBuf;
#[cfg(windows)]
use std::process::Command;
use std::time::Duration;
use std::fmt;

use crossterm::event::{poll, read, Event, KeyCode};

use tui::backend::Backend;
use tui::layout::{Constraint, Direction, Layout, Rect};
use tui::style::{Modifier, Style};
use tui::text::{Span, Text};
use tui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use tui::{Frame, Terminal};

use unicase::UniCase;

use crate::book::Book;
use crate::database::{AppDatabase, DatabaseError};
use crate::parser::parser;
use crate::parser::{parse_args, BookIndex};
use crate::ui::autocomplete::AutoCompleter;
use crate::ui::settings::{InterfaceStyle, Settings, SortSettings};
use crate::ui::PageView;

use itertools::Itertools;

struct EditState {
    pub active: bool,
    pub selected: usize,
    pub started_edit: bool,
    pub orig_value: String,
    pub new_value: String
}

impl Default for EditState {
    fn default() -> Self {
        EditState {
            active: false,
            selected: 0,
            started_edit: false,
            orig_value: "".to_string(),
            new_value: "".to_string()
        }
    }
}

impl EditState {
    fn new<S: AsRef<str>>(orig_value: S, selected: usize) -> Self {
        EditState {
            active: true,
            selected,
            started_edit: false,
            orig_value: orig_value.as_ref().to_string(),
            new_value: "".to_string()
        }
    }

    fn del(&mut self) {
        if self.started_edit {
            self.new_value.pop();
        } else {
            self.new_value.clear();
        }
        self.started_edit = true;
    }

    fn push(&mut self, c: char) {
        if !self.started_edit {
            self.new_value.clear();
        }
        self.started_edit = true;
        self.new_value.push(c);
    }

    fn edit_orig(&mut self) {
        if !self.started_edit {
            self.started_edit = true;
            self.new_value = self.orig_value.clone();
        }
    }

    fn reset_orig(&mut self, orig_value: String) {
        self.started_edit = false;
        self.orig_value = orig_value;
        self.new_value.clear();
    }

    fn visible(&self) -> &str {
        if self.started_edit {
            self.new_value.as_str()
        } else {
            self.orig_value.as_str()
        }
    }
}

struct CommandString {
    char_buf: Vec<char>,
    auto_fill: Option<AutoCompleter<PathBuf>>,
    autofilled: Option<String>,
    open_end: bool,
    keep_last: bool
}

impl CommandString {
    fn vals_to_string(values: &mut dyn Iterator<Item=&(bool, std::string::String)>) -> String {
        values.
            map(|(escaped, raw_str)| {
                if *escaped {
                    let mut s = '"'.to_string();
                    s.push_str(raw_str.as_str());
                    s.push('"');
                    s
                } else {
                    raw_str.clone()
                }
        }).join(" ")
    }

    fn new() -> Self {
        CommandString {
            char_buf: vec![],
            auto_fill: None,
            autofilled: None,
            open_end: true,
            keep_last: false,
        }
    }

    fn push(&mut self, c: char) {
        self.write_back();
        self.auto_fill = None;
        self.char_buf.push(c)
    }

    fn pop(&mut self) {
        self.write_back();
        self.auto_fill = None;
        self.char_buf.pop();
        self.open_end = true;
    }

    fn write_back(&mut self) {
        if self.autofilled.is_some() {
            let v = self.get_values_autofilled();
            self.char_buf = CommandString::vals_to_string(
                &mut v.iter()
                ).chars().collect();
            self.autofilled = None;
        }
    }

    fn clear(&mut self) {
        self.auto_fill = None;
        self.autofilled = None;
        self.char_buf.clear();
    }

    fn is_empty(&self) -> bool{
        self.char_buf.is_empty()
    }

    fn refresh_autofill(&mut self) -> Result<(), ()> {
        if self.auto_fill.is_some() {
            return Ok(());
        }

        let values = self.get_values();

        let val = if let Some(val) = values.last() {
            if !val.0 && val.1.starts_with("-") && self.char_buf.last().eq(&Some(&' ')) {
                self.keep_last = true;
                "".to_string()
            } else {
                self.keep_last = false;
                val.1.clone()
            }
        } else {
            "".to_string()
        };

        self.auto_fill = Some(AutoCompleter::new(val)?);
        Ok(())
    }

    /// Runs autofill on the last value.
    fn auto_fill(&mut self, dir: bool) {
        self.open_end = false;

        if let Some(af) = self.auto_fill.as_mut() {
            let path = if dir {
                af.get_next_word_by(&|x| x.is_dir())
            } else {
                af.get_next_word()
            };

            if let Some(p) = path {
                let s = p.display().to_string();
                self.autofilled = Some(s.clone());
            }
        }
    }

    fn get_values(&self) -> Vec<(bool, String)> {
        let mut values = vec![];
        let mut escaped = false;
        let mut start = 0;
        for (end, &c) in self.char_buf.iter().enumerate() {
            match c {
                ' ' => {
                    if !escaped {
                        if start != end {
                            values.push((escaped, self.char_buf[start..end].iter().collect()));
                            start = end + 1;
                        } else {
                            start += 1;
                        }
                    }
                },
                '"' => {
                    if escaped {
                        values.push((escaped, self.char_buf[start..end].iter().collect()));
                        start = end;
                        escaped = false;
                    } else if start == end {
                        escaped = true;
                        start = end + 1;
                    }
                }
                _ => {
                }
            }
        }

        if start < self.char_buf.len() {
            values.push((escaped, self.char_buf[start..self.char_buf.len()].iter().collect()));
        }
        values
    }

    fn get_values_autofilled(&self) -> Vec<(bool, String)> {
        let mut values = self.get_values();
        if let Some(s) = &self.autofilled {
            values.pop();
            values.push((s.contains(' '), s.clone()));
        }
        values
    }

}

impl fmt::Display for CommandString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut vals = CommandString::vals_to_string(&mut self.get_values_autofilled().iter());

        if self.open_end && vals.chars().last() == Some('"') {
            vals.pop();
        }
        write!(f, "{}", vals)
    }
}

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
    available_cols: Vec<UniCase<String>>,

    curr_command: CommandString,
    books: PageView<Book>,
    sort_settings: SortSettings,
    edit: EditState,
    selected_column: usize,
    updated: bool,

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
    BookNotFound(u32),
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
        ApplicationError::DatabaseError(e)
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

impl<D: AppDatabase> App<D> {
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
        let selected_cols: Vec<_> = settings.columns.iter().map(|s|
            UniCase::new(s.clone())
        ).collect();

        let available_cols = db
            .get_available_columns()
            .into_iter()
            .flatten()
            .map(|x| UniCase::new(x))
            .collect::<Vec<_>>();

        let books = db.get_all_books().into_iter().flatten().collect();
        let column_data = (0..selected_cols.len())
            .into_iter()
            .map(|_| vec![])
            .collect();

        Ok(App {
            name: name.as_ref().to_string(),
            db,
            selected_cols,
            available_cols,
            curr_command: CommandString::new(),
            books: PageView::new(0, books),
            sort_settings: settings.sort_settings,
            updated: true,
            update_columns: ColumnUpdate::Regenerate,
            column_data,
            style: settings.interface_style,
            terminal_size: None,
            edit: EditState::default(),
            selected_column: 0
        })
    }

    /// Gets the index of the book in the internal list, if it exists. May become invalidated
    /// if changes to the list occur between reading this value and using this value.
    ///
    /// # Arguments
    ///
    /// * ` id ` - The book ID.
    fn get_book_index(&mut self, id: u32) -> Option<usize> {
        self.books
            .data()
            .iter()
            .position(|b| b.get_id() == id)
    }

    /// Deletes the book with the given ID. If deleting the book reduces the number of books such
    /// that the books no longer fill the frame, the selection is decreased so that the last book
    /// is selected.
    ///
    /// # Arguments
    ///
    /// * ` id ` - The book ID.
    ///
    /// # Error
    /// Errors if deleting the book fails for any reason.
    fn delete_book(&mut self, id: u32) -> Result<(), ApplicationError> {
        self.db.remove_book(id)?;
        self.update_columns = ColumnUpdate::Regenerate;
        if let Some(index) = self.get_book_index(id) {
            self.books.data_mut().remove(index);
            self.books.refresh();
            if let Some(s) = self.books.selected() {
                if s == self.books.data().len() && s != 0 {
                    self.books.select(Some(s - 1));
                }
            }
        }
        Ok(())
    }

    /// Gets the book with the given ID, returning None if it does not exist.
    ///
    /// # Arguments
    ///
    /// * ` id ` - The book ID.
    fn get_book_with_id(&self, id: u32) -> Option<&Book> {
        self.books.data().iter().find(|b| b.get_id() == id)
    }

    /// Adds the provided books to the internal database and adjusts sorting settings.
    ///
    /// # Arguments
    ///
    /// * ` books ` - A collection of books.
    fn add_books(&mut self, books: impl IntoIterator<Item = Book>) {
        self.books.data_mut().extend(books);
        self.sort_settings.is_sorted = false;
        self.update_columns = ColumnUpdate::Regenerate;
    }

    /// Edits the book with the given ID, updating the selected column to the new value.
    ///
    /// # Arguments
    ///
    /// * ` column ` - The value in the book to update.
    /// * ` new_value` - What to update the value to.
    ///
    /// # Errors
    /// Errors if no book with the given ID exists.
    fn edit_book_with_id<S: AsRef<str>, T: AsRef<str>>(
        &mut self,
        id: u32,
        column: S,
        new_value: T,
    ) -> Result<(), ApplicationError> {
        if let Some(mut book) = self.get_book_with_id(id).cloned() {
            let _ = book.set_column(column, new_value);
            self.update_book(&book)
        } else {
            Err(ApplicationError::BookNotFound(id))
        }
    }

    /// Edits the selected book, updating the selected column to the new value.
    ///
    /// # Arguments
    ///
    /// * ` column ` - The value in the book to update.
    /// * ` new_value` - What to update the value to.
    ///
    /// # Errors
    /// Errors if no book is selected.
    fn edit_selected_book<S: AsRef<str>, T: AsRef<str>>(
        &mut self,
        column: S,
        new_value: T,
    ) -> Result<(), ApplicationError> {
        if let Some(mut book) = self.books.selected_item().cloned() {
            let _ = book.set_column(column, new_value);
            self.update_book(&book)
        } else {
            Err(ApplicationError::NoBookSelected)
        }
    }

    /// Adds the book to the internal database if a book with the same ID does not exist,
    /// otherwise overwrites the existing book with the same id.
    ///
    /// # Arguments
    ///
    /// * ` book ` - The book to add.
    fn update_book(&mut self, book: &Book) -> Result<(), ApplicationError> {
        let id = book.get_id();
        self.db.remove_book(id)?;
        self.db.insert_book(book.clone())?;

        if let Some(index) = self.get_book_index(id) {
            self.books.data_mut()[index] = book.clone();
        } else {
            self.books.data_mut().push(book.clone());
        }

        self.sort_settings.is_sorted = false;
        self.update_columns = ColumnUpdate::Regenerate;
        Ok(())
    }

    /// Updates the required sorting settings if the column changes.
    ///
    /// # Arguments
    ///
    /// * ` word ` - The column to sort the table on.
    /// * ` reverse ` - Whether to reverse the sort.
    fn update_selected_column(&mut self, word: UniCase<String>, reverse: bool) {
        if self.selected_cols.contains(&word) {
            self.sort_settings.column = word;
            self.sort_settings.is_sorted = false;
            self.sort_settings.reverse = reverse;
            self.update_columns = ColumnUpdate::Regenerate;
        }
    }

    #[allow(dead_code)]
    fn log(&self, msg: impl AsRef<str>) -> Result<(), ApplicationError> {
        let mut file = OpenOptions::new()
            .write(true)
            .append(true)
            .create(true)
            .open("log.txt")?;
        file.write_all(format!("{}\n", msg.as_ref()).as_bytes())?;
        Ok(())
    }

    /// Updates the table data if a change occurs.
    fn update_column_data(&mut self) -> bool {
        match &self.update_columns {
            ColumnUpdate::Regenerate => {
                self.updated = true;
                self.column_data = (0..self.selected_cols.len())
                    .into_iter()
                    .map(|_| Vec::with_capacity(self.books.window_size()))
                    .collect();

                if self.books.window_size() == 0 {
                    self.update_columns = ColumnUpdate::NoUpdate;
                    return false;
                }

                let _ = self.log(format!(
                    "update_column_data(ColumnUpdate::Regenerate): {} {} {:?}",
                    0,
                    self.books.window_size(),
                    self.books.selected()
                ));

                for b in self.books.window_slice() {
                    for (column_string, column) in
                        self.selected_cols.iter().zip(self.column_data.iter_mut())
                    {
                        column.push(b.get_column_or(column_string.as_str(), ""));
                    }
                }
            }
            ColumnUpdate::AddColumn(word) => {
                self.updated = true;
                if self.available_cols.contains(&word) && !self.selected_cols.contains(&word) {
                    self.selected_cols.push(word.clone());
                    let column_string = word.as_str();
                    self.column_data.push(
                        self.books
                            .window_slice()
                            .iter()
                            .map(|book| book.get_column_or(column_string, ""))
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

        assert!(
            self.column_data[0].len() >= self.books.window_size()
                || self.column_data[0].len() == self.books.data().len(),
            format!(
                "{:?} {} {}",
                self.update_columns,
                self.column_data[0].len(),
                self.books.window_size()
            )
        );

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
        fn cut_word_to_fit(word: &String, max_len: usize) -> String {
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

        let col_width = 100 / self.selected_cols.len() as u16;
        let hchunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(
                (0..self.selected_cols.len())
                    .into_iter()
                    .map(|_| Constraint::Percentage(col_width))
                    .collect::<Vec<Constraint>>()
                    .as_ref(),
            )
            .split(chunk);

        assert!(self
            .books
            .selected()
            .map(|x| { x <= chunk.height as usize })
            .unwrap_or(true),
            format!("{:?}", self.books.selected())
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
            .highlight_style(
                if self.edit.active && i == self.selected_column {
                    edit_style
                } else {
                    select_style
                });

            let mut selected_row = ListState::default();
            selected_row.select(self.books.selected());
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
                Style::default().add_modifier(Modifier::BOLD)
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
        let field_exists = Style::default()
            .add_modifier(Modifier::BOLD);
        let field_not_provided = Style::default();

        let mut data = if let Some(t) = &book.title {
            Text::styled(t, field_exists)
        } else {
            Text::styled("No title provided", field_not_provided)
        };
        if let Some(t) = &book.authors {
            let mut s = "By: ".to_string();
            s.extend(t.join(", ").chars());
            data.extend(Text::styled(s, field_exists));
        } else {
            data.extend(Text::styled("No author provided", field_not_provided))
        };

        if let Some(columns) = book.get_extended_columns() {
            data.extend(Text::raw("\nTags provided:"));
            for (key, value) in columns.iter() {
                data.extend(Text::styled([key.as_str(), value.as_str()].join(": "), field_exists));
            }
        }

        if let Some(variants) = book.get_variants() {
            for variant in variants {
                if let Some(paths) = variant.get_paths() {
                    data.extend(Text::raw("\nVariant paths:"));
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
                                        self.column_data[self.selected_column][self.edit.selected] = self.edit.orig_value.clone();
                                        return Ok(false);
                                    }
                                    self.edit_selected_book(
                                        self.selected_cols[self.selected_column].clone(),
                                        self.edit.new_value.clone()
                                    )?;
                                    self.edit.active = false;
                                    self.sort_settings.is_sorted = false;
                                    return Ok(false);
                                }
                                KeyCode::Esc => {
                                    self.edit.active = false;
                                    self.updated = true;
                                    self.column_data[self.selected_column][self.edit.selected] = self.edit.orig_value.clone();
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
                                            self.edit_selected_book(
                                                self.selected_cols[self.selected_column].clone(),
                                                self.edit.new_value.clone()
                                            )?;
                                        }
                                        self.selected_column += 1;
                                    }
                                    self.edit.reset_orig(self.column_data[self.selected_column][self.edit.selected].clone());
                                }
                                KeyCode::Up => {
                                    if self.selected_column > 0 {
                                        if self.edit.started_edit {
                                            self.edit_selected_book(
                                                self.selected_cols[self.selected_column].clone(),
                                                self.edit.new_value.clone()
                                            )?;
                                        }
                                        self.selected_column -= 1;
                                    }
                                    self.edit.reset_orig(self.column_data[self.selected_column][self.edit.selected].clone());
                                }
                                _ => return Ok(false),
                            }
                        }
                        _ => return Ok(false),
                    }
                    self.column_data[self.selected_column][self.edit.selected] = self.edit.visible().to_string();
                } else {
                    match read()? {
                        Event::Resize(_, _) => {}
                        Event::Key(event) => {
                            // Text input
                            match event.code {
                                KeyCode::F(2) => {
                                    if let Some(x) = self.books.selected() {
                                        self.edit = EditState::new(&self.column_data[self.selected_column][x], x);
                                    }
                                }
                                KeyCode::Backspace => {
                                    self.curr_command.pop();
                                }
                                KeyCode::Char(x) => {
                                    self.curr_command.push(x);
                                }
                                KeyCode::Enter => {
                                    let args: Vec<_> = self.curr_command
                                        .get_values_autofilled().into_iter().map(|(_, a)| a).collect();

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
                                    self.books.select(None);
                                }
                                KeyCode::Delete => {
                                    if self.curr_command.is_empty() {
                                        let id = if let Some(b) = self.books.selected_item() {
                                            Some(b.get_id())
                                        } else {
                                            None
                                        };

                                        if let Some(id) = id {
                                            self.delete_book(id)?;
                                        }
                                    } else {
                                        // TODO: Add code to delete forwards
                                        //  (requires implementing cursor logic)
                                    }
                                }
                                // Scrolling
                                KeyCode::Up => {
                                    if self.books.select_up() {
                                        self.update_columns = ColumnUpdate::Regenerate;
                                    }
                                }
                                KeyCode::Down => {
                                    if self.books.select_down() {
                                        self.update_columns = ColumnUpdate::Regenerate;
                                    }
                                }
                                KeyCode::PageDown => {
                                    if self.books.page_down() {
                                        self.update_columns = ColumnUpdate::Regenerate;
                                    }
                                }
                                KeyCode::PageUp => {
                                    if self.books.page_up() {
                                        self.update_columns = ColumnUpdate::Regenerate;
                                    }
                                }
                                KeyCode::Home => {
                                    if self.books.home() {
                                        self.update_columns = ColumnUpdate::Regenerate;
                                    }
                                }
                                KeyCode::End => {
                                    if self.books.end() {
                                        self.update_columns = ColumnUpdate::Regenerate;
                                    }
                                }
                                _ => return Ok(false),
                            }
                        }
                        _ => return Ok(false),
                    }
                }
                break;
            }
        }
        self.updated = true;
        Ok(false)
    }

    /// Returns the first available path amongst the variants of the book, or None if no such
    /// path exists.
    ///
    /// # Arguments
    ///
    /// * ` book ` - The book to find a path for.
    fn get_first_book_path(book: &Book) -> Option<std::path::PathBuf> {
        if let Some(variants) = book.get_variants() {
            for variant in variants {
                if let Some(paths) = variant.get_paths() {
                    if let Some((_, path)) = paths.first() {
                        return Some(path.to_owned());
                    }
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
    fn open_book(&self, book: &Book) -> Result<(), ApplicationError> {
        if let Some(path) = Self::get_first_book_path(book) {
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
    fn open_book_in_dir(&self, book: &Book) -> Result<(), ApplicationError> {
        if let Some(path) = Self::get_first_book_path(book) {
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
    fn get_book(&self, b: BookIndex) -> Option<&Book> {
        match b {
            BookIndex::Selected => self.books.selected_item(),
            BookIndex::BookID(id) => self.get_book_with_id(id),
        }
    }

    /// Runs the command currently in the current command string. On success, returns a bool
    /// indicating whether to continue or not.
    ///
    /// # Arguments
    ///
    /// * ` command ` - The command to run.
    pub(crate) fn run_command(&mut self, command: parser::Command) -> Result<bool, ApplicationError> {
        match command {
            parser::Command::DeleteBook(b) => {
                let id = if let Some(b) = self.get_book(b) {
                    Some(b.get_id())
                } else {
                    None
                };

                if let Some(id) = id {
                    self.delete_book(id)?;
                }
            }
            parser::Command::DeleteAll => {
                let ids = self.books.data().iter().map(|b| {
                    b.get_id()
                }).collect::<Vec<_>>();
                self.db.remove_books(ids)?;
                self.books.data_mut().clear();
                self.books.refresh();
                self.update_columns = ColumnUpdate::Regenerate;
            }
            parser::Command::EditBook(b, field, new_value) => {
                match b {
                    BookIndex::Selected => self.edit_selected_book(field, new_value)?,
                    BookIndex::BookID(id) => self.edit_book_with_id(id, field, new_value)?,
                };
            }
            parser::Command::AddBookFromFile(f) => {
                let id = self.db.read_book_from_file(f);
                if let Ok(id) = id {
                    if let Some(book) = self.db.get_book(id) {
                        self.add_books(std::iter::once(book));
                    }
                }
            }
            parser::Command::AddBooksFromDir(dir) => {
                if let Ok(ids) = self.db.read_books_from_dir(dir) {
                    self.add_books(self.db.get_books(ids).into_iter().flatten());
                }
            }
            parser::Command::AddColumn(column) => {
                self.update_columns = ColumnUpdate::AddColumn(UniCase::new(column));
            }
            parser::Command::RemoveColumn(column) => {
                self.update_columns = ColumnUpdate::RemoveColumn(UniCase::new(column));
            }
            parser::Command::SortColumn(column, rev) => {
                self.update_selected_column(UniCase::new(column), rev);
            }
            #[cfg(windows)]
            parser::Command::OpenBookInApp(b) => {
                if let Some(b) = self.get_book(b) {
                    self.open_book(b)?;
                }
            }
            #[cfg(windows)]
            parser::Command::OpenBookInExplorer(b) => {
                if let Some(b) = self.get_book(b) {
                    self.open_book_in_dir(b)?;
                }
            }
            parser::Command::Write => {
                self.db.save()?;
            }
            parser::Command::Quit => {
                return Ok(false);
            }
            parser::Command::WriteAndQuit => {
                self.db.save()?;
                return Ok(false);
            }
            _ => {
                return Ok(true);
            }
        }
        Ok(true)
    }

    /// Sorts the books internally, using the current sort settings.
    fn sort_books_by_col(&mut self) {
        let col_string = self.sort_settings.column.as_str();
        if self.sort_settings.reverse {
            self.books
                .data_mut()
                .sort_by(|a, b| b.cmp_column(a, col_string))
        } else {
            self.books
                .data_mut()
                .sort_by(|a, b| a.cmp_column(b, col_string))
        };
        self.sort_settings.is_sorted = true;
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
        // self.visible_rows = terminal.size()?.height as usize;
        self.books
            .refresh_window_size(terminal.size()?.height as usize);

        loop {
            if !self.sort_settings.is_sorted {
                self.sort_books_by_col();
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
                        .constraints([
                            Constraint::Percentage(75),
                            Constraint::Percentage(25),
                        ])
                        .split(f.size());

                    let vchunks = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([
                            Constraint::Length(f.size().height - 3),
                            Constraint::Length(1),
                        ])
                        .split(hchunks[0]);

                    if vchunks[0].height != 0 {
                        if self.books.window_size() != vchunks[0].height as usize - 1 {
                            self.books
                                .refresh_window_size(vchunks[0].height as usize - 1);
                            self.update_columns = ColumnUpdate::Regenerate;

                            self.update_column_data();
                            if self.edit.active {
                                self.column_data[self.selected_column][self.edit.selected] = self.edit.visible().to_string();
                            }
                        }
                    }

                    let block = Block::default()
                        .title(format!(" bookshop || {} ", self.name))
                        .borders(Borders::ALL);
                    f.render_widget(block, f.size());

                    self.render_columns(f, vchunks[0]);
                    self.render_command_prompt(f, vchunks[1]);
                    if let Some(b) = self.books.selected_item() {
                        self.render_book_into_view(b, f, hchunks[1]);
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
//  web support - eg. directly to google drive - google_drive3
//  Conversion (using calibre?)
//  Help menu
//  Splash screen
//  New database
//  Copy books to central directory: -c flag && set dir in settings.toml
//  duplicate detection - blake3
//  add date column?
//  Convert format to media, convert book to something else
//  Infinite undo redo (!u, !r)
