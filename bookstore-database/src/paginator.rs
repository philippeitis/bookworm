use crate::search::{Matcher, Search};
use crate::{DatabaseError, IndexableDatabase};
use bookstore_records::book::{BookID, ColumnIdentifier};
use bookstore_records::{Book, ColumnOrder};
use std::borrow::Borrow;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::RwLock;

static ASCII_LOWER: [char; 26] = [
    'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q', 'r', 's',
    't', 'u', 'v', 'w', 'x', 'y', 'z',
];

/// Paginator provides a fast way to scroll through book databases which allow both sorting
/// and greater than comparisons.
/// The paginator stores information about existing matching and sorting rules, and when scrolled
/// to the end, will automatically fetch new items, in the correct order, which match the internal
/// rules.
pub struct Paginator<D: IndexableDatabase> {
    books: Vec<Arc<Book>>,
    window_size: usize,
    // relative to start of books
    window_top: usize,
    // Some set of sorting rules.
    sorting_rules: Box<[(ColumnIdentifier, ColumnOrder)]>,
    matching_rules: Box<[Box<dyn Matcher>]>,
    // Used to make sure that we don't endlessly retry matches.
    last_compared: Option<Arc<Book>>,
    // Store selected values (no relative indices).
    // When up/down etc is called, find first selected value based on ordering scheme & scroll from there.
    selected: HashSet<Arc<Book>>,
    db: Arc<RwLock<D>>,
}

pub enum Variable {
    Int(i64),
    Str(String),
}

fn read_column(column: &ColumnIdentifier, id: String) -> Option<(String, Option<String>)> {
    match column {
        ColumnIdentifier::Title => Some((
            format!("(SELECT book_id, title as {} from books)", id),
            None,
        )),
        ColumnIdentifier::ID => Some((
            format!("(SELECT book_id, book_id as {} from books)", id),
            None,
        )),
        ColumnIdentifier::Series => None,
        ColumnIdentifier::Author => Some((
            format!(
                r#"(
    SELECT book_id, MIN(value) as {}
    FROM multimap_tags
    WHERE name="author"
    GROUP BY book_id
)"#,
                id
            ),
            None,
        )),
        ColumnIdentifier::NamedTag(tag_name) => Some((
            format!(
                r#"(
    SELECT book_id, value as {}
    FROM named_tags
    WHERE name=?
    GROUP BY book_id
)"#,
                id
            ),
            Some(tag_name.clone()),
        )), // named_tags / name, "value"
        ColumnIdentifier::Description => None, // variants / description
        ColumnIdentifier::MultiMap(_) => None, // unimplemented
        ColumnIdentifier::MultiMapExact(_, _) => None, // unimplemented
        ColumnIdentifier::Variants => None,    // unsortable
        ColumnIdentifier::Tags => None,        // unsortable
        ColumnIdentifier::ExactTag(_) => None, // unsortable
    }
}

fn order_to_cmp(primary: ColumnOrder, secondary: ColumnOrder) -> &'static str {
    match (primary, secondary) {
        (ColumnOrder::Ascending, ColumnOrder::Ascending) => "<",
        (ColumnOrder::Ascending, ColumnOrder::Descending) => ">",
        (ColumnOrder::Descending, ColumnOrder::Ascending) => ">",
        (ColumnOrder::Descending, ColumnOrder::Descending) => "<",
    }
}

fn order_repr(primary: ColumnOrder, secondary: ColumnOrder) -> &'static str {
    match (primary, secondary) {
        (ColumnOrder::Ascending, ColumnOrder::Ascending) => "DESC",
        (ColumnOrder::Ascending, ColumnOrder::Descending) => "ASC",
        (ColumnOrder::Descending, ColumnOrder::Ascending) => "ASC",
        (ColumnOrder::Descending, ColumnOrder::Descending) => "DESC",
    }
}

