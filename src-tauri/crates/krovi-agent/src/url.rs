//! Link safety: what Krovi is willing to open, and how text becomes part of a link.
//!
//! * [`validate`] / [`validate_full`]: only `http` and `https`; no `user:pass@` in the address;
//!   no `javascript:` / `file:` / `data:` / any other scheme; no UNC (`\\server\share`, `//host`);
//!   the host must be a real host name (or an IPv4 address / `localhost`); a length cap; no
//!   spaces, control characters, backslashes or non-ASCII.
//! * [`encode_component`]: our own RFC 3986 percent-encoder for search words and mail fields.
//! * [`check_openable`]: the last gate before the OS is asked to open anything. It accepts what
//!   this crate builds (a validated web link, or a `mailto:` link made by [`crate::mail`]).

use std::fmt;

/// Longest address Krovi will open.
pub const MAX_URL_LEN: usize = 2048;
/// Longest `mailto:` link (some mail clients and `ShellExecute` truncate beyond this).
pub const MAX_MAILTO_LEN: usize = 2000;
/// Longest search phrase, in characters.
pub const MAX_QUERY_CHARS: usize = 300;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UrlError {
    Empty,
    TooLong,
    /// A scheme other than http/https was given (empty when the address had no scheme but one is
    /// required).
    NotHttp(String),
    Credentials,
    UncPath,
    BadHost,
    BadPort,
    BadChars,
}

impl fmt::Display for UrlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UrlError::Empty => write!(f, "That address is empty."),
            UrlError::TooLong => write!(f, "That address is too long."),
            UrlError::NotHttp(s) if s.is_empty() => {
                write!(f, "Write the full address, starting with https://.")
            }
            UrlError::NotHttp(s) => {
                write!(f, "Only http and https links can be opened, not {s}:.")
            }
            UrlError::Credentials => {
                write!(
                    f,
                    "Links with a username or password in them are not opened."
                )
            }
            UrlError::UncPath => write!(f, "Network (UNC) paths are not web links."),
            UrlError::BadHost => write!(f, "That is not a valid website name."),
            UrlError::BadPort => write!(f, "That port number is not valid."),
            UrlError::BadChars => {
                write!(
                    f,
                    "That address has characters that are not allowed in a link."
                )
            }
        }
    }
}

impl std::error::Error for UrlError {}

/// A web address that passed validation, normalised (lower-case scheme and host).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidUrl {
    pub url: String,
    pub host: String,
    pub https: bool,
}

// ---------------------------------------------------------------------------------------------
// host names
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostKind {
    Name,
    Ipv4,
    Localhost,
}

/// What kind of host `host` is (lower case expected), or `None` if it is not a valid host.
pub fn classify_host(host: &str) -> Option<HostKind> {
    if host.is_empty() || host.len() > 253 || host.ends_with('.') {
        return None;
    }
    if host == "localhost" {
        return Some(HostKind::Localhost);
    }
    let labels: Vec<&str> = host.split('.').collect();
    for l in &labels {
        let ok = !l.is_empty()
            && l.len() <= 63
            && !l.starts_with('-')
            && !l.ends_with('-')
            && l.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        if !ok {
            return None;
        }
    }
    let last = labels[labels.len() - 1];
    if last.bytes().all(|b| b.is_ascii_digit()) {
        // only a dotted-quad IPv4 address may look numeric
        let quad = labels.len() == 4
            && labels.iter().all(|l| {
                l.len() <= 3
                    && l.parse::<u16>().is_ok_and(|n| n <= 255)
                    && (l.len() == 1 || !l.starts_with('0'))
            });
        return quad.then_some(HostKind::Ipv4);
    }
    let tld_ok = last.starts_with("xn--")
        || (last.len() >= 2 && last.bytes().all(|b| b.is_ascii_lowercase()));
    (labels.len() >= 2 && tld_ok).then_some(HostKind::Name)
}

/// Is this a plausible host name, IPv4 address or `localhost`?
pub fn valid_hostname(host: &str) -> bool {
    classify_host(host).is_some()
}

