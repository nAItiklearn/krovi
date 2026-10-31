//! File shelf (SPEC C4) — metadata model.
//!
//! Tracks files "parked" on one or more named shelves: display name, the
//! stored-copy path/reference, size, and when it was added. Copying bytes to/from
//! disk and OS drag-and-drop live in the integration layer; this module is pure
//! metadata and deterministic (the caller supplies `added_at`).

/// A parked file's metadata. `stored_path` references the copy the store layer
/// keeps under the Krovi data dir; this crate never touches the filesystem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShelfItem {
    pub id: u64,
    pub name: String,
    pub stored_path: String,
    pub size_bytes: u64,
    pub added_at: u64,
}

/// A named shelf holding items newest-first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shelf {
    pub name: String,
    items: Vec<ShelfItem>,
}

impl Shelf {
    pub fn items(&self) -> &[ShelfItem] {
        &self.items
    }
    pub fn len(&self) -> usize {
        self.items.len()
    }
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// The default shelf name created with a fresh [`FileShelf`].
pub const DEFAULT_SHELF: &str = "Default";

/// A collection of named shelves. There is always at least one shelf.
#[derive(Debug, Clone)]
pub struct FileShelf {
    next_id: u64,
    shelves: Vec<Shelf>,
}

impl Default for FileShelf {
    fn default() -> Self {
        Self::new()
    }
}

impl FileShelf {
    /// Create a file shelf containing a single empty shelf named [`DEFAULT_SHELF`].
    pub fn new() -> Self {
        Self {
            next_id: 1,
            shelves: vec![Shelf {
                name: DEFAULT_SHELF.to_string(),
                items: Vec::new(),
            }],
        }
    }

    pub fn shelf_names(&self) -> Vec<&str> {
        self.shelves.iter().map(|s| s.name.as_str()).collect()
    }

    pub fn shelf(&self, name: &str) -> Option<&Shelf> {
        self.shelves.iter().find(|s| s.name == name)
    }

    fn shelf_mut(&mut self, name: &str) -> Option<&mut Shelf> {
        self.shelves.iter_mut().find(|s| s.name == name)
    }

    /// Add a new empty shelf. Returns `false` if the name already exists.
    pub fn add_shelf(&mut self, name: impl Into<String>) -> bool {
        let name = name.into();
        if self.shelves.iter().any(|s| s.name == name) {
            return false;
        }
        self.shelves.push(Shelf {
            name,
            items: Vec::new(),
        });
        true
    }

    /// Remove a shelf and its items. Returns `false` if unknown or if it is the
    /// last remaining shelf (there is always at least one).
    pub fn remove_shelf(&mut self, name: &str) -> bool {
        if self.shelves.len() <= 1 {
            return false;
        }
        if let Some(pos) = self.shelves.iter().position(|s| s.name == name) {
            self.shelves.remove(pos);
            true
        } else {
            false
        }
    }

    /// Park a file on a shelf (newest first). Returns the new item id, or `None`
    /// if the shelf doesn't exist (in which case no id is consumed).
    pub fn add_item(
        &mut self,
        shelf: &str,
        name: impl Into<String>,
        stored_path: impl Into<String>,
        size_bytes: u64,
        added_at: u64,
    ) -> Option<u64> {
        let id = self.next_id;
        let name = name.into();
        let stored_path = stored_path.into();
        let s = self.shelf_mut(shelf)?;
        s.items.insert(
            0,
            ShelfItem {
                id,
                name,
                stored_path,
                size_bytes,
                added_at,
            },
        );
        self.next_id += 1;
        Some(id)
    }

    pub fn remove_item(&mut self, shelf: &str, id: u64) -> bool {
        if let Some(s) = self.shelf_mut(shelf) {
            if let Some(pos) = s.items.iter().position(|i| i.id == id) {
                s.items.remove(pos);
                return true;
            }
        }
        false
    }

    /// Move an item from one shelf to another, placing it newest on the
    /// destination. Returns `false` for a missing shelf, missing item, or a
    /// same-shelf move.
    pub fn move_item(&mut self, from: &str, to: &str, id: u64) -> bool {
        if from == to || self.shelf(to).is_none() {
            return false;
        }
        let item = {
            let src = match self.shelf_mut(from) {
                Some(s) => s,
                None => return false,
            };
            match src.items.iter().position(|i| i.id == id) {
                Some(pos) => src.items.remove(pos),
                None => return false,
            }
        };
        // Destination existence was checked above.
        self.shelf_mut(to).unwrap().items.insert(0, item);
        true
    }

    /// Total items across all shelves.
    pub fn total_items(&self) -> usize {
        self.shelves.iter().map(|s| s.items.len()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_has_single_empty_default_shelf() {
        let fs = FileShelf::new();
        assert_eq!(fs.shelf_names(), vec![DEFAULT_SHELF]);
        assert!(fs.shelf(DEFAULT_SHELF).unwrap().is_empty());
        assert_eq!(fs.total_items(), 0);
    }

    #[test]
    fn add_shelf_rejects_duplicates() {
        let mut fs = FileShelf::new();
        assert!(fs.add_shelf("Work"));
        assert!(!fs.add_shelf("Work"));
        assert_eq!(fs.shelf_names(), vec![DEFAULT_SHELF, "Work"]);
    }

    #[test]
    fn add_item_newest_first_and_missing_shelf_is_none() {
        let mut fs = FileShelf::new();
        let a = fs
            .add_item(DEFAULT_SHELF, "a.txt", "blobs/a", 10, 1)
            .unwrap();
        let b = fs
            .add_item(DEFAULT_SHELF, "b.txt", "blobs/b", 20, 2)
            .unwrap();
        let items = fs.shelf(DEFAULT_SHELF).unwrap().items();
        assert_eq!(items[0].id, b);
        assert_eq!(items[1].id, a);
        assert!(fs.add_item("Nope", "x", "blobs/x", 1, 3).is_none());
        // the failed add must not consume an id
        let c = fs
            .add_item(DEFAULT_SHELF, "c.txt", "blobs/c", 30, 4)
            .unwrap();
        assert_eq!(c, b + 1);
    }

    #[test]
    fn remove_item_works_and_reports_missing() {
        let mut fs = FileShelf::new();
        let a = fs.add_item(DEFAULT_SHELF, "a", "blobs/a", 1, 1).unwrap();
        assert!(fs.remove_item(DEFAULT_SHELF, a));
        assert!(!fs.remove_item(DEFAULT_SHELF, a));
        assert!(!fs.remove_item("Nope", a));
    }

    #[test]
    fn move_item_between_shelves() {
        let mut fs = FileShelf::new();
        fs.add_shelf("Work");
        let a = fs.add_item(DEFAULT_SHELF, "a", "blobs/a", 1, 1).unwrap();
        assert!(fs.move_item(DEFAULT_SHELF, "Work", a));
        assert_eq!(fs.shelf(DEFAULT_SHELF).unwrap().len(), 0);
        assert_eq!(fs.shelf("Work").unwrap().len(), 1);
        assert_eq!(fs.shelf("Work").unwrap().items()[0].id, a);
        // same-shelf and missing-destination moves fail
        assert!(!fs.move_item("Work", "Work", a));
        assert!(!fs.move_item("Work", "Ghost", a));
    }

    #[test]
    fn cannot_remove_last_shelf() {
        let mut fs = FileShelf::new();
        assert!(!fs.remove_shelf(DEFAULT_SHELF));
        fs.add_shelf("Work");
        assert!(fs.remove_shelf("Work"));
        assert!(!fs.remove_shelf(DEFAULT_SHELF)); // back to one shelf
    }
}
