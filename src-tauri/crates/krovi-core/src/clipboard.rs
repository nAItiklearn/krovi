//! In-memory clipboard history (SPEC C2).
//!
//! A recency-ordered store with dedup-on-recopy (promotes instead of duplicating),
//! pinning, a configurable capacity applied to *unpinned* entries only, and
//! substring / kind queries. Pure logic: no clock, no persistence, no I/O —
//! timestamps are supplied by the caller so behaviour is fully deterministic.

/// Default number of *unpinned* entries retained (SPEC C2).
pub const DEFAULT_CAP: usize = 200;

/// The nature of a clipboard entry. Images are stored *by reference* (a blob key
/// or path resolved by the store layer), never inline in this crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClipKind {
    Text,
    Image,
    Link,
}

/// A single clipboard history item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipEntry {
    pub id: u64,
    pub kind: ClipKind,
    /// Text, URL, or a blob reference for images.
    pub content: String,
    pub pinned: bool,
    /// Caller-supplied epoch milliseconds (deterministic in tests).
    pub created_at: u64,
}

/// Recency-ordered clipboard history. Internally index 0 is the newest entry.
#[derive(Debug, Clone)]
pub struct ClipboardHistory {
    cap: usize,
    next_id: u64,
    entries: Vec<ClipEntry>,
}

impl Default for ClipboardHistory {
    fn default() -> Self {
        Self::new(DEFAULT_CAP)
    }
}

impl ClipboardHistory {
    /// Create a history retaining up to `cap` unpinned entries (clamped to ≥ 1).
    pub fn new(cap: usize) -> Self {
        Self {
            cap: cap.max(1),
            next_id: 1,
            entries: Vec::new(),
        }
    }

    /// Rebuild a history from persisted entries (any order). Internal recency is
    /// restored by id (ids are monotonic, so a larger id is a newer copy) and
    /// `next_id` continues after the largest persisted id so ids never collide
    /// with restored ones. Capacity is applied to the unpinned entries.
    pub fn from_entries(cap: usize, mut entries: Vec<ClipEntry>) -> Self {
        entries.sort_by(|a, b| b.id.cmp(&a.id));
        let next_id = entries.iter().map(|e| e.id).max().unwrap_or(0) + 1;
        let mut h = Self {
            cap: cap.max(1),
            next_id,
            entries,
        };
        h.evict();
        h
    }

    /// The most recently created entry (largest id), pinned or not.
    pub fn newest(&self) -> Option<&ClipEntry> {
        self.entries.iter().max_by_key(|e| e.id)
    }

    /// The unpinned-entry capacity.
    pub fn cap(&self) -> usize {
        self.cap
    }

    /// Total number of entries currently held (pinned + unpinned).
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn unpinned_count(&self) -> usize {
        self.entries.iter().filter(|e| !e.pinned).count()
    }

    /// Record a copy. If an entry with the same `kind` **and** `content` already
    /// exists it is promoted to the front (keeping its pinned state and refreshing
    /// its timestamp) rather than duplicated. Returns the id of the front entry.
    pub fn add(&mut self, kind: ClipKind, content: impl Into<String>, created_at: u64) -> u64 {
        let content = content.into();
        let mut pinned = false;
        if let Some(pos) = self
            .entries
            .iter()
            .position(|e| e.kind == kind && e.content == content)
        {
            pinned = self.entries[pos].pinned;
            self.entries.remove(pos);
        }
        let id = self.next_id;
        self.next_id += 1;
        self.entries.insert(
            0,
            ClipEntry {
                id,
                kind,
                content,
                pinned,
                created_at,
            },
        );
        self.evict();
        id
    }

    /// Like [`add`](Self::add), but a copy identical (same kind and content) to the
    /// **newest** entry is ignored and `None` is returned. Some applications write
    /// the clipboard several times for one copy; that must neither create a second
    /// entry nor read as a second "new copy" to the UI. Copying an *older* entry again
    /// still promotes it (and returns the new id).
    pub fn add_if_new(
        &mut self,
        kind: ClipKind,
        content: impl Into<String>,
        created_at: u64,
    ) -> Option<u64> {
        let content = content.into();
        if self
            .newest()
            .is_some_and(|n| n.kind == kind && n.content == content)
        {
            return None;
        }
        Some(self.add(kind, content, created_at))
    }