/// `localhost` or an IPv4 loopback address.
pub fn is_loopback(host: &str) -> bool {
    host == "localhost" || (classify_host(host) == Some(HostKind::Ipv4) && host.starts_with("127."))
}

// ---------------------------------------------------------------------------------------------
// validation
// ---------------------------------------------------------------------------------------------

enum Scheme<'a> {
    /// No scheme at all (a bare host, or `host:port`).
    None,
    /// `name://rest`
    Url(&'a str, &'a str),
    /// `name:rest` without slashes (`javascript:`, `data:`, `mailto:`, `http:example.com`...).
    Other(&'a str),
}

fn split_scheme(s: &str) -> Scheme<'_> {
    let b = s.as_bytes();
    if b.is_empty() || !b[0].is_ascii_alphabetic() {
        return Scheme::None;
    }
    let mut i = 0;
    while i < b.len() && (b[i].is_ascii_alphanumeric() || matches!(b[i], b'+' | b'-' | b'.')) {
        i += 1;
    }
    if i >= b.len() || b[i] != b':' {
        return Scheme::None;
    }
    let (name, rest) = (&s[..i], &s[i + 1..]);
    if let Some(r) = rest.strip_prefix("//") {
        return Scheme::Url(name, r);
    }
    // `localhost:3000/x`, `example.com:8080`: digits right after the colon are a port, not a scheme
    let before_path = rest.split(['/', '?', '#']).next().unwrap_or("");
    if !before_path.is_empty() && before_path.bytes().all(|c| c.is_ascii_digit()) {
        return Scheme::None;
    }
    Scheme::Other(name)
}

/// Does `s` start with a scheme (`https://`, `javascript:`, `mailto:`...)? A bare `host:port`
/// is not a scheme.
pub fn has_scheme(s: &str) -> bool {
    !matches!(split_scheme(s.trim()), Scheme::None)
}

/// Every `%` is followed by two hex digits.
fn percent_ok(s: &str) -> bool {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let ok = b.get(i + 1).is_some_and(u8::is_ascii_hexdigit)
                && b.get(i + 2).is_some_and(u8::is_ascii_hexdigit);
            if !ok {
                return false;
            }
            i += 3;
        } else {
            i += 1;
        }
    }
    true
}

fn validate_inner(input: &str, require_scheme: bool) -> Result<ValidUrl, UrlError> {
    let s = input.trim();
    if s.is_empty() {
        return Err(UrlError::Empty);
    }
    if s.len() > MAX_URL_LEN {
        return Err(UrlError::TooLong);
    }
    if s.starts_with("\\\\") || s.starts_with("//") {
        return Err(UrlError::UncPath);
    }
    // A foreign scheme is reported as such ("not data:") before anything else is looked at.
    let (https, after) = match split_scheme(s) {
        Scheme::None if require_scheme => return Err(UrlError::NotHttp(String::new())),
        Scheme::None => (true, s),
        Scheme::Url(name, rest) => match name.to_ascii_lowercase().as_str() {
            "https" => (true, rest),
            "http" => (false, rest),
            other => return Err(UrlError::NotHttp(other.to_string())),
        },
        Scheme::Other(name) => return Err(UrlError::NotHttp(name.to_ascii_lowercase())),
    };
    for c in s.chars() {
        let bad = c.is_control()
            || c.is_whitespace()
            || !c.is_ascii()
            || matches!(c, '\\' | '"' | '<' | '>' | '^' | '`' | '{' | '|' | '}');
        if bad {
            return Err(UrlError::BadChars);
        }
    }

    let end = after.find(['/', '?', '#']).unwrap_or(after.len());
    let (authority, tail) = (&after[..end], &after[end..]);
    if authority.contains('@') {
        return Err(UrlError::Credentials);
    }
    if authority.is_empty() || authority.contains(['[', ']']) {
        return Err(UrlError::BadHost);
    }
    let (host_part, port) = match authority.rfind(':') {
        Some(i) => {
            let p = &authority[i + 1..];
            let ok = !p.is_empty()
                && p.len() <= 5
                && p.bytes().all(|b| b.is_ascii_digit())
                && p.parse::<u32>().is_ok_and(|n| (1..=65_535).contains(&n));
            if !ok {
                return Err(UrlError::BadPort);
            }
            (&authority[..i], Some(p))
        }
        None => (authority, None),
    };
    let host = host_part.to_ascii_lowercase();
    if !valid_hostname(&host) {
        return Err(UrlError::BadHost);
    }
    if !percent_ok(tail) {
        return Err(UrlError::BadChars);
    }
    let url = format!(
        "{}://{}{}{}",
        if https { "https" } else { "http" },
        host,
        port.map(|p| format!(":{p}")).unwrap_or_default(),
        tail
    );
    Ok(ValidUrl { url, host, https })
}

