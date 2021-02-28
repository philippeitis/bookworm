use unicase::UniCase;

use bookstore_database::{IndexableDatabase, ScrollableBookView};
use bookstore_records::book::ColumnIdentifier;

use crate::ApplicationError;

macro_rules! book {
    ($book: ident) => {
        $book.as_ref().read().unwrap()
    };
}

pub struct TableView {
    selected_cols: Vec<UniCase<String>>,
    column_data: Vec<Vec<String>>,
}

impl Default for TableView {
    fn default() -> Self {
        TableView {
            selected_cols: vec![],
            column_data: vec![],
        }
    }
}

impl TableView {
    pub fn new() -> Self {
        TableView {
            selected_cols: vec![],
            column_data: vec![],
        }
    }

    pub fn update_value<S: AsRef<str>>(&mut self, col: usize, row: usize, new_value: S) {
        self.column_data[col][row] = new_value.as_ref().to_owned();
    }

    pub fn get_value(&self, col: usize, row: usize) -> &str {
        &self.column_data[col][row]
    }

    /// Updates the table data if a change occurs.
    pub fn regenerate_columns<D: IndexableDatabase, S: ScrollableBookView<D>>(
        &mut self,
        bv: &S,
    ) -> Result<(), ApplicationError> {
        self.column_data = vec![Vec::with_capacity(bv.window_size()); self.selected_cols.len()];

        if bv.window_size() == 0 {
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
        Ok(())
    }

    pub fn remove_column(&mut self, column: UniCase<String>) {
        let index = self.selected_cols.iter().position(|x| x.eq(&column));
        if let Some(index) = index {
            self.selected_cols.remove(index);
            self.column_data.remove(index);
        }
    }

    pub fn add_column(&mut self, column: UniCase<String>) {
        if !self.selected_cols.contains(&column) {
            self.selected_cols.push(column);
        }
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
