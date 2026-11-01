//! Keyless local weather (SPEC C7).
//!
//! Uses the public, **keyless** Open-Meteo forecast endpoint: it takes a coarse
//! latitude/longitude and returns current conditions plus a short hourly and
//! daily forecast. No API key, no account, and the only data sent is the coarse
//! coordinate (coarsened in `krovi-sys`). URL building, WMO weather-code mapping,
//! weekday derivation, and response parsing into typed structs are all pure and
//! unit-tested; the network fetch is isolated in [`fetch_forecast`].

use serde::{Deserialize, Serialize};

const API: &str = "https://api.open-meteo.com/v1/forecast";

/// Build the forecast request URL for a coarse coordinate.
pub fn forecast_url(lat: f64, lon: f64) -> String {
    format!(
        "{API}?latitude={lat:.4}&longitude={lon:.4}\
&current=temperature_2m,weather_code,wind_speed_10m,relative_humidity_2m,apparent_temperature\
&hourly=temperature_2m,weather_code\
&daily=weather_code,temperature_2m_max,temperature_2m_min,uv_index_max\
&forecast_days=5&timezone=auto"
    )
}

// --- Raw wire shapes (Open-Meteo) --------------------------------------------

#[derive(Deserialize)]
struct Raw {
    current: RawCurrent,
    hourly: RawHourly,
    daily: RawDaily,
}

#[derive(Deserialize)]
struct RawCurrent {
    /// Local observation time (`2026-11-01T13:15`, in the place's own timezone).
    #[serde(default)]
    time: Option<String>,
    #[serde(rename = "temperature_2m")]
    temp: f64,
    #[serde(rename = "weather_code")]
    code: i32,
    #[serde(rename = "wind_speed_10m")]
    wind: f64,
    #[serde(rename = "relative_humidity_2m")]
    humidity: f64,
    #[serde(rename = "apparent_temperature")]
    feels: f64,
}

#[derive(Deserialize)]
struct RawHourly {
    time: Vec<String>,
    #[serde(rename = "temperature_2m")]
    temp: Vec<f64>,
    #[serde(rename = "weather_code")]
    code: Vec<i32>,
}

#[derive(Deserialize)]
struct RawDaily {
    time: Vec<String>,
    #[serde(rename = "weather_code")]
    code: Vec<i32>,
    #[serde(rename = "temperature_2m_max")]
    max: Vec<f64>,
    #[serde(rename = "temperature_2m_min")]
    min: Vec<f64>,
    #[serde(rename = "uv_index_max")]
    uv: Vec<f64>,
}

// --- Normalized output (camelCase to match the existing frontend DTO) --------

