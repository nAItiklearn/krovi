//! The one place in Krovi that opens a socket. **CI-only.**
//!
//! Compiled only under the `net` feature (see the crate docs and DECISIONS.md
//! D-009). It performs a plain HTTPS GET with a short timeout using `ureq` over
//! its pure-Rust rustls TLS stack — no OpenSSL / native-TLS C dependency. There
//! is no retry, no cookies, no redirect of bodies anywhere; the response text is
//! handed straight back to a pure parser.
//!
//! NOTE: this module is not compiled or exercised on the local 32-bit-gcc host;
//! it is built and run only in CI on a full toolchain. No local test results are
//! claimed for it.

use std::time::Duration;

use crate::NetError;

const TIMEOUT: Duration = Duration::from_secs(10);

/// GET `url` and return the response body as text.
pub fn get(url: &str) -> crate::Result<String> {
    let resp = ureq::get(url)
        .timeout(TIMEOUT)
        .call()
        .map_err(|e| NetError::Request(e.to_string()))?;
    resp.into_string()
        .map_err(|e| NetError::Request(e.to_string()))
}
