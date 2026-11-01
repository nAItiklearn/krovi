//! The complete URL surface of the server, as a pure function. Anything that is not one
//! of these exact shapes is `NotFound`, and the server answers every `NotFound` (and every
//! unknown token) with the same 404: no listing, no hints.
//!
//!   GET|HEAD /s/<token>            send page (the Download buttons)
//!   GET|HEAD /s/<token>/<n>        file number n of that share (Range supported)
//!   GET|HEAD /r/<token>            receive page (the upload form)
//!   POST     /r/<token>/up?n=<name> raw upload body, one file per request

use crate::names::percent_decode;
use crate::token::plausible_token;

#[derive(Debug, PartialEq, Eq)]
pub enum Route<'a> {
    SendPage { token: &'a str, query: &'a str },
    SendFile { token: &'a str, index: usize },
    ReceivePage { token: &'a str, query: &'a str },
    ReceiveUpload { token: &'a str, query: &'a str },
    NotFound,
}

pub fn parse(target: &str) -> Route<'_> {
    // The request target must be plain origin-form; reject anything exotic up front.
    if !target.starts_with('/')
        || target.len() > 512
        || target.bytes().any(|b| b <= b' ' || b == 0x7f || b == b'#')
    {
        return Route::NotFound;
    }
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let mut seg = path[1..].split('/');
    let kind = seg.next().unwrap_or("");
    let token = seg.next().unwrap_or("");
    let rest = seg.next();
    let extra = seg.next();
    if !plausible_token(token) {
        return Route::NotFound;
    }
    match (kind, rest, extra) {
        ("s", None, None) | ("s", Some(""), None) => Route::SendPage { token, query },
        ("s", Some(n), None) => match parse_index(n) {
            Some(index) => Route::SendFile { token, index },
            None => Route::NotFound,
        },
        ("r", None, None) | ("r", Some(""), None) => Route::ReceivePage { token, query },
        ("r", Some("up"), None) => Route::ReceiveUpload { token, query },
        _ => Route::NotFound,
    }
}

fn parse_index(s: &str) -> Option<usize> {
    if s.is_empty() || s.len() > 6 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

/// First value of `key` in a raw query string, percent-decoded.
pub fn query_get(query: &str, key: &str) -> Option<String> {
    for pair in query.split('&') {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        if k == key {
            return percent_decode(v, true);
        }
    }
    None
}

/// Value of cookie `name` in a `Cookie:` header.
pub fn cookie_get<'a>(header: &'a str, name: &str) -> Option<&'a str> {
    header.split(';').find_map(|part| {
        let (k, v) = part.trim().split_once('=')?;
        (k == name).then_some(v)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: &str = "AbCdEfGhIjKlMnOpQrStUvWxYz0";

    #[test]
    fn the_four_valid_shapes() {
        assert_eq!(
            parse(&format!("/s/{T}")),
            Route::SendPage {
                token: T,
                query: ""
            }
        );
        assert_eq!(
            parse(&format!("/s/{T}/")),
            Route::SendPage {
                token: T,
                query: ""
            }
        );
        assert_eq!(
            parse(&format!("/s/{T}/3")),
            Route::SendFile { token: T, index: 3 }
        );
        assert_eq!(
            parse(&format!("/r/{T}")),
            Route::ReceivePage {
                token: T,
                query: ""
            }
        );
        assert_eq!(
            parse(&format!("/r/{T}/up?n=a.txt")),
            Route::ReceiveUpload {
                token: T,
                query: "n=a.txt"
            }
        );
        assert_eq!(
            parse(&format!("/s/{T}?pin=1234")),
            Route::SendPage {
                token: T,
                query: "pin=1234"
            }
        );
    }

    #[test]
    fn everything_else_is_not_found() {
        for bad in [
            "/",
            "",
            "/index.html",
            "/favicon.ico",
            "/s",
            "/s/",
            "/r/",
            "/x/AbCdEfGhIjKlMnOpQrStUvWxYz0",
            "/s/short",
            "/s/AbCdEfGhIjKlMnOpQrStUvWxYz0/abc",
            "/s/AbCdEfGhIjKlMnOpQrStUvWxYz0/1/2",
            "/s/AbCdEfGhIjKlMnOpQrStUvWxYz0/-1",
            "/s/AbCdEfGhIjKlMnOpQrStUvWxYz0/1234567",
            "/r/AbCdEfGhIjKlMnOpQrStUvWxYz0/down",
            "/r/AbCdEfGhIjKlMnOpQrStUvWxYz0/up/extra",
            "//s/AbCdEfGhIjKlMnOpQrStUvWxYz0",
            "/s/../../etc/passwd",
            "/s/AbCdEfGhIjKlMnOpQrStUvWxYz0%2f1",
            "http://evil/s/AbCdEfGhIjKlMnOpQrStUvWxYz0",
            "*",
        ] {
            assert_eq!(parse(bad), Route::NotFound, "{bad}");
        }
    }

    #[test]
    fn control_characters_and_fragments_are_rejected() {
        assert_eq!(
            parse("/s/AbCdEfGhIjKlMnOpQrStUvWxYz0\r\nX: y"),
            Route::NotFound
        );
        assert_eq!(parse("/s/AbCdEfGhIjKlMnOpQrStUvWxYz0 "), Route::NotFound);
        assert_eq!(
            parse("/s/AbCdEfGhIjKlMnOpQrStUvWxYz0#frag"),
            Route::NotFound
        );
        assert_eq!(
            parse(&format!("/s/{T}?{}", "a".repeat(600))),
            Route::NotFound
        );
    }

    #[test]
    fn query_values_are_decoded() {
        assert_eq!(
            query_get("n=My%20File.pdf&x=1", "n").as_deref(),
            Some("My File.pdf")
        );
        assert_eq!(query_get("pin=1234", "pin").as_deref(), Some("1234"));
        assert_eq!(query_get("a=1", "pin"), None);
        assert_eq!(query_get("n=%FF", "n"), None);
    }

    #[test]
    fn cookie_lookup() {
        assert_eq!(cookie_get("a=1; kvabc=XYZ; b=2", "kvabc"), Some("XYZ"));
        assert_eq!(cookie_get("a=1", "kvabc"), None);
        assert_eq!(cookie_get("", "x"), None);
    }
}
