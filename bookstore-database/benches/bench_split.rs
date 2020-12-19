#![feature(test)]
extern crate test;
use test::Bencher;
use unicase::UniCase;

use std::collections::{HashMap, HashSet};
use std::path;

use indexmap::IndexMap;
use rand::distributions::Alphanumeric;
use rand::prelude::ThreadRng;
use rand::{seq::IteratorRandom, thread_rng, Rng};
use std::ffi::OsString;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Identifier {
    ISBN,
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// Enumerates all supported book types.
pub(crate) enum BookType {
    EPUB,
    MOBI,
    PDF,
    Unsupported(OsString),
}

#[derive(Default, Clone, Debug, PartialEq)]
pub(crate) struct BookVariant {
    pub(crate) local_title: Option<String>,
    pub(crate) identifier: Option<Identifier>,
    pub(crate) paths: Option<Vec<(BookType, path::PathBuf)>>,
    pub(crate) language: Option<String>,
    pub(crate) additional_authors: Option<Vec<String>>,
    pub(crate) translators: Option<Vec<String>>,
    pub(crate) description: Option<String>,
    pub(crate) id: Option<u32>,
}

#[derive(Default, Clone, Debug, PartialEq)]
/// The struct which contains the major fields a book will have, a set of variants,
/// which corresponds to particular file formats of this book (eg. a EPUB and MOBI version),
/// or even differing realizations of the book (eg. a French and English of the same book).
/// Contains an unique ID, and provides storage for additional tags which are not specified here.
pub(crate) struct Book {
    pub(crate) title: Option<String>,
    pub(crate) authors: Option<Vec<String>>,
    pub(crate) series: Option<(String, Option<f32>)>,
    pub(crate) variants: Option<Vec<BookVariant>>,
    pub(crate) id: u32,
    pub(crate) extended_tags: Option<HashMap<String, String>>,
}

fn generate_random_string(rng: &mut ThreadRng, len: usize) -> String {
    rng.sample_iter(&Alphanumeric).take(len).collect()
}

fn generate_random_book(rng: &mut ThreadRng, cols: &[String], id: u32) -> Book {
    let mut b = Book::default();
    b.id = id;

    if rng.gen_bool(0.8) {
        let len = rng.gen_range(3, 25);
        b.title = Some(generate_random_string(rng, len));
    }

    if rng.gen_bool(0.8) {
        let num = rng.gen_range(1, 3);
        let authors = (0..num)
            .into_iter()
            .map(|_| {
                let len = rng.gen_range(3, 25);
                generate_random_string(rng, len)
            })
            .collect();
        b.authors = Some(authors);
    }

    if rng.gen_bool(0.8) {
        let len = rng.gen_range(3, 25);
        let series = generate_random_string(rng, len);
        if rng.gen_bool(0.8) {
            b.series = Some((series, Some(rng.gen())));
        } else {
            b.series = Some((series, None));
        }
    }

    let num_tags = rng.gen_range(0usize, 18).saturating_sub(10);
    if num_tags != 0 {
        let mut h = HashMap::new();
        let cols = cols.iter().choose_multiple(rng, num_tags);
        for col in cols {
            let len = rng.gen_range(3, 15);
            h.insert(col.to_owned(), generate_random_string(rng, len));
        }
        b.extended_tags = Some(h);
    };

    b
}

fn generate_random_dataset() -> IndexMap<u32, Book> {
    let mut rng = thread_rng();
    let mut rand_cols = vec![];
    let mut books = IndexMap::new();
    for _ in 0..100 {
        let len = rng.gen_range(3, 15);
        rand_cols.push(generate_random_string(&mut rng, len));
    }

    for id in 0..1_000_000 {
        books.insert(id, generate_random_book(&mut rng, &rand_cols, id));
    }

    books
}

#[bench]
fn bench_unicase_owned(b: &mut Bencher) {
    let db = generate_random_dataset();
    b.iter(|| {
        let mut c = HashSet::new();
        for &col in &["title", "authors", "series", "id"] {
            c.insert(UniCase::new(col.to_owned()));
        }

        for book in db.values() {
            if let Some(e) = &book.extended_tags {
                for key in e.keys() {
                    c.insert(UniCase::new(key.to_owned()));
                }
            }
        }

        assert_ne!(c.len(), 0);
    });
}

#[bench]
fn bench_unicase_borrow_post_process(b: &mut Bencher) {
    let db = generate_random_dataset();
    b.iter(|| {
        let mut c = HashSet::new();

        for &col in &["title", "authors", "series", "id"] {
            c.insert(UniCase::new(col));
        }

        for book in db.values() {
            if let Some(e) = &book.extended_tags {
                for key in e.keys() {
                    c.insert(UniCase::new(key));
                }
            }
        }
        let owned_c: HashSet<_> = c
            .iter()
            .map(|&c| UniCase::new(c.as_ref().to_owned()))
            .collect();
        assert_ne!(owned_c.len(), 0);
    });
}

#[bench]
fn bench_borrow_post_process(b: &mut Bencher) {
    let db = generate_random_dataset();
    b.iter(|| {
        let mut c = HashSet::new();

        for &col in &["title", "authors", "series", "id"] {
            c.insert(col);
        }

        for book in db.values() {
            if let Some(e) = &book.extended_tags {
                for key in e.keys() {
                    c.insert(key);
                }
            }
        }
        let owned_c: HashSet<_> = c.iter().map(|&c| UniCase::new(c.to_owned())).collect();
        assert_ne!(owned_c.len(), 0);
    });
}

#[test]
fn test_methods_are_same() {
    let db = generate_random_dataset();

    let a = {
        let mut c = HashSet::new();
        for &col in &["title", "authors", "series", "id"] {
            c.insert(UniCase::new(col.to_owned()));
        }

        for book in db.values() {
            if let Some(e) = &book.extended_tags {
                for key in e.keys() {
                    c.insert(UniCase::new(key.to_owned()));
                }
            }
        }
        c
    };

    let b = {
        let mut c = HashSet::new();

        for &col in &["title", "authors", "series", "id"] {
            c.insert(UniCase::new(col));
        }

        for book in db.values() {
            if let Some(e) = &book.extended_tags {
                for key in e.keys() {
                    c.insert(UniCase::new(key));
                }
            }
        }
        let owned_c: HashSet<_> = c
            .iter()
            .map(|&c| UniCase::new(c.as_ref().to_owned()))
            .collect();
        owned_c
    };

    let c = {
        let mut c = HashSet::new();

        for &col in &["title", "authors", "series", "id"] {
            c.insert(col);
        }

        for book in db.values() {
            if let Some(e) = &book.extended_tags {
                for key in e.keys() {
                    c.insert(key);
                }
            }
        }
        let owned_c: HashSet<_> = c.iter().map(|&c| UniCase::new(c.to_owned())).collect();
        owned_c
    };

    assert_eq!(a, b);
    assert_eq!(b, c);
    assert_eq!(a, c);
}
