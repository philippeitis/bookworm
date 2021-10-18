bookstore is a fast TUI library management system, like Calibre.

# Features
- High performance reading of ebook metadata
  - bookstore can read ~8000 books per second, whereas Calibre, on the same system, with the same books, takes multiple minutes
- Command based interface, usable from both TUI and CLI
- SQLite backend
  - Modifications are synchronized to the SQLite backend at the path specified via --database
- Instant startup
  - bookstore only reads books into memory when they're needed, allowing a database with millions of books to be opened instantly
# Installation
## Download binary
Binary downloads are available at https://github.com/philippeitis/bookstore/releases

The Linux version requires installing dependencies for copy & paste beforehand, using the following command:
```bash
sudo apt-get install xorg-dev libxcb1-dev libxcb-shape0-dev libxcb-xfixes0-dev
```

## Build from source
On Windows, MacOS
```bash
git clone https://github.com/philippeitis/bookstore.git
cd bookstore
cargo install --path bookstore-tui --features copypaste 
```

On Linux distros, additional dependencies are required for copy-paste support:
```bash
git clone https://github.com/philippeitis/bookstore.git
cd bookstore
sudo apt-get install xorg-dev libxcb1-dev libxcb-shape0-dev libxcb-xfixes0-dev
cargo install --path bookstore-tui --features copypaste
```

The minimum supported Rust version is current stable.

Note that not all terminals are fully supported - Windows Command Prompt and Ubuntu's default terminal are tested and work correctly. However, scrolling with the mouse does not work in Windows Terminal. 

# Interaction
- Adding single books and books from directories
- Command based interaction (`[id]?` - if no id is provided, the selected item is used.)
  - Edit: `:e [id]? ((-a|-r|-d)? [column] [new_value])+`
  - Delete books matching predicates, all books, or selected book: `:d ((-r|-e|-x)? [column] [search_str])+`, `:d -a`, `:d`
  - Sort ascending/descending: `:s ([column] -d?)*`
  - Add/Remove columns: `:c (-?[column])+`
  - Add a single book, multiple books, or books matching a glob: `:a -r? ((-d|-p|-g)? path+ -r?)+`
  - Quit: `:q` | CTRL + Q
  - Write: `:w` | CTRL + S
  - Write and quit: `:wq`
  - Merging all books with matching metadata: `:m -a`
  - Opening books in default app or file manager (Windows, MacOS, Linux): `:o [id]?` | `:o -f [id]?`
  - Finding books with regex or exact substring string or exact string or default: `:f ((-r|-e|-x)? [column] [search_str])+`
  - Jumping to a book with regex or exact substring string or exact string or default: `:j ((-r|-e|-x)? [column] [search_str])+`
  - View help strings: `:h (command)?`
  - Supplying one or more commands from CLI: `bookstore [args] (-- [command])*`
- Hotkey navigation and interaction
  - Scrolling up and down using:
    - scroll wheel on mouse
    - up / down arrow keys
    - page up / page down
    - home / end
  - Selecting books and editing their metadata using F2, or deleting them using Del
  - Specifying settings (selection colours, default columns, default sort settings) via TOML file
  - Copying and pasting supported fields, on supported platforms via CTRL+C, CTRL+V

# Planned Features
- Cloud synchronization (eg. back up database and all books to Google Drive)
- Support for supplementary files (eg. more than one cover for a book)
- Reflecting external libraries as if they are native (eg. access Project Gutenberg directly)
