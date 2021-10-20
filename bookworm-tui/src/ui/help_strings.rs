pub const ADD_BOOKS_HELP_STRING: &str = r#"USAGE:
:a: Add books from a specified path.

FLAGS:
-d: Specifies that the following strings are directory paths
    -r: The given directory should be recursively navigated.
-g: Specifies that the following strings are globs
-p: Specifies that the following strings are path

ARGUMENTS:
-r: (Optional, only applies if -d selected): The maximum depth from which books will be retrieved. If not specified, only files in directory root will be read. If specified without a depth, depth is set to 255.
(FLAG? <string>+)+:

<string>+: A string to fetch books from, according to the flag. If no flag set, treats the string as a file.
"#;

pub const WRITE_FILE_HELP_STRING: &str = r#"USAGE:
:w: Save the database to its current location.
"#;

pub const QUIT_HELP_STRING: &str = r#"USAGE:
:q: Closes the program without saving changes.
"#;

pub const WRITE_AND_QUIT_HELP_STRING: &str = r#"USAGE:
:wq: Save the database, and then close the program.
"#;

pub const DELETE_HELP_STRING: &str = r#"USAGE:
:d: Delete the specified item(s).

FLAGS:
-a: Specifies that everything should be deleted.
-r: Uses <match> as a regular expression.
-e: Uses <match> as an exact substring.
-x: Uses <match> as an exact string.

ARGUMENTS:
If arguments provided, books matching predicates will be deleted.
(FLAG? <column> <match>)+:
FLAG: A flag describing how to use <match>. If none is provided, uses fuzzy search.
<column>: The column to match
<match>: The value to match on
"#;

pub const EDIT_HELP_STRING: &str = r#"USAGE:
:e: Edit the specified item.

FLAGS:
-a: Specifies that the specified column should be appended to.
-d: Specifies that the specified column should be deleted.
-r: Sepcifies that the specified column should be replaced.

ARGUMENTS:
<id>: (Optional) The numeric ID of the book to edit. If not specified, edits the selected item.
(FLAG? <column>, <new_value>? <new_tag_value>?)+:
FLAG: A flag describing what should happen to <column>. If no flag is specified, <column> is replaced.
<column>: The column to operate on
<new_value>: Required if no flag, or -a, is specified.
<new_tag_value>: If no flag, or -a is specified, and <column> is 'tag', <new_tag_value> either
replaces, or is appended to the preexisting tag with value <new_value>.
"#;

pub const MERGE_HELP_STRING: &str = r#"USAGE:
:m: Merge the specified books.

FLAGS:
-a: Specifies that all books should be merged.
"#;

pub const COLUMN_HELP_STRING: &str = r#"USAGE:
:c: Add or remove columns from the UI.

ARGUMENTS:
(-?<column>)+: The column of interest.
If in the form -<column>, column will be removed.
If in the form <column>, column will be added.
"#;

pub const SORT_HELP_STRING: &str = r#"USAGE:
:s: Sort the specified column.

FLAGS:
-d: Sort descending.

ARGUMENTS:
(<-d>? <column>)+: Sort by column. If -d specified, sort column descending
"#;

pub const SEARCH_HELP_STRING: &str = r#"USAGE:
:f: Finds all books matching the given predicates. Will enter a nested screen with all results -
to leave a particular search result page, press ESC.

FLAGS:
-r: Uses <match> as a regular expression.
-e: Uses <match> as an exact substring.
-x: Uses <match> as an exact string.

ARGUMENTS:
(FLAG? <column> <match>)+:
FLAG: A flag describing how to use <match>. If none is provided, uses fuzzy search.
<column>: The column to match
<match>: The value to match on
"#;

pub const JUMP_HELP_STRING: &str = r#"USAGE:
:j: Jumps to the first book matching the given predicate.

FLAGS:
-r: Uses <match> as a regular expression.
-e: Uses <match> as an exact substring.
-x: Uses <match> as an exact string.

ARGUMENTS:
(FLAG? <column> <match>)+:
FLAG: A flag describing how to use <match>. If none is provided, uses fuzzy search.
<column>: The column to match
<match>: The value to match on
"#;

pub const OPEN_HELP_STRING: &str = r#"USAGE:
:o: Open the specified value.

FLAGS:
-f: Open the book in the native file explorer. Windows only.

ARGUMENTS:
<book>: (Optional) The book to open. If not specified, opens the selected item.
<index>: (Optional) The index of the variant to open.
"#;

pub const HELP_HELP_STRING: &str = r#"USAGE:
:h: Show the help string for the specified command.

ARGUMENTS:
<command>: The command of interest. The following commands are available.
    :a: Add books from a specified path.
    :w: Save the database to its current location.
    :q: Closes the program without saving changes.
    :wq: Save the database, and then close the program.
    :d: Delete the specified item(s).
    :e: Edit the specified item.
    :m: Merge the specified books.
    :s: Sort the specified column.
    :c: Add or remove columns from the UI.
    :f: Finds all books with the specified value.
    :o: Open the specified value.
    :h: Find the help string for the specified command.
"#;

pub const GENERAL_HELP: &str = r#"USAGE:
<COMMAND ...>: Runs the command with the specified arguments.
Use :h to find help for a specific command.

COMMANDS:
:a: Add books from a specified path.
:w: Save the database to its current location.
:q: Closes the program without saving changes.
:wq: Save the database, and then close the program.
:d: Delete the specified item(s).
:e: Edit the specified item.
:m: Merge the specified books.
:s: Sort the specified column.
:c: Add or remove columns from the UI.
:f: Finds all books with the specified value.
:o: Open the specified value.
:h: Find the help string for the specified command.
"#;

pub fn help_strings(command: &str) -> Option<&'static str> {
    match command {
        ":a" => Some(ADD_BOOKS_HELP_STRING),
        ":w" => Some(WRITE_FILE_HELP_STRING),
        ":q" => Some(QUIT_HELP_STRING),
        ":wq" => Some(WRITE_AND_QUIT_HELP_STRING),
        ":d" => Some(DELETE_HELP_STRING),
        ":e" => Some(EDIT_HELP_STRING),
        ":m" => Some(MERGE_HELP_STRING),
        ":s" => Some(SORT_HELP_STRING),
        ":c" => Some(COLUMN_HELP_STRING),
        ":f" => Some(SEARCH_HELP_STRING),
        ":j" => Some(JUMP_HELP_STRING),
        ":o" => Some(OPEN_HELP_STRING),
        ":h" => Some(HELP_HELP_STRING),
        _ => None,
    }
}
