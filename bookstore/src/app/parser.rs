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

pub enum CommandError {
    UnknownCommand,
    InsufficientArguments,
    InvalidCommand,
}

impl Command {
    pub(crate) fn requires_ui(&self) -> bool {
        match self {
            Command::DeleteBook(b) => b == &BookIndex::Selected,
            Command::EditBook(b, _, _) => b == &BookIndex::Selected,
            Command::OpenBookInApp(b, _) => b == &BookIndex::Selected,
            Command::OpenBookInExplorer(b, _) => b == &BookIndex::Selected,
            Command::AddColumn(_) => true,
            Command::RemoveColumn(_) => true,
            Command::SortColumn(_, _) => true,
            Command::FindMatches(_, _, _) => true,
            _ => false,
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
pub(crate) fn parse_command_string<S: ToString>(s: S) -> Result<Command, CommandError> {
    let s = s.to_string();
    match shellwords::split(s.as_str()) {
        Ok(vec) => parse_args(vec),
        Err(_) => Err(CommandError::InvalidCommand),
    }
}

/// Reads args and returns the appropriate command. If the command format is incorrect,
/// returns Command::InvalidCommand, or Command::InvalidCommand if the first argument is not
/// a recognized command.
pub(crate) fn parse_args(args: Vec<String>) -> Result<Command, CommandError> {
    let c = if let Some(c) = args.first() {
        c.clone()
    } else {
        return Err(CommandError::InsufficientArguments);
    };

    let insuf = || CommandError::InsufficientArguments;

    let flags = read_flags(args);
    match c.as_str() {
        "!q" => Ok(Command::Quit),
        "!w" => Ok(Command::Write),
        "!wq" => Ok(Command::WriteAndQuit),
        "!a" => {
            let mut d = false;
            let mut path_exists = false;
            let mut path = PathBuf::new();

            for flag in flags {
                match flag {
                    Flag::Flag(c) => {
                        d |= c == "d";
                        if d && path_exists {
                            return Ok(Command::AddBooksFromDir(path));
                        }
                    }
                    Flag::FlagWithArgument(c, args) => {
                        d |= c == "d";
                        if d {
                            return Ok(Command::AddBooksFromDir(PathBuf::from(&args[0])));
                        }
                    }
                    Flag::PositionalArg(args) => {
                        if !path_exists {
                            path_exists = true;
                            path = PathBuf::from(&args[0]);
                        }
                        if d {
                            return Ok(Command::AddBooksFromDir(path));
                        }
                    }
                };
            }
            if path_exists {
                Ok(if d {
                    Command::AddBooksFromDir(path)
                } else {
                    Command::AddBookFromFile(path)
                })
            } else {
                Err(CommandError::InsufficientArguments)
            }
        }
        "!d" => {
            if let Some(flag) = flags.first() {
                match flag {
                    Flag::Flag(a) => {
                        if a == "a" {
                            Ok(Command::DeleteAll)
                        } else {
                            Err(CommandError::InvalidCommand)
                        }
                    }
                    Flag::PositionalArg(args) => {
                        if let Ok(i) = u32::from_str(args[0].as_str()) {
                            Ok(Command::DeleteBook(BookIndex::BookID(i)))
                        } else {
                            Err(CommandError::InvalidCommand)
                        }
                    }
                    _ => Err(CommandError::InvalidCommand),
                }
            } else {
                Ok(Command::DeleteBook(BookIndex::Selected))
            }
        }
        "!e" => {
            match flags.into_iter().next() {
                Some(Flag::PositionalArg(args)) => {
                    let mut args = args.into_iter();
                    let a = args.next().ok_or_else(insuf)?;
                    let b = args.next().ok_or_else(insuf)?;

                    if let Some(c) = args.next() {
                        if let Ok(id) = u32::from_str(a.as_str()) {
                            return Ok(Command::EditBook(BookIndex::BookID(id), b, c));
                        }
                    }

                    Ok(Command::EditBook(BookIndex::Selected, a, b))
                }
                _ => Err(CommandError::InvalidCommand),
            }
        }
        "!m" => {
            match flags.first() {
                Some(Flag::Flag(a)) => {
                    if a == "a" {
                        Ok(Command::TryMergeAllBooks)
                    } else {
                        Err(CommandError::InvalidCommand)
                    }
                }
                _ => Err(CommandError::InvalidCommand),
            }
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
                                return Ok(Command::SortColumn(col, true));
                            }
                            d = true;
                        }
                    }
                    Flag::FlagWithArgument(f, args) => {
                        d |= f == "d";
                        if d {
                            return Ok(Command::SortColumn(
                                args.into_iter().next().ok_or_else(insuf)?,
                                d,
                            ));
                        }
                    }
                    Flag::PositionalArg(args) => {
                        if d {
                            return Ok(Command::SortColumn(
                                args.into_iter().next().ok_or_else(insuf)?,
                                true,
                            ));
                        }

                        if !col_exists {
                            col_exists = true;
                            col = args.into_iter().next().ok_or_else(insuf)?;
                        }
                    }
                };
            }

