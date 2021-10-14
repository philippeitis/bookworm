#![deny(unused_must_use)]
#![deny(unused_imports)]

pub use book::Book;
pub use error::BookError;
pub use variant::BookVariant;

pub mod book;
mod epub;
pub mod error;
mod mobi;
pub mod series;
pub mod variant;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ColumnOrder {
    Ascending,
    Descending,
}

impl ColumnOrder {
    pub fn as_bool(&self) -> bool {
        match self {
            ColumnOrder::Ascending => false,
            ColumnOrder::Descending => true,
        }
    }

    pub fn from_bool(reversed: bool) -> Self {
        match reversed {
            false => ColumnOrder::Ascending,
            true => ColumnOrder::Descending,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Edit {
    Delete,
    Replace(String),
    Append(String),
}

impl AsRef<Edit> for Edit {
    fn as_ref(&self) -> &Edit {
        self
    }
}
