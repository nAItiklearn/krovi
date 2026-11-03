//! Opening a small, fixed set of web addresses in the user's default browser.
//!
//! Two things Krovi ever opens for Circle to Ask, both only when the user clicks a button:
//!
//! * a Google search for text the user selected ([`google_search_url`]), and
//! * the page where the user creates an API key for the vision provider they chose
//!   ([`keys_page`]).
//!
//! The webview cannot name a URL. The address is built here from the text, and
//! [`open_url`] refuses anything that is not exactly one of those shapes, so a compromised
//! page cannot use it to launch a file, a program or an arbitrary site. Krovi itself sends
//! nothing: the browser makes the request, as if the user had typed the address.

/// Prefix of every search address Krovi opens.
pub const GOOGLE_SEARCH_BASE: &str = "https://www.google.com/search?q=";
/// Longest search text, in characters. Longer selections are cut (the UI says so).
pub const MAX_QUERY_CHARS: usize = 300;

/// Where each supported vision provider's API-key page is. Opened only from the "Get a key"
/// button in settings. These are fixed, public pages; nothing is sent to them.
const KEYS_PAGES: &[(&str, &str)] = &[
    ("openai", "https://platform.openai.com/api-keys"),
    ("anthropic", "https://console.anthropic.com/settings/keys"),
    ("gemini", "https://aistudio.google.com/apikey"),
];

/// The API-key page for a provider id (`openai`, `anthropic`, `gemini`).
pub fn keys_page(provider: &str) -> Option<&'static str> {
    KEYS_PAGES
        .iter()
        .find(|(id, _)| *id == provider)
        .map(|(_, url)| *url)
}

/// The text to search for: control characters dropped, runs of whitespace collapsed to one
/// space, cut to [`MAX_QUERY_CHARS`] characters on a character boundary. `None` if nothing
/// is left.
pub fn search_query(text: &str) -> Option<String> {
    let mut out = String::new();
    let mut pending_space = false;
    let mut count = 0usize;
    for c in text.chars() {
        if c.is_whitespace() {
            pending_space = !out.is_empty();
            continue;
        }
        if c.is_control() {
            continue;
        }
        if count >= MAX_QUERY_CHARS {
            break;
        }
        if pending_space {
            out.push(' ');
            count += 1;
            pending_space = false;
            if count >= MAX_QUERY_CHARS {
                break;
            }
        }
        out.push(c);
        count += 1;
    }
    let out = out.trim_end().to_string();
    (!out.is_empty()).then_some(out)
}

/// RFC 3986 percent-encoding of the UTF-8 bytes of `s`; only `A-Z a-z 0-9 - . _ ~` stay.
pub fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The Google search address for `text`, or `None` when there is nothing to search for.
pub fn google_search_url(text: &str) -> Option<String> {
    let q = search_query(text)?;
    Some(format!("{GOOGLE_SEARCH_BASE}{}", percent_encode(&q)))
}

/// Is `url` exactly one of the shapes Krovi opens: a search built by [`google_search_url`]
/// (only an encoded query, no extra parameters) or a provider key page?
pub fn is_allowed_url(url: &str) -> bool {
    if KEYS_PAGES.iter().any(|(_, u)| *u == url) {
        return true;
    }
    match url.strip_prefix(GOOGLE_SEARCH_BASE) {
        Some(rest) => {
            !rest.is_empty()
                && rest
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~' | b'%'))
        }
        None => false,
    }
}

/// Open `url` in the default browser if (and only if) [`is_allowed_url`] accepts it.
pub fn open_url(url: &str) -> Result<(), String> {
    if !is_allowed_url(url) {
        return Err("That address isn't one Krovi opens.".into());
    }
    imp::open(url)
}

#[cfg(all(windows, feature = "native"))]
mod imp {
    pub fn open(url: &str) -> Result<(), String> {
        use windows::core::{w, HSTRING};
        use windows::Win32::UI::Shell::ShellExecuteW;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        // SAFETY: a plain ShellExecute "open" of an https URL that passed the allow-list.
        let h = unsafe {
            ShellExecuteW(
                None,
                w!("open"),
                &HSTRING::from(url),
                None,
                None,
                SW_SHOWNORMAL,
            )
        };
        // ShellExecute reports success with a value greater than 32.
        if h.0 as isize > 32 {
            Ok(())
        } else {
            Err("Windows could not open your browser.".into())
        }
    }
}

#[cfg(not(all(windows, feature = "native")))]
mod imp {
    pub fn open(_url: &str) -> Result<(), String> {
        Err("Opening the browser needs Windows.".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queries_are_cleaned_and_encoded() {
        assert_eq!(
            google_search_url("  Rust   lifetimes\nexplained \t"),
            Some("https://www.google.com/search?q=Rust%20lifetimes%20explained".into())
        );
        assert_eq!(
            google_search_url("a&b=c?d#e"),
            Some("https://www.google.com/search?q=a%26b%3Dc%3Fd%23e".into())
        );
        assert_eq!(
            google_search_url("café"),
            Some("https://www.google.com/search?q=caf%C3%A9".into())
        );
        assert_eq!(
            google_search_url("a-b_c.d~e"),
            Some("https://www.google.com/search?q=a-b_c.d~e".into())
        );
    }

    #[test]
    fn empty_or_blank_text_has_no_search() {
        assert_eq!(google_search_url(""), None);
        assert_eq!(google_search_url("  \n\t "), None);
        assert_eq!(google_search_url("\u{0}\u{7}"), None);
    }

    #[test]
    fn control_characters_are_dropped() {
        assert_eq!(search_query("he\u{0}llo\u{7}"), Some("hello".into()));
    }

    #[test]
    fn long_text_is_cut_on_a_character_boundary() {
        let long = "é".repeat(1000);
        let q = search_query(&long).unwrap();
        assert_eq!(q.chars().count(), MAX_QUERY_CHARS);
        let words = "word ".repeat(200);
        let q = search_query(&words).unwrap();
        assert!(q.chars().count() <= MAX_QUERY_CHARS);
        assert!(!q.ends_with(' '));
    }

    #[test]
    fn built_urls_are_allowed() {
        let u = google_search_url("hello world & more").unwrap();
        assert!(is_allowed_url(&u));
        for p in ["openai", "anthropic", "gemini"] {
            assert!(is_allowed_url(keys_page(p).unwrap()), "{p}");
        }
        assert_eq!(keys_page("nope"), None);
    }

    #[test]
    fn everything_else_is_refused() {
        for bad in [
            "",
            "https://www.google.com/search?q=",
            "https://www.google.com/search?q=a&hl=en",
            "https://www.google.com/search?q=a b",
            "http://www.google.com/search?q=a",
            "https://www.google.com/search?q=a#frag",
            "https://google.com/search?q=a",
            "https://www.google.com.evil.com/search?q=a",
            "https://evil.com/?https://www.google.com/search?q=a",
            "https://platform.openai.com/api-keys?x=1",
            "https://platform.openai.com/",
            "file:///C:/Windows/System32/calc.exe",
            "calc.exe",
            "javascript:alert(1)",
        ] {
            assert!(!is_allowed_url(bad), "{bad:?}");
            assert!(open_url(bad).is_err(), "{bad:?}");
        }
    }
}