/// Returns a query which returns the book ids, relative to the provided book.
/// Results will not include the provided book.
pub fn join_cols(
    book: Option<&Book>,
    sort_rules: &[(ColumnIdentifier, ColumnOrder)],
    order: ColumnOrder,
) -> (String, Vec<Variable>) {
    let mut from = String::new();
    let mut where_str = String::new();
    let mut join = String::new();
    let mut order_str = String::new();
    let mut num_ops = 0;
    let mut bind_vars = vec![];
    // SELECT book_id, MIN(value) from multimap_tags WHERE name={} GROUP BY book_id
    // or max
    // multimap / description: use GROUP_CONCAT (+ sort) or just regular min to join authors in sorted order
    // SELECT
    //     all fields involved
    // FROM
    //     books
    // INNER JOIN named_tags AS TAGS ON
    // TAGS.book_id = books.book_id
    // AND TAGS.name = {named_tag}
    // WHERE (sort_start > ...)
    // ORDER BY COL1 ASC, COL2 DESC
    // LIMIT n;
    // select all keys needed to sort
    // need FROM () as below to select a particular mmap key
    // Need rule for reading each multimap key
    // Need rule for reading books if title
    // Need rule for reading named tag
    // SELECT books.book_id FROM (
    //     SELECT book_id, MIN(value) as mvalue
    //     FROM multimap_tags
    //     WHERE name= "author"
    //     GROUP BY book_id
    // ) as A INNER JOIN books ON (A.book_id = books.book_id)
    // WHERE (books.book_id > 291 AND mvalue >= "Alastair")
    // ORDER BY mvalue ASC, ..., books.book_id ASC
    // LIMIT 5;

    // Add the id comparison to ensure pagination doesn't repeat items
    let id_rule = if !sort_rules
        .iter()
        .any(|(col_id, _)| matches!(col_id, ColumnIdentifier::ID))
    {
        Some((ColumnIdentifier::ID, ColumnOrder::Ascending))
    } else {
        None
    };

    for ((col_id, col_ord), alias) in sort_rules
        .iter()
        .chain(id_rule.iter())
        .zip(ASCII_LOWER.iter())
    {
        match read_column(col_id, alias.to_string()) {
            None => {}
            Some((select, bound)) => {
                bind_vars.extend(bound.map(Variable::Str).into_iter());
                let cmp = order_to_cmp(col_ord.clone(), order.clone());
                let table_alias = format!("{}TABLE", alias.to_ascii_uppercase());
                if num_ops == 0 {
                    from.push_str(&select);
                    from.push_str(&format!(" as {}TABLE ", alias.to_ascii_uppercase()));
                } else {
                    join.push_str(&format!(
                        "INNER JOIN {} as {} ON {}.book_id = ATABLE.book_id ",
                        select, table_alias, table_alias
                    ));
                }
                if let Some(book) = book {
                    if matches!(col_id, ColumnIdentifier::ID) {
                        where_str.push_str(&format!("{}.{} {} ? AND ", table_alias, alias, cmp));
                        bind_vars.push(Variable::Int(u64::from(book.id()) as i64));
                    } else {
                        let cmp_key = book.get_column(col_id).unwrap_or_default();
                        where_str.push_str(&format!("{}.{} {}= ? AND ", table_alias, alias, cmp));
                        bind_vars.push(Variable::Str(cmp_key.to_string()));
                    }
                }
                order_str.push_str(&format!(
                    "{} {}, ",
                    alias,
                    order_repr(col_ord.clone(), order.clone())
                ));
                num_ops += 1;
            }
        }
    }

    // Clean up string.
    from.pop();
    if book.is_some() {
        where_str.truncate(where_str.len() - 5);
        where_str = format!("WHERE ({})", where_str);
    }
    order_str.pop();
    order_str.pop();
    (
        format!(
            "SELECT ATABLE.book_id FROM {} {} {} ORDER BY {} LIMIT ?;",
            from, join, where_str, order_str
        ),
        bind_vars,
    )
}

impl<D: IndexableDatabase> Paginator<D> {
    pub fn new(
        db: Arc<RwLock<D>>,
        window_size: usize,
        sorting_rules: Box<[(ColumnIdentifier, ColumnOrder)]>,
    ) -> Self {
        Self {
            books: vec![],
            window_size,
            window_top: 0,
            sorting_rules,
            matching_rules: vec![].into_boxed_slice(),
            last_compared: None,
            selected: Default::default(),
            db,
        }
    }

    pub fn window(&self) -> &[Arc<Book>] {
        &self.books[self.window_top..self.window_top + self.window_size]
    }

    // TODO: Check that page is full & attempt to fill it by scrolling upwards.
    pub async fn scroll_down(&mut self, len: usize) -> Result<(), DatabaseError<D::Error>> {
        if self.window_top + self.window_size + len > self.books.len() {
            // need items from top of window + len, covering window size items
            let start_from = self.window_top;
            let limit = self.window_size + len;
            let (query, mut bindings) = join_cols(
                self.books.get(start_from).map(|book| book.as_ref()),
                &self.sorting_rules,
                ColumnOrder::Descending,
            );
            // skip len items, read full window
            bindings.push(Variable::Int((len + self.window_size) as i64));
            let mut books = self
                .db
                .write()
                .await
                .perform_query(&query, &bindings)
                .await?
                .into_iter();
            if let Some(slice) = self.books.get_mut(self.window_top + 1..) {
                for old_book in slice {
                    if let Some(new_book) = books.next() {
                        *old_book = new_book;
                    } else {
                        break;
                    }
                }
            }
            self.books.extend(books);
            self.window_top += len;
            Ok(())
        } else {
            self.window_top += len;
            Ok(())
        }
    }

    pub async fn home(&mut self) -> Result<(), DatabaseError<D::Error>> {
        if self.selected.is_empty() {
            self.window_top = 0;
            self.books.clear();
            self.scroll_down(0).await
        } else {
            unimplemented!()
        }
    }

    pub async fn end(&mut self) -> Result<(), DatabaseError<D::Error>> {
        if self.selected.is_empty() {
            self.window_top = 0;
            let (query, mut bindings) =
                join_cols(None, &self.sorting_rules, ColumnOrder::Ascending);
            bindings.push(Variable::Int(self.window_size as i64));
            let mut books = self
                .db
                .write()
                .await
                .perform_query(&query, &bindings)
                .await?;
            books.reverse();
            self.books = books;
            Ok(())
        } else {
            unimplemented!()
        }
    }

    // TODO: Check that page is full & attempt to fill it by scrolling downwards.
    pub async fn scroll_up(&mut self, len: usize) -> Result<(), DatabaseError<D::Error>> {
        match self.window_top.checked_sub(len) {
            None => {
                let start_from = self.window_top;
                let (query, mut bindings) = join_cols(
                    self.books.get(start_from).map(|book| book.as_ref()),
                    &self.sorting_rules,
                    ColumnOrder::Ascending,
                );
                // skip len items, read full window
                bindings.push(Variable::Int((len - self.window_top) as i64));
                let mut books = self
                    .db
                    .write()
                    .await
                    .perform_query(&query, &bindings)
                    .await?;
                //
                // need to prepend
                if let Some(slice) = self.books.get_mut(..self.window_top) {
                    for old_book in slice.iter_mut().rev() {
                        if let Some(new_book) = books.pop() {
                            *old_book = new_book;
                        } else {
                            break;
                        }
                    }
                }
                books.reverse();
                books.extend_from_slice(&mut self.books);
                self.books = books;
                self.window_top = 0;
                Ok(())
            }
            Some(window_top) => {
                self.window_top = window_top;
                Ok(())
            }
        }
    }
}
