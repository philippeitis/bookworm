use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;

use itertools::Itertools;

use bookstore_database::search::{Search, SearchMode};
use bookstore_records::book::{BookID, ColumnIdentifier};
use bookstore_records::{ColumnOrder, Edit};

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum BookIndex {
    Selected,
    ID(BookID),
}

#[derive(Debug)]
pub enum CommandError {
    UnknownCommand,
    InsufficientArguments,
    UnknownFlag,
    UnexpectedArguments,
    ConflictingArguments,
}

enum CommandRoot {
    Delete,
    Edit,
    AddBooks,
    ModifyColumns,
    SortColumns,
    OpenBook,
    MergeBooks,
    Quit,
    Write,
    WriteQuit,
    FindMatches,
    JumpTo,
    Help,
}

impl FromStr for CommandRoot {
    type Err = CommandError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            ":d" => CommandRoot::Delete,
            ":e" => CommandRoot::Edit,
            ":a" => CommandRoot::AddBooks,
            ":c" => CommandRoot::ModifyColumns,
            ":s" => CommandRoot::SortColumns,
            ":o" => CommandRoot::OpenBook,
            ":m" => CommandRoot::MergeBooks,
            ":q" => CommandRoot::Quit,
            ":w" => CommandRoot::Write,
            ":wq" => CommandRoot::WriteQuit,
            ":f" => CommandRoot::FindMatches,
            ":j" => CommandRoot::JumpTo,
            ":h" => CommandRoot::Help,
            _ => return Err(CommandError::UnknownCommand),
        })
    }
}

#[cfg_attr(test, derive(PartialEq))]
#[derive(Debug)]
pub enum Command {
    // TODO: Add field=val notation for matches?
    DeleteSelected,
    DeleteMatching(Box<[Search]>),
    DeleteAll,
    EditBook(BookIndex, Box<[(ColumnIdentifier, Edit)]>),
    AddBooks(Box<[Source]>),
    ModifyColumns(Box<[ModifyColumn]>),
    SortColumns(Box<[(ColumnIdentifier, ColumnOrder)]>),
    OpenBookInApp(BookIndex, usize),
    OpenBookInExplorer(BookIndex, usize),
    TryMergeAllBooks,
    Quit,
    Write,
    WriteAndQuit,
    FilterMatches(Box<[Search]>),
    JumpTo(Box<[Search]>),
    Help(String),
    GeneralHelp,
    // TODO:
    //  eg. :m 1 2 -> merge 2 into 1
    //      Add MergeBooks with criteria?
    //  eg. :m -c (Search)*
    //  filter books based on other features - like a preemptive delete?
}

impl Command {
    pub fn requires_ui(&self) -> bool {
        use Command::*;
        match self {
            EditBook(b, _) | OpenBookInApp(b, _) | OpenBookInExplorer(b, _) => {
                b == &BookIndex::Selected
            }
            DeleteSelected | ModifyColumns(_) | SortColumns(_) | FilterMatches(_) => true,
            _ => false,
        }
    }
}

trait CommandParser: Sized + Into<Command> {
    fn from_args(
        start_args: Vec<String>,
        trailing_args: Vec<(String, Vec<String>)>,
    ) -> Result<Self, CommandError>;
}

fn insuf() -> CommandError {
    CommandError::InsufficientArguments
}

fn remove_string_quotes(mut s: String) -> String {
    match (s.chars().next(), s.chars().last()) {
        (Some('"'), Some('"')) | (Some('\''), Some('\'')) => {
            s.pop();
            s.remove(0);
            s
        }
        _ => s,
    }
}

