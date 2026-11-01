//! Cache-or-fetch resolution for the two data tools (weather, FX).
//!
//! The rule, in one place and pure (the caller supplies `now`, the persisted
//! [`Cached`] entry and a `fetch` closure):
//!
//! * a cached entry for the **same key** that is still within its TTL is returned
//!   without touching the network (unless `force`);
//! * otherwise `fetch` runs; success replaces the cache;
//! * if `fetch` fails but an entry for the same key exists, that out-of-date entry is
//!   returned **flagged stale** together with the error — so the UI keeps working
//!   offline and can say how old the numbers are;
//! * if there is nothing cached either, the error is returned with no data.
//!
//! Nothing here fabricates data: with no cache and no network the result is empty.

use serde::{Deserialize, Serialize};

/// A persisted, stamped value. `key` ties it to its inputs (place / base currency).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cached<T> {
    pub key: String,
    /// Epoch milliseconds when it was fetched.
    pub fetched_at: u64,
    pub data: T,
}

/// Outcome of [`resolve`].
#[derive(Debug, Clone, PartialEq)]
pub struct Resolved<T> {
    pub data: Option<T>,
    pub fetched_at: Option<u64>,
    /// `true` when `data` is an out-of-date cache entry served because the refresh failed.
    pub stale: bool,
    pub error: Option<String>,
    /// A new cache entry the caller should persist (set only after a successful fetch).
    pub updated: Option<Cached<T>>,
}

/// See the module docs.
pub fn resolve<T: Clone>(
    cached: Option<&Cached<T>>,
    key: &str,
    now_ms: u64,
    ttl_ms: u64,
    force: bool,
    fetch: impl FnOnce() -> Result<T, String>,
) -> Resolved<T> {
    let usable = cached.filter(|c| c.key == key);
    if let Some(c) = usable {
        let age = now_ms.saturating_sub(c.fetched_at);
        if !force && age <= ttl_ms {
            return Resolved {
                data: Some(c.data.clone()),
                fetched_at: Some(c.fetched_at),
                stale: false,
                error: None,
                updated: None,
            };
        }
    }
    match fetch() {
        Ok(data) => Resolved {
            fetched_at: Some(now_ms),
            updated: Some(Cached {
                key: key.to_string(),
                fetched_at: now_ms,
                data: data.clone(),
            }),
            data: Some(data),
            stale: false,
            error: None,
        },
        Err(e) => match usable {
            Some(c) => Resolved {
                data: Some(c.data.clone()),
                fetched_at: Some(c.fetched_at),
                stale: true,
                error: Some(e),
                updated: None,
            },
            None => Resolved {
                data: None,
                fetched_at: None,
                stale: false,
                error: Some(e),
                updated: None,
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn cache(key: &str, at: u64, v: i32) -> Cached<i32> {
        Cached {
            key: key.into(),
            fetched_at: at,
            data: v,
        }
    }

    #[test]
    fn fresh_cache_skips_the_network() {
        let calls = Cell::new(0);
        let c = cache("k", 1_000, 7);
        let r = resolve(Some(&c), "k", 1_500, 1_000, false, || {
            calls.set(calls.get() + 1);
            Ok(99)
        });
        assert_eq!(calls.get(), 0);
        assert_eq!(r.data, Some(7));
        assert_eq!(r.fetched_at, Some(1_000));
        assert!(!r.stale && r.error.is_none() && r.updated.is_none());
    }

    #[test]
    fn expired_cache_refreshes_and_restamps() {
        let c = cache("k", 1_000, 7);
        let r = resolve(Some(&c), "k", 5_000, 1_000, false, || Ok(99));
        assert_eq!(r.data, Some(99));
        assert_eq!(r.fetched_at, Some(5_000));
        assert_eq!(r.updated, Some(cache("k", 5_000, 99)));
        assert!(!r.stale);
    }

    #[test]
    fn force_refetches_even_when_fresh() {
        let c = cache("k", 1_000, 7);
        let r = resolve(Some(&c), "k", 1_001, 1_000_000, true, || Ok(8));
        assert_eq!(r.data, Some(8));
    }

    #[test]
    fn failed_refresh_serves_the_stale_entry_flagged() {
        let c = cache("k", 1_000, 7);
        let r = resolve(Some(&c), "k", 9_000, 1_000, false, || Err("offline".into()));
        assert_eq!(r.data, Some(7));
        assert_eq!(r.fetched_at, Some(1_000));
        assert!(r.stale);
        assert_eq!(r.error.as_deref(), Some("offline"));
        assert!(r.updated.is_none());
    }

    #[test]
    fn failed_fetch_with_no_cache_returns_no_data() {
        let r: Resolved<i32> = resolve(None, "k", 1, 1, false, || Err("no network".into()));
        assert_eq!(r.data, None);
        assert_eq!(r.error.as_deref(), Some("no network"));
        assert!(!r.stale);
    }

    #[test]
    fn a_cache_for_a_different_key_is_never_served() {
        let c = cache("pune", 1_000, 7);
        // different place: must not show Pune's weather for London, even offline
        let r = resolve(Some(&c), "london", 1_001, 1_000_000, false, || {
            Err("offline".into())
        });
        assert_eq!(r.data, None);
        assert!(!r.stale);
        // and online it fetches instead of reusing it
        let r = resolve(Some(&c), "london", 1_001, 1_000_000, false, || Ok(1));
        assert_eq!(r.data, Some(1));
    }

    #[test]
    fn a_clock_that_went_backwards_counts_as_fresh_not_a_panic() {
        let c = cache("k", 10_000, 7);
        let r = resolve(Some(&c), "k", 1_000, 500, false, || Ok(0));
        assert_eq!(r.data, Some(7));
    }

    #[test]
    fn cached_round_trips_through_json() {
        let c = cache("k", 5, 3);
        let s = serde_json::to_string(&c).unwrap();
        assert_eq!(serde_json::from_str::<Cached<i32>>(&s).unwrap(), c);
    }
}
