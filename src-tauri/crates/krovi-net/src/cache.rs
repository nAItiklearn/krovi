//! A tiny time-to-live cache.
//!
//! Pure and deterministic: the caller supplies the current time (`now_ms`, epoch
//! milliseconds), exactly like the `now`-injection convention in `krovi-core`
//! (DECISIONS.md D-007). It is used so the currency tool can reuse the last
//! fetched rates when they are still fresh, and fall back to the last known
//! rates when offline (SPEC C10: "currency via cached keyless rates").

/// Holds a single most-recent value plus the time it was stored.
#[derive(Debug, Clone)]
pub struct TtlCache<T> {
    ttl_ms: u64,
    entry: Option<(T, u64)>,
}

impl<T: Clone> TtlCache<T> {
    /// Create a cache whose entries are considered fresh for `ttl_ms`.
    pub fn new(ttl_ms: u64) -> Self {
        Self {
            ttl_ms,
            entry: None,
        }
    }

    /// Store `value`, stamped with `now_ms`, replacing any previous entry.
    pub fn put(&mut self, value: T, now_ms: u64) {
        self.entry = Some((value, now_ms));
    }

    /// Milliseconds since the stored value was cached, or `None` if empty.
    /// Saturates at 0 if `now_ms` is earlier than the stored stamp.
    pub fn age_ms(&self, now_ms: u64) -> Option<u64> {
        self.entry.as_ref().map(|(_, at)| now_ms.saturating_sub(*at))
    }

    /// Whether a value is present and still within its TTL.
    pub fn is_fresh(&self, now_ms: u64) -> bool {
        self.age_ms(now_ms).map(|age| age <= self.ttl_ms).unwrap_or(false)
    }

    /// The value if it is present and still fresh, else `None`.
    pub fn get_fresh(&self, now_ms: u64) -> Option<T> {
        if self.is_fresh(now_ms) {
            self.entry.as_ref().map(|(v, _)| v.clone())
        } else {
            None
        }
    }

    /// The last stored value regardless of age — the offline fallback.
    pub fn get_stale(&self) -> Option<T> {
        self.entry.as_ref().map(|(v, _)| v.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_cache_has_nothing() {
        let c: TtlCache<i32> = TtlCache::new(1000);
        assert_eq!(c.get_fresh(0), None);
        assert_eq!(c.get_stale(), None);
        assert_eq!(c.age_ms(0), None);
        assert!(!c.is_fresh(0));
    }

    #[test]
    fn fresh_within_ttl_stale_after() {
        let mut c = TtlCache::new(1000);
        c.put(42, 10_000);
        // exactly at the boundary is still fresh
        assert!(c.is_fresh(11_000));
        assert_eq!(c.get_fresh(11_000), Some(42));
        // one past the TTL: not fresh, but still available as a stale fallback
        assert!(!c.is_fresh(11_001));
        assert_eq!(c.get_fresh(11_001), None);
        assert_eq!(c.get_stale(), Some(42));
    }

    #[test]
    fn put_replaces_and_restamps() {
        let mut c = TtlCache::new(500);
        c.put("old", 0);
        c.put("new", 1000);
        assert_eq!(c.get_fresh(1200), Some("new"));
        assert_eq!(c.age_ms(1500), Some(500));
    }

    #[test]
    fn age_saturates_if_clock_goes_backwards() {
        let mut c = TtlCache::new(500);
        c.put(1, 1000);
        assert_eq!(c.age_ms(500), Some(0));
    }
}
