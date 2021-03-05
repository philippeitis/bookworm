#[derive(Debug, Eq, PartialEq)]
pub enum SearchMode {
    Regex,
    ExactSubstring,
    Default,
}
