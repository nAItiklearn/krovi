//! Keyless currency conversion (SPEC C10).
//!
//! Rates come from the public, **keyless** Frankfurter endpoint (ECB reference
//! rates). The only thing ever sent is the base currency code; no key, account,
//! or user data. Rates are fetched once and then cached (see
//! [`crate::TtlCache`]) so conversions work offline against the last known
//! rates. The conversion math and response parsing here are pure and tested; the
//! network fetch is isolated in [`fetch_rates`].

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::NetError;

/// Default base currency used when the caller does not specify one.
pub const DEFAULT_BASE: &str = "USD";

// The current Frankfurter host. The legacy `api.frankfurter.app` 301-redirects here,
// so we call it directly: one host, no redirect hop.
const API: &str = "https://api.frankfurter.dev/v1";

/// Build the rates request URL for `base` (uppercased).
pub fn rates_url(base: &str) -> String {
    format!("{API}/latest?base={}", base.to_uppercase())
}

/// A table of exchange rates relative to [`Rates::base`]. The base currency
/// itself is implicitly `1.0` and is not present in [`Rates::rates`] (matching
/// the Frankfurter response shape).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rates {
    pub base: String,
    pub date: String,
    pub rates: BTreeMap<String, f64>,
}

/// Parse a rates response body into a [`Rates`].
pub fn parse_rates(json: &str) -> crate::Result<Rates> {
    let rates: Rates = serde_json::from_str(json)?;
    Ok(rates)
}

/// Convert `amount` from `from` to `to` using `rates`.
///
/// Handles cross rates through the base currency: `amount * rate(to) / rate(from)`
/// where the base currency's rate is `1.0`. Currency codes are case-insensitive.
/// Returns [`NetError::InvalidInput`] if either code is unknown.
pub fn convert(amount: f64, from: &str, to: &str, rates: &Rates) -> crate::Result<f64> {
    let from = from.to_uppercase();
    let to = to.to_uppercase();
    if from == to {
        return Ok(amount);
    }
    let base = rates.base.to_uppercase();
    let rate_of = |code: &str| -> Option<f64> {
        if code == base {
            Some(1.0)
        } else {
            rates.rates.get(code).copied()
        }
    };
    let rf = rate_of(&from)
        .ok_or_else(|| NetError::InvalidInput(format!("unknown currency: {from}")))?;
    let rt =
        rate_of(&to).ok_or_else(|| NetError::InvalidInput(format!("unknown currency: {to}")))?;
    Ok(amount * rt / rf)
}

/// Fetch the latest rates for `base`.
///
/// **CI-only network path.** Only performs a real request when the crate is built
/// with the `net` feature; otherwise returns [`NetError::Disabled`] so the rest
/// of the app still links and runs. The parsing it delegates to
/// ([`parse_rates`]) is covered by unit tests regardless of the feature.
#[cfg(feature = "net")]
pub fn fetch_rates(base: &str) -> crate::Result<Rates> {
    let body = crate::http::get(&rates_url(base))?;
    parse_rates(&body)
}

/// See the `net`-enabled variant. Without `net`, no socket code is compiled.
#[cfg(not(feature = "net"))]
pub fn fetch_rates(base: &str) -> crate::Result<Rates> {
    let _ = base;
    Err(NetError::Disabled)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
        "amount": 1.0,
        "base": "USD",
        "date": "2026-11-01",
        "rates": { "EUR": 0.92, "GBP": 0.79, "INR": 83.2, "JPY": 149.5 }
    }"#;

    fn sample() -> Rates {
        parse_rates(SAMPLE).expect("sample parses")
    }

    #[test]
    fn url_uppercases_base() {
        assert_eq!(
            rates_url("usd"),
            "https://api.frankfurter.dev/v1/latest?base=USD"
        );
    }

    #[test]
    fn parses_base_date_and_rates() {
        let r = sample();
        assert_eq!(r.base, "USD");
        assert_eq!(r.date, "2026-11-01");
        assert_eq!(r.rates.get("EUR"), Some(&0.92));
        // the base currency is not listed in the table
        assert_eq!(r.rates.get("USD"), None);
    }

    #[test]
    fn same_currency_is_identity() {
        let r = sample();
        assert_eq!(convert(10.0, "USD", "USD", &r).unwrap(), 10.0);
        assert_eq!(convert(10.0, "eur", "EUR", &r).unwrap(), 10.0);
    }

    #[test]
    fn converts_from_base() {
        let r = sample();
        let out = convert(100.0, "USD", "EUR", &r).unwrap();
        assert!((out - 92.0).abs() < 1e-9);
    }

    #[test]
    fn converts_to_base() {
        let r = sample();
        // 92 EUR back to USD should be ~100
        let out = convert(92.0, "EUR", "USD", &r).unwrap();
        assert!((out - 100.0).abs() < 1e-9);
    }

    #[test]
    fn converts_cross_rate_via_base() {
        let r = sample();
        // EUR -> GBP = amount * rate(GBP) / rate(EUR)
        let out = convert(10.0, "EUR", "GBP", &r).unwrap();
        let expected = 10.0 * 0.79 / 0.92;
        assert!((out - expected).abs() < 1e-9);
    }

    #[test]
    fn case_insensitive_codes() {
        let r = sample();
        let a = convert(5.0, "usd", "inr", &r).unwrap();
        let b = convert(5.0, "USD", "INR", &r).unwrap();
        assert!((a - b).abs() < 1e-12);
    }

    #[test]
    fn unknown_currency_errors_without_fetching() {
        let r = sample();
        assert!(matches!(
            convert(1.0, "USD", "XXX", &r),
            Err(NetError::InvalidInput(_))
        ));
        assert!(matches!(
            convert(1.0, "XXX", "USD", &r),
            Err(NetError::InvalidInput(_))
        ));
    }

    #[cfg(not(feature = "net"))]
    #[test]
    fn fetch_is_disabled_without_net_feature() {
        assert!(matches!(fetch_rates("USD"), Err(NetError::Disabled)));
    }
}
