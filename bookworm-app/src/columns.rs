use std::borrow::Cow;

use unicase::UniCase;

use bookworm_records::book::ColumnIdentifier;
use bookworm_records::Book;

#[derive(Default)]
pub struct Columns {
    selected_cols: Vec<UniCase<String>>,
}

impl Columns {
    pub fn remove_column(&mut self, column: &UniCase<String>) {
        self.selected_cols.retain(|x| x != column);
    }

    pub fn add_column(&mut self, column: UniCase<String>) {
        if !self.selected_cols.contains(&column) {
            self.selected_cols.push(column);
        }
    }

    pub fn read_columns<'s, 'a, B: AsRef<Book>>(
        &'s self,
        books: &'a [B],
    ) -> impl Iterator<Item = (&'s UniCase<String>, impl Iterator<Item = Cow<'a, str>> + 'a)> {
        Box::new(
            self.selected_cols
                .iter()
                .map(|col| (col, ColumnIdentifier::from(col)))
                .map(move |(col, col_id)| {
                    (
                        col,
                        books.iter().map(move |book| {
                            book.as_ref()
                                .get_column(&col_id)
                                .unwrap_or(Cow::Borrowed(""))
                        }),
                    )
                }),
        )
    }

    pub fn selected_cols(&self) -> &[UniCase<String>] {
        &self.selected_cols
    }
}

impl From<Vec<String>> for Columns {
    fn from(selected_cols: Vec<String>) -> Self {
        Columns {
            selected_cols: selected_cols.into_iter().map(UniCase::new).collect(),
        }
    }
}
