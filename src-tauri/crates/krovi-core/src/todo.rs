//! To-do list (SPEC O5).
//!
//! Lightweight checkable tasks in creation order. Completing a task records when
//! it was completed; un-completing clears that. Deterministic: the caller passes
//! `now` (epoch ms) on mutations that need a time.

/// A single task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    pub id: u64,
    pub title: String,
    pub done: bool,
    pub created_at: u64,
    /// Set when the task is marked done, cleared when reopened.
    pub completed_at: Option<u64>,
}

/// An ordered list of tasks (creation order preserved).
#[derive(Debug, Clone, Default)]
pub struct TodoList {
    next_id: u64,
    tasks: Vec<Task>,
}

impl TodoList {
    pub fn new() -> Self {
        Self {
            next_id: 1,
            tasks: Vec::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.tasks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }

    /// Append a task (open) and return its id.
    pub fn add(&mut self, title: impl Into<String>, now: u64) -> u64 {
        let id = self.next_id.max(1);
        self.next_id = id + 1;
        self.tasks.push(Task {
            id,
            title: title.into(),
            done: false,
            created_at: now,
            completed_at: None,
        });
        id
    }

    /// Set a task's done state explicitly. Returns `false` if the id is unknown.
    pub fn set_done(&mut self, id: u64, done: bool, now: u64) -> bool {
        if let Some(t) = self.tasks.iter_mut().find(|t| t.id == id) {
            t.done = done;
            t.completed_at = if done { Some(now) } else { None };
            true
        } else {
            false
        }
    }

    /// Flip a task's done state. Returns `false` if the id is unknown.
    pub fn toggle(&mut self, id: u64, now: u64) -> bool {
        let cur = match self.get(id) {
            Some(t) => t.done,
            None => return false,
        };
        self.set_done(id, !cur, now)
    }

    pub fn get(&self, id: u64) -> Option<&Task> {
        self.tasks.iter().find(|t| t.id == id)
    }

    pub fn remove(&mut self, id: u64) -> bool {
        if let Some(pos) = self.tasks.iter().position(|t| t.id == id) {
            self.tasks.remove(pos);
            true
        } else {
            false
        }
    }

    /// Remove all completed tasks; returns how many were removed.
    pub fn clear_completed(&mut self) -> usize {
        let before = self.tasks.len();
        self.tasks.retain(|t| !t.done);
        before - self.tasks.len()
    }

    pub fn open_count(&self) -> usize {
        self.tasks.iter().filter(|t| !t.done).count()
    }

    pub fn done_count(&self) -> usize {
        self.tasks.iter().filter(|t| t.done).count()
    }

    /// All tasks in creation order.
    pub fn tasks(&self) -> &[Task] {
        &self.tasks
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_keeps_creation_order_and_is_open() {
        let mut l = TodoList::new();
        let a = l.add("first", 1);
        let b = l.add("second", 2);
        let ids: Vec<_> = l.tasks().iter().map(|t| t.id).collect();
        assert_eq!(ids, vec![a, b]);
        assert_eq!(l.open_count(), 2);
        assert_eq!(l.done_count(), 0);
    }

    #[test]
    fn toggle_sets_and_clears_completed_at() {
        let mut l = TodoList::new();
        let a = l.add("task", 1);
        assert!(l.toggle(a, 50));
        let t = l.get(a).unwrap();
        assert!(t.done);
        assert_eq!(t.completed_at, Some(50));

        assert!(l.toggle(a, 99)); // reopen
        let t = l.get(a).unwrap();
        assert!(!t.done);
        assert_eq!(t.completed_at, None);
    }

    #[test]
    fn counts_and_clear_completed() {
        let mut l = TodoList::new();
        let a = l.add("a", 1);
        let _b = l.add("b", 2);
        let c = l.add("c", 3);
        l.set_done(a, true, 10);
        l.set_done(c, true, 11);
        assert_eq!(l.done_count(), 2);
        assert_eq!(l.open_count(), 1);
        assert_eq!(l.clear_completed(), 2);
        assert_eq!(l.len(), 1);
        assert_eq!(l.tasks()[0].title, "b");
    }

    #[test]
    fn remove_and_unknown_ids() {
        let mut l = TodoList::new();
        let a = l.add("a", 1);
        assert!(l.remove(a));
        assert!(!l.remove(a));
        assert!(!l.toggle(999, 1));
        assert!(!l.set_done(999, true, 1));
        assert!(l.get(999).is_none());
    }
}
