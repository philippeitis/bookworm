use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;

use bookstore_records::book::{BookID, ColumnIdentifier};
use bookstore_records::{Book, ColumnOrder};

use crate::search::Matcher;
use crate::{DatabaseError, IndexableDatabase};

static ASCII_LOWER: [char; 26] = [
    'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q', 'r', 's',
    't', 'u', 'v', 'w', 'x', 'y', 'z',
];

struct QueryBuilder {
    order: ColumnOrder,
    sort_rules: Vec<(ColumnIdentifier, ColumnOrder)>,
    id_inclusive: bool,
    limit: usize,
}

impl Default for QueryBuilder {
    fn default() -> Self {
        Self {
            order: ColumnOrder::Descending,
            sort_rules: vec![(ColumnIdentifier::ID, ColumnOrder::Ascending)],
            id_inclusive: false,
            limit: 0,
        }
    }
}

impl QueryBuilder {
    fn sort_rules(mut self, sort_rules: &[(ColumnIdentifier, ColumnOrder)]) -> Self {
        self.sort_rules = sort_rules.to_vec();
        if !self
            .sort_rules
            .iter()
            .any(|(col_id, _)| matches!(col_id, ColumnIdentifier::ID))
        {
            self.sort_rules
                .push((ColumnIdentifier::ID, ColumnOrder::Ascending));
        };
        self
    }

    fn order(mut self, order: ColumnOrder) -> Self {
        self.order = order;
        self
    }

    fn include_id(mut self, include_id: bool) -> Self {
        self.id_inclusive = include_id;
        self
    }

    fn limit(mut self, limit: usize) -> Self {
        self.limit = limit;
        self
    }

