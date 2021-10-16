enum Edit {
    MoveLeft(usize),
    MoveRight(usize),
    Delete(usize),
    Insert(Vec<char>),
}

enum EditStart {
    DeleteAll,
    FromStart,
    FromEnd,
}

/// The `UserEdit` struct is intended to provide functionality for editing an arbitrarily large
/// number of items without
struct UserEdit {
    edits: Vec<Edit>,
    start: EditStart,
}

impl UserEdit {
    /// Applies the internal sequence of commands to the target
    fn render(&self, target: &str) -> String {
        unimplemented!()
    }

    /// Returns the SQL which will transform a string into the target
    /// Deletion will include wrapping SUBSTR excluding the deleted portion
    /// Insert will be SUBSTR || text || SUBSTR
    /// EditStart with DeleteAll is simply a text replacement -
    ///     using COLUMN = ...;
    /// other variants will adjust the cursor appropriately
    /// SQLite's substring allows negative indices.
    fn sql(&self) -> String {
        unimplemented!()
    }
}
