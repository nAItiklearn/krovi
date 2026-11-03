//! Screen-time tracking (SPEC C11) — local-only aggregation.
//!
//! Accumulates active-application time per day. The integration layer samples the
//! foreground window's **process name** plus a duration and calls [`ScreenTime::record`];
//! this module only aggregates. It stores **no window titles, URLs, or any other
//! content**, and nothing it holds ever leaves the machine. `day` is a
//! caller-supplied day index (the app uses the user's **local** calendar day, as days
//! since 1970-01-01) so the logic is deterministic and testable.
//!
//! ## Persistence bookkeeping (no I/O here)
//! The aggregate also remembers *which* `(day, app)` totals changed since they were
//! last written ([`ScreenTime::dirty_rows`]) and *when* they were last written, so the
//! caller can upsert only the changed rows and at most once per [`FLUSH_EVERY_MS`]
//! instead of rewriting the whole table on every 5 s sample. It also knows the
//! retention window ([`RETENTION_DAYS`]): [`ScreenTime::prune_before`] drops older days.
//! Every clock value comes from the caller (DECISIONS D-007); nothing here reads one.

use std::collections::{BTreeMap, BTreeSet};

/// How many calendar days of history are kept (today plus the 89 before it).
pub const RETENTION_DAYS: u32 = 90;

/// Minimum spacing between two batched writes of changed rows.
pub const FLUSH_EVERY_MS: u64 = 60_000;

/// The oldest day kept when `today` is the current day: rows with `day` below this
/// are expired. Saturates at 0 for very small day numbers.
pub fn retention_cutoff(today: u32) -> u32 {
    today.saturating_sub(RETENTION_DAYS - 1)
}

/// Per-day, per-application accumulated active time (milliseconds).
#[derive(Debug, Clone, Default)]
pub struct ScreenTime {
    per_day: BTreeMap<u32, BTreeMap<String, u64>>,
    /// `(day, app)` totals changed since the last successful flush.
    dirty: BTreeSet<(u32, String)>,
    /// When the last flush (or deferred retry) happened; `None` before the first.
    last_flush_ms: Option<u64>,
    /// The day for which retention was last applied.
    pruned_for: Option<u32>,
}

impl ScreenTime {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add `ms` of active time for `app` on `day`. Zero is a no-op.
    pub fn record(&mut self, day: u32, app: impl Into<String>, ms: u64) {
        if ms == 0 {
            return;
        }
        let app = app.into();
        *self
            .per_day
            .entry(day)
            .or_default()
            .entry(app.clone())
            .or_insert(0) += ms;
        self.dirty.insert((day, app));
    }

    /// Total time for one app on one day (0 if unseen).
    pub fn app_total(&self, day: u32, app: &str) -> u64 {
        self.per_day
            .get(&day)
            .and_then(|m| m.get(app))
            .copied()
            .unwrap_or(0)
    }

    /// Total time across all apps on one day.
    pub fn day_total(&self, day: u32) -> u64 {
        self.per_day
            .get(&day)
            .map(|m| m.values().sum())
            .unwrap_or(0)
    }

    /// Apps for a day sorted by time descending, ties broken by name ascending.
    /// `limit == 0` returns all apps.
    pub fn top_apps(&self, day: u32, limit: usize) -> Vec<(String, u64)> {
        let mut v: Vec<(String, u64)> = match self.per_day.get(&day) {
            Some(m) => m.iter().map(|(k, &ms)| (k.clone(), ms)).collect(),
            None => return Vec::new(),
        };
        v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        if limit > 0 {
            v.truncate(limit);
        }
        v
    }

    /// Days that have any recorded time, ascending.
    pub fn days(&self) -> Vec<u32> {
        self.per_day.keys().copied().collect()
    }

    // ----------------------------------------------------------------- dirty rows

    /// Whether any total changed since it was last written.
    pub fn has_dirty(&self) -> bool {
        !self.dirty.is_empty()
    }

    /// The totals that changed since the last flush as `(day, app, total_ms)`,
    /// ordered by day then app. Only these need to be upserted.
    pub fn dirty_rows(&self) -> Vec<(u32, String, u64)> {
        self.dirty
            .iter()
            .map(|(day, app)| (*day, app.clone(), self.app_total(*day, app)))
            .collect()
    }

    /// Forget which rows are changed without recording a flush time. Used right after
    /// rebuilding the aggregate from storage, where nothing is yet out of date.
    pub fn clear_dirty(&mut self) {
        self.dirty.clear();
    }

    /// Whether a batched write is due: something changed **and** it has been at least
    /// [`FLUSH_EVERY_MS`] since the last write (or there has been none yet). A clock
    /// that went backwards counts as due, so a bad clock can never stall persistence.
    pub fn flush_due(&self, now_ms: u64) -> bool {
        if self.dirty.is_empty() {
            return false;
        }
        match self.last_flush_ms {
            None => true,
            Some(last) => now_ms < last || now_ms - last >= FLUSH_EVERY_MS,
        }
    }