/// Read a white-space split command string, and collect flags and arguments together.
/// Returns a vec of Flags corresponding to the input.
/// Flag-argument sets of the following forms are handled:
/// * -f1 arg1 arg2 ... -f2?
/// * arg1 arg2 arg3 ... -f1?
/// * -f1 -f2
///
/// Note: Does not handle -abcdef.
///
/// # Arguments
/// * ` args ` - A vector of command line arguments in sequential order.
fn read_args(args: Vec<String>) -> (Vec<String>, Vec<(String, Vec<String>)>) {
    if args.len() <= 1 {
        return (Vec::new(), Vec::new());
    }

    let mut v_iter = args.into_iter().peekable();
    v_iter.next();

    let start_args = v_iter.peeking_take_while(|v| !v.starts_with('-')).collect();

    let mut trailing_args = Vec::new();
    while let Some(v) = v_iter.next() {
        let flag_args = v_iter.peeking_take_while(|v| !v.starts_with('-')).collect();
        trailing_args.push((v, flag_args));
    }

    (start_args, trailing_args)
}

/// Reads `args` and returns the corresponding command. If no corresponding command exists,
/// an error is returned.
///
/// # Arguments
/// * ` args ` - The arguments to turn into a command.
///
/// # Errors
/// If the command is missing required arguments, or is unrecognized, an error is returned.
pub fn parse_args(args: Vec<String>) -> Result<Command, CommandError> {
    let c = if let Some(c) = args.first() {
        c.clone()
    } else {
        return Err(CommandError::InsufficientArguments);
    };
    let (start, trail) = read_args(args);
    CommandRoot::from_str(&c)?.into_command(start, trail)
}

impl CommandRoot {
    pub fn into_command(
        self,
        start_args: Vec<String>,
        trailing_args: Vec<(String, Vec<String>)>,
    ) -> Result<Command, CommandError> {
        Ok(match self {
            CommandRoot::Delete => Delete::from_args(start_args, trailing_args)?.into(),
            CommandRoot::Edit => EditBook::from_args(start_args, trailing_args)?.into(),
            CommandRoot::AddBooks => AddBooks::from_args(start_args, trailing_args)?.into(),
            CommandRoot::ModifyColumns => {
                ModifyColumns::from_args(start_args, trailing_args)?.into()
            }
            CommandRoot::SortColumns => SortColumns::from_args(start_args, trailing_args)?.into(),
            CommandRoot::OpenBook => OpenBook::from_args(start_args, trailing_args)?.into(),
            CommandRoot::MergeBooks => Merge::from_args(start_args, trailing_args)?.into(),
            CommandRoot::Quit => Quit::from_args(start_args, trailing_args)?.into(),
            CommandRoot::Write => Write::from_args(start_args, trailing_args)?.into(),
            CommandRoot::WriteQuit => WriteQuit::from_args(start_args, trailing_args)?.into(),
            CommandRoot::FindMatches => Filter::from_args(start_args, trailing_args)?.into(),
            CommandRoot::JumpTo => Jump::from_args(start_args, trailing_args)?.into(),
            CommandRoot::Help => Help::from_args(start_args, trailing_args)?.into(),
        })
    }
}

enum Delete {
    // TODO: Rename Search to Match
    Matching(Box<[Search]>),
    Selected,
    All,
}

impl From<Delete> for Command {
    fn from(d: Delete) -> Self {
        match d {
            Delete::Selected => Command::DeleteSelected,
            Delete::All => Command::DeleteAll,
            Delete::Matching(matches) => Command::DeleteMatching(matches),
        }
    }
}

impl CommandParser for Delete {
    fn from_args(
        start_args: Vec<String>,
        trailing_args: Vec<(String, Vec<String>)>,
    ) -> Result<Self, CommandError> {
        // First arg can be one of three things:
        //  -a: Delete all
        //  index: Some non-zero index.
        //  field value -r?

        match (start_args.is_empty(), trailing_args.first()) {
            (false, None) => {
                return Ok(Delete::Selected);
            }
            (true, Some((a, args))) if a == "-a" => {
                return if !args.is_empty() || trailing_args.len() > 1 {
                    Err(CommandError::UnexpectedArguments)
                } else {
                    Ok(Delete::All)
                };
            }
            _ => {}
        }

        let matches = Matches::from_args(start_args, trailing_args)?;

        Ok(Delete::Matching(matches.matches))
    }
}

enum Merge {
    All,
}

impl From<Merge> for Command {
    fn from(m: Merge) -> Self {
        match m {
            Merge::All => Command::TryMergeAllBooks,
        }
    }
}