/// Normalized weather for one place: current conditions + short forecast.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WeatherData {
    /// Place label. Open-Meteo returns no name, so this is left empty for the
    /// caller to fill (e.g. from the user's saved location); never guessed.
    pub city: String,
    pub current: i32,
    pub condition: String,
    pub icon: String,
    pub high: i32,
    pub low: i32,
    pub feels_like: i32,
    pub humidity: i32,
    pub wind_kph: i32,
    pub uv_index: i32,
    pub hourly: Vec<HourPoint>,
    pub forecast: Vec<DayPoint>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HourPoint {
    pub time: String,
    pub temp: i32,
    pub icon: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DayPoint {
    pub label: String,
    pub icon: String,
    pub low: i32,
    pub high: i32,
}

fn round(v: f64) -> i32 {
    v.round() as i32
}

/// Parse an Open-Meteo forecast body into normalized [`WeatherData`].
pub fn parse_forecast(json: &str) -> crate::Result<WeatherData> {
    let raw: Raw = serde_json::from_str(json)?;

    // Open-Meteo's hourly series starts at 00:00 today. Start at the current hour
    // (the first slot whose "YYYY-MM-DDTHH" is >= the observation's) and label
    // that slot "Now". Without a `current.time` we fall back to the series start.
    let now_slot = raw.current.time.as_deref().and_then(|now| {
        let now_hour = now.get(..13)?;
        raw.hourly
            .time
            .iter()
            .position(|t| t.get(..13).is_some_and(|h| h >= now_hour))
    });
    let hourly: Vec<HourPoint> = raw
        .hourly
        .time
        .iter()
        .zip(raw.hourly.temp.iter())
        .zip(raw.hourly.code.iter())
        .skip(now_slot.unwrap_or(0))
        .take(6)
        .enumerate()
        .map(|(i, ((t, temp), code))| HourPoint {
            time: if i == 0 && now_slot.is_some() {
                "Now".to_string()
            } else {
                hour_label(t)
            },
            temp: round(*temp),
            icon: wmo_icon(*code).to_string(),
        })
        .collect();

    let day_count = raw.daily.time.len().min(5);
    let forecast: Vec<DayPoint> = (0..day_count)
        .map(|i| {
            let label = if i == 0 {
                "Today".to_string()
            } else {
                day_label(&raw.daily.time[i])
            };
            let code = *raw.daily.code.get(i).unwrap_or(&0);
            DayPoint {
                label,
                icon: wmo_icon(code).to_string(),
                low: round(raw.daily.min.get(i).copied().unwrap_or(0.0)),
                high: round(raw.daily.max.get(i).copied().unwrap_or(0.0)),
            }
        })
        .collect();

    Ok(WeatherData {
        city: String::new(),
        current: round(raw.current.temp),
        condition: wmo_condition(raw.current.code).to_string(),
        icon: wmo_icon(raw.current.code).to_string(),
        high: raw.daily.max.first().map(|v| round(*v)).unwrap_or(0),
        low: raw.daily.min.first().map(|v| round(*v)).unwrap_or(0),
        feels_like: round(raw.current.feels),
        humidity: round(raw.current.humidity),
        wind_kph: round(raw.current.wind),
        uv_index: raw.daily.uv.first().map(|v| round(*v)).unwrap_or(0),
        hourly,
        forecast,
    })
}

/// Fetch and normalize the forecast for a coarse coordinate.
///
/// **CI-only network path.** Only performs a real request when built with the
/// `net` feature; otherwise returns [`crate::NetError::Disabled`]. The URL
/// building and parsing it delegates to are unit-tested regardless of feature.
#[cfg(feature = "net")]
pub fn fetch_forecast(lat: f64, lon: f64) -> crate::Result<WeatherData> {
    let body = crate::http::get(&forecast_url(lat, lon))?;
    parse_forecast(&body)
}

/// See the `net`-enabled variant. Without `net`, no socket code is compiled.
#[cfg(not(feature = "net"))]
pub fn fetch_forecast(lat: f64, lon: f64) -> crate::Result<WeatherData> {
    let _ = (lat, lon);
    Err(crate::NetError::Disabled)
}

// --- Pure helpers ------------------------------------------------------------

fn hour_label(t: &str) -> String {
    // Open-Meteo times look like "2026-11-01T13:00"; show the "HH:MM" part.
    if t.len() >= 16 {
        t[11..16].to_string()
    } else {
        t.to_string()
    }
}

fn day_label(t: &str) -> String {
    match parse_ymd(t) {
        Some((y, m, d)) => weekday_abbrev(y, m, d).to_string(),
        None => t.to_string(),
    }
}

/// Parse a leading `YYYY-MM-DD` (optionally followed by `T...`) into parts.
fn parse_ymd(s: &str) -> Option<(i32, u32, u32)> {
    let date = s.split('T').next().unwrap_or(s);
    let mut it = date.split('-');
    let y = it.next()?.parse().ok()?;
    let m = it.next()?.parse().ok()?;
    let d = it.next()?.parse().ok()?;
    Some((y, m, d))
}

/// Three-letter weekday for a Gregorian date, via Zeller's congruence. Pure and
/// deterministic (no clock), so the forecast labels are testable.
pub fn weekday_abbrev(year: i32, month: u32, day: u32) -> &'static str {
    // January/February are counted as months 13/14 of the previous year.
    let (y, m) = if month < 3 {
        (year - 1, month + 12)
    } else {
        (year, month)
    };
    let q = day as i64;
    let m = m as i64;
    let k = (y as i64).rem_euclid(100);
    let j = (y as i64).div_euclid(100);
    let h = (q + (13 * (m + 1)) / 5 + k + k / 4 + j / 4 + 5 * j).rem_euclid(7);
    // Zeller: 0=Sat, 1=Sun, 2=Mon, ... 6=Fri
    ["Sat", "Sun", "Mon", "Tue", "Wed", "Thu", "Fri"][h as usize]
}

