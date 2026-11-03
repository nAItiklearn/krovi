//! The built-in table of well-known sites. "Open youtube" and "search github for krovi" resolve
//! here, so those URLs come from this file and never from user text: opening a known site is
//! `Safe`, anything the user types as an address is not.

/// A known website.
#[derive(Debug)]
pub struct Site {
    /// Stable id ("youtube").
    pub key: &'static str,
    /// Display name ("YouTube").
    pub name: &'static str,
    /// Short host label for summaries ("youtube.com").
    pub label: &'static str,
    /// Where "open <site>" goes.
    pub url: &'static str,
    /// Hosts that belong to this site (lower case, without `www.`).
    pub hosts: &'static [&'static str],
    /// Spoken names (lower case, space separated).
    pub aliases: &'static [&'static str],
    /// Search URL with a `{q}` slot for the percent-encoded query.
    pub search: Option<&'static str>,
    /// "<name> <query>" works as a search command ("google rust lifetimes").
    pub verb_search: bool,
}

pub static SITES: [Site; 20] = [
    Site {
        key: "youtube",
        name: "YouTube",
        label: "youtube.com",
        url: "https://www.youtube.com/",
        hosts: &["youtube.com", "youtu.be", "m.youtube.com"],
        aliases: &["youtube", "you tube", "yt"],
        search: Some("https://www.youtube.com/results?search_query={q}"),
        verb_search: true,
    },
    Site {
        key: "google",
        name: "Google",
        label: "google.com",
        url: "https://www.google.com/",
        hosts: &["google.com"],
        aliases: &["google"],
        search: Some("https://www.google.com/search?q={q}"),
        verb_search: true,
    },
    Site {
        key: "bing",
        name: "Bing",
        label: "bing.com",
        url: "https://www.bing.com/",
        hosts: &["bing.com"],
        aliases: &["bing"],
        search: Some("https://www.bing.com/search?q={q}"),
        verb_search: true,
    },
    Site {
        key: "duckduckgo",
        name: "DuckDuckGo",
        label: "duckduckgo.com",
        url: "https://duckduckgo.com/",
        hosts: &["duckduckgo.com"],
        aliases: &["duckduckgo", "duck duck go", "ddg"],
        search: Some("https://duckduckgo.com/?q={q}"),
        verb_search: true,
    },
    Site {
        key: "wikipedia",
        name: "Wikipedia",
        label: "wikipedia.org",
        url: "https://www.wikipedia.org/",
        hosts: &["wikipedia.org", "en.wikipedia.org"],
        aliases: &["wikipedia", "wiki"],
        search: Some("https://en.wikipedia.org/w/index.php?search={q}"),
        verb_search: true,
    },
    Site {
        key: "github",
        name: "GitHub",
        label: "github.com",
        url: "https://github.com/",
        hosts: &["github.com"],
        aliases: &["github", "git hub"],
        search: Some("https://github.com/search?q={q}"),
        verb_search: false,
    },
    Site {
        key: "gmail",
        name: "Gmail",
        label: "mail.google.com",
        url: "https://mail.google.com/",
        hosts: &["mail.google.com", "gmail.com"],
        aliases: &["gmail", "google mail"],
        search: None,
        verb_search: false,
    },
    Site {
        key: "outlook",
        name: "Outlook",
        label: "outlook.live.com",
        url: "https://outlook.live.com/mail/",
        hosts: &["outlook.live.com", "outlook.com"],
        aliases: &["outlook"],
        search: None,
        verb_search: false,
    },
    Site {
        key: "reddit",
        name: "Reddit",
        label: "reddit.com",
        url: "https://www.reddit.com/",
        hosts: &["reddit.com"],
        aliases: &["reddit"],
        search: Some("https://www.reddit.com/search/?q={q}"),
        verb_search: false,
    },
    Site {
        key: "x",
        name: "X",
        label: "x.com",
        url: "https://x.com/",
        hosts: &["x.com", "twitter.com"],
        aliases: &["x", "twitter"],
        search: Some("https://x.com/search?q={q}"),
        verb_search: false,
    },
    Site {
        key: "linkedin",
        name: "LinkedIn",
        label: "linkedin.com",
        url: "https://www.linkedin.com/",
        hosts: &["linkedin.com"],
        aliases: &["linkedin", "linked in"],
        search: Some("https://www.linkedin.com/search/results/all/?keywords={q}"),
        verb_search: false,
    },
    Site {
        key: "chatgpt",
        name: "ChatGPT",
        label: "chatgpt.com",
        url: "https://chatgpt.com/",
        hosts: &["chatgpt.com"],
        aliases: &["chatgpt", "chat gpt"],
        search: None,
        verb_search: false,
    },
    Site {
        key: "stackoverflow",
        name: "Stack Overflow",
        label: "stackoverflow.com",
        url: "https://stackoverflow.com/",
        hosts: &["stackoverflow.com"],
        aliases: &["stackoverflow", "stack overflow"],
        search: Some("https://stackoverflow.com/search?q={q}"),
        verb_search: false,
    },
    Site {
        key: "netflix",
        name: "Netflix",
        label: "netflix.com",
        url: "https://www.netflix.com/",
        hosts: &["netflix.com"],
        aliases: &["netflix"],
        search: None,
        verb_search: false,
    },
    Site {
        key: "spotify",
        name: "Spotify",
        label: "open.spotify.com",
        url: "https://open.spotify.com/",
        hosts: &["open.spotify.com", "spotify.com"],
        aliases: &["spotify"],
        search: Some("https://open.spotify.com/search/{q}"),
        verb_search: false,
    },
    Site {
        key: "maps",
        name: "Google Maps",
        label: "google.com/maps",
        url: "https://www.google.com/maps",
        hosts: &["maps.google.com"],
        aliases: &["google maps", "maps"],
        search: Some("https://www.google.com/maps/search/{q}"),
        verb_search: false,
    },
    Site {
        key: "drive",
        name: "Google Drive",
        label: "drive.google.com",
        url: "https://drive.google.com/",
        hosts: &["drive.google.com"],
        aliases: &["google drive"],
        search: None,
        verb_search: false,
    },
    Site {
        key: "whatsapp",
        name: "WhatsApp Web",
        label: "web.whatsapp.com",
        url: "https://web.whatsapp.com/",
        hosts: &["web.whatsapp.com", "whatsapp.com"],
        aliases: &["whatsapp", "whats app", "whatsapp web"],
        search: None,
        verb_search: false,
    },
    Site {
        key: "notion",
        name: "Notion",
        label: "notion.so",
        url: "https://www.notion.so/",
        hosts: &["notion.so"],
        aliases: &["notion"],
        search: None,
        verb_search: false,
    },
    Site {
        key: "docs",
        name: "Google Docs",
        label: "docs.google.com",
        url: "https://docs.google.com/",
        hosts: &["docs.google.com"],
        aliases: &["google docs"],
        search: None,
        verb_search: false,
    },
];

