use std::path::PathBuf;
use std::str::FromStr;

use bookstore_database::search::{Search, SearchMode};
use bookstore_records::book::{BookID, ColumnIdentifier};
use bookstore_records::{ColumnOrder, Edit};

#[derive(Debug)]
pub enum Flag {
    /// Flag followed by another flag or nothing.
    Flag(String),
    /// Flag followed by non-flag arguments.
    FlagWithArgument(String, Vec<String>),
    /// Arguments appearing without preceeding flag.
    StartingArguments(Vec<String>),
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum BookIndex {
    Selected,
    ID(BookID),
}

#[derive(Debug)]
pub enum CommandError {
    UnknownCommand,
    InsufficientArguments,
    InvalidCommand,
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

#[derive(Debug, PartialEq)]
pub enum Command {
    DeleteBook(BookIndex),
    DeleteAll,
    // TODO: Add + syntax for appending to existing text
    // TODO: Add deletion of fields and tags.
    EditBook(BookIndex, Box<[(ColumnIdentifier, Edit)]>),
    AddBookFromFile(PathBuf),
    AddBooksFromDir(PathBuf, u8),
    AddColumn(String),
    RemoveColumn(String),
    SortColumns(Box<[(ColumnIdentifier, ColumnOrder)]>),
    OpenBookInApp(BookIndex, usize),
    OpenBookInExplorer(BookIndex, usize),
    TryMergeAllBooks,
    Quit,
    Write,
    WriteAndQuit,
    FindMatches(Box<[Search]>),
    JumpTo(Box<[Search]>),
    Help(String),
    GeneralHelp,
    // TODO:
    //  eg. :m 1 2 -> merge 2 into 1
    //      Add MergeBooks with criteria?
    //  eg. :m -c (Search)*
    //      Allow adding multiple books or directories at once?
}

impl Command {
    pub fn requires_ui(&self) -> bool {
        use Command::*;
        match self {
            DeleteBook(b) | EditBook(b, _) | OpenBookInApp(b, _) | OpenBookInExplorer(b, _) => {
                b == &BookIndex::Selected
            }
            AddColumn(_) | RemoveColumn(_) | SortColumns(_) | FindMatches(_) => true,
            _ => false,
        }
    }
}

trait CommandParser: Sized + Into<Command> {
    fn from_args(args: Vec<Flag>) -> Result<Self, CommandError>;
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
fn read_flags(args: Vec<String>) -> Vec<Flag> {
    if args.len() <= 1 {
        return vec![];
    }

    let mut flags = vec![];
    let mut last_flag_valid = false;
    let mut flag = String::new();
    let mut flag_args = vec![];

    let mut v_iter = args.into_iter();
    v_iter.next();

    for v in v_iter {
        if v.starts_with('-') {
            if last_flag_valid {
                if flag_args.is_empty() {
                    flags.push(Flag::Flag(flag));
                } else {
                    flags.push(Flag::FlagWithArgument(flag, flag_args));
                    flag_args = vec![];
                }
            } else if !flag_args.is_empty() {
                flags.push(Flag::StartingArguments(flag_args));
                flag_args = vec![];
            }

            flag = v.trim_start_matches('-').to_owned();
            last_flag_valid = true;
        } else {
            flag_args.push(v);
        }
    }

    if last_flag_valid {
        if flag_args.is_empty() {
            flags.push(Flag::Flag(flag));
        } else {
            flags.push(Flag::FlagWithArgument(flag, flag_args));
        }
    } else {
        flags.push(Flag::StartingArguments(flag_args));
    }

    flags
}

#[allow(dead_code)]
/// Parses `s` into a command, using shell-style string splitting.
///
/// # Arguments
/// * ` s ` - The command in string format.
///
/// # Errors
/// Returns an error if the string does not have a matching command, or is a malformed command.
pub fn parse_command_string<S: AsRef<str>>(s: S) -> Result<Command, CommandError> {
    match shellwords::split(s.as_ref()) {
        Ok(vec) => parse_args(vec),
        Err(_) => Err(CommandError::InvalidCommand),
    }
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

    CommandRoot::from_str(&c)?.into_command(read_flags(args))
}

impl CommandRoot {
    pub fn into_command(self, args: Vec<Flag>) -> Result<Command, CommandError> {
        Ok(match self {
            CommandRoot::Delete => Delete::from_args(args)?.into(),
            CommandRoot::Edit => EditBook::from_args(args)?.into(),
            CommandRoot::AddBooks => AddBooks::from_args(args)?.into(),
            CommandRoot::ModifyColumns => ModifyColumns::from_args(args)?.into(),
            CommandRoot::SortColumns => SortColumns::from_args(args)?.into(),
            CommandRoot::OpenBook => OpenBook::from_args(args)?.into(),
            CommandRoot::MergeBooks => Merge::from_args(args)?.into(),
            CommandRoot::Quit => Quit::from_args(args)?.into(),
            CommandRoot::Write => Write::from_args(args)?.into(),
            CommandRoot::WriteQuit => WriteQuit::from_args(args)?.into(),
            CommandRoot::FindMatches => Find::from_args(args)?.into(),
            CommandRoot::JumpTo => Jump::from_args(args)?.into(),
            CommandRoot::Help => Help::from_args(args)?.into(),
        })
    }
}

enum Delete {
    Book(BookIndex),
    All,
}

impl Into<Command> for Delete {
    fn into(self) -> Command {
        match self {
            Delete::Book(bi) => Command::DeleteBook(bi),
            Delete::All => Command::DeleteAll,
        }
    }
}

impl CommandParser for Delete {
    fn from_args(flags: Vec<Flag>) -> Result<Self, CommandError> {
        if let Some(flag) = flags.first() {
            match flag {
                Flag::Flag(a) => {
                    if a == "a" {
                        Ok(Delete::All)
                    } else {
                        Err(CommandError::InvalidCommand)
                    }
                }
                Flag::StartingArguments(args) => {
                    if let Ok(i) = BookID::from_str(args[0].as_str()) {
                        Ok(Delete::Book(BookIndex::ID(i)))
                    } else {
                        Err(CommandError::InvalidCommand)
                    }
                }
                _ => Err(CommandError::InvalidCommand),
            }
        } else {
            Ok(Delete::Book(BookIndex::Selected))
        }
    }
}

enum Merge {
    All,
}

impl Into<Command> for Merge {
    fn into(self) -> Command {
        match self {
            Merge::All => Command::TryMergeAllBooks,
        }
    }
}

impl CommandParser for Merge {
    fn from_args(flags: Vec<Flag>) -> Result<Self, CommandError> {
        match flags.first() {
            Some(Flag::Flag(a)) => {
                if a == "a" {
                    Ok(Merge::All)
                } else {
                    Err(CommandError::InvalidCommand)
                }
            }
            _ => Err(CommandError::InvalidCommand),
        }
    }
}

struct Quit;

impl Into<Command> for Quit {
    fn into(self) -> Command {
        Command::Quit
    }
}

impl CommandParser for Quit {
    fn from_args(_flags: Vec<Flag>) -> Result<Self, CommandError> {
        Ok(Quit)
    }
}

struct Write;

impl Into<Command> for Write {
    fn into(self) -> Command {
        Command::Write
    }
}

impl CommandParser for Write {
    fn from_args(_flags: Vec<Flag>) -> Result<Self, CommandError> {
        Ok(Write)
    }
}

struct WriteQuit;

impl Into<Command> for WriteQuit {
    fn into(self) -> Command {
        Command::WriteAndQuit
    }
}

impl CommandParser for WriteQuit {
    fn from_args(_flags: Vec<Flag>) -> Result<Self, CommandError> {
        Ok(WriteQuit)
    }
}

enum AddBooks {
    FromFile(PathBuf),
    FromDir(PathBuf, u8),
}

impl Into<Command> for AddBooks {
    fn into(self) -> Command {
        match self {
            AddBooks::FromFile(path) => Command::AddBookFromFile(path),
            AddBooks::FromDir(path, depth) => Command::AddBooksFromDir(path, depth),
        }
    }
}

impl CommandParser for AddBooks {
    fn from_args(flags: Vec<Flag>) -> Result<Self, CommandError> {
        let mut from_dir = false;
        let mut depth = 1;
        let mut path = None;

        for flag in flags.into_iter() {
            match flag {
                Flag::Flag(c) => match c.as_str() {
                    "r" => {
                        depth = 255;
                    }
                    "d" => {
                        from_dir = true;
                    }
                    _ => return Err(CommandError::InvalidCommand),
                },
                Flag::FlagWithArgument(c, args) => match c.as_str() {
                    "r" => match u8::from_str(&args[0]) {
                        Ok(i) => depth = i,
                        Err(_) => return Err(CommandError::InvalidCommand),
                    },
                    "d" => {
                        from_dir = true;
                        path = Some(PathBuf::from(&args[0]));
                    }
                    _ => return Err(CommandError::InvalidCommand),
                },
                Flag::StartingArguments(args) => {
                    path = Some(PathBuf::from(&args[0]));
                }
            };
        }
        if let Some(path) = path {
            Ok(if from_dir {
                AddBooks::FromDir(path, depth)
            } else {
                AddBooks::FromFile(path)
            })
        } else {
            Err(CommandError::InsufficientArguments)
        }
    }
}

struct EditBook {
    index: BookIndex,
    edits: Box<[(ColumnIdentifier, Edit)]>,
}

impl Into<Command> for EditBook {
    fn into(self) -> Command {
        Command::EditBook(self.index, self.edits)
    }
}

impl CommandParser for EditBook {
    fn from_args(flags: Vec<Flag>) -> Result<Self, CommandError> {
        let mut edits = Vec::new();
        let mut id = None;
        for flag in flags.into_iter() {
            match flag {
                Flag::FlagWithArgument(f, args) => {
                    let mut args = args.into_iter();
                    let edit = match f.as_str() {
                        "d" => match ColumnIdentifier::from(args.next().ok_or_else(insuf)?) {
                            ColumnIdentifier::Tags => match args.next() {
                                None => (ColumnIdentifier::Tags, Edit::Delete),
                                Some(tag) => (ColumnIdentifier::ExactTag(tag), Edit::Delete),
                            },
                            column => (column, Edit::Delete),
                        },
                        "a" => match ColumnIdentifier::from(args.next().ok_or_else(insuf)?) {
                            ColumnIdentifier::Tags => match (args.next(), args.next()) {
                                (Some(value), None) => {
                                    (ColumnIdentifier::Tags, Edit::Append(value))
                                }
                                (Some(tag), Some(value)) => {
                                    (ColumnIdentifier::ExactTag(tag), Edit::Append(value))
                                }
                                _ => return Err(CommandError::InsufficientArguments),
                            },
                            column => (column, Edit::Append(args.next().ok_or_else(insuf)?)),
                        },
                        "r" => match ColumnIdentifier::from(args.next().ok_or_else(insuf)?) {
                            ColumnIdentifier::Tags => match (args.next(), args.next()) {
                                (Some(value), None) => {
                                    (ColumnIdentifier::Tags, Edit::Replace(value))
                                }
                                (Some(tag), Some(value)) => {
                                    (ColumnIdentifier::ExactTag(tag), Edit::Replace(value))
                                }
                                _ => return Err(CommandError::InsufficientArguments),
                            },
                            column => (column, Edit::Replace(args.next().ok_or_else(insuf)?)),
                        },
                        _ => return Err(CommandError::InvalidCommand),
                    };

                    edits.push(edit);

                    while let Some(col) = args.next() {
                        edits.push((
                            ColumnIdentifier::from(col),
                            Edit::Replace(args.next().ok_or_else(insuf)?),
                        ));
                    }
                }
                Flag::StartingArguments(args) => {
                    let mut args = if args.len() % 2 == 1 {
                        let mut args = args.into_iter();
                        id = Some(
                            BookID::from_str(&args.next().ok_or_else(insuf)?)
                                .map_err(|_| CommandError::InvalidCommand)?,
                        );
                        args
                    } else {
                        args.into_iter()
                    };
                    while let Some(col) = args.next() {
                        edits.push((
                            ColumnIdentifier::from(col),
                            Edit::Replace(args.next().ok_or_else(insuf)?),
                        ));
                    }
                }
                _ => return Err(CommandError::InvalidCommand),
            };
        }

        if edits.is_empty() {
            Err(CommandError::InsufficientArguments)
        } else {
            Ok(EditBook {
                index: id.map(|b| BookIndex::ID(b)).unwrap_or(BookIndex::Selected),
                edits: edits.into_boxed_slice(),
            })
        }
    }
}

struct SortColumns {
    sorts: Box<[(ColumnIdentifier, ColumnOrder)]>,
}

impl Into<Command> for SortColumns {
    fn into(self) -> Command {
        Command::SortColumns(self.sorts)
    }
}

impl CommandParser for SortColumns {
    fn from_args(flags: Vec<Flag>) -> Result<Self, CommandError> {
        let mut sort_cols = Vec::new();

        for flag in flags.into_iter() {
            match flag {
                Flag::FlagWithArgument(f, args) => {
                    if f != "d" {
                        return Err(CommandError::InvalidCommand);
                    }
                    let mut args = args.into_iter();
                    sort_cols.push((
                        ColumnIdentifier::from(args.next().ok_or_else(insuf)?),
                        ColumnOrder::Descending,
                    ));
                    sort_cols
                        .extend(args.map(|s| (ColumnIdentifier::from(s), ColumnOrder::Ascending)));
                }
                Flag::StartingArguments(args) => {
                    sort_cols.extend(
                        args.into_iter()
                            .map(|s| (ColumnIdentifier::from(s), ColumnOrder::Ascending)),
                    );
                }
                _ => {
                    return Err(CommandError::InvalidCommand);
                }
            };
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

enum ModifyColumns {
    Remove(String),
    Add(String),
}

impl Into<Command> for ModifyColumns {
    fn into(self) -> Command {
        match self {
            ModifyColumns::Remove(column) => Command::RemoveColumn(column),
            ModifyColumns::Add(column) => Command::AddColumn(column),
        }
    }
}

impl CommandParser for ModifyColumns {
    fn from_args(flags: Vec<Flag>) -> Result<Self, CommandError> {
        match flags.into_iter().next() {
            Some(Flag::StartingArguments(args)) => Ok(ModifyColumns::Add(
                args.into_iter().next().ok_or_else(insuf)?,
            )),
            Some(Flag::Flag(arg)) => Ok(ModifyColumns::Remove(arg)),
            _ => Err(CommandError::InvalidCommand),
        }
    }
}

struct Matches {
    matches: Box<[Search]>,
}

impl Matches {
    fn from_args(flags: Vec<Flag>) -> Result<Self, CommandError> {
        let mut matches = vec![];
        for flag in flags.into_iter() {
            let (mode, args) = match flag {
                Flag::StartingArguments(args) => Ok((SearchMode::Default, args)),
                Flag::FlagWithArgument(flag, args) => match flag.as_str() {
                    "r" => Ok((SearchMode::Regex, args)),
                    "e" => Ok((SearchMode::ExactSubstring, args)),
                    "x" => Ok((SearchMode::ExactString, args)),
                    _ => Err(CommandError::InvalidCommand),
                },
                _ => Err(CommandError::InvalidCommand),
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

impl Into<Command> for Jump {
    fn into(self) -> Command {
        Command::JumpTo(self.matches.matches)
    }
}

impl CommandParser for Jump {
    fn from_args(args: Vec<Flag>) -> Result<Self, CommandError> {
        Ok(Jump {
            matches: Matches::from_args(args)?,
        })
    }
}

struct Find {
    matches: Matches,
}

impl Into<Command> for Find {
    fn into(self) -> Command {
        Command::FindMatches(self.matches.matches)
    }
}

impl CommandParser for Find {
    fn from_args(args: Vec<Flag>) -> Result<Self, CommandError> {
        Ok(Find {
            matches: Matches::from_args(args)?,
        })
    }
}

struct Help {
    term: Option<String>,
}

impl Into<Command> for Help {
    fn into(self) -> Command {
        match self.term {
            None => Command::GeneralHelp,
            Some(term) => Command::Help(term),
        }
    }
}

impl CommandParser for Help {
    fn from_args(flags: Vec<Flag>) -> Result<Self, CommandError> {
        Ok(Help {
            term: match flags.into_iter().next() {
                Some(Flag::StartingArguments(args)) => Some(
                    args.into_iter()
                        .next()
                        .ok_or(CommandError::InsufficientArguments)?,
                ),
                _ => None,
            },
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

impl Into<Command> for OpenBook {
    fn into(self) -> Command {
        match self.target {
            Target::App => Command::OpenBookInApp(self.book_index, self.variant_index),
            Target::FileManager => Command::OpenBookInExplorer(self.book_index, self.variant_index),
        }
    }
}

impl CommandParser for OpenBook {
    fn from_args(flags: Vec<Flag>) -> Result<Self, CommandError> {
        let mut target = Target::App;
        let mut book_index = BookIndex::Selected;
        let mut variant_index = 0;
        for flag in flags {
            match flag {
                Flag::Flag(c) => {
                    if c == "f" {
                        target = Target::FileManager;
                    }
                }
                Flag::FlagWithArgument(c, args) => {
                    if c != "f" {
                        return Err(CommandError::InvalidCommand);
                    }
                    target = Target::FileManager;
                    let mut args = args.into_iter();
                    if let Some(ind_book) = args.next() {
                        if let Ok(bi) = BookID::from_str(ind_book.as_str()) {
                            let vi = args
                                .next()
                                .as_deref()
                                .map_or(Ok(0), usize::from_str)
                                .unwrap_or(0);
                            book_index = BookIndex::ID(bi);
                            variant_index = vi;
                            break;
                        }
                    } else {
                        return Err(CommandError::InvalidCommand);
                    }
                }
                Flag::StartingArguments(args) => {
                    let mut args = args.into_iter();

                    if let Some(l) = args.next() {
                        book_index = BookID::from_str(&l)
                            .map(BookIndex::ID)
                            .unwrap_or(BookIndex::Selected);
                    }

                    if let Some(i) = args.next() {
                        variant_index = usize::from_str(&i).unwrap_or(0);
                    }
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
                Command::AddBooksFromDir(PathBuf::from("hello world"), 1),
            ),
            (
                vec![":a", "-d", "hello world"],
                Command::AddBooksFromDir(PathBuf::from("hello world"), 1),
            ),
            (
                vec![":a", "-d", "hello world", "-r", "1"],
                Command::AddBooksFromDir(PathBuf::from("hello world"), 1),
            ),
            (
                vec![":a", "-r", "22", "-d", "hello world"],
                Command::AddBooksFromDir(PathBuf::from("hello world"), 22),
            ),
            (
                vec![":a", "-d", "hello world", "-r"],
                Command::AddBooksFromDir(PathBuf::from("hello world"), 255),
            ),
            (
                vec![":a", "-r", "-d", "hello world"],
                Command::AddBooksFromDir(PathBuf::from("hello world"), 255),
            ),
            (
                vec![":a", "hello world"],
                Command::AddBookFromFile(PathBuf::from("hello world")),
            ),
        ];

        for (args, command) in args {
            let args: Vec<_> = args.into_iter().map(|s| s.to_owned()).collect();
            let res = parse_args(args.clone()).expect("Parsing provided args should not fail");
            assert_eq!(res, command, "from {:?} expected {:?}", args, command);
        }
    }
}