impl CommandParser for Merge {
    fn from_args(
        start_args: Vec<String>,
        trailing_args: Vec<(String, Vec<String>)>,
    ) -> Result<Self, CommandError> {
        let mut trailing_args: HashMap<_, _> = trailing_args.into_iter().collect();
        match (start_args.is_empty(), trailing_args.remove("-a")) {
            (true, Some(a_args)) => {
                if !a_args.is_empty() || !trailing_args.is_empty() {
                    Err(CommandError::UnexpectedArguments)
                } else {
                    Ok(Merge::All)
                }
            }
            _ => Err(CommandError::ConflictingArguments),
        }
    }
}

struct Quit;

impl From<Quit> for Command {
    fn from(_q: Quit) -> Command {
        Command::Quit
    }
}

impl CommandParser for Quit {
    fn from_args(_sa: Vec<String>, _ta: Vec<(String, Vec<String>)>) -> Result<Self, CommandError> {
        Ok(Quit)
    }
}

struct Write;

impl From<Write> for Command {
    fn from(_w: Write) -> Self {
        Command::Write
    }
}

impl CommandParser for Write {
    fn from_args(_sa: Vec<String>, _ta: Vec<(String, Vec<String>)>) -> Result<Self, CommandError> {
        Ok(Write)
    }
}

struct WriteQuit;

impl From<WriteQuit> for Command {
    fn from(_wq: WriteQuit) -> Self {
        Command::WriteAndQuit
    }
}

impl CommandParser for WriteQuit {
    fn from_args(_sa: Vec<String>, _ta: Vec<(String, Vec<String>)>) -> Result<Self, CommandError> {
        Ok(WriteQuit)
    }
}

#[cfg_attr(test, derive(PartialEq))]
#[derive(Debug)]
pub enum Source {
    File(PathBuf),
    Dir(PathBuf, u8),
    Glob(String),
}

#[cfg_attr(test, derive(PartialEq))]
#[derive(Debug)]
struct AddBooks {
    sources: Box<[Source]>,
}

impl From<AddBooks> for Command {
    fn from(ab: AddBooks) -> Self {
        Command::AddBooks(ab.sources)
    }
}

impl CommandParser for AddBooks {
    fn from_args(
        start_args: Vec<String>,
        trailing_args: Vec<(String, Vec<String>)>,
    ) -> Result<Self, CommandError> {
        let mut sources: Vec<_> = start_args
            .into_iter()
            .map(PathBuf::from)
            .map(Source::File)
            .collect();
        let mut prev_ind = sources.len();

        let global_recursion = trailing_args
            .first()
            .map(|(r, a)| {
                if r == "-r" {
                    match a.first() {
                        None => 255,
                        Some(s) => u8::from_str(s).unwrap_or(255),
                    }
                } else {
                    1
                }
            })
            .unwrap_or(1);

        for (flag, args) in trailing_args {
            match flag.as_str() {
                "-g" => {
                    sources.extend(args.into_iter().map(Source::Glob));
                    prev_ind = sources.len();
                }
                "-d" => {
                    prev_ind = sources.len();
                    for path in args {
                        sources.push(Source::Dir(PathBuf::from(path), global_recursion));
                    }
                }
                "-r" => {
                    if prev_ind >= sources.len() {
                        debug_assert!(prev_ind == sources.len());
                        continue;
                    }

                    let local_depth = args.first().map(|s| u8::from_str(s).unwrap_or(255));

                    for source in &mut sources[prev_ind..] {
                        if let Source::Dir(_, depth) = source {
                            *depth = local_depth.unwrap_or(255);
                        }
                    }

                    let mut args = args.into_iter();
                    if local_depth.is_some() {
                        args.next();
                    }

                    sources.extend(args.map(PathBuf::from).map(Source::File));
                    prev_ind = sources.len();
                }
                "-p" => {
                    sources.extend(args.into_iter().map(PathBuf::from).map(Source::File));
                    prev_ind = sources.len();
                }
                _ => return Err(CommandError::UnknownFlag),
            }
        }

        Ok(AddBooks {
            sources: sources.into_boxed_slice(),
        })
    }
}