/// Validate a web address as typed. A missing scheme is taken to be `https://`.
pub fn validate(input: &str) -> Result<ValidUrl, UrlError> {
    validate_inner(input, false)
}

/// Validate an address that must already carry `http://` or `https://`.
pub fn validate_full(input: &str) -> Result<ValidUrl, UrlError> {
    validate_inner(input, true)
}

/// What kind of link passed [`check_openable`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Openable {
    Web,
    Mailto,
}

/// The gate in front of the OS "open this" call. Accepts a validated `http(s)` link or a
/// `mailto:` link made only of the characters [`crate::mail`] emits.
pub fn check_openable(url: &str) -> Result<Openable, UrlError> {
    let is_mailto = url
        .get(..7)
        .is_some_and(|p| p.eq_ignore_ascii_case("mailto:"));
    if !is_mailto {
        return validate_full(url).map(|_| Openable::Web);
    }
    if url.len() > MAX_MAILTO_LEN {
        return Err(UrlError::TooLong);
    }
    let rest = &url[7..];
    let chars_ok = rest.bytes().all(|b| {
        b.is_ascii_alphanumeric()
            || matches!(
                b,
                b'%' | b'@' | b'.' | b'_' | b'~' | b',' | b'+' | b'-' | b'?' | b'=' | b'&'
            )
    });
    if chars_ok && percent_ok(rest) {
        Ok(Openable::Mailto)
    } else {
        Err(UrlError::BadChars)
    }
}

// ---------------------------------------------------------------------------------------------
// percent-encoding and search links
// ---------------------------------------------------------------------------------------------

/// Percent-encode `s` (UTF-8 bytes) for use as one URL component: letters, digits and
/// `- . _ ~` stay, everything else becomes `%XX` (upper-case hex). A space is `%20`, never `+`.
pub fn encode_component(s: &str) -> String {
    encode_keeping(s, b"")
}

/// Like [`encode_component`], but also leaves the bytes in `keep` as they are.
pub fn encode_keeping(s: &str, keep: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(s.len() * 3);
    for &b in s.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') || keep.contains(&b)
        {
            out.push(b as char);
        } else {
            out.push('%');
            out.push(HEX[(b >> 4) as usize] as char);
            out.push(HEX[(b & 0x0F) as usize] as char);
        }
    }
    out
}

/// Collapse every run of whitespace/control characters to one space and trim.
pub fn normalize_query(q: &str) -> String {
    q.split(|c: char| c.is_whitespace() || c.is_control())
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchError {
    Empty,
    TooLong,
}

impl fmt::Display for SearchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SearchError::Empty => write!(f, "There is nothing to search for."),
            SearchError::TooLong => {
                write!(
                    f,
                    "That search is too long (over {MAX_QUERY_CHARS} characters)."
                )
            }
        }
    }
}

impl std::error::Error for SearchError {}

