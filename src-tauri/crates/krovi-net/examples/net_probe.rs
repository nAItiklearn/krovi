//! Live check of every outbound call Krovi makes (needs the `net` feature and internet).
//!
//! ```text
//! cargo run -p krovi-net --features net --example net_probe -- Pune
//! ```
//! Prints the geocode result, the forecast for its (coarsened) coordinate, and the FX
//! table, using the same functions the app's commands call.

fn main() {
    let city = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "Pune".to_string());

    let places = krovi_net::search_places(&city).expect("geocode");
    println!("geocode {city:?}: {} candidates", places.len());
    for p in &places {
        println!("  {} ({}, {})", p.name, p.lat, p.lon);
    }
    let best = places.first().expect("at least one place");
    let (lat, lon) = (
        (best.lat * 10.0).round() / 10.0,
        (best.lon * 10.0).round() / 10.0,
    );

    let w = krovi_net::fetch_forecast(lat, lon).expect("forecast");
    println!(
        "weather at ({lat}, {lon}): {}C {} (feels {}C, humidity {}%, wind {} km/h, UV {}) high {} / low {}",
        w.current, w.condition, w.feels_like, w.humidity, w.wind_kph, w.uv_index, w.high, w.low
    );
    println!(
        "  hourly: {}",
        w.hourly
            .iter()
            .map(|h| format!("{} {}C", h.time, h.temp))
            .collect::<Vec<_>>()
            .join(", ")
    );
    println!(
        "  daily: {}",
        w.forecast
            .iter()
            .map(|d| format!("{} {}-{}C {}", d.label, d.low, d.high, d.icon))
            .collect::<Vec<_>>()
            .join(", ")
    );

    let r = krovi_net::fetch_rates("USD").expect("rates");
    println!(
        "fx base {} on {}: {} currencies; EUR={:?} INR={:?} JPY={:?}",
        r.base,
        r.date,
        r.rates.len(),
        r.rates.get("EUR"),
        r.rates.get("INR"),
        r.rates.get("JPY")
    );
    println!(
        "100 USD -> INR = {:.2}",
        krovi_net::convert(100.0, "USD", "INR", &r).unwrap()
    );
}
