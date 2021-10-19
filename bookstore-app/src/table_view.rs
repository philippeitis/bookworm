use std::borrow::Cow;
use std::sync::Arc;

use unicase::UniCase;

use bookstore_database::{AppDatabase, BookView};
use bookstore_records::book::ColumnIdentifier;
use bookstore_records::Book;

#[derive(Default)]
pub struct TableView {
    selected_cols: Vec<UniCase<String>>,
    column_data: Vec<Vec<String>>,
}

impl TableView {
    /// Refreshes the table data according to the currently selected columns and the books
    /// in the BookView's cursor.
    pub fn regenerate_columns<D: AppDatabase + Send + Sync>(&mut self, bv: &BookView<D>) {
        self.column_data = self
            .read_columns(&bv.window())
            .map(|(_, col)| col.map(String::from).collect())
            .collect();
    }

    pub fn remove_column(&mut self, column: &UniCase<String>) {
        self.selected_cols.retain(|x| x != column);
    }

    pub fn add_column(&mut self, column: UniCase<String>) {
        if !self.selected_cols.contains(&column) {
            self.selected_cols.push(column);
        }
    }

    pub fn header_col_iter(&self) -> impl Iterator<Item = (&UniCase<String>, &Vec<String>)> {
        self.selected_cols.iter().zip(self.column_data.iter())
    }

    pub fn read_columns<'s, 'a>(
        &'s self,
        books: &'a [Arc<Book>],
    ) -> impl Iterator<Item = (&'s UniCase<String>, impl Iterator<Item = Cow<'a, str>> + 'a)> {
        Box::new(
            self.selected_cols
                .iter()
                .map(|col| (col, ColumnIdentifier::from(col)))
                .map(move |(col, col_id)| {
                    (
                        col,
                        books
                            .iter()
                            .map(move |book| book.get_column(&col_id).unwrap_or(Cow::Borrowed(""))),
                    )
                }),
        )
    }

    pub fn selected_cols(&self) -> &[UniCase<String>] {
        &self.selected_cols
    }

    pub fn get_column(&self, index: usize) -> &[String] {
        &self.column_data[index]
    }
}

impl From<Vec<String>> for TableView {
    fn from(selected_cols: Vec<String>) -> Self {
        TableView {
            selected_cols: selected_cols.into_iter().map(UniCase::new).collect(),
            column_data: vec![],
        }
    }
}