    /// The changed rows were written successfully at `now_ms`: they are clean again.
    pub fn mark_flushed(&mut self, now_ms: u64) {
        self.dirty.clear();
        self.last_flush_ms = Some(now_ms);
    }

    /// A write failed at `now_ms`: keep the rows dirty but wait a full interval before
    /// the next attempt, so a failing disk is not hammered every tick.
    pub fn defer_flush(&mut self, now_ms: u64) {
        self.last_flush_ms = Some(now_ms);
    }

    // ----------------------------------------------------------------- retention

    /// Drop every day before `min_day`. Returns how many `(day, app)` rows went.
    pub fn prune_before(&mut self, min_day: u32) -> usize {
        let keep = self.per_day.split_off(&min_day);
        let removed: usize = self.per_day.values().map(BTreeMap::len).sum();
        self.per_day = keep;
        self.dirty.retain(|(day, _)| *day >= min_day);
        removed
    }

    /// Whether retention has not yet been applied for `today` (so: once after
    /// loading, then once per day).
    pub fn needs_prune(&self, today: u32) -> bool {
        self.pruned_for != Some(today)
    }

    /// Retention was applied (in memory and in storage) for `today`.
    pub fn mark_pruned(&mut self, today: u32) {
        self.pruned_for = Some(today);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_accumulates_per_app_per_day() {
        let mut st = ScreenTime::new();
        st.record(100, "code.exe", 500);
        st.record(100, "code.exe", 250);
        st.record(100, "browser.exe", 1000);
        assert_eq!(st.app_total(100, "code.exe"), 750);
        assert_eq!(st.app_total(100, "browser.exe"), 1000);
        assert_eq!(st.day_total(100), 1750);
    }

    #[test]
    fn unseen_returns_zero() {
        let st = ScreenTime::new();
        assert_eq!(st.app_total(1, "nope.exe"), 0);
        assert_eq!(st.day_total(1), 0);
        assert!(st.top_apps(1, 5).is_empty());
    }

    #[test]
    fn zero_ms_is_a_noop() {
        let mut st = ScreenTime::new();
        st.record(1, "x.exe", 0);
        assert!(st.days().is_empty());
        assert!(!st.has_dirty(), "a no-op must not mark anything changed");
    }

    #[test]
    fn top_apps_orders_desc_with_name_tiebreak_and_limit() {
        let mut st = ScreenTime::new();
        st.record(1, "b.exe", 300);
        st.record(1, "a.exe", 300); // tie with b → name ascending wins
        st.record(1, "c.exe", 900);
        let top = st.top_apps(1, 2);
        assert_eq!(
            top,
            vec![("c.exe".to_string(), 900), ("a.exe".to_string(), 300)]
        );
        // limit 0 = all three
        assert_eq!(st.top_apps(1, 0).len(), 3);
    }

    #[test]
    fn days_are_sorted_ascending() {
        let mut st = ScreenTime::new();
        st.record(30, "x", 1);
        st.record(10, "x", 1);
        st.record(20, "x", 1);
        assert_eq!(st.days(), vec![10, 20, 30]);
    }

    // ---- dirty rows: only what changed is written ------------------------------

    #[test]
    fn dirty_rows_carry_current_totals_in_day_app_order() {
        let mut st = ScreenTime::new();
        st.record(11, "b.exe", 5);
        st.record(10, "b.exe", 7);
        st.record(10, "a.exe", 1);
        st.record(10, "a.exe", 2); // same row twice → one row, summed total
        assert_eq!(
            st.dirty_rows(),
            vec![
                (10, "a.exe".to_string(), 3),
                (10, "b.exe".to_string(), 7),
                (11, "b.exe".to_string(), 5),
            ]
        );
    }

    #[test]
    fn only_rows_touched_since_the_last_flush_are_dirty() {
        let mut st = ScreenTime::new();
        st.record(10, "a.exe", 100);
        st.record(10, "b.exe", 200);
        st.record(9, "old.exe", 50);
        assert_eq!(st.dirty_rows().len(), 3);

        st.mark_flushed(1_000);
        assert!(!st.has_dirty());
        assert!(st.dirty_rows().is_empty());

        // one app gets more time: exactly that one row is dirty, with its full total
        st.record(10, "b.exe", 25);
        assert_eq!(st.dirty_rows(), vec![(10, "b.exe".to_string(), 225)]);

        // a brand-new row shows up alone as well
        st.record(11, "new.exe", 9);
        assert_eq!(
            st.dirty_rows(),
            vec![
                (10, "b.exe".to_string(), 225),
                (11, "new.exe".to_string(), 9)
            ]
        );
    }

    #[test]
    fn clear_dirty_does_not_touch_totals_or_the_flush_clock() {
        let mut st = ScreenTime::new();
        st.record(1, "a", 10);
        st.clear_dirty();
        assert!(!st.has_dirty());
        assert_eq!(st.app_total(1, "a"), 10);
        // never flushed yet, so the next change is due immediately
        st.record(1, "a", 1);
        assert!(st.flush_due(0));
    }

    // ---- batching: at most one write per minute --------------------------------

    #[test]
    fn nothing_changed_means_no_flush_is_ever_due() {
        let mut st = ScreenTime::new();
        assert!(!st.flush_due(0));
        assert!(!st.flush_due(u64::MAX));
        st.record(1, "a", 1);
        st.mark_flushed(5);
        assert!(!st.flush_due(5 + 10 * FLUSH_EVERY_MS));
    }

    #[test]
    fn first_change_is_due_at_once_then_at_most_once_per_interval() {
        let mut st = ScreenTime::new();
        st.record(1, "a", 1);
        assert!(st.flush_due(1_000), "nothing written yet: write now");
        st.mark_flushed(1_000);

        // 5 s ticks keep changing the row, but no write until a minute has passed
        for t in (1..12).map(|i| 1_000 + i * 5_000) {
            st.record(1, "a", 5_000);
            assert!(!st.flush_due(t), "t={t} is inside the interval");
        }
        st.record(1, "a", 5_000);
        assert!(!st.flush_due(1_000 + FLUSH_EVERY_MS - 1));
        assert!(st.flush_due(1_000 + FLUSH_EVERY_MS));
    }

    #[test]
    fn a_backwards_clock_never_stalls_persistence() {
        let mut st = ScreenTime::new();
        st.record(1, "a", 1);
        st.mark_flushed(10_000_000);
        st.record(1, "a", 1);
        assert!(st.flush_due(5), "clock went back: write rather than wait");
    }

    #[test]
    fn a_failed_write_keeps_rows_dirty_but_backs_off_for_an_interval() {
        let mut st = ScreenTime::new();
        st.record(1, "a", 1);
        assert!(st.flush_due(0));
        st.defer_flush(0); // the write failed
        assert!(
            st.has_dirty(),
            "nothing was written, so the row is still dirty"
        );
        assert!(!st.flush_due(FLUSH_EVERY_MS - 1));
        assert!(st.flush_due(FLUSH_EVERY_MS));
        st.mark_flushed(FLUSH_EVERY_MS);
        assert!(!st.has_dirty());
    }

    // ---- retention --------------------------------------------------------------

    #[test]
    fn retention_keeps_ninety_days_including_today() {
        assert_eq!(RETENTION_DAYS, 90);
        // today = 200 → days 111..=200 are kept (that is exactly 90 days)
        assert_eq!(retention_cutoff(200), 111);
        assert_eq!(200 - retention_cutoff(200) + 1, 90);
        // not enough history yet: nothing can be older than day 0
        assert_eq!(retention_cutoff(0), 0);
        assert_eq!(retention_cutoff(50), 0);
        assert_eq!(retention_cutoff(89), 0);
        assert_eq!(retention_cutoff(90), 1);
    }

    #[test]
    fn prune_drops_days_before_the_cutoff_and_keeps_the_rest() {
        let mut st = ScreenTime::new();
        st.record(100, "a.exe", 1);
        st.record(100, "b.exe", 1);
        st.record(110, "a.exe", 2);
        st.record(111, "a.exe", 3); // the cutoff day itself is kept
        st.record(200, "a.exe", 4);

        let removed = st.prune_before(retention_cutoff(200));
        assert_eq!(removed, 3, "two apps on day 100 plus day 110");
        assert_eq!(st.days(), vec![111, 200]);
        assert_eq!(st.app_total(111, "a.exe"), 3);
        assert_eq!(st.app_total(100, "a.exe"), 0);

        // idempotent
        assert_eq!(st.prune_before(retention_cutoff(200)), 0);
    }

    #[test]
    fn pruned_rows_are_no_longer_dirty() {
        let mut st = ScreenTime::new();
        st.record(5, "old.exe", 1);
        st.record(500, "new.exe", 1);
        st.prune_before(retention_cutoff(500));
        assert_eq!(st.dirty_rows(), vec![(500, "new.exe".to_string(), 1)]);
    }

    #[test]
    fn pruning_an_empty_aggregate_is_harmless() {
        let mut st = ScreenTime::new();
        assert_eq!(st.prune_before(1_000), 0);
        assert!(st.days().is_empty());
    }

    #[test]
    fn retention_runs_once_after_load_then_once_per_day() {
        let mut st = ScreenTime::new();
        assert!(st.needs_prune(300), "fresh/loaded: prune on the first tick");
        st.mark_pruned(300);
        assert!(!st.needs_prune(300), "same day: nothing to do");
        assert!(st.needs_prune(301), "a new local day: prune again");
    }
}