    /// Returns a query which returns the book ids, relative to the provided book.
    /// Results will not include the provided book.
    fn join_cols(&self, book: Option<&Book>) -> (String, Vec<Variable>) {
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
        for ((col_id, col_ord), alias) in self.sort_rules.iter().zip(ASCII_LOWER.iter()) {
            match read_column(col_id, alias.to_string()) {
                None => {}
                Some((select, bound)) => {
                    bind_vars.extend(bound.map(Variable::Str).into_iter());
                    let cmp = order_to_cmp(col_ord.clone(), self.order.clone());
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
                            if self.id_inclusive {
                                where_str.push_str(&format!(
                                    "{}.{} {}= ? AND ",
                                    table_alias, alias, cmp
                                ));
                            } else {
                                where_str
                                    .push_str(&format!("{}.{} {} ? AND ", table_alias, alias, cmp));
                            }
                            bind_vars.push(Variable::Int(u64::from(book.id()) as i64));
                        } else {
                            let cmp_key = book.get_column(col_id).unwrap_or_default();
                            where_str
                                .push_str(&format!("{}.{} {}= ? AND ", table_alias, alias, cmp));
                            bind_vars.push(Variable::Str(cmp_key.to_string()));
                        }
                    }
                    order_str.push_str(&format!(
                        "{} {}, ",
                        alias,
                        order_repr(col_ord.clone(), self.order.clone())
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
        bind_vars.push(Variable::Int(self.limit as i64));
        (
            format!(
                "SELECT ATABLE.book_id FROM {} {} {} ORDER BY {} LIMIT ?;",
                from, join, where_str, order_str
            ),
            bind_vars,
        )
    }
}

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
    selected: HashMap<BookID, Arc<Book>>,
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

impl<D: IndexableDatabase> Paginator<D> {
    pub fn new(
        db: Arc<RwLock<D>>,
        window_size: usize,
        sorting_rules: Box<[(ColumnIdentifier, ColumnOrder)]>,
    ) -> Self {
        let sorting_rules = if !sorting_rules
            .iter()
            .any(|(col_id, _)| matches!(col_id, ColumnIdentifier::ID))
        {
            let mut sorting_rules = sorting_rules.to_vec();
            sorting_rules.push((ColumnIdentifier::ID, ColumnOrder::Ascending));
            sorting_rules.into_boxed_slice()
        } else {
            sorting_rules
        };
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

    async fn load_books_after_end(
        &mut self,
        num_books: usize,
    ) -> Result<(), DatabaseError<D::Error>> {
        let (query, bindings) = QueryBuilder::default()
            .sort_rules(&self.sorting_rules)
            .order(ColumnOrder::Descending)
            .limit(num_books)
            .join_cols(self.books.last().map(|x| x.as_ref()));
        let books = self
            .db
            .write()
            .await
            .perform_query(&query, &bindings)
            .await?;
        self.books.extend(books);
        Ok(())
    }

    // Should take the book by value to allow jumping around
    async fn load_books_before_start(
        &mut self,
        num_books: usize,
    ) -> Result<(), DatabaseError<D::Error>> {
        let (query, bindings) = QueryBuilder::default()
            .sort_rules(&self.sorting_rules)
            .order(ColumnOrder::Ascending)
            .limit(num_books)
            .join_cols(self.books.first().map(|x| x.as_ref()));
        // Read only the number of items needed to fill top.
        // TODO: If len is a large jump, we should do an OFFSET instead of loading
        //  everything in-between (check if len > some multiple of window size),
        //  and drop all ensuing books.
        let mut books = self
            .db
            .write()
            .await
            .perform_query(&query, &bindings)
            .await?;

        if !books.is_empty() {
            books.reverse();
            books.extend_from_slice(&mut self.books);
            self.books = books;
        }
        Ok(())
    }

    pub async fn scroll_down(&mut self, len: usize) -> Result<(), DatabaseError<D::Error>> {
        match (self.window_top + self.window_size + len).checked_sub(self.books.len()) {
            Some(limit) => {
                // TODO If len is a large jump, we should do an OFFSET instead of loading
                //  everything in-between (check if len > some multiple of window size),
                //  and drop all current books.
                self.load_books_after_end(limit).await?;
                match self.books.len().checked_sub(self.window_size) {
                    None => {
                        self.load_books_before_start(self.window_size - self.books.len())
                            .await?;
                        self.window_top = 0;
                    }
                    Some(window_top) => self.window_top = window_top,
                }
                Ok(())
            }
            // checked above that it will contain enough books to fill a window.
            None => {
                self.window_top += len;
                Ok(())
            }
        }
    }

    pub async fn scroll_up(&mut self, len: usize) -> Result<(), DatabaseError<D::Error>> {
        match self.window_top.checked_sub(len) {
            None => {
                self.load_books_before_start(len - self.window_top).await?;
                self.window_top = 0;
            }
            Some(window_top) => {
                self.window_top = window_top;
            }
        }

        match (self.window_top + self.window_size).checked_sub(self.books.len()) {
            // Attempt to scroll down, then scroll up to use all available books
            Some(limit) => {
                self.load_books_after_end(limit).await?;
                self.window_top = self
                    .books
                    .len()
                    .saturating_sub(self.window_size)
                    .min(self.window_top);
                Ok(())
            }
            None => Ok(()),
        }
    }

    pub async fn make_book_visible<B: AsRef<Book>>(
        &mut self,
        book: Option<B>,
    ) -> Result<(), DatabaseError<D::Error>> {
        let book_ref = match &book {
            None => None,
            Some(b) => Some(b.as_ref()),
        };
        let (query, bindings) = QueryBuilder::default()
            .sort_rules(&self.sorting_rules)
            .order(ColumnOrder::Ascending)
            .include_id(true)
            .limit(self.window_size)
            .join_cols(book_ref);

        self.books = self
            .db
            .write()
            .await
            .perform_query(&query, &bindings)
            .await?;
        self.window_top = 0;
        let limit = self.window_size - self.books.len();
        if limit != 0 {
            self.load_books_before_start(limit).await?;
        }
        Ok(())
    }

    pub async fn home(&mut self) -> Result<(), DatabaseError<D::Error>> {
        if self.selected.is_empty() {
            self.window_top = 0;
            self.books.clear();
            self.scroll_down(0).await
        } else {
            let target = self
                .selected
                .values()
                .min_by(|a, b| a.cmp_columns(b, &self.sorting_rules))
                .cloned();
            self.make_book_visible(target).await
        }
    }

    pub async fn end(&mut self) -> Result<(), DatabaseError<D::Error>> {
        if self.selected.is_empty() {
            self.window_top = 0;
            let (query, bindings) = QueryBuilder::default()
                .sort_rules(&self.sorting_rules)
                .order(ColumnOrder::Ascending)
                .limit(self.window_size)
                .join_cols(None);

            self.books = self
                .db
                .write()
                .await
                .perform_query(&query, &bindings)
                .await?;
            self.books.reverse();
            Ok(())
        } else {
            // Go to last selected item. Need to use ID w/ GEQ
            let target = self
                .selected
                .values()
                .max_by(|a, b| a.cmp_columns(b, &self.sorting_rules))
                .cloned();
            self.make_book_visible(target).await
        }
    }

    pub async fn update_window_size(
        &mut self,
        window_size: usize,
    ) -> Result<(), DatabaseError<D::Error>> {
        self.window_size = window_size;
        self.scroll_down(0).await
    }

    pub async fn refresh(&mut self) -> Result<(), DatabaseError<D::Error>> {
        // Find first selected which is visible, otherwise first item in window
        let target = self
            .window()
            .iter()
            .find(|x| self.selected.contains_key(&x.id()))
            .or_else(|| self.window().first())
            .cloned();
        self.make_book_visible(target).await
    }
}
