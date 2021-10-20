#![deny(unused_must_use)]
#![deny(unused_imports)]

pub mod autocomplete;
pub mod user_input;

use crate::user_input::InputRecorder;
pub use autocomplete::AutoCompleter;

#[derive(Debug, PartialEq)]
pub enum Edit {
    Delete,
    Replace(String),
    Append(String),
    Sequence(InputRecorder<bool>),
}