            if col_exists {
                Ok(Command::SortColumn(col, d))
            } else {
                Err(CommandError::InsufficientArguments)
            }
        }
        "!c" => {
            match flags.into_iter().next() {
                Some(Flag::PositionalArg(args)) => Ok(Command::AddColumn(
                    args.into_iter().next().ok_or_else(insuf)?,
                )),
                Some(Flag::Flag(arg)) => Ok(Command::RemoveColumn(arg)),
                _ => Err(CommandError::InvalidCommand),
            }
        }
        "!f" => {
            match flags.into_iter().next() {
                Some(Flag::PositionalArg(args)) => {
                    let mut args = args.into_iter();
                    Ok(Command::FindMatches(
                        Matching::Default,
                        args.next().ok_or_else(insuf)?,
                        args.next().ok_or_else(insuf)?,
                    ))
                }
                Some(Flag::FlagWithArgument(flag, args)) => match flag.as_str() {
                    "r" => {
                        let mut args = args.into_iter();
                        Ok(Command::FindMatches(
                            Matching::Regex,
                            args.next().ok_or_else(insuf)?,
                            args.next().ok_or_else(insuf)?,
                        ))
                    }
                    "c" => {
                        let mut args = args.into_iter();
                        Ok(Command::FindMatches(
                            Matching::CaseSensitive,
                            args.next().ok_or_else(insuf)?,
                            args.next().ok_or_else(insuf)?,
                        ))
                    }
                    _ => Err(CommandError::InvalidCommand),
                },
                _ => Err(CommandError::InvalidCommand),
            }
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
                                            return Ok(Command::OpenBookInExplorer(
                                                BookIndex::BookID(bi),
                                                vi,
                                            ));
                                        }
                                    }
                                    return Ok(Command::OpenBookInExplorer(
                                        BookIndex::BookID(bi),
                                        0,
                                    ));
                                }
                            }
                        }
                        return Err(CommandError::InvalidCommand);
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
                            return Ok(if f {
                                Command::OpenBookInExplorer(BookIndex::BookID(loc), index)
                            } else {
                                Command::OpenBookInApp(BookIndex::BookID(loc), index)
                            });
                        }
                    } else if f {
                        return Ok(Command::OpenBookInExplorer(BookIndex::BookID(loc), 0));
                    } else {
                        return Ok(Command::OpenBookInApp(BookIndex::BookID(loc), 0));
                    }
                }
            }
            Ok(if f {
                Command::OpenBookInExplorer(BookIndex::Selected, 0)
            } else {
                Command::OpenBookInApp(BookIndex::Selected, 0)
            })
        }
        _ => Err(CommandError::UnknownCommand),
    }
}