/// Fill a `{q}` search template with a query: whitespace normalised, then percent-encoded.
pub fn search_url(template: &str, query: &str) -> Result<String, SearchError> {
    let q = normalize_query(query);
    if q.is_empty() {
        return Err(SearchError::Empty);
    }
    if q.chars().count() > MAX_QUERY_CHARS {
        return Err(SearchError::TooLong);
    }
    Ok(template.replace("{q}", &encode_component(&q)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoder_table() {
        for (raw, want) in [
            ("", ""),
            ("rust", "rust"),
            ("rust lifetimes", "rust%20lifetimes"),
            ("a&b=c", "a%26b%3Dc"),
            ("100% sure #1?", "100%25%20sure%20%231%3F"),
            ("a/b\\c", "a%2Fb%5Cc"),
            ("1+1=2", "1%2B1%3D2"),
            ("-._~", "-._~"),
            ("caf\u{e9}", "caf%C3%A9"),
            ("\u{65e5}\u{672c}", "%E6%97%A5%E6%9C%AC"),
            ("\u{1f600}", "%F0%9F%98%80"),
            ("line\nbreak\ttab", "line%0Abreak%09tab"),
            (
                "\"quote\" <tag> 'single'",
                "%22quote%22%20%3Ctag%3E%20%27single%27",
            ),
            ("ends with space ", "ends%20with%20space%20"),
            ("a,b;c:d@e", "a%2Cb%3Bc%3Ad%40e"),
        ] {
            assert_eq!(encode_component(raw), want, "{raw:?}");
        }
    }

    #[test]
    fn encoder_output_is_always_url_safe_and_reversible() {
        fn decode(s: &str) -> String {
            let b = s.as_bytes();
            let mut out = Vec::new();
            let mut i = 0;
            while i < b.len() {
                if b[i] == b'%' {
                    out.push(u8::from_str_radix(&s[i + 1..i + 3], 16).unwrap());
                    i += 3;
                } else {
                    out.push(b[i]);
                    i += 1;
                }
            }
            String::from_utf8(out).unwrap()
        }
        for raw in [
            "plain",
            "a b c",
            "x=1&y=2",
            "\u{e9}\u{e8}\u{ea}",
            "\u{1f680} launch",
            "100%",
            "%41",
            "tab\there",
            "<script>alert(1)</script>",
            "https://evil.example/?q=1#frag",
        ] {
            let enc = encode_component(raw);
            assert!(
                enc.bytes()
                    .all(|b| b.is_ascii_alphanumeric()
                        || matches!(b, b'-' | b'.' | b'_' | b'~' | b'%')),
                "{enc}"
            );
            assert_eq!(decode(&enc), raw);
        }
    }

    #[test]
    fn encoder_can_keep_selected_bytes() {
        assert_eq!(encode_keeping("a@b.c,d e", b"@,"), "a@b.c,d%20e");
    }

    #[test]
    fn valid_addresses() {
        for (input, want) in [
            ("https://example.com", "https://example.com"),
            ("example.com", "https://example.com"),
            (
                "HTTPS://EXAMPLE.COM/Path?Q=1#Frag",
                "https://example.com/Path?Q=1#Frag",
            ),
            ("http://example.com/a", "http://example.com/a"),
            (
                "github.com/rust-lang/rust",
                "https://github.com/rust-lang/rust",
            ),
            (
                "https://sub.domain.example.co.uk/x/y?a=1&b=2",
                "https://sub.domain.example.co.uk/x/y?a=1&b=2",
            ),
            ("https://example.com:8443/x", "https://example.com:8443/x"),
            ("localhost:3000/app", "https://localhost:3000/app"),
            ("http://localhost:3000", "http://localhost:3000"),
            ("http://192.168.1.1/admin", "http://192.168.1.1/admin"),
            (
                "https://xn--bcher-kva.example/x",
                "https://xn--bcher-kva.example/x",
            ),
            (
                "https://medium.com/@someone/post",
                "https://medium.com/@someone/post",
            ),
            ("https://example.com/a%20b", "https://example.com/a%20b"),
            ("  https://example.com  ", "https://example.com"),
            ("https://example.com?x=1", "https://example.com?x=1"),
            ("https://my-site.example.com", "https://my-site.example.com"),
        ] {
            let v = validate(input).unwrap_or_else(|e| panic!("{input}: {e}"));
            assert_eq!(v.url, want, "{input}");
        }
    }

    #[test]
    fn host_and_https_flags() {
        let v = validate("http://Example.COM:81/x").unwrap();
        assert_eq!((v.host.as_str(), v.https), ("example.com", false));
        let v = validate("example.com").unwrap();
        assert!(v.https);
    }

    #[test]
    fn refused_addresses() {
        for (input, want) in [
            ("", UrlError::Empty),
            ("   ", UrlError::Empty),
            (
                "javascript:alert(1)",
                UrlError::NotHttp("javascript".into()),
            ),
            (
                "JavaScript:alert(1)",
                UrlError::NotHttp("javascript".into()),
            ),
            (
                "data:text/html,<script>alert(1)</script>",
                UrlError::NotHttp("data".into()),
            ),
            (
                "file:///C:/Windows/System32/calc.exe",
                UrlError::NotHttp("file".into()),
            ),
            ("ftp://example.com/x", UrlError::NotHttp("ftp".into())),
            ("mailto:a@b.co", UrlError::NotHttp("mailto".into())),
            ("vbscript:msgbox", UrlError::NotHttp("vbscript".into())),
            (
                "ms-settings:network",
                UrlError::NotHttp("ms-settings".into()),
            ),
            ("http:example.com", UrlError::NotHttp("http".into())),
            ("c:/windows/system32", UrlError::NotHttp("c".into())),
            ("\\\\server\\share\\file", UrlError::UncPath),
            ("//evil.example/path", UrlError::UncPath),
            ("https://user:pass@example.com", UrlError::Credentials),
            ("https://user@example.com", UrlError::Credentials),
            (
                "https://example.com:80@evil.example/",
                UrlError::Credentials,
            ),
            ("https://", UrlError::BadHost),
            ("https:///etc/passwd", UrlError::BadHost),
            ("https://-bad.example.com", UrlError::BadHost),
            ("https://bad_host.example.com", UrlError::BadHost),
            ("https://exa mple.com", UrlError::BadChars),
            ("https://example", UrlError::BadHost),
            ("https://example.c", UrlError::BadHost),
            ("https://example.123", UrlError::BadHost),
            ("https://999.1.1.1", UrlError::BadHost),
            ("https://01.1.1.1", UrlError::BadHost),
            ("https://1.2.3", UrlError::BadHost),
            ("https://[::1]/", UrlError::BadHost),
            ("https://example.com./", UrlError::BadHost),
            ("https://exa%6dple.com", UrlError::BadHost),
            ("https://example.com:0", UrlError::BadPort),
            ("https://example.com:99999", UrlError::BadPort),
            ("https://example.com:abc", UrlError::BadPort),
            ("https://example.com:", UrlError::BadPort),
            ("https://example.com/a\\b", UrlError::BadChars),
            ("https://example.com/\"onload=1", UrlError::BadChars),
            ("https://example.com/<x>", UrlError::BadChars),
            ("https://example.com/%zz", UrlError::BadChars),
            ("https://example.com/%4", UrlError::BadChars),
            ("https://example.com/\u{e9}", UrlError::BadChars),
            ("https://example.com/a\nb", UrlError::BadChars),
            ("https://example.com/\u{0}", UrlError::BadChars),
            (
                "https://\u{43f}\u{440}\u{438}\u{43c}\u{435}\u{440}.\u{440}\u{444}",
                UrlError::BadChars,
            ),
            ("notaurl", UrlError::BadHost),
        ] {
            assert_eq!(validate(input), Err(want), "{input:?}");
        }
    }

    #[test]
    fn max_length() {
        let ok = format!("https://example.com/{}", "a".repeat(MAX_URL_LEN - 20));
        assert_eq!(ok.len(), MAX_URL_LEN);
        assert!(validate(&ok).is_ok());
        let long = format!("{ok}a");
        assert_eq!(validate(&long), Err(UrlError::TooLong));
    }

    #[test]
    fn validate_full_requires_a_scheme() {
        assert_eq!(
            validate_full("example.com"),
            Err(UrlError::NotHttp(String::new()))
        );
        assert!(validate_full("https://example.com").is_ok());
        assert!(validate_full("http://example.com").is_ok());
        assert!(validate_full("HTTP://example.com").is_ok());
        assert!(validate_full("javascript:alert(1)").is_err());
    }

    #[test]
    fn hostnames() {
        for ok in [
            "example.com",
            "a.b.c.example.org",
            "localhost",
            "127.0.0.1",
            "xn--80ak6aa92e.com",
            "a-b.io",
        ] {
            assert!(valid_hostname(ok), "{ok}");
        }
        for bad in [
            "",
            ".",
            "a..b.com",
            "-a.com",
            "a-.com",
            "a_b.com",
            "example",
            "x.c",
            "1.2.3",
            "256.1.1.1",
            "Example.com",
            "a b.com",
            &"a".repeat(64),
        ] {
            assert!(!valid_hostname(bad), "{bad}");
        }
        let long_label = format!("{}.com", "a".repeat(63));
        assert!(valid_hostname(&long_label));
        assert!(is_loopback("localhost") && is_loopback("127.0.0.1") && is_loopback("127.1.2.3"));
        assert!(!is_loopback("example.com") && !is_loopback("10.0.0.1"));
        assert_eq!(classify_host("127.0.0.1"), Some(HostKind::Ipv4));
        assert_eq!(classify_host("example.com"), Some(HostKind::Name));
    }

    #[test]
    fn search_urls_are_encoded() {
        let t = "https://www.google.com/search?q={q}";
        assert_eq!(
            search_url(t, "rust lifetimes").unwrap(),
            "https://www.google.com/search?q=rust%20lifetimes"
        );
        assert_eq!(
            search_url(t, "  a   b\n c ").unwrap(),
            "https://www.google.com/search?q=a%20b%20c"
        );
        // an attempt to break out of the query stays inside it
        assert_eq!(
            search_url(t, "x&q=evil#frag").unwrap(),
            "https://www.google.com/search?q=x%26q%3Devil%23frag"
        );
        assert_eq!(
            search_url("https://open.spotify.com/search/{q}", "lo-fi beats").unwrap(),
            "https://open.spotify.com/search/lo-fi%20beats"
        );
        assert_eq!(search_url(t, "   "), Err(SearchError::Empty));
        assert_eq!(
            search_url(t, &"x".repeat(MAX_QUERY_CHARS + 1)),
            Err(SearchError::TooLong)
        );
        assert!(search_url(t, &"x".repeat(MAX_QUERY_CHARS)).is_ok());
        // the result of every search passes the same gate as any other link
        assert!(validate_full(&search_url(t, "<script>\"'").unwrap()).is_ok());
    }

    #[test]
    fn search_is_counted_in_characters_not_bytes() {
        let t = "https://www.google.com/search?q={q}";
        assert!(search_url(t, &"\u{e9}".repeat(MAX_QUERY_CHARS)).is_ok());
    }

    #[test]
    fn openable_gate() {
        assert_eq!(check_openable("https://example.com/x"), Ok(Openable::Web));
        assert_eq!(
            check_openable("mailto:a@b.co?subject=Hi%20there&body=x"),
            Ok(Openable::Mailto)
        );
        assert_eq!(check_openable("MAILTO:a@b.co"), Ok(Openable::Mailto));
        for bad in [
            "javascript:alert(1)",
            "file:///C:/x",
            "calc.exe",
            "C:\\Windows\\System32\\calc.exe",
            "https://user:pw@example.com",
            "mailto:a@b.co?x=\"><script>",
            "mailto:a@b.co?x=%zz",
            "mailto:a b@c.co",
            "",
        ] {
            assert!(check_openable(bad).is_err(), "{bad:?}");
        }
        let long = format!("mailto:a@b.co?body={}", "a".repeat(MAX_MAILTO_LEN));
        assert_eq!(check_openable(&long), Err(UrlError::TooLong));
    }

    #[test]
    fn error_messages_are_human() {
        assert!(UrlError::NotHttp("javascript".into())
            .to_string()
            .contains("javascript"));
        assert!(UrlError::Credentials.to_string().contains("password"));
        assert!(UrlError::NotHttp(String::new())
            .to_string()
            .contains("https://"));
    }
}
