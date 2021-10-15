use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;

use bookstore_records::book::{BookID, ColumnIdentifier};
use bookstore_records::{Book, ColumnOrder};

use crate::search::Matcher;
use crate::{log, AppDatabase, DatabaseError};

static ASCII_LOWER: [char; 26] = [
    'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q', 'r', 's',
    't', 'u', 'v', 'w', 'x', 'y', 'z',
];

type PaginatorResult<E> = Result<(), DatabaseError<E>>;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
}

pub enum Selection {
    All,
    Partial(
        HashMap<BookID, Arc<Book>>,
        Box<[(ColumnIdentifier, ColumnOrder)]>,
    ),
    Range(
        Arc<Book>,
        Arc<Book>,
        Box<[(ColumnIdentifier, ColumnOrder)]>,
        Direction,
    ),
    Empty,
}

impl Selection {
    pub fn is_empty(&self) -> bool {
        match self {
            Selection::All => false,
            Selection::Partial(books, _) => books.is_empty(),
            Selection::Range(_, _, _, _) => false,
            Selection::Empty => true,
        }
    }

    fn contains(&self, book: &Book) -> bool {
        match self {
            Selection::All => true,
            Selection::Partial(books, _) => books.contains_key(&book.id()),
            Selection::Range(start, stop, cols, _) => {
                start.cmp_columns(book, cols).is_le() && stop.cmp_columns(book, cols).is_ge()
            }
            Selection::Empty => false,
        }
    }

    fn clear(&mut self) {
        *self = Selection::Empty;
    }

    pub fn front(&self) -> Option<&Arc<Book>> {
        match self {
            Selection::All => None,
            Selection::Partial(books, sorting_rules) => books
                .values()
                .min_by(|a, b| a.cmp_columns(b, sorting_rules)),
            Selection::Range(start, _, _, Direction::Up) => Some(start),
            Selection::Range(_, end, _, Direction::Down) => Some(end),
            Selection::Empty => None,
        }
    }

    pub fn first(&self) -> Option<&Arc<Book>> {
        match self {
            Selection::All => None,
            Selection::Partial(books, sorting_rules) => books
                .values()
                .min_by(|a, b| a.cmp_columns(b, sorting_rules)),
            Selection::Range(start, _, _, _) => Some(start),
            Selection::Empty => None,
        }
    }

    fn last(&self) -> Option<&Arc<Book>> {
        match self {
            Selection::All => None,
            Selection::Partial(books, sorting_rules) => books
                .values()
                .max_by(|a, b| a.cmp_columns(b, sorting_rules)),
            Selection::Range(_, end, _, _) => Some(end),
            Selection::Empty => None,
        }
    }

    fn is_single(&self) -> bool {
        match self {
            Selection::All => false,
            Selection::Partial(books, sorting_rules) => books.len() == 1,
            Selection::Range(start, end, _, _) => start.id() == end.id(),
            Selection::Empty => false,
        }
    }
}

pub fn range_select_query(
    start: &Book,
    end: &Book,
    sort: &[(ColumnIdentifier, ColumnOrder)],
) -> (String, Vec<Variable>) {
    QueryBuilder::default()
        .sort_rules(sort)
        .between_books(start, end)
}

struct QueryBuilder {
    order: ColumnOrder,
    sort_rules: Vec<(ColumnIdentifier, ColumnOrder)>,
    id_inclusive: bool,
}

