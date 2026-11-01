//! City-name lookup for the weather "set your city" fallback.
//!
//! Uses Open-Meteo's **keyless** geocoding endpoint. Only the text the user typed
//! into the "set your city" box is sent (a place name, nothing else); the reply is
//! a handful of candidate places with coordinates. Used only when Windows
//! Location is unavailable or denied. URL building and parsing are pure and
//! unit-tested; the GET is isolated behind the `net` feature like the rest of
//! this crate.

use serde::{Deserialize, Serialize};

const API: &str = "https://geocoding-api.open-meteo.com/v1/search";

/// Longest accepted query (characters). Keeps a pasted paragraph off the wire.
pub const MAX_QUERY_CHARS: usize = 80;

/// A candidate place.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Place {
    /// Display name, e.g. `Pune, Maharashtra, India`.
    pub name: String,
    pub lat: f64,
    pub lon: f64,
}

/// Percent-encode a query component (RFC 3986 unreserved passthrough).
fn encode_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Normalize a user-typed query: trim, collapse whitespace, reject empty / too long.
pub fn clean_query(q: &str) -> crate::Result<String> {
    let q = q.split_whitespace().collect::<Vec<_>>().join(" ");
    if q.is_empty() {
        return Err(crate::NetError::InvalidInput("empty city name".into()));
    }
    if q.chars().count() > MAX_QUERY_CHARS {
        return Err(crate::NetError::InvalidInput("city name too long".into()));
    }
    Ok(q)
}

/// Build the geocoding request URL for an already-cleaned query.
pub fn geocode_url(query: &str, count: u8) -> String {
    format!(
        "{API}?name={}&count={}&language=en&format=json",
        encode_component(query),
        count.clamp(1, 10)
    )
}

#[derive(Deserialize)]
struct RawResponse {
    #[serde(default)]
    results: Vec<RawPlace>,
}

#[derive(Deserialize)]
struct RawPlace {
    name: String,
    latitude: f64,
    longitude: f64,
    #[serde(default)]
    admin1: Option<String>,
    #[serde(default)]
    country: Option<String>,
}

/// Parse a geocoding body into candidate places (possibly empty).
pub fn parse_places(json: &str) -> crate::Result<Vec<Place>> {
    let raw: RawResponse = serde_json::from_str(json)?;
    Ok(raw
        .results
        .into_iter()
        .map(|p| {
            let mut name = p.name;
            for extra in [p.admin1, p.country].into_iter().flatten() {
                if !extra.is_empty() && !name.contains(&extra) {
                    name.push_str(", ");
                    name.push_str(&extra);
                }
            }
            Place {
                name,
                lat: p.latitude,
                lon: p.longitude,
            }
        })
        .collect())
}

/// Look up candidate places for a typed city name (best match first).
#[cfg(feature = "net")]
pub fn search_places(query: &str) -> crate::Result<Vec<Place>> {
    let q = clean_query(query)?;
    let body = crate::http::get(&geocode_url(&q, 5))?;
    parse_places(&body)
}

/// See the `net`-enabled variant. Without `net`, no socket code is compiled.
#[cfg(not(feature = "net"))]
pub fn search_places(query: &str) -> crate::Result<Vec<Place>> {
    let _ = clean_query(query)?;
    Err(crate::NetError::Disabled)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_is_keyless_and_encodes_the_query() {
        let url = geocode_url("São Paulo", 5);
        assert!(url.starts_with("https://geocoding-api.open-meteo.com/v1/search?name="));
        assert!(url.contains("name=S%C3%A3o%20Paulo"));
        assert!(url.contains("count=5"));
        assert!(!url.contains("key="));
        // injection characters are percent-encoded, never raw
        let evil = geocode_url("a&count=999#x", 99);
        assert!(evil.contains("name=a%26count%3D999%23x"));
        assert!(evil.contains("count=10")); // clamped
    }

    #[test]
    fn query_cleaning() {
        assert_eq!(clean_query("  new   york ").unwrap(), "new york");
        assert!(clean_query("   ").is_err());
        assert!(clean_query(&"x".repeat(MAX_QUERY_CHARS + 1)).is_err());
    }

    #[test]
    fn parses_places_with_region_and_country() {
        let body = r#"{"results":[
          {"name":"Pune","latitude":18.51957,"longitude":73.85535,"admin1":"Maharashtra","country":"India"},
          {"name":"Pune","latitude":-9.36944,"longitude":124.31722,"country":"Timor-Leste"}
        ],"generationtime_ms":0.5}"#;
        let p = parse_places(body).unwrap();
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].name, "Pune, Maharashtra, India");
        assert!((p[0].lat - 18.51957).abs() < 1e-9);
        assert_eq!(p[1].name, "Pune, Timor-Leste");
    }

    #[test]
    fn no_results_key_means_no_places() {
        assert!(parse_places(r#"{"generationtime_ms":0.2}"#)
            .unwrap()
            .is_empty());
        assert!(matches!(
            parse_places("not json"),
            Err(crate::NetError::Parse(_))
        ));
    }

    #[cfg(not(feature = "net"))]
    #[test]
    fn search_is_disabled_without_net_feature() {
        assert!(matches!(
            search_places("Pune"),
            Err(crate::NetError::Disabled)
        ));
    }
}
