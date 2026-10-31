//! Coarse location for weather (SPEC C7).
//!
//! The only location data Krovi ever obtains is a **coarse** latitude/longitude,
//! used solely to request weather. It is rounded to ~0.1° (~11 km) before it can
//! leave the device so a precise position is never transmitted. Validation and
//! coarsening are pure and tested; the Windows Geolocation call is isolated in
//! the `native` submodule (CI-only).

/// Whether a coordinate is finite and within valid lat/long ranges.
pub fn valid_coords(lat: f64, lon: f64) -> bool {
    lat.is_finite()
        && lon.is_finite()
        && (-90.0..=90.0).contains(&lat)
        && (-180.0..=180.0).contains(&lon)
}

/// Round a coordinate to ~0.1° (~11 km) so only a coarse location is ever used
/// or transmitted. The privacy boundary for the weather feature.
pub fn coarsen(lat: f64, lon: f64) -> (f64, f64) {
    ((lat * 10.0).round() / 10.0, (lon * 10.0).round() / 10.0)
}

/// The device's current coarse location, if available.
///
/// Returns `Ok(None)` when no location is available **and** in any build without
/// the `native` feature. Callers fall back to a user-set location (SPEC C7:
/// Windows location *or* account-free manual choice). Any value returned is
/// already coarsened.
#[cfg(all(windows, feature = "native"))]
pub fn current_location() -> crate::Result<Option<(f64, f64)>> {
    native::current_location()
}

/// See the native variant. Without `native`, no OS binding is compiled.
#[cfg(not(all(windows, feature = "native")))]
pub fn current_location() -> crate::Result<Option<(f64, f64)>> {
    Ok(None)
}

// ---------------------------------------------------------------------------
// Native implementation — CI-only.
//
// Authored against windows-rs 0.58. Not compiled/exercised on the local
// 32-bit-gcc host; no local test results claimed. Returns ONLY a coarsened
// coordinate — nothing finer ever leaves this function.
// ---------------------------------------------------------------------------
#[cfg(all(windows, feature = "native"))]
mod native {
    use crate::SysError;
    use windows::Devices::Geolocation::Geolocator;

    fn err<E: std::fmt::Display>(e: E) -> SysError {
        SysError::Message(e.to_string())
    }

    pub(super) fn current_location() -> crate::Result<Option<(f64, f64)>> {
        let geo = Geolocator::new().map_err(err)?;
        let position = geo.GetGeopositionAsync().map_err(err)?.get().map_err(err)?;
        let coordinate = position.Coordinate().map_err(err)?;
        let point = coordinate.Point().map_err(err)?;
        let basic = point.Position().map_err(err)?;
        Ok(Some(super::coarsen(basic.Latitude, basic.Longitude)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_ranges() {
        assert!(valid_coords(0.0, 0.0));
        assert!(valid_coords(90.0, 180.0));
        assert!(valid_coords(-90.0, -180.0));
        assert!(!valid_coords(90.1, 0.0));
        assert!(!valid_coords(0.0, 180.1));
        assert!(!valid_coords(f64::NAN, 0.0));
        assert!(!valid_coords(0.0, f64::INFINITY));
    }

    #[test]
    fn coarsen_rounds_to_tenths() {
        let (lat, lon) = coarsen(18.5204, 73.8567);
        assert!((lat - 18.5).abs() < 1e-9);
        assert!((lon - 73.9).abs() < 1e-9);
    }

    #[test]
    fn coarsen_handles_negatives() {
        let (lat, lon) = coarsen(-37.84, -122.47);
        assert!((lat - -37.8).abs() < 1e-9);
        assert!((lon - -122.5).abs() < 1e-9);
    }

    #[cfg(not(all(windows, feature = "native")))]
    #[test]
    fn location_is_noop_without_native() {
        assert_eq!(current_location().unwrap(), None);
    }
}
