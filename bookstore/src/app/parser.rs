use std::path::PathBuf;
use std::str::FromStr;

extern crate shellwords;

use crate::database::Matching;

#[derive(Debug)]
pub enum Flag {
    Flag(String),
    FlagWithArgument(String, Vec<String>),
    PositionalArg(Vec<String>),
}

#[derive(Debug, Eq, PartialEq)]
pub enum BookIndex {
    Selected,
    BookID(u32),
}

#[derive(Debug)]
pub enum Command {
    // NoCommand,
    // IncompleteCommand,
    UnknownCommand,
    InvalidCommand,
    DeleteBook(BookIndex),
    DeleteAll,
    EditBook(BookIndex, String, String),
    AddBookFromFile(PathBuf),
    AddBooksFromDir(PathBuf),
    AddColumn(String),
    RemoveColumn(String),
    SortColumn(String, bool),
    OpenBookInApp(BookIndex, usize),
    OpenBookInExplorer(BookIndex, usize),
    TryMergeAllBooks,
    Quit,
    Write,
    WriteAndQuit,
    FindMatches(Matching, String, String),
}

impl Command {
    pub(crate) fn requires_ui(&self) -> bool {
        match self {
            Command::UnknownCommand => false,
            Command::InvalidCommand => false,
            Command::DeleteBook(b) => b == &BookIndex::Selected,
            Command::EditBook(b, _, _) => b == &BookIndex::Selected,
            Command::AddBookFromFile(_) => false,
            Command::AddBooksFromDir(_) => false,
            Command::AddColumn(_) => true,
            Command::RemoveColumn(_) => true,
            Command::SortColumn(_, _) => true,
            Command::OpenBookInApp(b, _) => b == &BookIndex::Selected,
            Command::OpenBookInExplorer(b, _) => b == &BookIndex::Selected,
            Command::Quit => true,
            Command::Write => true,
            Command::WriteAndQuit => true,
            Command::DeleteAll => false,
            Command::TryMergeAllBooks => false,
            Command::FindMatches(_, _, _) => true,
        }
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
    let mut ended = false;

    let mut v_iter = args.into_iter();
    v_iter.next();

    for v in v_iter {
        ended = false;
        if v.starts_with('-') {
            if last_flag_valid {
                if flag_args.is_empty() {
                    flags.push(Flag::Flag(flag));
                } else {
                    flags.push(Flag::FlagWithArgument(flag, flag_args));
                    flag_args = vec![];
                    ended = true;
                }
            } else if !flag_args.is_empty() {
                flags.push(Flag::PositionalArg(flag_args));
                flag_args = vec![];
            }

            flag = v.trim_start_matches('-').to_owned();
            last_flag_valid = true;
        } else {
            flag_args.push(v);
        }
    }

    if !ended {
        if last_flag_valid {
            if !flag_args.is_empty() {
                flags.push(Flag::FlagWithArgument(flag, flag_args));
            } else {
                flags.push(Flag::Flag(flag));
            }
        } else {
            flags.push(Flag::PositionalArg(flag_args));
        }
    }

    flags
}

#[allow(dead_code)]
/// Reads a string which acts as a command, splits into into its component words,
/// and then parses the result into a command which can be run.
pub(crate) fn parse_command_string<S: ToString>(s: S) -> Command {
    let s = s.to_string();
    match shellwords::split(s.as_str()) {
        Ok(vec) => parse_args(vec),
        Err(_) => Command::InvalidCommand,
    }
}

/// Reads args and returns the appropriate command. If the command format is incorrect,
/// returns Command::InvalidCommand, or Command::InvalidCommand if the first argument is not
/// a recognized command.
pub(crate) fn parse_args(args: Vec<String>) -> Command {
    let c = if let Some(c) = args.first() {
        c.clone()
    } else {
        return Command::InvalidCommand;
    };

    let flags = read_flags(args);
    match c.as_str() {
        "!q" => {
            return Command::Quit;
        }
        "!w" => {
            return Command::Write;
        }
        "!wq" => {
            return Command::WriteAndQuit;
        }
        "!a" => {
            let mut d = false;
            let mut path_exists = false;
            let mut path = PathBuf::new();

            for flag in flags {
                match flag {
                    Flag::Flag(c) => {
                        d |= c == "d";
                        if d && path_exists {
                            return Command::AddBooksFromDir(path);
                        }
                    }
                    Flag::FlagWithArgument(c, args) => {
                        d |= c == "d";
                        if d {
                            return Command::AddBooksFromDir(PathBuf::from(&args[0]));
                        }
                    }
                    Flag::PositionalArg(args) => {
                        if !path_exists {
                            path_exists = true;
                            path = PathBuf::from(&args[0]);
                        }
                        if d {
                            return Command::AddBooksFromDir(path);
                        }
                    }
                };
            }
            if path_exists {
                return if d {
                    Command::AddBooksFromDir(path)
                } else {
                    Command::AddBookFromFile(path)
                };
            }
            Command::InvalidCommand
        }
        "!d" => {
            return if let Some(flag) = flags.first() {
                match flag {
                    Flag::Flag(a) => {
                        if a == "a" {
                            Command::DeleteAll
                        } else {
                            Command::InvalidCommand
                        }
                    }
                    Flag::PositionalArg(args) => {
                        if let Ok(i) = u32::from_str(args[0].as_str()) {
                            Command::DeleteBook(BookIndex::BookID(i))
                        } else {
                            Command::InvalidCommand
                        }
                    }
                    _ => Command::InvalidCommand,
                }
            } else {
                Command::DeleteBook(BookIndex::Selected)
            };
        }
        "!e" => {
            return match flags.into_iter().next() {
                Some(Flag::PositionalArg(args)) => {
                    let mut args = args.into_iter();
                    if let Some(a) = args.next() {
                        if let Some(b) = args.next() {
                            if let Some(c) = args.next() {
                                if let Ok(id) = u32::from_str(a.as_str()) {
                                    return Command::EditBook(BookIndex::BookID(id), b, c);
                                }
                            }

                            return Command::EditBook(BookIndex::Selected, a, b);
                        }
                    }
                    Command::InvalidCommand
                }
                _ => Command::InvalidCommand,
            };
        }
        "!m" => {
            return match flags.first() {
                Some(Flag::Flag(a)) => {
                    if a == "a" {
                        Command::TryMergeAllBooks
                    } else {
                        Command::InvalidCommand
                    }
                }
                _ => Command::InvalidCommand,
            };
        }
        "!s" => {
            let mut d = false;
            let mut col_exists = false;
            let mut col = String::new();

            for flag in flags.into_iter() {
                match flag {
                    Flag::Flag(f) => {
                        if f == "d" {
                            if col_exists {
                                return Command::SortColumn(col, true);
                            }
                            d = true;
                        }
                    }
                    Flag::FlagWithArgument(f, args) => {
                        d |= f == "d";
                        if d {
                            return Command::SortColumn(args.into_iter().next().unwrap(), d);
                        }
                    }
                    Flag::PositionalArg(args) => {
                        if d {
                            return Command::SortColumn(args.into_iter().next().unwrap(), true);
                        }

                        if !col_exists {
                            col_exists = true;
                            col = args.into_iter().next().unwrap();
                        }
                    }
                };
            }
            return if col_exists {
                Command::SortColumn(col, d)
            } else {
                Command::InvalidCommand
            };
        }
        "!c" => {
            return match flags.into_iter().next() {
                Some(Flag::PositionalArg(args)) => {
                    Command::AddColumn(args.into_iter().next().unwrap())
                }
                Some(Flag::Flag(arg)) => Command::RemoveColumn(arg),
                _ => Command::InvalidCommand,
            };
        }
        "!f" => {
            return match flags.into_iter().next() {
                Some(Flag::PositionalArg(args)) => {
                    let mut args = args.into_iter();
                    Command::FindMatches(
                        Matching::Default,
                        args.next().unwrap(),
                        args.next().unwrap(),
                    )
                }
                Some(Flag::FlagWithArgument(flag, args)) => match flag.as_str() {
                    "r" => {
                        let mut args = args.into_iter();
                        Command::FindMatches(
                            Matching::Regex,
                            args.next().unwrap(),
                            args.next().unwrap(),
                        )
                    }
                    "c" => {
                        let mut args = args.into_iter();
                        Command::FindMatches(
                            Matching::CaseSensitive,
                            args.next().unwrap(),
                            args.next().unwrap(),
                        )
                    }
                    _ => Command::InvalidCommand,
                },
                _ => Command::InvalidCommand,
            };
        }
        "!o" => {
            let mut f = false;
            let mut loc_exists = false;
            let mut loc = String::new();
            let mut index_exists = false;
            let mut index = String::new();
            for flag in flags {
                match flag {
                    Flag::Flag(c) => {
                        f |= c == "f";
                    }
                    Flag::FlagWithArgument(c, args) => {
                        if c == "f" {
                            if let Some(ind_book) = args.get(0) {
                                if let Ok(bi) = u32::from_str(ind_book.as_str()) {
                                    if let Some(ind_var) = args.get(1) {
                                        if let Ok(vi) = usize::from_str(ind_var.as_str()) {
                                            return Command::OpenBookInExplorer(
                                                BookIndex::BookID(bi),
                                                vi,
                                            );
                                        }
                                    }
                                    return Command::OpenBookInExplorer(BookIndex::BookID(bi), 0);
                                }
                            }
                        }
                        return Command::InvalidCommand;
                    }
                    Flag::PositionalArg(args) => {
                        let mut args = args.into_iter();

                        if let Some(l) = args.next() {
                            loc_exists = true;
                            loc = l;
                        }

                        if let Some(i) = args.next() {
                            index_exists = true;
                            index = i;
                        }
                    }
                }
            }
            if loc_exists {
                if let Ok(loc) = u32::from_str(loc.as_str()) {
                    if index_exists {
                        if let Ok(index) = usize::from_str(index.as_str()) {
                            return if f {
                                Command::OpenBookInExplorer(BookIndex::BookID(loc), index)
                            } else {
                                Command::OpenBookInApp(BookIndex::BookID(loc), index)
                            };
                        }
                    } else if f {
                        return Command::OpenBookInExplorer(BookIndex::BookID(loc), 0);
                    } else {
                        return Command::OpenBookInApp(BookIndex::BookID(loc), 0);
                    }
                }
            }
            return if f {
                Command::OpenBookInExplorer(BookIndex::Selected, 0)
            } else {
                Command::OpenBookInApp(BookIndex::Selected, 0)
            };
        }
        _ => return Command::UnknownCommand,
    };
    Command::InvalidCommand
}
