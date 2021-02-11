bookstore is a fast (hopefully) terminal based library management system (like Calibre).

# Current Features
- Adding single books and books from directories
- Command based interaction (`[id]?` - if no id is provided, the selected item is used.)
  - Edit: `!e [id]? [column] [new_value]`
  - Delete (a specific book, or all books): `!d [id]?`, `!d -a`
  - Sort ascending/descending: `!s [column] -d?`
  - Add/Remove column: `!c -?[column]`
  - Add book(s): `!a path\to\book.ext` | `!a -d path\to\books`
  - Quit: `!q`
  - Write: `!w`
  - Write and quit: `!wq`
  - Merging all books with matching metadata: `!m -a`
  - Opening books in native file viewer (Windows, MacOS, Linux) or File Explorer (Windows only): `!o [id]?` | `!o -f [id]?`
  - Finding books with regex or case-sensitive string or default: `!f (-r|-c)? [column] [search_str]`
  - View help strings: `!h (command)?`
  - Supplying one or more commands from CLI: `bookstore [args] (-- [command])*`
- Hotkey navigation and interaction
  - Scrolling up and down using:
    - scroll wheel on mouse
    - up / down arrow keys
    - page up / page down
    - home / end
  - Selecting books and editing their metadata using F2, or deleting them using Del
  - Specifying settings (selection colours, default columns, default sort settings) via TOML file
 
# Planned Features
- Cloud synchronization (eg. back up database and all books to Google Drive)
- More robust backend database (eg. SQLite)
- Support for supplementary files (eg. more than one cover for a book)
- Thorough Deduplication
- Reflecting external libraries as if they are native (eg. access Project Gutenberg directly)
- Providing a mechanism to use external extensions to take advantage of existing scripts for the features below:
  - Metadata scraping (eg. fetch book data)
  - Conversion of ebooks
  
# Installation

```bash
git clone https://github.com/philippeitis/bookstore.git
cd bookstore
cargo install --path bookstore_tui
```

The minimum supported Rust version is current stable.

Note that not all terminals are fully supported - Windows Command Prompt and Ubuntu's default terminal are tested and work correctly. However, scrolling with the mouse does not work in Windows Terminal. 