// TODO: Rapid scrolling is slow
impl Default for QueryBuilder {
    fn default() -> Self {
        Self {
            order: ColumnOrder::Descending,
            sort_rules: vec![(ColumnIdentifier::ID, ColumnOrder::Ascending)],
            id_inclusive: false,
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

    /// Returns a query which returns the book ids, relative to the provided book.
    /// Results will not include the provided book.
    fn join_cols(
        &self,
        book: Option<&Book>,
        match_rules: &Box<[Box<dyn Matcher + Send + Sync>]>,
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
        let mut ascii_iter = ASCII_LOWER.iter();
        for ((col_id, col_ord), alias) in self.sort_rules.iter().zip(ascii_iter.by_ref()) {
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

        for (match_rule, alias) in match_rules.iter().zip(ascii_iter) {
            let (col_id, query_str, var) = match_rule.sql_query();
            match read_column(col_id, alias.to_string()) {
                None => {}
                Some((select, bound)) => {
                    bind_vars.extend(bound.map(Variable::Str).into_iter());
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
                    where_str.push_str(&format!("{}.{} {} AND ", table_alias, alias, query_str));
                    bind_vars.extend(var.into_iter());
                    num_ops += 1;
                }
            }
        }
        // Clean up string.
        from.pop();
        if !where_str.is_empty() {
            where_str.truncate(where_str.len() - 5);
            where_str = format!("WHERE ({})", where_str);
        }
        order_str.pop();
        order_str.pop();
        let query = format!(
            "SELECT ATABLE.book_id FROM {} {} {} ORDER BY {} LIMIT ?;",
            from, join, where_str, order_str
        );
        log(&query);
        (query, bind_vars)
    }

    fn between_books(&self, start: &Book, end: &Book) -> (String, Vec<Variable>) {
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

        let op_order = match self.order {
            ColumnOrder::Ascending => ColumnOrder::Descending,
            ColumnOrder::Descending => ColumnOrder::Ascending,
        };
        // Add the id comparison to ensure pagination doesn't repeat items
        for ((col_id, col_ord), alias) in self.sort_rules.iter().zip(ASCII_LOWER.iter()) {
            match read_column(col_id, alias.to_string()) {
                None => {}
                Some((select, bound)) => {
                    bind_vars.extend(bound.map(Variable::Str).into_iter());
                    let start_cmp = order_to_cmp(col_ord.clone(), self.order.clone());
                    let end_cmp = order_to_cmp(col_ord.clone(), op_order.clone());
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
                    if matches!(col_id, ColumnIdentifier::ID) {
                        for (id, cmp) in [(start.id(), &start_cmp), (end.id(), &end_cmp)] {
                            if self.id_inclusive {
                                where_str.push_str(&format!(
                                    "{}.{} {}= ? AND ",
                                    table_alias, alias, cmp
                                ));
                            } else {
                                where_str
                                    .push_str(&format!("{}.{} {} ? AND ", table_alias, alias, cmp));
                            }
                            bind_vars.push(Variable::Int(u64::from(id) as i64));
                        }
                    } else {
                        for (book, cmp) in [(&start, &start_cmp), (&end, &end_cmp)] {
                            if let Some(cmp_key) = book.get_column(col_id) {
                                where_str.push_str(&format!(
                                    "{}.{} {}= ? AND ",
                                    table_alias, alias, cmp
                                ));
                                bind_vars.push(Variable::Str(cmp_key.to_string()));
                            }
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
        where_str.truncate(where_str.len() - 5);
        where_str = format!("WHERE ({})", where_str);
        order_str.pop();
        order_str.pop();
        (
            format!(
                "SELECT ATABLE.book_id FROM {} {} {} ORDER BY {};",
                from, join, where_str, order_str
            ),
            bind_vars,
        )
    }

    // fn select(&self, book: Option<&Book>) -> Select {
    //     let mut select = Select::default();
    //     let mut num_ops = 0;
    //     for ((col_id, col_ord), alias) in self.sort_rules.iter().zip(ASCII_LOWER.iter()) {
    //         let alias = alias.to_string();
    //         match column_select(col_id, alias.clone()) {
    //             None => {}
    //             Some(sub_select) => {
    //                 let table_alias = format!("{}TABLE", alias.to_ascii_uppercase());
    //                 let table = Table::from(sub_select).alias(&table_alias);
    //
    //                 if num_ops == 0 {
    //                     select = Select::from_table(table)
    //                         .column(Column::from("book_id").table(table_alias.clone()));
    //                 } else {
    //                     select = select.inner_join(
    //                         table.on(Column::new("book_id")
    //                             .table(table_alias.clone())
    //                             .equals(Column::new("book_id").table("ATABLE"))),
    //                     );
    //                 }
    //
    //                 if let Some(book) = book {
    //                     if matches!(col_id, ColumnIdentifier::ID) {
    //                         let cmp_key = u64::from(book.id()) as i64;
    //                         let column = Column::new(&alias).table(table_alias.clone());
    //                         let compare = match (self.id_inclusive, col_ord == &self.order) {
    //                             (false, false) => column.greater_than(cmp_key),
    //                             (false, true) => column.less_than(cmp_key),
    //                             (true, false) => column.greater_than_or_equals(cmp_key),
    //                             (true, true) => column.less_than_or_equals(cmp_key),
    //                         };
    //                         select = select.and_where(compare);
    //                     } else if let Some(cmp_key) = book.get_column(col_id) {
    //                         let cmp_key = cmp_key.to_string();
    //                         let column = Column::new(&alias).table(table_alias.clone());
    //                         let compare = if col_ord == &self.order {
    //                             column.less_than_or_equals(cmp_key)
    //                         } else {
    //                             column.greater_than_or_equals(cmp_key)
    //                         };
    //
    //                         select = select.and_where(compare);
    //                     }
    //                 }
    //                 let ordering = match (col_ord, self.order) {
    //                     (ColumnOrder::Ascending, ColumnOrder::Ascending) => alias.descend(),
    //                     (ColumnOrder::Ascending, ColumnOrder::Descending) => alias.ascend(),
    //                     (ColumnOrder::Descending, ColumnOrder::Ascending) => alias.ascend(),
    //                     (ColumnOrder::Descending, ColumnOrder::Descending) => alias.descend(),
    //                 };
    //
    //                 select = select.order_by(ordering);
    //                 num_ops += 1;
    //             }
    //         }
    //     }
    //     select
    // }
}

/// Paginator provides a fast way to scroll through book databases which allow both sorting
/// and greater than comparisons.
/// The paginator stores information about existing matching and sorting rules, and when scrolled
/// to the end, will automatically fetch new items, in the correct order, which match the internal
/// rules.
pub struct Paginator<D: AppDatabase + 'static> {
    books: Vec<Arc<Book>>,
    window_size: usize,
    // relative to start of books
    window_top: usize,
    // Some set of sorting rules.
    sorting_rules: Box<[(ColumnIdentifier, ColumnOrder)]>,
    matching_rules: Box<[Box<dyn Matcher + Send + Sync>]>,
    // Used to make sure that we don't endlessly retry matches.
    last_compared: Option<Arc<Book>>,
    // Store selected values (no relative indices).
    // When up/down etc is called, find first selected value based on ordering scheme & scroll from there.
    selected: Selection,
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

// fn column_select(column: &ColumnIdentifier, id: String) -> Option<Select> {
//     match column {
//         ColumnIdentifier::Title => Some(
//             Select::from_table("books")
//                 .column("book_id")
//                 .column(Column::from("title").alias(id)),
//         ),
//         ColumnIdentifier::ID => Some(
//             Select::from_table("books")
//                 .column("book_id")
//                 .column(Column::from("book_id").alias(id)),
//         ),
//         ColumnIdentifier::Series => None,
//         ColumnIdentifier::Author => Some(
//             Select::from_table("multimap_tags")
//                 .column("book_id")
//                 .column(Column::from("MIN(value)").alias(id))
//                 .and_where("name".equals("author"))
//                 .group_by("book_id"),
//         ),
//         ColumnIdentifier::NamedTag(tag_name) => Some(
//             Select::from_table("named_tags")
//                 .column("book_id")
//                 .column(Column::from("value").alias(id))
//                 .and_where("name".equals(tag_name.clone()))
//                 .group_by("book_id"),
//         ),
//         ColumnIdentifier::Description => None, // variants / description
//         ColumnIdentifier::MultiMap(_) => None, // unimplemented
//         ColumnIdentifier::MultiMapExact(_, _) => None, // unimplemented
//         ColumnIdentifier::Variants => None,    // unsortable
//         ColumnIdentifier::Tags => None,        // unsortable
//         ColumnIdentifier::ExactTag(_) => None, // unsortable
//     }
// }

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

fn add_id(
    sorting_rules: Box<[(ColumnIdentifier, ColumnOrder)]>,
) -> Box<[(ColumnIdentifier, ColumnOrder)]> {
    if !sorting_rules
        .iter()
        .any(|(col_id, _)| matches!(col_id, ColumnIdentifier::ID))
    {
        let mut sorting_rules = sorting_rules.to_vec();
        sorting_rules.push((ColumnIdentifier::ID, ColumnOrder::Ascending));
        sorting_rules.into_boxed_slice()
    } else {
        sorting_rules
    }
}

impl<D: AppDatabase + Send + Sync> Paginator<D> {
    pub fn new(
        db: Arc<RwLock<D>>,
        window_size: usize,
        sorting_rules: Box<[(ColumnIdentifier, ColumnOrder)]>,
    ) -> Self {
        Self {
            books: vec![],
            window_size,
            window_top: 0,
            sorting_rules: add_id(sorting_rules),
            matching_rules: vec![].into_boxed_slice(),
            last_compared: None,
            selected: Selection::Empty,
            db,
        }
    }

    pub fn selected(&self) -> &Selection {
        &self.selected
    }

    pub fn bind_match(mut self, matching_rules: Box<[Box<dyn Matcher + Send + Sync>]>) -> Self {
        self.matching_rules = matching_rules;
        self
    }

    pub fn matchers(&self) -> &Box<[Box<dyn Matcher + Send + Sync>]> {
        &self.matching_rules
    }

    pub async fn sort_by(
        &mut self,
        sorting_rules: &[(ColumnIdentifier, ColumnOrder)],
    ) -> Result<(), DatabaseError<D::Error>> {
        self.sorting_rules = add_id(sorting_rules.to_vec().into_boxed_slice());
        log(format!("{:?}", self.sorting_rules));
        let target = self.window().first().cloned();
        self.books.clear();
        self.make_book_visible(target).await
    }

    pub fn window(&self) -> &[Arc<Book>] {
        &self
            .books
            .get(self.window_top..(self.window_top + self.window_size).min(self.books.len()))
            .unwrap_or(&[])
    }

    async fn load_books_after_end(
        &mut self,
        num_books: usize,
    ) -> Result<(), DatabaseError<D::Error>> {
        if num_books == 0 {
            return Ok(());
        }
        let (query, bindings) = QueryBuilder::default()
            .sort_rules(&self.sorting_rules)
            .order(ColumnOrder::Descending)
            .join_cols(self.books.last().map(|x| x.as_ref()), &self.matching_rules);
        let books = self
            .db
            .write()
            .await
            .perform_query(&query, &bindings, num_books)
            .await?;

        let db = self.db.clone();
        tokio::spawn(async move {
            let start = std::time::Instant::now();
            let _ = db
                .write()
                .await
                .perform_query(&query, &bindings, num_books * 5)
                .await;
            let end = std::time::Instant::now();
            log(format!("Took {}s to prefetch", (end - start).as_secs_f32()));
        });

        self.books.extend(books);
        Ok(())
    }

    // Should take the book by value to allow jumping around
    async fn load_books_before_start(
        &mut self,
        num_books: usize,
    ) -> Result<(), DatabaseError<D::Error>> {
        if num_books == 0 {
            return Ok(());
        }

        let (query, bindings) = QueryBuilder::default()
            .sort_rules(&self.sorting_rules)
            .order(ColumnOrder::Ascending)
            .join_cols(self.books.first().map(|x| x.as_ref()), &self.matching_rules);
        // Read only the number of items needed to fill top.
        // TODO: If len is a large jump, we should do an OFFSET instead of loading
        //  everything in-between (check if len > some multiple of window size),
        //  and drop all ensuing books.
        let mut books = self
            .db
            .write()
            .await
            .perform_query(&query, &bindings, num_books)
            .await?;

        let db = self.db.clone();
        tokio::spawn(async move {
            let start = std::time::Instant::now();
            let _ = db
                .write()
                .await
                .perform_query(&query, &bindings, num_books * 5)
                .await;
            let end = std::time::Instant::now();
            log(format!("Took {}s to prefetch", (end - start).as_secs_f32()));
        });

        if !books.is_empty() {
            books.reverse();
            books.extend_from_slice(&mut self.books);
            self.books = books;
        }
        Ok(())
    }

    pub async fn make_book_visible<B: AsRef<Book>>(
        &mut self,
        book: Option<B>,
    ) -> Result<(), DatabaseError<D::Error>> {
        let builder = QueryBuilder::default()
            .sort_rules(&self.sorting_rules)
            .order(ColumnOrder::Descending)
            .include_id(true);

        let (query, bindings) = match &book {
            None => builder.join_cols(None, &self.matching_rules),
            Some(b) => {
                if self
                    .window()
                    .iter()
                    .any(|book| book.id() == b.as_ref().id())
                {
                    return Ok(());
                }

                match self.db.read().await.get_book(b.as_ref().id()).await {
                    Ok(fresh) => builder.join_cols(Some(&fresh), &self.matching_rules),
                    Err(_) => builder.join_cols(Some(b.as_ref()), &self.matching_rules),
                }
            }
        };

        self.books = self
            .db
            .write()
            .await
            .perform_query(&query, &bindings, self.window_size)
            .await?;
        self.window_top = 0;
        let limit = self.window_size - self.books.len();
        if limit != 0 {
            self.load_books_before_start(limit).await?;
        }
        Ok(())
    }

    async fn select_and_make_visible(
        &mut self,
        target: Arc<Book>,
    ) -> Result<(), DatabaseError<D::Error>> {
        self.make_book_visible(Some(target.clone())).await?;

        self.selected = Selection::Range(
            target.clone(),
            target.clone(),
            self.sorting_rules.clone(),
            Direction::Down,
        );

        Ok(())
    }

    async fn scroll_up_move_select(&mut self, len: usize) -> Result<(), DatabaseError<D::Error>> {
        if let Some(target) = self.selected.first().cloned() {
            return if self.selected.is_single() {
                let (query, bindings) = QueryBuilder::default()
                    .sort_rules(&self.sorting_rules)
                    .order(ColumnOrder::Ascending)
                    .join_cols(Some(target.as_ref()), &self.matching_rules);
                let book = self
                    .db
                    .write()
                    .await
                    .perform_query(&query, &bindings, len)
                    .await?
                    .pop()
                    .unwrap_or_else(|| target.clone());
                if !self.window().iter().any(|x| x.id() == book.id()) {
                    self.scroll_up(len).await?;
                }

                self.selected = Selection::Range(
                    book.clone(),
                    book.clone(),
                    self.sorting_rules.clone(),
                    Direction::Down,
                );
                Ok(())
            } else {
                self.select_and_make_visible(target).await
            };
        }

        self.scroll_up(len).await
    }

    async fn scroll_down_move_select(&mut self, len: usize) -> Result<(), DatabaseError<D::Error>> {
        if let Some(target) = self.selected.last().cloned() {
            return if self.selected.is_single() {
                let (query, bindings) = QueryBuilder::default()
                    .sort_rules(&self.sorting_rules)
                    .order(ColumnOrder::Descending)
                    .join_cols(Some(target.as_ref()), &self.matching_rules);
                let book = self
                    .db
                    .write()
                    .await
                    .perform_query(&query, &bindings, len)
                    .await?
                    .pop()
                    .unwrap_or_else(|| target.clone());
                if !self.window().iter().any(|x| x.id() == book.id()) {
                    self.scroll_down(len).await?;
                }

                self.selected = Selection::Range(
                    book.clone(),
                    book.clone(),
                    self.sorting_rules.clone(),
                    Direction::Down,
                );
                Ok(())
            } else {
                self.select_and_make_visible(target).await
            };
        }

        self.scroll_down(len).await
    }

    pub async fn scroll_down(&mut self, len: usize) -> Result<(), DatabaseError<D::Error>> {
        match (self.window_top + self.window_size + len).checked_sub(self.books.len()) {
            None => {
                self.window_top += len;
                Ok(())
            }
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
            } // checked above that it will contain enough books to fill a window.
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

    pub async fn home(&mut self) -> Result<(), DatabaseError<D::Error>> {
        match (self.selected.first().cloned(), self.selected.is_single()) {
            (None, _) | (Some(_), true) => {
                self.window_top = 0;
                self.books.clear();
                self.scroll_down(0).await?;
                if self.selected.is_single() {
                    if let Some(target) = self.window().first().cloned() {
                        self.selected = Selection::Range(
                            target.clone(),
                            target,
                            self.sorting_rules.clone(),
                            Direction::Down,
                        );
                    }
                }
                Ok(())
            }
            (Some(target), false) => self.select_and_make_visible(target).await,
        }
    }

    pub async fn end(&mut self) -> Result<(), DatabaseError<D::Error>> {
        match (self.selected.last().cloned(), self.selected.is_single()) {
            (None, _) | (Some(_), true) => {
                self.window_top = 0;
                let (query, bindings) = QueryBuilder::default()
                    .sort_rules(&self.sorting_rules)
                    .order(ColumnOrder::Ascending)
                    .join_cols(None, &self.matching_rules);

                self.books = self
                    .db
                    .write()
                    .await
                    .perform_query(&query, &bindings, self.window_size)
                    .await?;
                self.books.reverse();

                if self.selected.is_single() {
                    if let Some(target) = self.window().last().cloned() {
                        self.selected = Selection::Range(
                            target.clone(),
                            target,
                            self.sorting_rules.clone(),
                            Direction::Down,
                        );
                    }
                }

                Ok(())
            }
            (Some(target), false) => self.select_and_make_visible(target).await,
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
            .find(|x| self.selected.contains(&x))
            .or_else(|| self.window().first())
            .cloned();
        self.books.clear();
        self.make_book_visible(target).await
    }

    pub fn window_size(&self) -> usize {
        self.window_size
    }

    pub fn deselect(&mut self) {
        self.selected.clear();
    }
    // https://github.com/rusqlite/rusqlite/blob/6a22bb7a56d4be48f5bea81c40ccc496fc74bb57/src/functions.rs#L844

    pub fn relative_selections(&self) -> Vec<usize> {
        self.window()
            .iter()
            .enumerate()
            .filter(|(i, book)| self.selected.contains(book.as_ref()))
            .map(|(i, _)| i)
            .collect()
    }

    pub fn sort_rules(&self) -> &[(ColumnIdentifier, ColumnOrder)] {
        &self.sorting_rules
    }
}

impl<D: AppDatabase + Send + Sync> Paginator<D> {
    pub async fn page_up(&mut self) -> PaginatorResult<D::Error> {
        self.scroll_up_move_select(self.window_size).await
    }

    pub async fn page_down(&mut self) -> PaginatorResult<D::Error> {
        self.scroll_down_move_select(self.window_size).await
    }

    pub async fn up(&mut self) -> PaginatorResult<D::Error> {
        self.scroll_up_move_select(1).await
    }

    pub async fn down(&mut self) -> PaginatorResult<D::Error> {
        self.scroll_down_move_select(1).await
    }

    async fn select_up_on(
        &mut self,
        len: usize,
        selection: Selection,
    ) -> Result<Selection, DatabaseError<D::Error>> {
        match selection {
            Selection::All => {
                self.scroll_up(len).await?;
                Ok(Selection::All)
            }
            Selection::Partial(books, rules) => Ok(Selection::Partial(books, rules)),
            Selection::Range(start, end, sorting_rules, Direction::Down) => {
                // Need to get end + len'th book.
                let (query, bound_variables) = QueryBuilder::default()
                    .order(ColumnOrder::Ascending)
                    .sort_rules(&self.sorting_rules)
                    .join_cols(Some(end.as_ref()), &self.matching_rules);
                let book = self
                    .db
                    .write()
                    .await
                    .perform_query(&query, &bound_variables, len)
                    .await?
                    .pop();
                if let Some(book) = book {
                    // need to flip
                    if !self.window().iter().any(|x| x.id() == book.id()) {
                        self.scroll_up(len).await?;
                    }

                    if book.cmp_columns(&start, &sorting_rules).is_lt() {
                        Ok(Selection::Range(book, start, sorting_rules, Direction::Up))
                    } else {
                        Ok(Selection::Range(
                            start,
                            book,
                            sorting_rules,
                            Direction::Down,
                        ))
                    }
                } else {
                    Ok(Selection::Range(start, end, sorting_rules, Direction::Down))
                }
            }
            Selection::Range(start, end, sorting_rules, Direction::Up) => {
                // Need to get end + len'th book.
                let (query, bound_variables) = QueryBuilder::default()
                    .order(ColumnOrder::Ascending)
                    .sort_rules(&self.sorting_rules)
                    .join_cols(Some(start.as_ref()), &self.matching_rules);
                let book = self
                    .db
                    .write()
                    .await
                    .perform_query(&query, &bound_variables, len)
                    .await?
                    .pop();
                if let Some(book) = book {
                    if !self.window().iter().any(|x| x.id() == book.id()) {
                        self.scroll_up(len).await?;
                    }

                    Ok(Selection::Range(book, end, sorting_rules, Direction::Up))
                } else {
                    Ok(Selection::Range(start, end, sorting_rules, Direction::Up))
                }
            }
            Selection::Empty => {
                let (query, bound_variables) = QueryBuilder::default()
                    .order(ColumnOrder::Ascending)
                    .sort_rules(&self.sorting_rules)
                    .include_id(true)
                    .join_cols(
                        self.window().last().map(|x| x.as_ref()),
                        &self.matching_rules,
                    );
                let book = self
                    .db
                    .write()
                    .await
                    .perform_query(&query, &bound_variables, len)
                    .await?
                    .pop();
                if let Some(book) = book {
                    let end = self
                        .window()
                        .last()
                        .cloned()
                        .unwrap_or_else(|| book.clone());
                    if !self.window().iter().any(|x| x.id() == book.id()) {
                        self.scroll_up(len).await?;
                    }
                    Ok(Selection::Range(
                        book,
                        end,
                        self.sorting_rules.clone(),
                        Direction::Down,
                    ))
                } else {
                    Ok(Selection::Empty)
                }
            }
        }
    }

    pub async fn select_up(&mut self, len: usize) -> PaginatorResult<D::Error> {
        let selection = std::mem::replace(&mut self.selected, Selection::Empty);
        let candidate = self.select_up_on(len, selection).await;
        match candidate {
            Ok(selection) => {
                self.selected = selection;
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    async fn select_down_on(
        &mut self,
        len: usize,
        selection: Selection,
    ) -> Result<Selection, DatabaseError<D::Error>> {
        match selection {
            Selection::All => {
                self.scroll_down(len).await?;
                Ok(Selection::All)
            }
            Selection::Partial(books, rules) => Ok(Selection::Partial(books, rules)),
            Selection::Range(start, end, sorting_rules, Direction::Down) => {
                // Need to get end + len'th book.
                let (query, bound_variables) = QueryBuilder::default()
                    .order(ColumnOrder::Descending)
                    .sort_rules(&self.sorting_rules)
                    .join_cols(Some(end.as_ref()), &self.matching_rules);
                let book = self
                    .db
                    .write()
                    .await
                    .perform_query(&query, &bound_variables, len)
                    .await?
                    .pop();
                if let Some(book) = book {
                    if !self.window().iter().any(|x| x.id() == book.id()) {
                        self.scroll_down(len).await?;
                    }

                    Ok(Selection::Range(
                        start,
                        book,
                        sorting_rules,
                        Direction::Down,
                    ))
                } else {
                    Ok(Selection::Range(start, end, sorting_rules, Direction::Down))
                }
            }
            Selection::Range(start, end, sorting_rules, Direction::Up) => {
                // Need to get end + len'th book.
                let (query, bound_variables) = QueryBuilder::default()
                    .order(ColumnOrder::Descending)
                    .sort_rules(&self.sorting_rules)
                    .join_cols(Some(start.as_ref()), &self.matching_rules);
                let book = self
                    .db
                    .write()
                    .await
                    .perform_query(&query, &bound_variables, len)
                    .await?
                    .pop();
                if let Some(book) = book {
                    // need to flip
                    if !self.window().iter().any(|x| x.id() == book.id()) {
                        self.scroll_down(len).await?;
                    }

                    if book.cmp_columns(&end, &sorting_rules).is_gt() {
                        Ok(Selection::Range(end, book, sorting_rules, Direction::Down))
                    } else {
                        Ok(Selection::Range(book, end, sorting_rules, Direction::Up))
                    }
                } else {
                    Ok(Selection::Range(start, end, sorting_rules, Direction::Up))
                }
            }
            Selection::Empty => {
                let (query, bound_variables) = QueryBuilder::default()
                    .order(ColumnOrder::Descending)
                    .sort_rules(&self.sorting_rules)
                    .include_id(true)
                    .join_cols(
                        self.window().first().map(|x| x.as_ref()),
                        &self.matching_rules,
                    );
                let book = self
                    .db
                    .write()
                    .await
                    .perform_query(&query, &bound_variables, len)
                    .await?
                    .pop();
                if let Some(book) = book {
                    let start = self
                        .window()
                        .first()
                        .cloned()
                        .unwrap_or_else(|| book.clone());
                    if !self.window().iter().any(|x| x.id() == book.id()) {
                        self.scroll_down(len).await?;
                    }
                    Ok(Selection::Range(
                        start,
                        book,
                        self.sorting_rules.clone(),
                        Direction::Down,
                    ))
                } else {
                    Ok(Selection::Empty)
                }
            }
        }
    }

    pub async fn select_down(&mut self, len: usize) -> PaginatorResult<D::Error> {
        let selection = std::mem::replace(&mut self.selected, Selection::Empty);
        let candidate = self.select_down_on(len, selection).await;
        match candidate {
            Ok(selection) => {
                self.selected = selection;
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    pub async fn select_all(&mut self) -> PaginatorResult<D::Error> {
        self.selected = Selection::All;
        Ok(())
    }

    pub async fn select_page_up(&mut self) -> PaginatorResult<D::Error> {
        self.select_up(self.window_size).await
    }

    pub async fn select_page_down(&mut self) -> PaginatorResult<D::Error> {
        self.select_down(self.window_size).await
    }

    pub async fn select_to_start(&mut self) -> PaginatorResult<D::Error> {
        match self.selected.last().cloned() {
            None => Ok(()),
            Some(book) => {
                self.selected = Selection::Empty;
                self.home().await?;
                self.selected = Selection::Range(
                    book.clone(),
                    self.window().last().cloned().unwrap_or(book),
                    self.sorting_rules.clone(),
                    Direction::Up,
                );
                Ok(())
            }
        }
    }

    pub async fn select_to_end(&mut self) -> PaginatorResult<D::Error> {
        match self.selected.first().cloned() {
            None => Ok(()),
            Some(book) => {
                self.selected = Selection::Empty;
                self.end().await?;
                self.selected = Selection::Range(
                    self.window()
                        .first()
                        .cloned()
                        .unwrap_or_else(|| book.clone()),
                    book,
                    self.sorting_rules.clone(),
                    Direction::Down,
                );
                Ok(())
            }
        }
    }
}
