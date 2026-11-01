//! The reply objects the weather and currency commands return to the UI, built from a
//! [`Resolved`] cache-or-fetch result. Pure, so the status rules ("ok" / "offline" /
//! "needs-city", staleness, the city label, the implicit base-currency rate) are
//! unit-tested without a network or the Tauri runtime.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::currency::Rates;
use crate::resolve::Resolved;
use crate::weather::WeatherData;

/// Where the weather is for.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct PlaceDto {
    pub city: String,
    pub lat: f64,
    pub lon: f64,
}

/// `status`: `"ok"` (fresh), `"offline"` (the refresh failed; `weather` may still be
/// a stale cache — see `stale`/`error`), or `"needs-city"` (no location available:
/// the UI should ask for a city).
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WeatherReply {
    pub status: String,
    pub weather: Option<WeatherData>,
    pub stale: bool,
    pub fetched_at: Option<u64>,
    pub place: Option<PlaceDto>,
    pub error: Option<String>,
}

/// The reply when there is no place to look up.
pub fn needs_city() -> WeatherReply {
    WeatherReply {
        status: "needs-city".into(),
        weather: None,
        stale: false,
        fetched_at: None,
        place: None,
        error: None,
    }
}

/// Build the reply for a resolved forecast. The forecast API returns no place name,
/// so the `city` label comes from the place the user chose (or "Current location").
pub fn weather_reply(place: PlaceDto, resolved: Resolved<WeatherData>) -> WeatherReply {
    let weather = resolved.data.map(|mut w| {
        w.city = place.city.clone();
        w
    });
    WeatherReply {
        status: if resolved.error.is_some() {
            "offline"
        } else {
            "ok"
        }
        .into(),
        weather,
        stale: resolved.stale,
        fetched_at: resolved.fetched_at,
        place: Some(place),
        error: resolved.error,
    }
}

/// Rates per 1 unit of `base` (the base itself is included as `1.0`) plus provenance.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FxReply {
    pub rates: BTreeMap<String, f64>,
    pub base: String,
    /// The publisher's date for the table (`yyyy-mm-dd`).
    pub date: Option<String>,
    pub updated_at: Option<u64>,
    pub stale: bool,
    pub error: Option<String>,
}

/// Build the reply for resolved rates; empty (never invented) when nothing is available.
pub fn fx_reply(base: &str, resolved: Resolved<Rates>) -> FxReply {
    match resolved.data {
        Some(table) => {
            let mut rates = table.rates.clone();
            rates.insert(table.base.to_uppercase(), 1.0);
            FxReply {
                rates,
                base: table.base,
                date: Some(table.date),
                updated_at: resolved.fetched_at,
                stale: resolved.stale,
                error: resolved.error,
            }
        }
        None => FxReply {
            rates: BTreeMap::new(),
            base: base.to_string(),
            date: None,
            updated_at: None,
            stale: false,
            error: resolved.error,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::weather::parse_forecast;

    fn sample_weather() -> WeatherData {
        parse_forecast(
            r#"{"current":{"temperature_2m":27.3,"weather_code":2,"wind_speed_10m":11.4,
              "relative_humidity_2m":58.0,"apparent_temperature":29.1},
             "hourly":{"time":["2026-11-01T12:00"],"temperature_2m":[27.0],"weather_code":[2]},
             "daily":{"time":["2026-11-01"],"weather_code":[2],"temperature_2m_max":[31.2],
              "temperature_2m_min":[22.1],"uv_index_max":[5.2]}}"#,
        )
        .unwrap()
    }

    fn place() -> PlaceDto {
        PlaceDto {
            city: "Pune, Maharashtra, India".into(),
            lat: 18.5,
            lon: 73.9,
        }
    }

    fn resolved(
        data: Option<WeatherData>,
        stale: bool,
        error: Option<&str>,
    ) -> Resolved<WeatherData> {
        Resolved {
            fetched_at: data.as_ref().map(|_| 1_000),
            data,
            stale,
            error: error.map(String::from),
            updated: None,
        }
    }

    #[test]
    fn fresh_weather_is_ok_and_labelled_with_the_place() {
        let r = weather_reply(place(), resolved(Some(sample_weather()), false, None));
        assert_eq!(r.status, "ok");
        assert_eq!(r.weather.as_ref().unwrap().city, "Pune, Maharashtra, India");
        assert!(!r.stale && r.error.is_none());
        assert_eq!(r.fetched_at, Some(1_000));
    }

    #[test]
    fn a_stale_cache_is_offline_but_still_carries_the_data() {
        let r = weather_reply(
            place(),
            resolved(Some(sample_weather()), true, Some("timed out")),
        );
        assert_eq!(r.status, "offline");
        assert!(r.stale);
        assert!(r.weather.is_some());
        assert_eq!(r.error.as_deref(), Some("timed out"));
    }

    #[test]
    fn offline_with_nothing_cached_has_no_weather_at_all() {
        let r = weather_reply(place(), resolved(None, false, Some("no network")));
        assert_eq!(r.status, "offline");
        assert!(r.weather.is_none());
        assert_eq!(r.place.as_ref().unwrap().city, "Pune, Maharashtra, India");
    }

    #[test]
    fn needs_city_is_empty_and_distinct() {
        let r = needs_city();
        assert_eq!(r.status, "needs-city");
        assert!(r.weather.is_none() && r.place.is_none() && r.error.is_none());
    }

    #[test]
    fn weather_reply_serializes_camel_case() {
        let v = serde_json::to_value(weather_reply(
            place(),
            resolved(Some(sample_weather()), false, None),
        ))
        .unwrap();
        for k in ["status", "weather", "stale", "fetchedAt", "place", "error"] {
            assert!(v.get(k).is_some(), "missing {k}");
        }
        assert_eq!(v["weather"]["feelsLike"], 29);
        assert_eq!(v["place"]["city"], "Pune, Maharashtra, India");
    }

    fn table() -> Rates {
        crate::currency::parse_rates(
            r#"{"amount":1.0,"base":"USD","date":"2026-10-30","rates":{"EUR":0.9,"INR":94.9}}"#,
        )
        .unwrap()
    }

    #[test]
    fn fx_includes_the_base_as_one() {
        let r = fx_reply(
            "USD",
            Resolved {
                data: Some(table()),
                fetched_at: Some(5),
                stale: false,
                error: None,
                updated: None,
            },
        );
        assert_eq!(r.rates.get("USD"), Some(&1.0));
        assert_eq!(r.rates.get("INR"), Some(&94.9));
        assert_eq!(r.date.as_deref(), Some("2026-10-30"));
        assert_eq!(r.updated_at, Some(5));
        let v = serde_json::to_value(&r).unwrap();
        assert!(v.get("updatedAt").is_some());
    }

    #[test]
    fn fx_with_nothing_available_is_empty_not_invented() {
        let r = fx_reply(
            "USD",
            Resolved {
                data: None,
                fetched_at: None,
                stale: false,
                error: Some("offline".into()),
                updated: None,
            },
        );
        assert!(r.rates.is_empty());
        assert_eq!(r.base, "USD");
        assert_eq!(r.error.as_deref(), Some("offline"));
    }
}
