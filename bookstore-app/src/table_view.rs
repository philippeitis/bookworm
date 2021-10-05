use std::borrow::Cow;

use unicase::UniCase;

use bookstore_database::bookview::BookViewError;
use bookstore_database::{BookView, IndexableDatabase};
use bookstore_records::book::ColumnIdentifier;

/// TableView acts as a way to avoid errors in the rendering step - by pre-loading all
/// data before entering the rendering step, the rendering step itself can avoid
/// BookView::get_books_cursored()'s Result.
#[derive(Default)]
pub struct TableView {
    selected_cols: Vec<UniCase<String>>,
    column_data: Vec<Vec<String>>,
}

impl TableView {
    /// Refreshes the table data according to the currently selected columns and the books
    /// in the BookView's cursor.
    pub async fn regenerate_columns<D: IndexableDatabase + Send + Sync, S: BookView<D>>(
        &mut self,
        bv: &S,
    ) -> Result<(), BookViewError<D::Error>> {
        self.column_data = vec![Vec::with_capacity(bv.window_size()); self.selected_cols.len()];

        if bv.window_size() == 0 {
            return Ok(());
        }

        // bv.get_books_cursored() and ColumnIdentifier::from are expensive, so
        // we collect the ColumnIdentifiers into a Vec and only call get_books_cursored()
        // once.
        let cols = self
            .selected_cols
            .iter()
            .map(|col| ColumnIdentifier::from(col.as_str()))
            .collect::<Vec<_>>();

        for book in bv.get_books_cursored().await?.iter() {
            for (col, column) in cols.iter().zip(self.column_data.iter_mut()) {
                column.push(match book.get_column(&col) {
                    None => String::new(),
                    Some(Cow::Borrowed(s)) => s.to_string(),
                    Some(Cow::Owned(s)) => s,
                });
            }
        }

        Ok(())
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
