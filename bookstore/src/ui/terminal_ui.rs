use std::time::Duration;
#[cfg(windows)]
use std::{path::PathBuf, process::Command};

use crossterm::event::{poll, read};

use tui::backend::Backend;
use tui::layout::Rect;
use tui::Terminal;

use unicase::UniCase;

use crate::database::{DatabaseError, SelectableDatabase};
use crate::parser::command_parser;
use crate::parser::BookIndex;
use crate::record::book::ColumnIdentifier;
use crate::record::Book;
use crate::ui::settings::{InterfaceStyle, NavigationSettings, Settings, SortSettings};
use crate::ui::user_input::{CommandString, EditState};
use crate::ui::views::{AppView, ApplicationTask, ColumnWidget, EditWidget, View};
use crate::ui::widgets::{BorderWidget, Widget};

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) enum ColumnUpdate {
    Regenerate,
    AddColumn(UniCase<String>),
    RemoveColumn(UniCase<String>),
    NoUpdate,
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

pub(crate) struct App<D> {
    // Application data
    db: D,
    selected_cols: Vec<UniCase<String>>,
    column_data: Vec<Vec<String>>,

    // Actions to run on data
    sort_settings: SortSettings,
    pub(crate) update_columns: ColumnUpdate,
    updated: bool,
}

impl<D: SelectableDatabase> App<D> {
    pub(crate) fn new(db: D) -> Self {
        App {
            db,
            selected_cols: vec![],
            sort_settings: SortSettings::default(),
            updated: true,
            update_columns: ColumnUpdate::Regenerate,
            column_data: vec![],
        }
    }

    pub(crate) fn update_value<S: AsRef<str>>(&mut self, col: usize, row: usize, new_value: S) {
        self.updated = true;
        self.column_data[col][row] = new_value.as_ref().to_string();
    }

    pub(crate) fn get_value(&self, col: usize, row: usize) -> &str {
        &self.column_data[col][row]
    }

    pub(crate) fn selected_item(&self) -> Result<Book, DatabaseError> {
        self.db.selected_item()
    }

    pub(crate) fn edit_selected_book_with_column<S: AsRef<str>>(
        &mut self,
        column: usize,
        new_value: S,
    ) -> Result<(), DatabaseError> {
        self.db
            .edit_selected_book(&self.selected_cols[column], new_value)?;
        self.updated = true;
        Ok(())
    }

    pub(crate) fn remove_selected_book(&mut self) -> Result<(), DatabaseError> {
        self.db.remove_selected_book()?;
        self.updated = true;
        self.update_columns = ColumnUpdate::Regenerate;
        Ok(())
    }

    pub(crate) fn selected(&self) -> Option<usize> {
        self.db.selected()
    }

    /// Gets the book that selected by the BookIndex,
    /// or None if the particular book does not exist.
    ///
    /// # Arguments
    ///
    /// * ` b ` - A BookIndex which either represents an exact ID, or the selected book.
    pub(crate) fn get_book(&self, b: BookIndex) -> Result<Book, ApplicationError> {
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
                    BookIndex::Selected => self.remove_selected_book()?,
                    BookIndex::BookID(id) => self.db.remove_book(id)?,
                };
                self.update_columns = ColumnUpdate::Regenerate;
            }
            command_parser::Command::DeleteAll => {
                self.db.clear()?;
                self.update_columns = ColumnUpdate::Regenerate;
            }
            command_parser::Command::EditBook(b, field, new_value) => {
                match b {
                    BookIndex::Selected => self.db.edit_selected_book(field, new_value)?,
                    BookIndex::BookID(id) => self.db.edit_book_with_id(id, field, new_value)?,
                };
                self.sort_settings.is_sorted = false;
                self.update_columns = ColumnUpdate::Regenerate;
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

    /// Updates the table data if a change occurs.
    pub(crate) fn update_column_data(&mut self) {
        match &self.update_columns {
            ColumnUpdate::Regenerate => {
                self.updated = true;
                self.column_data = (0..self.selected_cols.len())
                    .into_iter()
                    .map(|_| Vec::with_capacity(self.db.cursor().window_size()))
                    .collect();

                if self.db.cursor().window_size() == 0 {
                    self.update_columns = ColumnUpdate::NoUpdate;
                    return;
                }

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

        self.update_columns = ColumnUpdate::NoUpdate;
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
        if let Some(path) = App::<D>::get_book_path(book, index) {
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

    pub(crate) fn scroll_up(&mut self, scroll: usize) -> bool {
        if self.db.cursor_mut().scroll_up(scroll) {
            self.updated = true;
            self.update_columns = ColumnUpdate::Regenerate;
            true
        } else {
            false
        }
    }

    pub(crate) fn scroll_down(&mut self, scroll: usize) -> bool {
        if self.db.cursor_mut().scroll_down(scroll) {
            self.updated = true;
            self.update_columns = ColumnUpdate::Regenerate;
            true
        } else {
            false
        }
    }

    pub(crate) fn deselect(&mut self) -> bool {
        if self.db.cursor_mut().select(None) {
            self.updated = true;
            true
        } else {
            false
        }
    }

    pub(crate) fn select_down(&mut self) -> bool {
        if self.db.cursor_mut().select_down() {
            self.updated = true;
            self.update_columns = ColumnUpdate::Regenerate;
            true
        } else {
            false
        }
    }

    pub(crate) fn select_up(&mut self) -> bool {
        if self.db.cursor_mut().select_up() {
            self.updated = true;
            self.update_columns = ColumnUpdate::Regenerate;
            true
        } else {
            false
        }
    }

    pub(crate) fn page_up(&mut self) -> bool {
        if self.db.cursor_mut().page_up() {
            self.updated = true;
            self.update_columns = ColumnUpdate::Regenerate;
            true
        } else {
            false
        }
    }

    pub(crate) fn page_down(&mut self) -> bool {
        if self.db.cursor_mut().page_down() {
            self.updated = true;
            self.update_columns = ColumnUpdate::Regenerate;
            true
        } else {
            false
        }
    }

    pub(crate) fn end(&mut self) -> bool {
        if self.db.cursor_mut().end() {
            self.updated = true;
            self.update_columns = ColumnUpdate::Regenerate;
            true
        } else {
            false
        }
    }

    pub(crate) fn home(&mut self) -> bool {
        if self.db.cursor_mut().home() {
            self.updated = true;
            self.update_columns = ColumnUpdate::Regenerate;
            true
        } else {
            false
        }
    }

    pub(crate) fn apply_sort(&mut self) -> Result<(), DatabaseError> {
        if !self.sort_settings.is_sorted {
            self.db.sort_books_by_col(
                self.sort_settings.column.as_str(),
                self.sort_settings.reverse,
            )?;
            self.sort_settings.is_sorted = true;
        }
        Ok(())
    }

    pub(crate) fn num_cols(&self) -> usize {
        self.selected_cols.len()
    }

    pub(crate) fn refresh_window_size(&mut self, size: usize) -> bool {
        if self.db.cursor().window_size() != size {
            self.db.cursor_mut().refresh_window_size(size);
            true
        } else {
            false
        }
    }

    pub(crate) fn header_col_iter(&self) -> impl Iterator<Item = (&UniCase<String>, &Vec<String>)> {
        self.selected_cols.iter().zip(self.column_data.iter())
    }

    pub(crate) fn take_update(&mut self) -> bool {
        std::mem::replace(&mut self.updated, false)
    }
}

#[derive(Default)]
pub(crate) struct UIState {
    pub(crate) style: InterfaceStyle,
    pub(crate) nav_settings: NavigationSettings,
    pub(crate) curr_command: CommandString,
    pub(crate) selected_column: usize,
}

pub(crate) struct AppInterface<D, B> {
    app: App<D>,
    border_widget: BorderWidget,
    active_view: Box<dyn View<D, B>>,
}

impl<D: SelectableDatabase, B: Backend> AppInterface<D, B> {
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
        mut app: App<D>,
    ) -> Result<Self, ApplicationError> {
        let selected_cols: Vec<_> = settings
            .columns
            .iter()
            .map(|s| UniCase::new(s.clone()))
            .collect();

        let column_data = (0..selected_cols.len())
            .into_iter()
            .map(|_| vec![])
            .collect();

        app.selected_cols = selected_cols;
        app.column_data = column_data;

        Ok(AppInterface {
            app,
            border_widget: BorderWidget::new(name.as_ref().to_string()),
            active_view: Box::new(ColumnWidget {
                state: UIState {
                    style: settings.interface_style,
                    nav_settings: settings.navigation_settings,
                    curr_command: CommandString::new(),
                    selected_column: 0,
                },
            }),
        })
    }

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
                match self.active_view.handle_input(read()?, &mut self.app)? {
                    ApplicationTask::Quit => return Ok(true),
                    ApplicationTask::Update => self.app.updated = true,
                    ApplicationTask::SwitchView(view) => {
                        self.app.updated = true;
                        let state = self.active_view.get_owned_state();
                        match view {
                            AppView::ColumnView => {
                                self.active_view = Box::new(ColumnWidget { state })
                            }
                            AppView::EditView => {
                                if let Some(x) = self.app.selected() {
                                    self.active_view = Box::new(EditWidget {
                                        edit: EditState::new(&self.app.column_data[0][x], x),
                                        state,
                                    })
                                }
                            }
                        }
                    }
                    ApplicationTask::DoNothing => {}
                }
                break;
            }
        }
        Ok(false)
    }

    /// Runs the application - including handling user inputs and refreshing the output.
    ///
    /// # Arguments
    ///
    /// * ` terminal ` - The terminal to output text to.
    ///
    /// # Errors
    /// This function will return an error if running the program fails for any reason.
    pub(crate) fn run(&mut self, terminal: &mut Terminal<B>) -> Result<(), ApplicationError> {
        loop {
            self.app.apply_sort()?;
            self.app.update_column_data();

            if self.app.take_update() {
                terminal.draw(|f| {
                    self.border_widget.render_into_frame(f, f.size());

                    let chunk = {
                        let s = f.size();
                        Rect::new(
                            s.x + 1,
                            s.y + 1,
                            s.width.saturating_sub(2),
                            s.height.saturating_sub(2),
                        )
                    };

                    self.active_view.render_into_frame(&mut self.app, f, chunk);
                })?;
            }

            match self.get_input() {
                Ok(true) => return Ok(terminal.clear()?),
                _ => {
                    // TODO: Handle errors here.
                }
            }
        }
    }
}

// TODO:
//  Live search & search by tags - sqllite? meillisearch?
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