struct EditBook {
    index: BookIndex,
    edits: Box<[(ColumnIdentifier, Edit)]>,
}

impl From<EditBook> for Command {
    fn from(eb: EditBook) -> Self {
        Command::EditBook(eb.index, eb.edits)
    }
}

impl CommandParser for EditBook {
    fn from_args(
        start_args: Vec<String>,
        trailing_args: Vec<(String, Vec<String>)>,
    ) -> Result<Self, CommandError> {
        let (id, mut start_args) = if start_args.len() % 2 == 1 {
            let mut args = start_args.into_iter();
            let id = Some(
                BookID::from_str(&args.next().ok_or_else(insuf)?)
                    .map_err(|_| CommandError::UnexpectedArguments)?,
            );
            (id, args)
        } else {
            (None, start_args.into_iter())
        };

        let mut edits = Vec::new();
        while let Some(col) = start_args.next() {
            edits.push((
                ColumnIdentifier::from(col),
                Edit::Replace(start_args.next().ok_or_else(insuf)?),
            ));
        }

        for (flag, args) in trailing_args.into_iter() {
            let mut args = args.into_iter();
            let edit = match flag.as_str() {
                "-d" => match ColumnIdentifier::from(args.next().ok_or_else(insuf)?) {
                    ColumnIdentifier::Tags => match args.next() {
                        None => (ColumnIdentifier::Tags, Edit::Delete),
                        Some(tag) => (ColumnIdentifier::ExactTag(tag), Edit::Delete),
                    },
                    column => (column, Edit::Delete),
                },
                "-a" => match ColumnIdentifier::from(args.next().ok_or_else(insuf)?) {
                    ColumnIdentifier::Tags => match (args.next(), args.next()) {
                        (Some(value), None) => (ColumnIdentifier::Tags, Edit::Append(value)),
                        (Some(tag), Some(value)) => {
                            (ColumnIdentifier::ExactTag(tag), Edit::Append(value))
                        }
                        _ => return Err(CommandError::InsufficientArguments),
                    },
                    column => (column, Edit::Append(args.next().ok_or_else(insuf)?)),
                },
                "-r" => match ColumnIdentifier::from(args.next().ok_or_else(insuf)?) {
                    ColumnIdentifier::Tags => match (args.next(), args.next()) {
                        (Some(value), None) => (ColumnIdentifier::Tags, Edit::Replace(value)),
                        (Some(tag), Some(value)) => {
                            (ColumnIdentifier::ExactTag(tag), Edit::Replace(value))
                        }
                        _ => return Err(CommandError::InsufficientArguments),
                    },
                    column => (column, Edit::Replace(args.next().ok_or_else(insuf)?)),
                },
                _ => return Err(CommandError::UnknownFlag),
            };

            edits.push(edit);

            while let Some(col) = args.next() {
                edits.push((
                    ColumnIdentifier::from(col),
                    Edit::Replace(args.next().ok_or_else(insuf)?),
                ));
            }
        }

        if edits.is_empty() {
            Err(CommandError::InsufficientArguments)
        } else {
            Ok(EditBook {
                index: id.map(BookIndex::ID).unwrap_or(BookIndex::Selected),
                edits: edits.into_boxed_slice(),
            })
        }
    }
}

struct SortColumns {
    sorts: Box<[(ColumnIdentifier, ColumnOrder)]>,
}

impl From<SortColumns> for Command {
    fn from(sc: SortColumns) -> Self {
        Command::SortColumns(sc.sorts)
    }
}

impl CommandParser for SortColumns {
    fn from_args(
        start_args: Vec<String>,
        trailing_args: Vec<(String, Vec<String>)>,
    ) -> Result<Self, CommandError> {
        let mut sort_cols: Vec<_> = start_args
            .into_iter()
            .map(|s| (ColumnIdentifier::from(s), ColumnOrder::Ascending))
            .collect();

        for (flag, args) in trailing_args.into_iter() {
            if flag != "d" {
                return Err(CommandError::UnknownFlag);
            }
            let mut args = args.into_iter();
            sort_cols.push((
                ColumnIdentifier::from(args.next().ok_or_else(insuf)?),
                ColumnOrder::Descending,
            ));
            sort_cols.extend(args.map(|s| (ColumnIdentifier::from(s), ColumnOrder::Ascending)));
        }

        if sort_cols.is_empty() {
            Err(CommandError::InsufficientArguments)
        } else {
            Ok(SortColumns {
                sorts: sort_cols.into_boxed_slice(),
            })
        }
    }
}

