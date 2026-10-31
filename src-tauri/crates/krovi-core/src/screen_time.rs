//! Screen-time tracking (SPEC C11) — local-only aggregation.
//!
//! Accumulates active-application time per day. The integration layer samples the
//! foreground window's **process name** plus a duration and calls [`ScreenTime::record`];
//! this module only aggregates. It stores **no window titles, URLs, or any other
//! content**, and nothing it holds ever leaves the machine. `day` is a
//! caller-supplied day index (e.g. days since the Unix epoch) so the logic is
//! deterministic and testable.

use std::collections::BTreeMap;

/// Per-day, per-application accumulated active time (milliseconds).
#[derive(Debug, Clone, Default)]
pub struct ScreenTime {
    per_day: BTreeMap<u32, BTreeMap<String, u64>>,
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
        *self
            .per_day
            .entry(day)
            .or_default()
            .entry(app.into())
            .or_insert(0) += ms;
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
}