/// Map a WMO weather code to a frontend icon key
/// (`sun` / `partly` / `cloud` / `rain` / `snow` / `storm`).
pub fn wmo_icon(code: i32) -> &'static str {
    match code {
        0 => "sun",
        1 | 2 => "partly",
        3 | 45 | 48 => "cloud",
        51 | 53 | 55 | 56 | 57 | 61 | 63 | 65 | 66 | 67 | 80 | 81 | 82 => "rain",
        71 | 73 | 75 | 77 | 85 | 86 => "snow",
        95 | 96 | 99 => "storm",
        _ => "cloud",
    }
}

/// Map a WMO weather code to a short human-readable condition.
pub fn wmo_condition(code: i32) -> &'static str {
    match code {
        0 => "Clear sky",
        1 => "Mainly clear",
        2 => "Partly cloudy",
        3 => "Overcast",
        45 | 48 => "Fog",
        51 | 53 | 55 => "Drizzle",
        56 | 57 => "Freezing drizzle",
        61 | 63 | 65 => "Rain",
        66 | 67 => "Freezing rain",
        71 | 73 | 75 => "Snow",
        77 => "Snow grains",
        80 | 81 | 82 => "Rain showers",
        85 | 86 => "Snow showers",
        95 => "Thunderstorm",
        96 | 99 => "Thunderstorm with hail",
        _ => "Unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
        "latitude": 18.5, "longitude": 73.8,
        "current": {
            "temperature_2m": 27.3, "weather_code": 2, "wind_speed_10m": 11.4,
            "relative_humidity_2m": 58.0, "apparent_temperature": 29.1
        },
        "hourly": {
            "time": ["2026-11-01T12:00","2026-11-01T13:00","2026-11-01T14:00",
                     "2026-11-01T15:00","2026-11-01T16:00","2026-11-01T17:00","2026-11-01T18:00"],
            "temperature_2m": [27.0, 28.6, 30.1, 30.0, 28.2, 26.4, 25.0],
            "weather_code": [2, 0, 0, 2, 3, 3, 61]
        },
        "daily": {
            "time": ["2026-11-01","2026-11-02","2026-11-03","2026-11-04","2026-11-05"],
            "weather_code": [2, 0, 2, 61, 3],
            "temperature_2m_max": [31.2, 33.0, 32.1, 28.4, 29.0],
            "temperature_2m_min": [22.1, 23.0, 22.4, 21.2, 21.0],
            "uv_index_max": [5.2, 6.0, 5.5, 3.1, 4.0]
        }
    }"#;

    #[test]
    fn url_includes_coarse_coords_and_keyless_host() {
        let url = forecast_url(18.52, 73.85);
        assert!(url.starts_with("https://api.open-meteo.com/v1/forecast"));
        assert!(url.contains("latitude=18.5200"));
        assert!(url.contains("longitude=73.8500"));
        assert!(!url.contains("apikey") && !url.contains("key="));
    }

    #[test]
    fn parses_current_conditions() {
        let w = parse_forecast(SAMPLE).unwrap();
        assert_eq!(w.current, 27);
        assert_eq!(w.feels_like, 29);
        assert_eq!(w.humidity, 58);
        assert_eq!(w.wind_kph, 11);
        assert_eq!(w.condition, "Partly cloudy");
        assert_eq!(w.icon, "partly");
        // today's high/low and uv come from the first daily entry
        assert_eq!(w.high, 31);
        assert_eq!(w.low, 22);
        assert_eq!(w.uv_index, 5);
    }

    #[test]
    fn hourly_is_capped_and_labeled() {
        let w = parse_forecast(SAMPLE).unwrap();
        assert_eq!(w.hourly.len(), 6); // 7 provided, capped at 6
        assert_eq!(w.hourly[0].time, "12:00");
        assert_eq!(w.hourly[1].temp, 29); // 28.6 rounds to 29
        assert_eq!(w.hourly[1].icon, "sun"); // code 0
    }

    #[test]
    fn hourly_starts_at_the_current_hour_and_labels_now() {
        // current.time 13:15 -> the 13:00 slot is "Now", then 14:00.. (not midnight).
        let body = SAMPLE.replace(
            "\"current\": {",
            "\"current\": { \"time\": \"2026-11-01T13:15\",",
        );
        let w = parse_forecast(&body).unwrap();
        assert_eq!(w.hourly[0].time, "Now");
        assert_eq!(w.hourly[0].temp, 29); // 28.6 -> the 13:00 slot
        assert_eq!(w.hourly[1].time, "14:00");
        assert_eq!(w.hourly.len(), 6); // 13:00..18:00 inclusive in the sample
        assert_eq!(w.hourly[5].time, "18:00");
    }

    #[test]
    fn hourly_past_the_series_end_is_empty_not_a_panic() {
        let body = SAMPLE.replace(
            "\"current\": {",
            "\"current\": { \"time\": \"2026-11-09T09:00\",",
        );
        let w = parse_forecast(&body).unwrap();
        // no slot at/after that hour: fall back to the series start, unlabelled.
        assert_eq!(w.hourly[0].time, "12:00");
        assert_eq!(w.hourly.len(), 6);
    }

    #[test]
    fn forecast_labels_first_today_then_weekdays() {
        let w = parse_forecast(SAMPLE).unwrap();
        assert_eq!(w.forecast.len(), 5);
        assert_eq!(w.forecast[0].label, "Today");
        // 2026-11-02 is a Monday
        assert_eq!(w.forecast[1].label, "Mon");
        assert_eq!(w.forecast[3].icon, "rain"); // code 61
    }

    #[test]
    fn weekday_matches_known_dates() {
        assert_eq!(weekday_abbrev(2000, 1, 1), "Sat"); // Y2K was a Saturday
        assert_eq!(weekday_abbrev(2021, 1, 1), "Fri");
        assert_eq!(weekday_abbrev(2026, 11, 2), "Mon");
    }

    #[test]
    fn wmo_mapping_covers_each_family() {
        assert_eq!(wmo_icon(0), "sun");
        assert_eq!(wmo_icon(2), "partly");
        assert_eq!(wmo_icon(3), "cloud");
        assert_eq!(wmo_icon(65), "rain");
        assert_eq!(wmo_icon(75), "snow");
        assert_eq!(wmo_icon(95), "storm");
        assert_eq!(wmo_condition(0), "Clear sky");
        assert_eq!(wmo_condition(95), "Thunderstorm");
    }

    #[test]
    fn malformed_body_is_a_parse_error() {
        assert!(matches!(
            parse_forecast("{ not json"),
            Err(crate::NetError::Parse(_))
        ));
    }

    #[cfg(not(feature = "net"))]
    #[test]
    fn fetch_is_disabled_without_net_feature() {
        assert!(matches!(
            fetch_forecast(18.5, 73.8),
            Err(crate::NetError::Disabled)
        ));
    }
}