    /// Drop oldest unpinned entries until the unpinned count is within `cap`.
    fn evict(&mut self) {
        while self.unpinned_count() > self.cap {
            // Oldest unpinned = the last unpinned entry in recency order.
            if let Some(pos) = self.entries.iter().rposition(|e| !e.pinned) {
                self.entries.remove(pos);
            } else {
                break; // only pinned entries remain; nothing to evict
            }
        }
    }

    pub fn get(&self, id: u64) -> Option<&ClipEntry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// Set the pinned state of an entry. Returns `false` if the id is unknown.
    /// Unpinning may push the history over capacity, which triggers eviction.
    pub fn set_pinned(&mut self, id: u64, pinned: bool) -> bool {
        if let Some(e) = self.entries.iter_mut().find(|e| e.id == id) {
            e.pinned = pinned;
            if !pinned {
                self.evict();
            }
            true
        } else {
            false
        }
    }

    pub fn pin(&mut self, id: u64) -> bool {
        self.set_pinned(id, true)
    }

    pub fn unpin(&mut self, id: u64) -> bool {
        self.set_pinned(id, false)
    }

    pub fn toggle_pin(&mut self, id: u64) -> bool {
        let cur = match self.get(id) {
            Some(e) => e.pinned,
            None => return false,
        };
        self.set_pinned(id, !cur)
    }

    /// Remove a single entry by id. Returns `false` if the id is unknown.
    pub fn remove(&mut self, id: u64) -> bool {
        if let Some(pos) = self.entries.iter().position(|e| e.id == id) {
            self.entries.remove(pos);
            true
        } else {
            false
        }
    }

    /// Remove all unpinned entries; pinned entries are kept.
    pub fn clear_unpinned(&mut self) {
        self.entries.retain(|e| e.pinned);
    }

    /// Remove everything, pinned included.
    pub fn clear_all(&mut self) {
        self.entries.clear();
    }

    /// All entries in display order: pinned first, then unpinned, each newest-first.
    pub fn visible(&self) -> Vec<&ClipEntry> {
        let mut out: Vec<&ClipEntry> = self.entries.iter().filter(|e| e.pinned).collect();
        out.extend(self.entries.iter().filter(|e| !e.pinned));
        out
    }

    /// Query by optional kind and optional case-insensitive substring, in display
    /// order. A `None` or empty needle matches all content.
    pub fn query(&self, kind: Option<ClipKind>, needle: Option<&str>) -> Vec<&ClipEntry> {
        let needle = needle.map(|n| n.to_lowercase());
        self.visible()
            .into_iter()
            .filter(|e| kind.map_or(true, |k| e.kind == k))
            .filter(|e| match &needle {
                Some(n) if !n.is_empty() => e.content.to_lowercase().contains(n),
                _ => true,
            })
            .collect()
    }

    /// Case-insensitive substring search across all kinds, in display order.
    pub fn search(&self, needle: &str) -> Vec<&ClipEntry> {
        self.query(None, Some(needle))
    }

    /// All entries of a single kind, in display order.
    pub fn filter_kind(&self, kind: ClipKind) -> Vec<&ClipEntry> {
        self.query(Some(kind), None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(h: &mut ClipboardHistory, s: &str, t: u64) -> u64 {
        h.add(ClipKind::Text, s, t)
    }

    #[test]
    fn default_cap_is_200() {
        assert_eq!(ClipboardHistory::default().cap(), DEFAULT_CAP);
    }

    #[test]
    fn cap_is_clamped_to_at_least_one() {
        assert_eq!(ClipboardHistory::new(0).cap(), 1);
    }

    #[test]
    fn add_prepends_newest_first() {
        let mut h = ClipboardHistory::new(10);
        text(&mut h, "a", 1);
        text(&mut h, "b", 2);
        let v = h.visible();
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].content, "b");
        assert_eq!(v[1].content, "a");
    }

