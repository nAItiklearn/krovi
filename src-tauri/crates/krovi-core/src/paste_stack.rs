//! Paste stack (SPEC O2).
//!
//! A small, explicit LIFO of up to [`MAX_SLOTS`] numbered slots that the user
//! *actively* pushes onto and pops from — distinct from the passive clipboard
//! history. Slot 1 is always the top (most recently pushed). Pushing past the
//! cap drops the bottom (oldest) slot.

/// Maximum number of addressable slots (1..=9).
pub const MAX_SLOTS: usize = 9;

/// An explicit, bounded paste stack. Internally index 0 is slot 1 (the top).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PasteStack {
    items: Vec<String>,
}

impl PasteStack {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Push a value as the new top (slot 1). If this exceeds [`MAX_SLOTS`], the
    /// bottom (oldest) slot is dropped.
    pub fn push(&mut self, value: impl Into<String>) {
        self.items.insert(0, value.into());
        if self.items.len() > MAX_SLOTS {
            self.items.truncate(MAX_SLOTS);
        }
    }

    /// Pop and return the top (slot 1), or `None` if empty.
    pub fn pop(&mut self) -> Option<String> {
        if self.items.is_empty() {
            None
        } else {
            Some(self.items.remove(0))
        }
    }

    /// Borrow the top value without removing it.
    pub fn peek(&self) -> Option<&str> {
        self.items.first().map(|s| s.as_str())
    }

    /// Get a value by its 1-based slot number (1..=9). Slot 0 or out-of-range
    /// returns `None`.
    pub fn get(&self, slot: usize) -> Option<&str> {
        if slot == 0 {
            return None;
        }
        self.items.get(slot - 1).map(|s| s.as_str())
    }

    /// All occupied slots as `(slot_number, value)` pairs, top first.
    pub fn slots(&self) -> Vec<(usize, &str)> {
        self.items
            .iter()
            .enumerate()
            .map(|(i, s)| (i + 1, s.as_str()))
            .collect()
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_orders_top_first() {
        let mut s = PasteStack::new();
        s.push("a");
        s.push("b");
        s.push("c");
        assert_eq!(s.len(), 3);
        assert_eq!(s.get(1), Some("c"));
        assert_eq!(s.get(2), Some("b"));
        assert_eq!(s.get(3), Some("a"));
        assert_eq!(s.peek(), Some("c"));
    }

    #[test]
    fn overflow_drops_oldest_bottom_slot() {
        let mut s = PasteStack::new();
        for n in 1..=10 {
            s.push(n.to_string());
        }
        assert_eq!(s.len(), MAX_SLOTS);
        assert_eq!(s.get(1), Some("10")); // newest on top
        assert_eq!(s.get(9), Some("2")); // "1" was dropped
        assert_eq!(s.get(10), None);
    }

    #[test]
    fn pop_removes_top_and_empties() {
        let mut s = PasteStack::new();
        s.push("a");
        s.push("b");
        assert_eq!(s.pop(), Some("b".to_string()));
        assert_eq!(s.pop(), Some("a".to_string()));
        assert_eq!(s.pop(), None);
        assert!(s.is_empty());
    }

    #[test]
    fn slot_numbering_is_one_based() {
        let mut s = PasteStack::new();
        s.push("only");
        assert_eq!(s.get(0), None);
        assert_eq!(s.get(1), Some("only"));
        assert_eq!(s.get(2), None);
        assert_eq!(s.slots(), vec![(1, "only")]);
    }

    #[test]
    fn clear_empties() {
        let mut s = PasteStack::new();
        s.push("a");
        s.push("b");
        s.clear();
        assert!(s.is_empty());
        assert_eq!(s.peek(), None);
    }
}
