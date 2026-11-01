//! `krovi-net` — Krovi's single outbound network surface.
//!
//! Per SPEC §7, the **only** calls Krovi ever makes off the device are:
//!   1. a keyless weather forecast (Open-Meteo-class; coarse lat/long in, no key,
//!      no account — see [`weather`]),
//!   2. keyless FX rates (public ECB-backed endpoint; nothing sent but the base
//!      currency — see [`currency`]), and
//!   3. keyless city-name geocoding, used only for the "set your city" fallback
//!      (Open-Meteo geocoding; sends only the text the user typed — see
//!      [`geocode`]).
//!
//! There is **no** telemetry, analytics, or account traffic anywhere. The
//! endpoints are listed for the Phase-4 security audit in the crate docs below
//! and in `docs/SECURITY-network-audit.md`.
//!
//! ## Pure by default, network behind a feature (DECISIONS.md D-009)
//! This host cannot compile most TLS/native stacks (32-bit MinGW gcc), so every
//! piece of *logic* — request-URL building, parsing responses into typed structs,
//! currency conversion math, and TTL caching (so currency can reuse cached rates
//! offline) — lives in modules with **no** socket code and is unit-tested here.
//! The real HTTP GET is isolated in [`http`] behind the off-by-default `net`
//! feature; the `fetch_*` wrappers exist in every build but return
//! [`NetError::Disabled`] when `net` is not enabled.
//!
//! ## Outbound endpoints (for the security audit)
//! * Weather:  `https://api.open-meteo.com/v1/forecast`  (keyless, GET, coarse lat/long)
//! * FX rates: `https://api.frankfurter.dev/v1/latest`   (keyless, GET, base currency only)
//! * Geocode:  `https://geocoding-api.open-meteo.com/v1/search` (keyless, GET, typed city name only)

mod cache;
pub mod currency;
pub mod geocode;
pub mod reply;
pub mod resolve;
pub mod weather;

#[cfg(feature = "net")]
mod http;

pub use cache::TtlCache;
pub use currency::{convert, fetch_rates, parse_rates, rates_url, Rates, DEFAULT_BASE};
pub use geocode::{clean_query, geocode_url, parse_places, search_places, Place};
pub use reply::{fx_reply, needs_city, weather_reply, FxReply, PlaceDto, WeatherReply};
pub use resolve::{resolve, Cached, Resolved};
pub use weather::{
    fetch_forecast, forecast_url, parse_forecast, weekday_abbrev, wmo_condition, wmo_icon,
    DayPoint, HourPoint, WeatherData,
};

/// Errors from the network layer.
///
/// Hand-rolled (not `thiserror`) to keep the dependency surface small, matching
/// the convention in `krovi-core` / `krovi-store`.
#[derive(Debug)]
#[non_exhaustive]
pub enum NetError {
    /// Caller passed something invalid (e.g. an out-of-range coordinate or an
    /// unknown currency code). No request is made.
    InvalidInput(String),
    /// The response body could not be parsed into the expected shape.
    Parse(String),
    /// The HTTP request itself failed (only reachable with the `net` feature).
    Request(String),
    /// Networking is not compiled into this build (the `net` feature is off).
    Disabled,
}

impl std::fmt::Display for NetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NetError::InvalidInput(m) => write!(f, "invalid input: {m}"),
            NetError::Parse(m) => write!(f, "parse error: {m}"),
            NetError::Request(m) => write!(f, "request error: {m}"),
            NetError::Disabled => write!(
                f,
                "networking is disabled in this build (enable the `net` feature)"
            ),
        }
    }
}

impl std::error::Error for NetError {}

impl From<serde_json::Error> for NetError {
    fn from(e: serde_json::Error) -> Self {
        NetError::Parse(e.to_string())
    }
}

/// Convenience alias for fallible network-layer operations.
pub type Result<T> = std::result::Result<T, NetError>;
