#[derive(Debug, Eq, PartialEq)]
pub enum Search {
    Regex(String, String),
    ExactSubstring(String, String),
    Default(String, String),
}
