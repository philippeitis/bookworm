use unicase::UniCase;

use bookstore_database::{IndexableDatabase, ScrollableBookView};
use bookstore_records::book::ColumnIdentifier;

use crate::ApplicationError;

macro_rules! book {
    ($book: ident) => {
        $book.as_ref().read().unwrap()
    };
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ColumnUpdate {
    Regenerate,
    AddColumn(UniCase<String>),
    RemoveColumn(UniCase<String>),
    NoUpdate,
}

pub struct TableView {
    selected_cols: Vec<UniCase<String>>,
    column_data: Vec<Vec<String>>,
    column_update: ColumnUpdate,
}

impl Default for TableView {
    fn default() -> Self {
        TableView {
            selected_cols: vec![],
            column_data: vec![],
            column_update: ColumnUpdate::Regenerate,
        }
    }
}

impl TableView {
    pub fn new() -> Self {
        TableView {
            selected_cols: vec![],
            column_data: vec![],
            column_update: ColumnUpdate::Regenerate,
        }
    }

    pub fn update_value<S: AsRef<str>>(&mut self, col: usize, row: usize, new_value: S) {
        self.column_data[col][row] = new_value.as_ref().to_owned();
    }

    pub fn get_value(&self, col: usize, row: usize) -> &str {
        &self.column_data[col][row]
    }

    /// Updates the table data if a change occurs.
    pub fn update_column_data<D: IndexableDatabase, S: ScrollableBookView<D>>(
        &mut self,
        bv: &S,
    ) -> Result<(), ApplicationError> {
        match std::mem::replace(&mut self.column_update, ColumnUpdate::NoUpdate) {
            ColumnUpdate::Regenerate => {
                self.column_data =
                    vec![Vec::with_capacity(bv.window_size()); self.selected_cols.len()];

                if bv.window_size() == 0 {
                    self.column_update = ColumnUpdate::NoUpdate;
                    return Ok(());
                }

                let cols = self
                    .selected_cols
                    .iter()
                    .map(|col| ColumnIdentifier::from(col.as_str()))
                    .collect::<Vec<_>>();

                for b in bv.get_books_cursored()? {
                    for (col, column) in cols.iter().zip(self.column_data.iter_mut()) {
                        column.push(book!(b).get_column_or(&col, ""));
                    }
                }
            }
            ColumnUpdate::AddColumn(word) => {
                if bv.has_column(&word) && !self.selected_cols.contains(&word) {
                    self.selected_cols.push(word.clone());
                    let column_string = ColumnIdentifier::from(word.as_str());
                    self.column_data.push(
                        bv.get_books_cursored()?
                            .iter()
                            .map(|book| book!(book).get_column_or(&column_string, ""))
                            .collect(),
                    );
                }
            }
            ColumnUpdate::RemoveColumn(word) => {
                let index = self.selected_cols.iter().position(|x| x.eq(&word));
                if let Some(index) = index {
                    self.selected_cols.remove(index);
                    self.column_data.remove(index);
                }
            }
            ColumnUpdate::NoUpdate => {}
        }

        Ok(())
    }

    pub fn set_column_update(&mut self, column_update: ColumnUpdate) {
        self.column_update = column_update;
    }

    pub fn header_col_iter(&self) -> impl Iterator<Item = (&UniCase<String>, &Vec<String>)> {
        self.selected_cols.iter().zip(self.column_data.iter())
    }

    pub fn set_selected_columns(&mut self, cols: Vec<String>) {
        self.selected_cols = cols.into_iter().map(UniCase::new).collect();
        self.column_data = vec![vec![]; self.selected_cols.len()];
    }

    pub fn num_cols(&self) -> usize {
        self.selected_cols.len()
    }

    pub fn selected_cols(&self) -> &[UniCase<String>] {
        &self.selected_cols
    }

    pub fn get_column(&self, index: usize) -> &UniCase<String> {
        &self.selected_cols[index]
    }
}
