use crate::search::{Matcher, Search};
use crate::{AppDatabase, DatabaseError};
use bookstore_records::book::{BookID, ColumnIdentifier};
use bookstore_records::{Book, ColumnOrder};
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
struct Paginator<D: AppDatabase> {
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
    let id_rule = if !sort_rules
        .iter()
        .any(|(col_id, _)| matches!(col_id, ColumnIdentifier::ID))
    {
        println!("ADDED ID_RULE");
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
    from.pop();
    where_str.truncate(where_str.len() - 5);
    order_str.pop();
    order_str.pop();
    bind_vars.push(Variable::Int(5));
    (
        format!(
            "SELECT ATABLE.book_id FROM {} {} WHERE ({}) ORDER BY {} LIMIT ?;",
            from, join, where_str, order_str
        ),
        bind_vars,
    )
}

// /// Need to get value from book, and need to determine which table the column identifier belongs to.
// fn sort_rule_to_sql(book: &Book, sort_rule: &(ColumnIdentifier, ColumnOrder), order: ColumnOrder) -> String {
//     match &sort_rule.0 {
//         ColumnIdentifier::Title => {} // books, "title"
//         ColumnIdentifier::ID => {} // books, "book_id"
//         ColumnIdentifier::Series => {} // books, "series_name", "series_id"
//         ColumnIdentifier::Author => {} // ? how do you sort by author pl? multimap_tags, "author"
//         ColumnIdentifier::NamedTag(_) => {} // named_tags / name, "value"
//         ColumnIdentifier::Description => {} // variants / description
//         ColumnIdentifier::MultiMap(_) => {} // unimplemented
//         ColumnIdentifier::MultiMapExact(_, _) => {} // unimplemented
//         ColumnIdentifier::Variants => {} // unsortable
//         ColumnIdentifier::Tags => {} // unsortable
//         ColumnIdentifier::ExactTag(_) => {} // unsortable
//     }
//
//     // SELECT book_id, MIN(value) from multimap_tags WHERE name={} GROUP BY book_id
//     // or max
//     // multimap / description: use GROUP_CONCAT (+ sort) or just regular min to join authors in sorted order
//     // SELECT
//     //     all fields involved
//     // FROM
//     //     books
//     // INNER JOIN named_tags AS TAGS ON
//     // TAGS.book_id = books.book_id
//     // AND TAGS.name = {named_tag}
//     // WHERE (sort_start > ...)
//     // ORDER BY COL1 ASC, COL2 DESC
//     // LIMIT n;
//     // select all keys needed to sort
//     // need FROM () as below to select a particular mmap key
//     // Need rule for reading each multimap key
//     // Need rule for reading books if title
//     // Need rule for reading named tag
//     // SELECT books.book_id FROM (
//     //     SELECT book_id, MIN(value) as mvalue
//     //     FROM multimap_tags
//     //     WHERE name= "author"
//     //     GROUP BY book_id
//     // ) as A INNER JOIN books ON (A.book_id = books.book_id)
//     // WHERE (books.book_id > 291 AND mvalue >= "Alastair")
//     // ORDER BY mvalue ASC, ..., books.book_id ASC
//     // LIMIT 5;
//     unimplemented!()
// }

// /// Returns an SQL query which returns the next n books.
// fn sort_rules_to_sql(book: &Book, sorting_rules: &[(ColumnIdentifier, ColumnOrder)], order: ColumnOrder) -> String {
//     let unique_id_rule = if !sorting_rules.iter().any(|(col_id, col_ord)| col_id == ColumnIdentifier::ID) {
//         Some((ColumnIdentifier::ID, ColumnOrder::Ascending))
//     } else {
//         None
//     };
//
//     sorting_rules.iter().chain(unique_id_rule.as_ref().iter()).map(|x|
//         sort_rule_to_sql(book, x, order)
//     ).collect()
// }

// impl<D: AppDatabase> Paginator<D> {
//     async fn scroll_down(&mut self, len: usize) -> Result<(), D::Error> {
//         if self.window_top + self.window_size + len > self.books.len() {
//             // need items from top of window + len, covering window size items
//             let start_from = self.window_top;
//             let limit = self.window_size + len;
//             let sort_rules = sort_rules_to_sql(&self.books[start_from], &self.sorting_rules, ColumnOrder::Ascending);
//         } else {
//             self.window_top += len;
//             Ok(())
//         }
//     }
// }
