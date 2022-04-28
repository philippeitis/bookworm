# bookworm
![CI](https://github.com/philippeitis/bookworm/actions/workflows/rust.yml/badge.svg)
[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](https://www.gnu.org/licenses/gpl-3.0)

bookworm is a fast TUI library management system, like Calibre.

# Features
- High performance reading of ebook metadata
  - bookworm can read ~8000 books per second, whereas Calibre, on the same system, with the same books, takes multiple minutes
- Command based interface, usable from both TUI and CLI
- SQLite backend
  - Modifications are synchronized to the SQLite backend at the path specified via --database
- Instant startup
  - bookworm only reads books into memory when they're needed, allowing a database with millions of books to be opened instantly
# Installation
## Download binary
Binary downloads are available at https://github.com/philippeitis/bookworm/releases

To use copy and paste functionality on Linux, the following dependencies must be installed:
```bash
sudo apt-get install xorg-dev libxcb1-dev libxcb-shape0-dev libxcb-xfixes0-dev
```

## Build from source
On Windows, MacOS
```bash
git clone https://github.com/philippeitis/bookworm.git
cd bookworm
cargo install --path bookworm-tui --features copypaste 
```

On Linux distros, additional dependencies are required for copy-paste support:
```bash
git clone https://github.com/philippeitis/bookworm.git
cd bookworm
sudo apt-get install xorg-dev libxcb1-dev libxcb-shape0-dev libxcb-xfixes0-dev
cargo install --path bookworm-tui --features copypaste
```

## Compatibility
The minimum supported Rust version is current stable.

Note that not all terminals are fully supported -  Ubuntu's default terminal works correctly. Windows Terminal does not currently support mouse scrolling.

# Interaction
- Adding single books and books from directories
- Command based interaction
- Hotkey navigation and interaction
  - Selecting books and editing their metadata using F2, or deleting them using Del
  - Specifying settings (selection colours, default columns, default sort settings) via TOML file
  - Copying and pasting supported fields, on supported platforms via CTRL+C, CTRL+V
## Commands
Arguments which take `[id]?` will modify the selected items if no id is provided.

All commands which don't make use of UI interaction can be used from the command line, using `bookworm [args] (-- [command])*`

| Command                                            | Description                                                                     |
|:---------------------------------------------------|---------------------------------------------------------------------------------|
| `:q`                                               | Quit                                                                            |
| `:w`                                               | Write                                                                           |
| `:wq`                                              | Write and then quit                                                             |
| `:h (command)?`                                    | View help information (for a particular command)                                |
| `:o (-f)? [id]?`                                   | Open a book in default app / file manager with given id                         |
| `:c (-?[column])+`                                 | Add/Remove columns                                                              |
| `:s ([column] -d?)*`                               | Sort by column, ascending (default) or descending                               |
| `:a -r? ((-d / -p / -g)? path+ -r?)+`              | Add a single book, multiple books, or books matching a glob                     |
| `:e [id]? ((-a / -r / -d)? [column] [new_value])+` | Edit the book                                                                   |
| `:m -a`                                            | Merge all books with matching metadata                                          |
| `:d`                                               | Delete selected book                                                            |
| `:d -a`                                            | Delete all books                                                                |
| `:d ((-r /-e / -x)? [column] [search_str])+`       | Delete books matching predicates                                                |
| `:j ((-r / -e / -x)? [column] [search_str])+`      | Jumping to a book matching the regex / exact substring / exact string / default |
| `:f ((-r / -e / -x)? [column] [search_str])+`      | Finding books matching the regex / exact substring / exact string / default     |

## Keybindings
| Keybinding    | Description               |
|:--------------|---------------------------|
| `CTRL + Q`    | Quit                      |
| `CTRL + S`    | Save all changes          |
| `PAGE UP`     | Go up one page of books   | 
| `PAGE DOWN`   | Go down one page of books |
| `HOME`        | First book in collection  |
| `END`         | Last book in collection   |
| `UP`          | Go up one book            |
| `DOWN`        | Go down one book          |
| `SCROLL UP`   | Go up n books             |
| `SCROLL DOWN` | Go down n books           |

# Planned Features
- Cloud synchronization (eg. back up database and all books to Google Drive)
- Support for supplementary files (eg. more than one cover for a book)
- Reflecting external libraries as if they are native (eg. access Project Gutenberg directly)
