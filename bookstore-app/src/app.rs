use std::path::Path;
#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
use std::process::Command as ProcessCommand;

use rayon::prelude::*;
#[cfg(feature = "sqlite")]
use sqlx::Error as SQLError;
use unicase::UniCase;

use bookstore_database::{
    bookview::BookViewError, AppDatabase, BookView, DatabaseError, IndexableDatabase,
    NestedBookView, ScrollableBookView, SearchableBookView,
};
use bookstore_records::{
    book::{ColumnIdentifier, RawBook},
    Book, BookError,
};

use crate::help_strings::{help_strings, GENERAL_HELP};
use crate::parser;
use crate::parser::{BookIndex, Command};
use crate::settings::SortSettings;

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ColumnUpdate {
    Regenerate,
    AddColumn(UniCase<String>),
    RemoveColumn(UniCase<String>),
    NoUpdate,
}

#[derive(Debug)]
pub enum ApplicationError {
    IO(std::io::Error),
    Book(BookError),
    Database(DatabaseError),
    BookView(BookViewError),
    NoBookSelected,
    Err(()),
    #[cfg(feature = "sqlite")]
    SQL(SQLError),
}

impl From<std::io::Error> for ApplicationError {
    fn from(e: std::io::Error) -> Self {
        ApplicationError::IO(e)
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
            DatabaseError::Io(e) => ApplicationError::IO(e),
            DatabaseError::Book(e) => ApplicationError::Book(e),
            e => ApplicationError::Database(e),
        }
    }
}

impl From<BookError> for ApplicationError {
    fn from(e: BookError) -> Self {
        ApplicationError::Book(e)
    }
}

impl From<BookViewError> for ApplicationError {
    fn from(e: BookViewError) -> Self {
        match e {
            BookViewError::NoBookSelected => ApplicationError::NoBookSelected,
            x => ApplicationError::BookView(x),
        }
    }
}

#[cfg(feature = "sqlite")]
impl From<SQLError> for ApplicationError {
    fn from(e: SQLError) -> Self {
        ApplicationError::SQL(e)
    }
}

// 0.75
fn books_in_dir<P: AsRef<Path>>(dir: P, depth: u8) -> Result<Vec<RawBook>, std::io::Error> {
    // TODO: Handle errored reads somehow.
    Ok(jwalk::WalkDir::new(dir)
        .min_depth(0)
        .max_depth(depth as usize)
        .into_iter()
        .filter_map(|res| res.map(|e| e.path()).ok())
        .collect::<Vec<_>>()
        .par_iter()
        .filter_map(|path| RawBook::generate_from_file(path).ok())
        .collect::<Vec<_>>())
}

// fn books_in_dir<P: AsRef<Path>>(dir: P) -> Result<Vec<RawBook>, std::io::Error> {
//     // TODO: Look at libraries with parallel directory reading.
//     //  Handle errored reads somehow.
//     use futures::future::join_all;
//     use futures::executor::block_on;
//
//     let start = std::time::Instant::now();
//     let books = block_on(join_all(fs::read_dir(dir)?
//         .filter_map(|res| res.map(|e| e.path()).ok())
//         .map(|p| async move { RawBook::generate_from_file(p) })))
//         .into_iter()
//         .filter_map(|x| x.ok())
//         .collect();
//
//     let elapsed = start.elapsed().as_secs_f32();
//     println!("{}", elapsed);
//
//     Ok(books)
// }

pub struct App<D: AppDatabase> {
    // Application data
    bv: SearchableBookView<D>,
    selected_cols: Vec<UniCase<String>>,
    column_data: Vec<Vec<String>>,
    active_help_string: Option<&'static str>,
    // Actions to run on data
    sort_settings: SortSettings,
    column_update: ColumnUpdate,
    updated: bool,
}

impl<D: IndexableDatabase> App<D> {
    pub fn new(db: D) -> Self {
        App {
            bv: SearchableBookView::new(db),
            selected_cols: vec![],
            sort_settings: SortSettings::default(),
            updated: true,
            column_update: ColumnUpdate::Regenerate,
            column_data: vec![],
            active_help_string: None,
        }
    }

    pub fn update_value<S: AsRef<str>>(&mut self, col: usize, row: usize, new_value: S) {
        self.register_update();
        self.column_data[col][row] = new_value.as_ref().to_owned();
    }

    pub fn get_value(&self, col: usize, row: usize) -> &str {
        &self.column_data[col][row]
    }

