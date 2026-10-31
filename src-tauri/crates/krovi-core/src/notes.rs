//! Quick notes (SPEC C8).
//!
//! A small collection of free-text scratch notes kept in most-recently-updated
//! order. Autosave and real timestamps are the integration layer's concern; here
//! the caller supplies `now` (epoch ms) on every mutation so behaviour is
//! deterministic.

/// A single scratch note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub id: u64,
    pub body: String,
    pub created_at: u64,
    pub updated_at: u64,
}

/// A collection of notes, newest-updated first.
#[derive(Debug, Clone, Default)]
pub struct Notes {
    next_id: u64,
    notes: Vec<Note>,
}

impl Notes {
    pub fn new() -> Self {
        Self {
            next_id: 1,
            notes: Vec::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.notes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.notes.is_empty()
    }

    /// Create a note and return its id. New notes go to the front.
    pub fn create(&mut self, body: impl Into<String>, now: u64) -> u64 {
        let id = self.next_id.max(1);
        self.next_id = id + 1;
        self.notes.insert(
            0,
            Note {
                id,
                body: body.into(),
                created_at: now,
                updated_at: now,
            },
        );
        id
    }

    /// Replace a note's body, bump its `updated_at`, and float it to the front.
    /// Returns `false` if the id is unknown.
    pub fn update(&mut self, id: u64, body: impl Into<String>, now: u64) -> bool {
        if let Some(pos) = self.notes.iter().position(|n| n.id == id) {
            let mut note = self.notes.remove(pos);
            note.body = body.into();
            note.updated_at = now;
            self.notes.insert(0, note);
            true
        } else {
            false
        }
    }

    pub fn get(&self, id: u64) -> Option<&Note> {
        self.notes.iter().find(|n| n.id == id)
    }

    pub fn remove(&mut self, id: u64) -> bool {
        if let Some(pos) = self.notes.iter().position(|n| n.id == id) {
            self.notes.remove(pos);
            true
        } else {
            false
        }
    }

    /// All notes, most-recently-updated first.
    pub fn list(&self) -> &[Note] {
        &self.notes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_lists_newest_first() {
        let mut n = Notes::new();
        let a = n.create("first", 1);
        let b = n.create("second", 2);
        assert_eq!(n.len(), 2);
        let list = n.list();
        assert_eq!(list[0].id, b);
        assert_eq!(list[1].id, a);
    }

    #[test]
    fn update_changes_body_timestamp_and_order() {
        let mut n = Notes::new();
        let a = n.create("a", 1);
        let _b = n.create("b", 2);
        assert!(n.update(a, "a-edited", 9));
        let list = n.list();
        assert_eq!(list[0].id, a); // floated to front on update
        assert_eq!(list[0].body, "a-edited");
        assert_eq!(list[0].updated_at, 9);
        assert_eq!(list[0].created_at, 1); // creation time preserved
    }

    #[test]
    fn get_and_remove() {
        let mut n = Notes::new();
        let a = n.create("a", 1);
        assert_eq!(n.get(a).unwrap().body, "a");
        assert!(n.remove(a));
        assert!(!n.remove(a));
        assert!(n.get(a).is_none());
        assert!(n.is_empty());
    }

    #[test]
    fn update_unknown_id_is_false() {
        let mut n = Notes::new();
        assert!(!n.update(123, "x", 1));
    }
}
