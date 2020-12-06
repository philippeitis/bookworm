#[cfg(windows)]
use std::{path::PathBuf, process::Command as ProcessCommand};

use unicase::UniCase;

use crate::app::settings::SortSettings;
use crate::app::{parser, BookIndex, Command};
use crate::database::bookview::BookViewError;
use crate::database::{
    AppDatabase, BasicBookView, BookView, DatabaseError, IndexableDatabase, ScrollableBookView,
};
use crate::record::book::ColumnIdentifier;
use crate::record::Book;

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
    BookViewError(BookViewError),
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

impl From<BookViewError> for ApplicationError {
    fn from(e: BookViewError) -> Self {
        match e {
            BookViewError::NoBookSelected => ApplicationError::NoBookSelected,
            x => ApplicationError::BookViewError(x),
        }
    }
}

impl From<crossterm::ErrorKind> for ApplicationError {
    fn from(e: crossterm::ErrorKind) -> Self {
        ApplicationError::TerminalError(e)
    }
}

pub(crate) struct App<D: AppDatabase> {
    // Application data
    db: BasicBookView<D>,
    selected_cols: Vec<UniCase<String>>,
    column_data: Vec<Vec<String>>,

    // Actions to run on data
    sort_settings: SortSettings,
    column_update: ColumnUpdate,
    updated: bool,
}

impl<D: IndexableDatabase> App<D> {
    pub(crate) fn new(db: D) -> Self {
        App {
            db: BasicBookView::new(db),
            selected_cols: vec![],
            sort_settings: SortSettings::default(),
            updated: true,
            column_update: ColumnUpdate::Regenerate,
            column_data: vec![],
        }
    }

    pub(crate) fn update_value<S: AsRef<str>>(&mut self, col: usize, row: usize, new_value: S) {
        self.register_update();
        self.column_data[col][row] = new_value.as_ref().to_owned();
    }

    pub(crate) fn get_value(&self, col: usize, row: usize) -> &str {
        &self.column_data[col][row]
    }

    pub(crate) fn selected_item(&self) -> Result<Book, ApplicationError> {
        Ok(self.db.get_selected_book()?)
    }

    pub(crate) fn edit_selected_book_with_column<S: AsRef<str>>(
        &mut self,
        column: usize,
        new_value: S,
    ) -> Result<(), ApplicationError> {
        self.db
            .edit_selected_book(&self.selected_cols[column], new_value)?;
        self.register_update();
        Ok(())
    }

    pub(crate) fn remove_selected_book(&mut self) -> Result<(), ApplicationError> {
        self.db.remove_selected_book()?;
        self.register_update();
        self.column_update = ColumnUpdate::Regenerate;
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
            BookIndex::Selected => Ok(self.db.get_selected_book()?),
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
        command: parser::Command,
    ) -> Result<bool, ApplicationError> {
        match command {
            Command::DeleteBook(b) => {
                match b {
                    BookIndex::Selected => self.db.remove_selected_book()?,
                    BookIndex::BookID(id) => self.db.remove_book(id)?,
                };
                self.column_update = ColumnUpdate::Regenerate;
            }
            Command::DeleteAll => {
                self.db.write(|db| db.clear())?;
                self.column_update = ColumnUpdate::Regenerate;
            }
            Command::EditBook(b, field, new_value) => {
                match b {
                    BookIndex::Selected => self.db.edit_selected_book(field, new_value)?,
                    BookIndex::BookID(id) => self
                        .db
                        .write(|db| db.edit_book_with_id(id, &field, &new_value))?,
                };
                self.sort_settings.is_sorted = false;
                self.column_update = ColumnUpdate::Regenerate;
            }
            Command::AddBookFromFile(f) => {
                self.db.write(|db| db.read_book_from_file(&f))?;
                self.sort_settings.is_sorted = false;
                self.column_update = ColumnUpdate::Regenerate;
            }
            Command::AddBooksFromDir(dir) => {
                // TODO: Handle failed reads.
                self.db.write(|db| db.read_books_from_dir(&dir))?;
                self.sort_settings.is_sorted = false;
                self.column_update = ColumnUpdate::Regenerate;
            }
            Command::AddColumn(column) => {
                self.column_update = ColumnUpdate::AddColumn(UniCase::new(column));
            }
            Command::RemoveColumn(column) => {
                self.column_update = ColumnUpdate::RemoveColumn(UniCase::new(column));
            }
            Command::SortColumn(column, rev) => {
                self.update_selected_column(UniCase::new(column), rev);
            }
            #[cfg(windows)]
            Command::OpenBookInApp(b, index) => {
                if let Ok(b) = self.get_book(b) {
                    self.open_book(&b, index)?;
                }
            }
            #[cfg(windows)]
            Command::OpenBookInExplorer(b, index) => {
                if let Ok(b) = self.get_book(b) {
                    self.open_book_in_dir(&b, index)?;
                }
            }
            Command::FindMatches(_matching, _column, _pattern) => {
                unimplemented!("Searches not implemented yet.")
                // self.matches = Some(self.db.find_matches(matching, &column, &pattern)?);
            }
            Command::Write => self.db.inner().save()?,
            Command::Quit => return Ok(false),
            Command::WriteAndQuit => {
                self.db.inner().save()?;
                return Ok(false);
            }
            Command::TryMergeAllBooks => {
                self.db.write(|db| db.merge_similar())?;
                self.register_update();
                self.column_update = ColumnUpdate::Regenerate;
            }
            #[cfg(not(windows))]
            _ => return Ok(true),
        }
        Ok(true)
    }