    pub fn selected_item(&self) -> Result<Book, ApplicationError> {
        Ok(self.bv.get_selected_book()?)
    }

    pub fn edit_selected_book_with_column<S: AsRef<str>>(
        &mut self,
        column: usize,
        new_value: S,
    ) -> Result<(), ApplicationError> {
        self.bv
            .edit_selected_book(&self.selected_cols[column], new_value)?;
        self.register_update();
        Ok(())
    }

    pub fn remove_selected_book(&mut self) -> Result<(), ApplicationError> {
        self.bv.remove_selected_book()?;
        self.register_update();
        self.column_update = ColumnUpdate::Regenerate;
        Ok(())
    }

    pub fn selected(&self) -> Option<usize> {
        self.bv.selected()
    }

    /// Gets the book specified by the `BookIndex`,
    /// or None if the particular book does not exist.
    ///
    /// # Arguments
    ///
    /// * ` b ` - A `BookIndex` to get a book by ID or by current selection.
    pub fn get_book(&self, b: BookIndex) -> Result<Book, ApplicationError> {
        match b {
            BookIndex::Selected => Ok(self.bv.get_selected_book()?),
            BookIndex::BookID(id) => Ok(self.bv.get_book(id)?),
        }
    }

    /// Runs the command currently in the current command string. On success, returns a bool
    /// indicating whether to continue or not.
    ///
    /// # Arguments
    ///
    /// * ` command ` - The command to run.
    pub fn run_command(&mut self, command: parser::Command) -> Result<bool, ApplicationError> {
        match command {
            Command::DeleteBook(b) => {
                match b {
                    BookIndex::Selected => self.bv.remove_selected_book()?,
                    BookIndex::BookID(id) => self.bv.remove_book(id)?,
                };
                self.column_update = ColumnUpdate::Regenerate;
            }
            Command::DeleteAll => {
                self.bv.write(|db| db.clear())?;
                self.column_update = ColumnUpdate::Regenerate;
            }
            Command::EditBook(b, field, new_value) => {
                match b {
                    BookIndex::Selected => self.bv.edit_selected_book(field, new_value)?,
                    BookIndex::BookID(id) => self
                        .bv
                        .write(|db| db.edit_book_with_id(id, &field, &new_value))?,
                };
                self.sort_settings.is_sorted = false;
                self.column_update = ColumnUpdate::Regenerate;
            }
            Command::AddBookFromFile(f) => {
                self.bv
                    .write(|db| db.insert_book(RawBook::generate_from_file(&f)?))?;
                self.updated = true;
                self.sort_settings.is_sorted = false;
                self.column_update = ColumnUpdate::Regenerate;
            }
            Command::AddBooksFromDir(dir, depth) => {
                // TODO: Handle failed reads.
                self.bv
                    .write(|db| db.insert_books(books_in_dir(&dir, depth)?))?;
                self.updated = true;
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
            #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
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
            Command::FindMatches(search) => {
                self.bv.push_scope(search)?;
                self.updated = true;
                self.column_update = ColumnUpdate::Regenerate;
            }
            Command::Write => self.bv.write(|d| d.save())?,
            // TODO: A warning pop-up when user is about to exit
            //  with unsaved changes.
            Command::Quit => return Ok(false),
            Command::WriteAndQuit => {
                self.bv.write(|d| d.save())?;
                return Ok(false);
            }
            Command::TryMergeAllBooks => {
                self.bv.write(|db| db.merge_similar())?;
                self.register_update();
                self.column_update = ColumnUpdate::Regenerate;
            }
            Command::Help(flag) => {
                if let Some(s) = help_strings(&flag) {
                    self.active_help_string = Some(s);
                } else {
                    self.active_help_string = Some(GENERAL_HELP);
                }
            }
            Command::GeneralHelp => {
                self.active_help_string = Some(GENERAL_HELP);
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
        match word.to_ascii_lowercase().as_str() {
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
    pub fn update_column_data(&mut self) -> Result<(), ApplicationError> {
        match &self.column_update {
            ColumnUpdate::Regenerate => {
                self.updated = true;
                self.column_data =
                    vec![Vec::with_capacity(self.bv.window_size()); self.selected_cols.len()];

                if self.bv.window_size() == 0 {
                    self.column_update = ColumnUpdate::NoUpdate;
                    return Ok(());
                }

                let cols = self
                    .selected_cols
                    .iter()
                    .map(|col| ColumnIdentifier::from(col.as_str()))
                    .collect::<Vec<_>>();

                for b in self.bv.get_books_cursored()? {
                    for (col, column) in cols.iter().zip(self.column_data.iter_mut()) {
                        column.push(b.get_column_or(&col, ""));
                    }
                }
            }
            ColumnUpdate::AddColumn(word) => {
                self.updated = true;
                if self.bv.inner().has_column(&word) && !self.selected_cols.contains(&word) {
                    self.selected_cols.push(word.clone());
                    let column_string = ColumnIdentifier::from(word.as_str());
                    self.column_data.push(
                        self.bv
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

    /// Returns the first available path amongst the variants of the book, or None if no such
    /// path exists.
    ///
    /// # Arguments
    ///
    /// * ` book ` - The book to find a path for.
    fn get_book_path(book: &Book, index: usize) -> Option<&Path> {
        Some(book.get_variants()?.get(index)?.path())
    }

    #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
    /// Opens the book in the native system viewer.
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
            #[cfg(target_os = "windows")]
            {
                ProcessCommand::new("cmd.exe")
                    .args(&["/C", "start", "explorer"])
                    .arg(path)
                    .spawn()?;
            }
            #[cfg(target_os = "linux")]
            {
                ProcessCommand::new("xdg-open").arg(path).spawn()?;
            }
            #[cfg(target_os = "macos")]
            {
                ProcessCommand::new("open").arg(path).spawn()?;
            }
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

    fn modify_db(&mut self, f: impl Fn(&mut SearchableBookView<D>) -> bool) -> bool {
        if f(&mut self.bv) {
            self.register_update();
            self.column_update = ColumnUpdate::Regenerate;
            true
        } else {
            false
        }
    }

    pub fn scroll_up(&mut self, scroll: usize) -> bool {
        self.modify_db(|db| db.scroll_up(scroll))
    }

    pub fn scroll_down(&mut self, scroll: usize) -> bool {
        self.modify_db(|db| db.scroll_down(scroll))
    }

    pub fn deselect(&mut self) -> bool {
        self.modify_db(|db| db.deselect())
    }

    pub fn select_up(&mut self) -> bool {
        self.modify_db(|db| db.select_up())
    }

    pub fn select_down(&mut self) -> bool {
        self.modify_db(|db| db.select_down())
    }

    pub fn page_up(&mut self) -> bool {
        self.modify_db(|db| db.page_up())
    }

    pub fn page_down(&mut self) -> bool {
        self.modify_db(|db| db.page_down())
    }

    pub fn home(&mut self) -> bool {
        self.modify_db(|db| db.home())
    }

    pub fn end(&mut self) -> bool {
        self.modify_db(|db| db.end())
    }

    pub fn apply_sort(&mut self) -> Result<(), DatabaseError> {
        if !self.sort_settings.is_sorted {
            self.bv.sort_by_column(
                self.sort_settings.column.as_str(),
                self.sort_settings.reverse,
            )?;
            self.sort_settings.is_sorted = true;
        }
        Ok(())
    }

    pub fn num_cols(&self) -> usize {
        self.selected_cols.len()
    }

    pub fn refresh_window_size(&mut self, size: usize) -> bool {
        self.bv.refresh_window_size(size)
    }

    pub fn set_column_update(&mut self, column_update: ColumnUpdate) {
        self.column_update = column_update;
    }

    pub fn header_col_iter(&self) -> impl Iterator<Item = (&UniCase<String>, &Vec<String>)> {
        self.selected_cols.iter().zip(self.column_data.iter())
    }

    pub fn register_update(&mut self) {
        self.updated = true;
    }

    pub fn take_update(&mut self) -> bool {
        std::mem::replace(&mut self.updated, false)
    }

    pub fn set_selected_columns(&mut self, cols: Vec<String>) {
        self.selected_cols = cols.into_iter().map(UniCase::new).collect();
        self.column_data = vec![vec![]; self.selected_cols.len()];
    }

    pub fn saved(&mut self) -> bool {
        self.bv.inner().saved()
    }

    pub fn pop_scope(&mut self) -> bool {
        self.modify_db(|db| db.pop_scope())
    }

    pub fn has_help_string(&self) -> bool {
        self.active_help_string.is_some()
    }

    pub fn take_help_string(&mut self) -> &'static str {
        std::mem::take(&mut self.active_help_string).unwrap_or(GENERAL_HELP)
    }
}