/// The site with this key.
pub fn get(key: &str) -> Option<&'static Site> {
    SITES.iter().find(|s| s.key == key)
}

/// The site whose spoken name starts `words` (lower case), with how many words it used. The
/// longest name wins, so "google maps pizza" is Google Maps, not Google.
pub fn find_alias(words: &[&str]) -> Option<(&'static Site, usize)> {
    let mut best: Option<(&'static Site, usize)> = None;
    for site in &SITES {
        for alias in site.aliases {
            let parts: Vec<&str> = alias.split(' ').collect();
            let longer = match best {
                None => true,
                Some((_, n)) => parts.len() > n,
            };
            if longer && words.len() >= parts.len() && words[..parts.len()] == parts[..] {
                best = Some((site, parts.len()));
            }
        }
    }
    best
}

/// The known site for a host name (`www.` ignored, case ignored).
pub fn find_host(host: &str) -> Option<&'static Site> {
    let h = host.to_ascii_lowercase();
    let h = h.strip_prefix("www.").unwrap_or(&h);
    SITES.iter().find(|s| s.hosts.contains(&h))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::url;

    #[test]
    fn table_is_well_formed() {
        let mut keys = std::collections::HashSet::new();
        for s in &SITES {
            assert!(keys.insert(s.key), "duplicate key {}", s.key);
            // every built-in URL passes the same validation as a typed one
            let v = url::validate_full(s.url).unwrap_or_else(|e| panic!("{}: {e}", s.key));
            assert!(v.https, "{} must be https", s.key);
            for a in s.aliases {
                assert_eq!(*a, a.to_lowercase(), "alias must be lower case: {a}");
            }
            if let Some(t) = s.search {
                assert_eq!(t.matches("{q}").count(), 1, "{}: one {{q}} slot", s.key);
                let probe = t.replace("{q}", "x");
                assert!(url::validate_full(&probe).is_ok(), "{}: {probe}", s.key);
            }
            if s.verb_search {
                assert!(s.search.is_some(), "{} verb search needs a template", s.key);
            }
        }
        assert!(SITES.len() >= 18, "keep the table useful");
    }

    #[test]
    fn longest_alias_wins() {
        let (s, n) = find_alias(&["google", "maps", "pizza"]).unwrap();
        assert_eq!((s.key, n), ("maps", 2));
        let (s, n) = find_alias(&["google", "rust"]).unwrap();
        assert_eq!((s.key, n), ("google", 1));
        let (s, n) = find_alias(&["duck", "duck", "go", "x"]).unwrap();
        assert_eq!((s.key, n), ("duckduckgo", 3));
        assert!(find_alias(&["notepad"]).is_none());
        assert!(find_alias(&[]).is_none());
        assert_eq!(find_alias(&["twitter"]).unwrap().0.key, "x");
    }

    #[test]
    fn hosts_resolve_with_or_without_www() {
        assert_eq!(find_host("www.YouTube.com").unwrap().key, "youtube");
        assert_eq!(find_host("github.com").unwrap().key, "github");
        assert_eq!(find_host("twitter.com").unwrap().key, "x");
        assert_eq!(find_host("mail.google.com").unwrap().key, "gmail");
        assert!(find_host("github.com.evil.example").is_none());
        assert!(find_host("evilgithub.com").is_none());
    }

    #[test]
    fn get_by_key() {
        assert_eq!(get("youtube").unwrap().name, "YouTube");
        assert!(get("nope").is_none());
    }
}
