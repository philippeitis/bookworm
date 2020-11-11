use std::path::PathBuf;
use std::str::FromStr;

extern crate shellwords;

#[derive(Debug)]
pub enum Flag {
    Flag(String),
    FlagWithArgument(String, Vec<String>),
    PositionalArg(Vec<String>),
}

#[derive(Debug)]
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
    EditBook(BookIndex, String, String),
    AddBookFromFile(PathBuf),
    AddBooksFromDir(PathBuf),
    AddColumn(String),
    RemoveColumn(String),
    SortColumn(String, bool),
    OpenBookInApp(BookIndex),
    OpenBookInExplorer(BookIndex),
    Quit(bool),
}

// Get flags and corresponding values
fn read_flags(vec: &[String]) -> Vec<Flag> {
    if vec.is_empty() {
        return vec![];
    }
    let mut flags = vec![];
    let mut last_flag_valid = false;
    let mut flag = String::new();
    let mut flag_args = vec![];
    let mut ended = false;
    for v in vec.iter() {
        ended = false;
        if v.starts_with("-") {
            if last_flag_valid {
                if flag_args.is_empty() {
                    flags.push(Flag::Flag(flag.clone()));
                } else {
                    flags.push(Flag::FlagWithArgument(flag.clone(), flag_args.clone()));
                    flag_args.clear();
                    ended = true;
                }
            } else {
                if !flag_args.is_empty() {
                    flags.push(Flag::PositionalArg(flag_args.clone()));
                    flag_args.clear();
                }
            }
            flag = v.trim_start_matches('-').to_string();
            last_flag_valid = true;
        } else {
            flag_args.push(v.clone());
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
            flags.push(Flag::PositionalArg(flag_args.clone()));
        }
    }
    flags
}

pub(crate) fn parse_command_string<S: ToString>(s: S) -> Command {
    let s = s.to_string();
    match shellwords::split(s.as_str()) {
        Ok(vec) => {
            let c = if let Some(c) = vec.first() {
                c
            } else {
                return Command::InvalidCommand;
            };
            let flags = read_flags(&vec[1..]);
            match c.as_str() {
                "!a" => {
                    match &flags[0] {
                        Flag::FlagWithArgument(d, _) => {
                            if d == &"d".to_string() {
                                if let Some(s) = s.strip_prefix("!a -d ") {
                                    return Command::AddBooksFromDir(PathBuf::from(s));
                                }
                            }
                        }
                        _ => {
                            if let Some(f) = flags.last() {
                                match f {
                                    Flag::Flag(d) => {
                                        if d == &"d".to_string() {
                                            if let Some(s) = s.strip_prefix("!a -d") {
                                                return Command::AddBooksFromDir(PathBuf::from(s));
                                            }
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                    let s = s.chars().skip(3).collect::<String>();
                    return Command::AddBookFromFile(PathBuf::from(s));
                }
                "!q" => {
                    for flag in flags {
                        match flag {
                            Flag::Flag(c) => {
                                if c == "s".to_string() {
                                    return Command::Quit(false);
                                }
                            }
                            Flag::FlagWithArgument(c, _) => {
                                if c == "s".to_string() {
                                    return Command::Quit(false);
                                }
                            }
                            _ => {}
                        };
                    }
                    return Command::Quit(true);
                }
                "!d" => {
                    for flag in flags {
                        return match flag {
                            Flag::PositionalArg(args) => {
                                if let Ok(i) = u32::from_str(args[0].as_str()) {
                                    Command::DeleteBook(BookIndex::BookID(i))
                                } else {
                                    Command::InvalidCommand
                                }
                            }
                            _ => Command::InvalidCommand,
                        };
                    }
                    return Command::DeleteBook(BookIndex::Selected);
                }
                "!e" => {
                    for flag in flags {
                        return match flag {
                            Flag::PositionalArg(args) => {
                                if args.len() == 3 {
                                    if let Ok(i) = u32::from_str(args[0].as_str()) {
                                        Command::EditBook(
                                            BookIndex::BookID(i),
                                            args[1].clone(),
                                            args[2].clone(),
                                        )
                                    } else {
                                        Command::InvalidCommand
                                    }
                                } else if args.len() == 2 {
                                    Command::EditBook(
                                        BookIndex::Selected,
                                        args[0].clone(),
                                        args[1].clone(),
                                    )
                                } else {
                                    Command::InvalidCommand
                                }
                            }
                            _ => Command::InvalidCommand,
                        };
                    }
                    return Command::InvalidCommand;
                }
                "!s" => {
                    for flag in flags {
                        return match flag {
                            Flag::PositionalArg(args) => {
                                if let Some(s) = args.get(1) {
                                    if s == "d" {
                                        return Command::SortColumn(args[0].to_string(), true);
                                    }
                                    return Command::InvalidCommand;
                                }
                                Command::SortColumn(args[0].to_string(), false)
                            }
                            _ => {
                                return Command::InvalidCommand;
                            }
                        };
                    }
                }
                "!c" => {
                    for flag in flags {
                        return match flag {
                            Flag::PositionalArg(args) => Command::AddColumn(args[0].clone()),
                            Flag::Flag(arg) => Command::RemoveColumn(arg),
                            _ => Command::InvalidCommand,
                        };
                    }
                    return Command::InvalidCommand;
                }
                "!o" => {
                    let mut f = false;
                    let mut loc_exists = false;
                    let mut loc = String::new();
                    for flag in flags {
                        match flag {
                            Flag::Flag(c) => {
                                f |= c == "f".to_string();
                            }
                            Flag::FlagWithArgument(c, args) => {
                                if c == "f".to_string() {
                                    if let Ok(i) = u32::from_str(args[0].as_str()) {
                                        return Command::OpenBookInExplorer(BookIndex::BookID(i));
                                    }
                                    return Command::OpenBookInExplorer(BookIndex::Selected);
                                }
                                return Command::InvalidCommand;
                            }
                            Flag::PositionalArg(args) => {
                                loc_exists = true;
                                loc = args[0].clone();
                            }
                        }
                        if f && loc_exists {
                            if let Ok(i) = u32::from_str(loc.as_str()) {
                                return Command::OpenBookInExplorer(BookIndex::BookID(i));
                            }
                            return Command::OpenBookInExplorer(BookIndex::Selected);
                        }
                    }
                    if loc_exists {
                        if let Ok(i) = u32::from_str(loc.as_str()) {
                            return Command::OpenBookInApp(BookIndex::BookID(i));
                        }
                        return Command::OpenBookInApp(BookIndex::Selected);
                    } else {
                        if f {
                            return Command::OpenBookInExplorer(BookIndex::Selected);
                        }
                        return Command::OpenBookInApp(BookIndex::Selected);
                    }
                }
                _ => return Command::UnknownCommand,
            }
        }
        Err(_) => return Command::InvalidCommand,
    }
    Command::InvalidCommand
}