    /// Updates the required sorting settings if the column changes.
    ///
    /// # Arguments
    ///
    /// * ` word ` - The column to sort the table on.
    /// * ` reverse ` - Whether to reverse the sort.
    fn update_selected_column(&mut self, mut word: UniCase<String>, reverse: bool) {
        match word.as_str() {
            "author" => word = UniCase::from(String::from("authors")),
            _ => {}
        }

        if self.selected_cols.contains(&word) {
            self.sort_settings.column = word;
            self.sort_settings.is_sorted = false;
            self.sort_settings.reverse = reverse;
            self.column_update = ColumnUpdate::Regenerate;
        }
    }

    /// Updates the table data if a change occurs.
    pub(crate) fn update_column_data(&mut self) -> Result<(), ApplicationError> {
        match &self.column_update {
            ColumnUpdate::Regenerate => {
                self.updated = true;
                self.column_data = (0..self.selected_cols.len())
                    .into_iter()
                    .map(|_| Vec::with_capacity(self.db.window_size()))
                    .collect();

                if self.db.window_size() == 0 {
                    self.column_update = ColumnUpdate::NoUpdate;
                    return Ok(());
                }

                let cols = self
                    .selected_cols
                    .iter()
                    .map(|col| ColumnIdentifier::from(col.as_str()))
                    .collect::<Vec<_>>();

                for b in self.db.get_books_cursored()? {
                    for (col, column) in cols.iter().zip(self.column_data.iter_mut()) {
                        column.push(b.get_column_or(&col, ""));
                    }
                }
            }
            ColumnUpdate::AddColumn(word) => {
                self.updated = true;
                if self.db.inner().has_column(&word) && !self.selected_cols.contains(&word) {
                    self.selected_cols.push(word.clone());
                    let column_string = ColumnIdentifier::from(word.as_str());
                    self.column_data.push(
                        self.db
                            .get_books_cursored()?
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

        self.column_update = ColumnUpdate::NoUpdate;
        Ok(())
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
            ProcessCommand::new("cmd.exe")
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
    /// * ` index ` - The index of the path to open.
    ///
    /// # Errors
    /// This function may error if the book's variants do not exist,
    /// or if the command itself fails.
    fn open_book_in_dir(&self, book: &Book, index: usize) -> Result<(), ApplicationError> {
        // TODO: This doesn't work when run with install due to relative paths.
        if let Some(path) = App::<D>::get_book_path(book, index) {
            let open_book_path = PathBuf::from(".\\src\\open_book.py").canonicalize()?;
            // TODO: Find a way to do this entirely in Rust
            ProcessCommand::new("python")
                .args(&[
                    open_book_path.display().to_string().as_str(),
                    path.display().to_string().as_str(),
                ])
                .spawn()?;
        }
        Ok(())
    }

    fn modify_db(&mut self, f: impl Fn(&mut BasicBookView<D>) -> bool) -> bool {
        if f(&mut self.db) {
            self.register_update();
            self.column_update = ColumnUpdate::Regenerate;
            true
        } else {
            false
        }
    }

    pub(crate) fn scroll_up(&mut self, scroll: usize) -> bool {
        self.modify_db(|db| db.scroll_up(scroll))
    }

    pub(crate) fn scroll_down(&mut self, scroll: usize) -> bool {
        self.modify_db(|db| db.scroll_down(scroll))
    }

    pub(crate) fn deselect(&mut self) -> bool {
        self.modify_db(|db| db.deselect())
    }

    pub(crate) fn select_up(&mut self) -> bool {
        self.modify_db(|db| db.select_up())
    }

    pub(crate) fn select_down(&mut self) -> bool {
        self.modify_db(|db| db.select_down())
    }

    pub(crate) fn page_up(&mut self) -> bool {
        self.modify_db(|db| db.page_up())
    }

    pub(crate) fn page_down(&mut self) -> bool {
        self.modify_db(|db| db.page_down())
    }

    pub(crate) fn home(&mut self) -> bool {
        self.modify_db(|db| db.home())
    }

    pub(crate) fn end(&mut self) -> bool {
        self.modify_db(|db| db.end())
    }

    pub(crate) fn apply_sort(&mut self) -> Result<(), DatabaseError> {
        if !self.sort_settings.is_sorted {
            self.db.sort_by_column(
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
        if self.db.window_size() != size {
            self.db.refresh_window_size(size);
            true
        } else {
            false
        }
    }

    pub(crate) fn set_column_update(&mut self, column_update: ColumnUpdate) {
        self.column_update = column_update;
    }

    pub(crate) fn header_col_iter(&self) -> impl Iterator<Item = (&UniCase<String>, &Vec<String>)> {
        self.selected_cols.iter().zip(self.column_data.iter())
    }

    pub(crate) fn register_update(&mut self) {
        self.updated = true;
    }

    pub(crate) fn take_update(&mut self) -> bool {
        std::mem::replace(&mut self.updated, false)
    }

    pub(crate) fn set_selected_columns(&mut self, cols: Vec<String>) {
        self.selected_cols = cols.into_iter().map(UniCase::new).collect();
        self.column_data = vec![vec![]; self.selected_cols.len()];
    }
}