#[cfg_attr(test, derive(PartialEq))]
#[derive(Debug)]
pub enum ModifyColumn {
    Remove(String),
    Add(String),
}

struct ModifyColumns {
    columns: Box<[ModifyColumn]>,
}

impl From<ModifyColumns> for Command {
    fn from(mc: ModifyColumns) -> Self {
        Command::ModifyColumns(mc.columns)
    }
}

impl CommandParser for ModifyColumns {
    fn from_args(
        start_args: Vec<String>,
        trailing_args: Vec<(String, Vec<String>)>,
    ) -> Result<Self, CommandError> {
        let mut columns: Vec<_> = start_args.into_iter().map(ModifyColumn::Add).collect();

        for (remove_col, add_cols) in trailing_args.into_iter() {
            columns.push(ModifyColumn::Remove(
                remove_col
                    .strip_prefix('-')
                    .expect("col starts with '-' if it ends up in trailing_args")
                    .to_string(),
            ));
            columns.extend(add_cols.into_iter().map(ModifyColumn::Add));
        }

        Ok(ModifyColumns {
            columns: columns.into_boxed_slice(),
        })
    }
}

struct Matches {
    matches: Box<[Search]>,
}

impl Matches {
    fn from_args(
        start_args: Vec<String>,
        trailing_args: Vec<(String, Vec<String>)>,
    ) -> Result<Self, CommandError> {
        let mut matches = vec![];
        let mut arg_iter = start_args.into_iter();

        while let Some(col) = arg_iter.next() {
            let search = arg_iter.next().ok_or_else(insuf)?;
            matches.push(Search {
                mode: SearchMode::Default,
                column: ColumnIdentifier::from(col),
                search: remove_string_quotes(search),
            });
        }

        for (flag, args) in trailing_args {
            let mode = match flag.as_str() {
                "-r" => Ok(SearchMode::Regex),
                "-e" => Ok(SearchMode::ExactSubstring),
                "-x" => Ok(SearchMode::ExactString),
                _ => Err(CommandError::UnknownFlag),
            }?;

            let mut args = args.into_iter();
            matches.push(Search {
                mode,
                column: ColumnIdentifier::from(args.next().ok_or_else(insuf)?),
                search: remove_string_quotes(args.next().ok_or_else(insuf)?),
            });
            while let Some(col) = args.next() {
                let search = args.next().ok_or_else(insuf)?;
                matches.push(Search {
                    mode: SearchMode::Default,
                    column: ColumnIdentifier::from(col),
                    search: remove_string_quotes(search),
                });
            }
        }

        if matches.is_empty() {
            Err(CommandError::InsufficientArguments)
        } else {
            Ok(Matches {
                matches: matches.into_boxed_slice(),
            })
        }
    }
}

struct Jump {
    matches: Matches,
}

impl From<Jump> for Command {
    fn from(j: Jump) -> Self {
        Command::JumpTo(j.matches.matches)
    }
}

impl CommandParser for Jump {
    fn from_args(
        start_args: Vec<String>,
        trailing_args: Vec<(String, Vec<String>)>,
    ) -> Result<Self, CommandError> {
        Ok(Jump {
            matches: Matches::from_args(start_args, trailing_args)?,
        })
    }
}

struct Filter {
    matches: Matches,
}

impl From<Filter> for Command {
    fn from(f: Filter) -> Self {
        Command::FilterMatches(f.matches.matches)
    }
}

impl CommandParser for Filter {
    fn from_args(
        start_args: Vec<String>,
        trailing_args: Vec<(String, Vec<String>)>,
    ) -> Result<Self, CommandError> {
        Ok(Filter {
            matches: Matches::from_args(start_args, trailing_args)?,
        })
    }
}

