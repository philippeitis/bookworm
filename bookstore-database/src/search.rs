#[derive(Debug, Eq, PartialEq)]
pub enum Search {
    Regex(String, String),
    CaseSensitive(String, String),
    Default(String, String),
}
