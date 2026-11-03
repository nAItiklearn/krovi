//! Input limits for UI-owned collections the backend persists on the UI's behalf
//! (file shelves, world clocks). The commands validate with these before touching
//! state, so a buggy or hostile webview payload cannot bloat the database.

pub const MAX_SHELVES: usize = 32;
pub const MAX_SHELF_ITEMS: usize = 1_000;
pub const MAX_SHELF_NAME: usize = 80;
pub const MAX_PATH: usize = 32_767; // Windows extended-length path limit
pub const MAX_CLOCKS: usize = 24;
pub const MAX_CLOCK_LABEL: usize = 60;
pub const MAX_TZ: usize = 64;

/// A shelf name: non-blank, no control characters, bounded.
pub fn valid_shelf_name(name: &str) -> bool {
    let t = name.trim();
    !t.is_empty() && t.chars().count() <= MAX_SHELF_NAME && !t.chars().any(char::is_control)
}

/// A parked item's display name and path: bounded, no control characters.
pub fn valid_shelf_item(name: &str, path: &str) -> bool {
    name.chars().count() <= 260
        && path.chars().count() <= MAX_PATH
        && !(name.trim().is_empty() && path.trim().is_empty())
        && !name.chars().any(char::is_control)
        && !path.chars().any(char::is_control)
}

/// An IANA zone id ("Asia/Kolkata", "America/Argentina/Buenos_Aires", "UTC",
/// "Etc/GMT+5") or empty (= the machine's local zone). Shape check only: the webview's
/// `Intl` decides whether the zone exists.
pub fn valid_tz(tz: &str) -> bool {
    tz.is_empty()
        || (tz.len() <= MAX_TZ
            && tz
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '_' | '-' | '+'))
            && !tz.starts_with('/')
            && !tz.contains("//"))
}

pub fn valid_clock_label(label: &str) -> bool {
    let t = label.trim();
    !t.is_empty() && t.chars().count() <= MAX_CLOCK_LABEL && !t.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shelf_names() {
        assert!(valid_shelf_name("Desk"));
        assert!(valid_shelf_name("  Work stuff "));
        assert!(!valid_shelf_name("   "));
        assert!(!valid_shelf_name("a\nb"));
        assert!(!valid_shelf_name(&"x".repeat(MAX_SHELF_NAME + 1)));
    }

    #[test]
    fn shelf_items() {
        assert!(valid_shelf_item("a.pdf", r"C:\Users\me\a.pdf"));
        assert!(valid_shelf_item("", r"C:\x"));
        assert!(!valid_shelf_item("", " "));
        assert!(!valid_shelf_item("a\u{0}", r"C:\x"));
        assert!(!valid_shelf_item("a", &"p".repeat(MAX_PATH + 1)));
    }

    #[test]
    fn time_zones() {
        for ok in [
            "",
            "UTC",
            "Asia/Kolkata",
            "America/Argentina/Buenos_Aires",
            "Etc/GMT+5",
            "Etc/GMT-14",
        ] {
            assert!(valid_tz(ok), "{ok}");
        }
        let long = "A".repeat(MAX_TZ + 1);
        for bad in [
            "/etc/passwd",
            "Asia//Kolkata",
            "Asia Kolkata",
            "<script>",
            long.as_str(),
        ] {
            assert!(!valid_tz(bad), "{bad}");
        }
    }

    #[test]
    fn clock_labels() {
        assert!(valid_clock_label("Mumbai"));
        assert!(!valid_clock_label(""));
        assert!(!valid_clock_label(&"m".repeat(MAX_CLOCK_LABEL + 1)));
    }
}
