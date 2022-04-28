#![deny(unused_must_use)]
#![deny(unused_imports)]
#![deny(unused_attributes)]
#![deny(unused_mut)]

pub mod autocomplete;
pub mod user_input;

use crate::user_input::EventBuffer;
pub use autocomplete::AutoCompleter;

#[derive(Debug, PartialEq)]
pub enum Edit {
    Delete,
    Replace(String),
    Append(String),
    Sequence(EventBuffer),
}
