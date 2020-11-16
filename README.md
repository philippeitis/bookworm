bookstore is a fast (hopefully) terminal based library management system (like Calibre).

# Current Features
- Adding single books and books from directories
- Command based interaction
  - Edit: `!e [id]? [column] [new_value]`
  - Delete (a specific book, or all books): `!d [id]?`, `!d -a`
  - Sort ascending/descending: `!s [column] -d?`
  - Add/Remove column: `!c -?[column]`
  - Add book(s): `!a path\to\book.ext` | `!a -d path\to\books`
  - Quit: `!q`
  - Write: `!w`
  - Write and quit: `!wq`
  - (Windows Only) Opening books in native file viewer or File Explorer: `!o [id]?` | `!o -f [id]?`
  - Supplying commands from CLI: `bookstore [args] -- [command]`
- Hotkey navigation and interaction
  - Scrolling up and down using up / down arrow keys, page up / page down, and home / end
  - Selecting books and editing their metadata using F2, or deleting them using Del
  - Specifying settings (selection colours, default columns, default sort settings) via TOML file
 
# Planned Features
- Cloud synchronization (eg. back up database and all books)
- More robust backend database
- Support for supplementary files (eg. more than one cover for a book)
- Deduplication
- Reflecting external libraries as if they are native (eg. access Project Gutenberg directly)
- Providing a mechanism to use external extensions to take advantage of existing scripts for the features below:
  - Metadata scraping (eg. fetch book data)
  - Conversion of ebooks