    #[test]
    fn duplicate_content_promotes_without_growing() {
        let mut h = ClipboardHistory::new(10);
        text(&mut h, "a", 1);
        text(&mut h, "b", 2);
        text(&mut h, "a", 3); // dup of the first entry
        assert_eq!(h.len(), 2);
        let v = h.visible();
        assert_eq!(v[0].content, "a");
        assert_eq!(v[0].created_at, 3); // timestamp refreshed on re-copy
        assert_eq!(v[1].content, "b");
    }

    #[test]
    fn same_content_different_kind_is_not_a_duplicate() {
        let mut h = ClipboardHistory::new(10);
        h.add(ClipKind::Text, "http://x", 1);
        h.add(ClipKind::Link, "http://x", 2);
        assert_eq!(h.len(), 2);
    }

    #[test]
    fn cap_evicts_oldest_unpinned() {
        let mut h = ClipboardHistory::new(3);
        text(&mut h, "a", 1);
        text(&mut h, "b", 2);
        text(&mut h, "c", 3);
        text(&mut h, "d", 4);
        assert_eq!(h.len(), 3);
        let contents: Vec<_> = h.visible().iter().map(|e| e.content.clone()).collect();
        assert_eq!(contents, vec!["d", "c", "b"]); // "a" evicted
    }

    #[test]
    fn pinned_entries_survive_eviction_and_float_to_top() {
        let mut h = ClipboardHistory::new(2);
        let a = text(&mut h, "a", 1);
        text(&mut h, "b", 2);
        assert!(h.pin(a));
        text(&mut h, "c", 3);
        text(&mut h, "d", 4);
        // unpinned cap = 2 → keep c,d; b evicted; a pinned retained.
        assert_eq!(h.len(), 3);
        let contents: Vec<_> = h.visible().iter().map(|e| e.content.clone()).collect();
        assert_eq!(contents, vec!["a", "d", "c"]);
        assert!(h.get(a).unwrap().pinned);
    }

    #[test]
    fn recopy_of_pinned_entry_keeps_the_pin() {
        let mut h = ClipboardHistory::new(5);
        let a = text(&mut h, "a", 1);
        h.pin(a);
        text(&mut h, "a", 9); // re-copy the pinned content
        let e = &h.visible()[0];
        assert_eq!(e.content, "a");
        assert!(e.pinned);
        assert_eq!(h.len(), 1);
    }

    #[test]
    fn unpinning_over_cap_triggers_eviction() {
        let mut h = ClipboardHistory::new(1);
        let a = text(&mut h, "a", 1);
        h.pin(a);
        text(&mut h, "b", 2); // unpinned = 1, within cap
        assert_eq!(h.len(), 2);
        assert!(h.unpin(a)); // unpinned becomes 2 > cap 1 → evict oldest unpinned (a)
        assert_eq!(h.len(), 1);
        assert_eq!(h.visible()[0].content, "b");
    }

    #[test]
    fn toggle_remove_and_clear() {
        let mut h = ClipboardHistory::new(10);
        let a = text(&mut h, "a", 1);
        let b = text(&mut h, "b", 2);

        assert!(h.toggle_pin(a));
        assert!(h.get(a).unwrap().pinned);
        assert!(h.toggle_pin(a));
        assert!(!h.get(a).unwrap().pinned);

        assert!(h.remove(b));
        assert!(!h.remove(b)); // already gone
        assert_eq!(h.len(), 1);

        h.pin(a);
        text(&mut h, "c", 3);
        h.clear_unpinned();
        assert_eq!(h.len(), 1);
        assert_eq!(h.visible()[0].content, "a");

        h.clear_all();
        assert!(h.is_empty());
    }