struct Help {
    term: Option<String>,
}

impl From<Help> for Command {
    fn from(h: Help) -> Self {
        match h.term {
            None => Command::GeneralHelp,
            Some(term) => Command::Help(term),
        }
    }
}

impl CommandParser for Help {
    fn from_args(
        start_args: Vec<String>,
        _ta: Vec<(String, Vec<String>)>,
    ) -> Result<Self, CommandError> {
        Ok(Help {
            term: start_args.into_iter().next(),
        })
    }
}

enum Target {
    App,
    FileManager,
}

struct OpenBook {
    target: Target,
    book_index: BookIndex,
    variant_index: usize,
}

impl From<OpenBook> for Command {
    fn from(ob: OpenBook) -> Self {
        match ob.target {
            Target::App => Command::OpenBookInApp(ob.book_index, ob.variant_index),
            Target::FileManager => Command::OpenBookInExplorer(ob.book_index, ob.variant_index),
        }
    }
}

impl CommandParser for OpenBook {
    fn from_args(
        start_args: Vec<String>,
        trailing_args: Vec<(String, Vec<String>)>,
    ) -> Result<Self, CommandError> {
        let mut args = start_args.into_iter();

        let mut book_index = args
            .next()
            .map(|l| {
                BookID::from_str(&l)
                    .map(BookIndex::ID)
                    .unwrap_or(BookIndex::Selected)
            })
            .unwrap_or(BookIndex::Selected);

        let mut variant_index = args
            .next()
            .map(|i| usize::from_str(&i).unwrap_or(0))
            .unwrap_or(0);

        let mut target = Target::App;

        for (flag, args) in trailing_args {
            if flag == "-f" {
                target = Target::FileManager;
            } else {
                return Err(CommandError::UnknownFlag);
            }
            let mut args = args.into_iter();
            if let Some(ind_book) = args.next() {
                if book_index != BookIndex::Selected {
                    return Err(CommandError::ConflictingArguments);
                }

                if let Ok(bi) = BookID::from_str(ind_book.as_str()) {
                    let vi = args
                        .next()
                        .as_deref()
                        .map_or(Ok(0), usize::from_str)
                        .unwrap_or(0);
                    book_index = BookIndex::ID(bi);
                    variant_index = vi;
                }
            }
        }

        Ok(OpenBook {
            target,
            book_index,
            variant_index,
        })
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_add_command() {
        let args = vec![
            (
                vec![":a", "hello world", "-d"],
                Command::AddBooks(
                    vec![Source::File(PathBuf::from("hello world"))].into_boxed_slice(),
                ),
            ),
            (
                vec![":a", "-d", "hello world"],
                Command::AddBooks(
                    vec![Source::Dir(PathBuf::from("hello world"), 1)].into_boxed_slice(),
                ),
            ),
            (
                vec![":a", "-d", "hello world", "-r", "1"],
                Command::AddBooks(
                    vec![Source::Dir(PathBuf::from("hello world"), 1)].into_boxed_slice(),
                ),
            ),
            (
                vec![":a", "-r", "22", "-d", "hello world"],
                Command::AddBooks(
                    vec![Source::Dir(PathBuf::from("hello world"), 22)].into_boxed_slice(),
                ),
            ),
            (
                vec![":a", "-d", "hello world", "-r"],
                Command::AddBooks(
                    vec![Source::Dir(PathBuf::from("hello world"), 255)].into_boxed_slice(),
                ),
            ),
            (
                vec![":a", "-r", "-d", "hello world"],
                Command::AddBooks(
                    vec![Source::Dir(PathBuf::from("hello world"), 255)].into_boxed_slice(),
                ),
            ),
            (
                vec![":a", "hello world"],
                Command::AddBooks(
                    vec![Source::File(PathBuf::from("hello world"))].into_boxed_slice(),
                ),
            ),
        ];

        for (args, command) in args {
            let args: Vec<_> = args.into_iter().map(|s| s.to_owned()).collect();
            let res = parse_args(args.clone()).expect("Parsing provided args should not fail");
            assert_eq!(res, command, "from {:?} expected {:?}", args, command);
        }
    }
}