    #[test]
    fn search_is_case_insensitive_substring() {
        let mut h = ClipboardHistory::new(10);
        text(&mut h, "Hello World", 1);
        text(&mut h, "goodbye", 2);
        let hits = h.search("WOR");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].content, "Hello World");
        assert_eq!(h.search("").len(), 2); // empty needle matches all
    }

    #[test]
    fn filter_by_kind_and_combined_query() {
        let mut h = ClipboardHistory::new(10);
        h.add(ClipKind::Text, "note one", 1);
        h.add(ClipKind::Link, "https://example.com", 2);
        h.add(ClipKind::Link, "https://rust-lang.org", 3);
        assert_eq!(h.filter_kind(ClipKind::Link).len(), 2);
        assert_eq!(h.filter_kind(ClipKind::Text).len(), 1);
        let combined = h.query(Some(ClipKind::Link), Some("rust"));
        assert_eq!(combined.len(), 1);
        assert_eq!(combined[0].content, "https://rust-lang.org");
    }

    #[test]
    fn add_if_new_ignores_a_repeat_of_the_newest_entry() {
        let mut h = ClipboardHistory::new(10);
        assert_eq!(h.add_if_new(ClipKind::Text, "a", 1), Some(1));
        assert_eq!(h.add_if_new(ClipKind::Text, "a", 2), None);
        assert_eq!(h.len(), 1);
        assert_eq!(h.visible()[0].created_at, 1); // untouched
    }

    #[test]
    fn add_if_new_promotes_an_older_entry_with_a_new_id() {
        let mut h = ClipboardHistory::new(10);
        h.add_if_new(ClipKind::Text, "a", 1);
        h.add_if_new(ClipKind::Text, "b", 2);
        let id = h.add_if_new(ClipKind::Text, "a", 3).expect("promoted");
        assert_eq!(h.len(), 2);
        assert_eq!(h.newest().unwrap().id, id);
        assert_eq!(h.visible()[0].content, "a");
    }

    #[test]
    fn add_if_new_treats_a_different_kind_as_new() {
        let mut h = ClipboardHistory::new(10);
        h.add_if_new(ClipKind::Text, "http://x.y", 1);
        assert!(h.add_if_new(ClipKind::Link, "http://x.y", 2).is_some());
        assert_eq!(h.len(), 2);
    }

    #[test]
    fn from_entries_restores_recency_and_continues_ids() {
        let saved = vec![
            ClipEntry {
                id: 2,
                kind: ClipKind::Text,
                content: "pinned-old".into(),
                pinned: true,
                created_at: 5,
            },
            ClipEntry {
                id: 9,
                kind: ClipKind::Text,
                content: "newest".into(),
                pinned: false,
                created_at: 90,
            },
            ClipEntry {
                id: 4,
                kind: ClipKind::Link,
                content: "https://x.y".into(),
                pinned: false,
                created_at: 40,
            },
        ];
        let mut h = ClipboardHistory::from_entries(10, saved);
        assert_eq!(h.len(), 3);
        assert_eq!(h.newest().unwrap().content, "newest");
        // pinned floats first in the display order, then newest-first.
        let order: Vec<_> = h.visible().iter().map(|e| e.content.clone()).collect();
        assert_eq!(order, vec!["pinned-old", "newest", "https://x.y"]);
        // a fresh copy must not reuse a restored id.
        let id = h.add(ClipKind::Text, "fresh", 100);
        assert_eq!(id, 10);
        assert_eq!(h.newest().unwrap().content, "fresh");
    }

    #[test]
    fn from_entries_enforces_cap_on_unpinned_only() {
        let mk = |id: u64, pinned: bool| ClipEntry {
            id,
            kind: ClipKind::Text,
            content: format!("c{id}"),
            pinned,
            created_at: id,
        };
        let h = ClipboardHistory::from_entries(
            2,
            vec![mk(1, true), mk(2, false), mk(3, false), mk(4, false)],
        );
        let order: Vec<_> = h.visible().iter().map(|e| e.content.clone()).collect();
        assert_eq!(order, vec!["c1", "c4", "c3"]);
    }

    #[test]
    fn from_entries_of_nothing_starts_at_one() {
        let mut h = ClipboardHistory::from_entries(5, Vec::new());
        assert!(h.newest().is_none());
        assert_eq!(h.add(ClipKind::Text, "a", 1), 1);
    }

    #[test]
    fn unknown_id_ops_return_false() {
        let mut h = ClipboardHistory::new(10);
        assert!(!h.pin(999));
        assert!(!h.unpin(999));
        assert!(!h.toggle_pin(999));
        assert!(!h.remove(999));
        assert!(h.get(999).is_none());
    }
}
