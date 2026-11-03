//! Saved text snippets: a name, an optional trigger keyword and a body.
//!
//! Local only, and deliberately small:
//!
//! * Stored by the host as a JSON array under [`SNIPPETS_KEY`] in Krovi's own settings table.
//! * Used on purpose: you pick one in the palette or the Snippets tab and press Enter. It is
//!   copied to the clipboard. Krovi does not watch your typing, has no global keyboard hook and
//!   never expands anything by itself; a "trigger" is only a keyword that finds the snippet in
//!   the palette.
//! * Three placeholders are filled in at the moment of use: `{date}` (`2026-11-03`), `{time}`
//!   (`14:05`, 24-hour, this PC's clock) and `{clipboard}` (what is on the clipboard right now).
//!   Everything else is literal text, and a substituted value is never expanded again.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Settings key the host persists the snippets under (a JSON array of [`Snippet`]).
pub const SNIPPETS_KEY: &str = "snippets.v1";
pub const MAX_SNIPPETS: usize = 200;
pub const MAX_NAME_CHARS: usize = 60;
pub const MAX_TRIGGER_CHARS: usize = 24;
pub const MAX_BODY_CHARS: usize = 8000;
/// The most clipboard text one `{clipboard}` inserts.
pub const MAX_CLIPBOARD_CHARS: usize = 20_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snippet {
    /// Assigned on first save; leave empty for a new snippet.
    #[serde(default)]
    pub id: String,
    pub name: String,
    /// Optional keyword that finds the snippet in the palette (for example `;sig`).
    #[serde(default)]
    pub trigger: String,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnippetError {
    NameEmpty,
    NameTooLong,
    NameBad,
    NameTaken,
    TriggerTooLong,
    TriggerBad,
    TriggerTaken,
    BodyEmpty,
    BodyTooLong,
    BodyBad,
    TooMany,
}

impl fmt::Display for SnippetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SnippetError::NameEmpty => write!(f, "Give the snippet a name."),
            SnippetError::NameTooLong => {
                write!(
                    f,
                    "That name is too long (over {MAX_NAME_CHARS} characters)."
                )
            }
            SnippetError::NameBad => write!(f, "The name cannot contain control characters."),
            SnippetError::NameTaken => write!(f, "A snippet with that name already exists."),
            SnippetError::TriggerTooLong => {
                write!(
                    f,
                    "That keyword is too long (over {MAX_TRIGGER_CHARS} characters)."
                )
            }
            SnippetError::TriggerBad => {
                write!(f, "A keyword cannot contain spaces or control characters.")
            }
            SnippetError::TriggerTaken => write!(f, "Another snippet already uses that keyword."),
            SnippetError::BodyEmpty => write!(f, "The snippet has no text."),
            SnippetError::BodyTooLong => {
                write!(
                    f,
                    "That text is too long (over {MAX_BODY_CHARS} characters)."
                )
            }
            SnippetError::BodyBad => write!(
                f,
                "The text contains a control character Krovi cannot store."
            ),
            SnippetError::TooMany => write!(f, "At most {MAX_SNIPPETS} snippets can be saved."),
        }
    }
}

impl std::error::Error for SnippetError {}

fn has_control(s: &str) -> bool {
    s.chars().any(char::is_control)
}

/// Check one snippet and return it tidied (name spacing, keyword trimmed, line endings `\n`).
/// The id is left as given.
pub fn validate_snippet(s: &Snippet) -> Result<Snippet, SnippetError> {
    let name = s.name.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        return Err(SnippetError::NameEmpty);
    }
    if has_control(&s.name) {
        return Err(SnippetError::NameBad);
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(SnippetError::NameTooLong);
    }
    let trigger = s.trigger.trim().to_string();
    if trigger.chars().count() > MAX_TRIGGER_CHARS {
        return Err(SnippetError::TriggerTooLong);
    }
    if trigger.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(SnippetError::TriggerBad);
    }
    let body = s.body.replace("\r\n", "\n").replace('\r', "\n");
    if body.trim().is_empty() {
        return Err(SnippetError::BodyEmpty);
    }
    if body
        .chars()
        .any(|c| c.is_control() && c != '\n' && c != '\t')
    {
        return Err(SnippetError::BodyBad);
    }
    if body.chars().count() > MAX_BODY_CHARS {
        return Err(SnippetError::BodyTooLong);
    }
    Ok(Snippet {
        id: s.id.clone(),
        name,
        trigger,
        body,
    })
}

fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let t = out.trim_matches('-');
    let t = if t.is_empty() { "snippet" } else { t };
    t.chars()
        .take(32)
        .collect::<String>()
        .trim_end_matches('-')
        .to_string()
}

/// The user's snippets. Order is insertion order.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Snippets {
    items: Vec<Snippet>,
}

impl Snippets {
    pub fn new() -> Self {
        Self::default()
    }

    /// Load the stored values. Lenient on purpose: an entry that no longer validates, or repeats an
    /// id, name or keyword, is dropped, so a hand-edited or older value can never wedge the feature.
    pub fn from_values(values: Vec<serde_json::Value>) -> Self {
        let mut out = Snippets::new();
        for v in values {
            let Ok(item) = serde_json::from_value::<Snippet>(v) else {
                continue;
            };
            if item.id.is_empty() || out.items.iter().any(|s| s.id == item.id) {
                continue;
            }
            let Ok(clean) = validate_snippet(&item) else {
                continue;
            };
            if out.clash(&clean, None).is_some() || out.items.len() >= MAX_SNIPPETS {
                continue;
            }
            out.items.push(clean);
        }
        out
    }

    pub fn list(&self) -> &[Snippet] {
        &self.items
    }

    pub fn get(&self, id: &str) -> Option<&Snippet> {
        self.items.iter().find(|s| s.id == id)
    }

    fn clash(&self, s: &Snippet, except: Option<&str>) -> Option<SnippetError> {
        for o in &self.items {
            if Some(o.id.as_str()) == except {
                continue;
            }
            if o.name.to_lowercase() == s.name.to_lowercase() {
                return Some(SnippetError::NameTaken);
            }
            if !s.trigger.is_empty() && o.trigger.to_lowercase() == s.trigger.to_lowercase() {
                return Some(SnippetError::TriggerTaken);
            }
        }
        None
    }

    fn fresh_id(&self, name: &str) -> String {
        let base = format!("sn-{}", slug(name));
        let mut id = base.clone();
        let mut n = 2;
        while self.items.iter().any(|s| s.id == id) {
            id = format!("{base}-{n}");
            n += 1;
        }
        id
    }

    /// Add a snippet, or update the one with the same id. Returns the stored snippet.
    pub fn upsert(&mut self, input: Snippet) -> Result<Snippet, SnippetError> {
        let clean = validate_snippet(&input)?;
        let existing = (!input.id.is_empty())
            .then(|| input.id.clone())
            .filter(|id| self.get(id).is_some());
        if let Some(e) = self.clash(&clean, existing.as_deref()) {
            return Err(e);
        }
        match existing {
            Some(id) => {
                let stored = Snippet {
                    id: id.clone(),
                    ..clean
                };
                if let Some(slot) = self.items.iter_mut().find(|s| s.id == id) {
                    *slot = stored.clone();
                }
                Ok(stored)
            }
            None => {
                if self.items.len() >= MAX_SNIPPETS {
                    return Err(SnippetError::TooMany);
                }
                let stored = Snippet {
                    id: self.fresh_id(&clean.name),
                    ..clean
                };
                self.items.push(stored.clone());
                Ok(stored)
            }
        }
    }

    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.items.len();
        self.items.retain(|s| s.id != id);
        self.items.len() != before
    }
}

// ---------------------------------------------------------------------------------------------
// placeholders
// ---------------------------------------------------------------------------------------------

/// What the placeholders are filled with. The host passes the clock and the clipboard in, so this
/// stays pure.
#[derive(Debug, Clone, Copy)]
pub struct ExpandCtx<'a> {
    pub utc_ms: i64,
    /// Local offset from UTC, in minutes (+330 for India).
    pub offset_minutes: i32,
    /// The clipboard text, if there is any. Only needed when [`uses_clipboard`] is true.
    pub clipboard: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expanded {
    pub text: String,
    /// `{clipboard}` was used but there was no text on the clipboard (it became nothing).
    pub clipboard_empty: bool,
    /// The clipboard text was cut to [`MAX_CLIPBOARD_CHARS`].
    pub clipboard_truncated: bool,
}

/// Does the body ask for the clipboard? The host reads the clipboard only when this is true.
pub fn uses_clipboard(body: &str) -> bool {
    body.contains("{clipboard}")
}

/// Days since 1970-01-01 -> (year, month, day). Howard Hinnant's `civil_from_days`.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// `2026-11-03` and `14:05` for the local time of `utc_ms`.
pub fn local_date_time(utc_ms: i64, offset_minutes: i32) -> (String, String) {
    let local = utc_ms + i64::from(offset_minutes) * 60_000;
    let days = local.div_euclid(86_400_000);
    let minutes = local.rem_euclid(86_400_000) / 60_000;
    let (y, m, d) = civil_from_days(days);
    (
        format!("{y:04}-{m:02}-{d:02}"),
        format!("{:02}:{:02}", minutes / 60, minutes % 60),
    )
}

/// Fill in `{date}`, `{time}` and `{clipboard}`. One pass: a value that was substituted is not
/// scanned again, so a clipboard that happens to contain `{date}` stays as it is.
pub fn expand(body: &str, ctx: &ExpandCtx<'_>) -> Expanded {
    let (date, time) = local_date_time(ctx.utc_ms, ctx.offset_minutes);
    let mut out = String::with_capacity(body.len() + 16);
    let mut clipboard_empty = false;
    let mut clipboard_truncated = false;
    let mut rest = body;
    while let Some(i) = rest.find('{') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        if let Some(r) = tail.strip_prefix("{date}") {
            out.push_str(&date);
            rest = r;
        } else if let Some(r) = tail.strip_prefix("{time}") {
            out.push_str(&time);
            rest = r;
        } else if let Some(r) = tail.strip_prefix("{clipboard}") {
            match ctx.clipboard.filter(|c| !c.is_empty()) {
                None => clipboard_empty = true,
                Some(c) => {
                    if c.chars().count() > MAX_CLIPBOARD_CHARS {
                        clipboard_truncated = true;
                        out.extend(c.chars().take(MAX_CLIPBOARD_CHARS));
                    } else {
                        out.push_str(c);
                    }
                }
            }
            rest = r;
        } else {
            out.push('{');
            rest = &tail[1..];
        }
    }
    out.push_str(rest);
    Expanded {
        text: out,
        clipboard_empty,
        clipboard_truncated,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn snip(name: &str, trigger: &str, body: &str) -> Snippet {
        Snippet {
            id: String::new(),
            name: name.into(),
            trigger: trigger.into(),
            body: body.into(),
        }
    }

    // 2023-11-14T22:13:20Z
    const T: i64 = 1_700_000_000_000;

    fn ctx(clip: Option<&str>) -> ExpandCtx<'_> {
        ExpandCtx {
            utc_ms: T,
            offset_minutes: 0,
            clipboard: clip,
        }
    }

    #[test]
    fn dates_and_times_follow_the_local_offset() {
        assert_eq!(local_date_time(0, 0), ("1970-01-01".into(), "00:00".into()));
        assert_eq!(local_date_time(T, 0), ("2023-11-14".into(), "22:13".into()));
        // India: +05:30 rolls over midnight
        assert_eq!(
            local_date_time(T, 330),
            ("2023-11-15".into(), "03:43".into())
        );
        // US Pacific: -08:00 stays on the 14th
        assert_eq!(
            local_date_time(T, -480),
            ("2023-11-14".into(), "14:13".into())
        );
        // a leap day
        assert_eq!(local_date_time(951_782_400_000, 0).0, "2000-02-29");
        // before 1970
        assert_eq!(
            local_date_time(-1, 0),
            ("1969-12-31".into(), "23:59".into())
        );
    }

    #[test]
    fn the_three_placeholders_are_filled_and_everything_else_is_literal() {
        let e = expand(
            "On {date} at {time}: {clipboard}! {name} {Date} {",
            &ctx(Some("hello")),
        );
        assert_eq!(e.text, "On 2023-11-14 at 22:13: hello! {name} {Date} {");
        assert!(!e.clipboard_empty && !e.clipboard_truncated);
    }

    #[test]
    fn a_substituted_value_is_not_expanded_again() {
        let e = expand("{clipboard}", &ctx(Some("{date} {time} {clipboard}")));
        assert_eq!(e.text, "{date} {time} {clipboard}");
    }

    #[test]
    fn an_empty_or_missing_clipboard_becomes_nothing_and_is_reported() {
        let e = expand("a{clipboard}b", &ctx(None));
        assert_eq!((e.text.as_str(), e.clipboard_empty), ("ab", true));
        let e = expand("a{clipboard}b", &ctx(Some("")));
        assert_eq!((e.text.as_str(), e.clipboard_empty), ("ab", true));
        // no clipboard placeholder, no report
        assert!(!expand("plain", &ctx(None)).clipboard_empty);
    }

    #[test]
    fn a_huge_clipboard_is_cut_and_reported() {
        let big = "x".repeat(MAX_CLIPBOARD_CHARS + 50);
        let e = expand("{clipboard}", &ctx(Some(&big)));
        assert_eq!(e.text.chars().count(), MAX_CLIPBOARD_CHARS);
        assert!(e.clipboard_truncated);
    }

    #[test]
    fn multibyte_text_survives_expansion() {
        let e = expand("héllo {date} — ✓ {clipboard}", &ctx(Some("日本")));
        assert_eq!(e.text, "héllo 2023-11-14 — ✓ 日本");
    }

    #[test]
    fn the_clipboard_is_only_needed_when_asked_for() {
        assert!(uses_clipboard("x {clipboard} y"));
        assert!(!uses_clipboard("x {date} y"));
        assert!(!uses_clipboard("{Clipboard}"));
    }

    #[test]
    fn validation_tidies_and_refuses() {
        let ok = validate_snippet(&snip("  Email   sign-off ", " ;sig ", "Thanks,\r\nA")).unwrap();
        assert_eq!(ok.name, "Email sign-off");
        assert_eq!(ok.trigger, ";sig");
        assert_eq!(ok.body, "Thanks,\nA");

        assert_eq!(
            validate_snippet(&snip("", "", "x")),
            Err(SnippetError::NameEmpty)
        );
        assert_eq!(
            validate_snippet(&snip("a\u{7}b", "", "x")),
            Err(SnippetError::NameBad)
        );
        assert_eq!(
            validate_snippet(&snip(&"n".repeat(MAX_NAME_CHARS + 1), "", "x")),
            Err(SnippetError::NameTooLong)
        );
        assert_eq!(
            validate_snippet(&snip("a", "has space", "x")),
            Err(SnippetError::TriggerBad)
        );
        assert_eq!(
            validate_snippet(&snip("a", &"t".repeat(MAX_TRIGGER_CHARS + 1), "x")),
            Err(SnippetError::TriggerTooLong)
        );
        assert_eq!(
            validate_snippet(&snip("a", "", "   \n ")),
            Err(SnippetError::BodyEmpty)
        );
        assert_eq!(
            validate_snippet(&snip("a", "", "bad\u{0}nul")),
            Err(SnippetError::BodyBad)
        );
        assert_eq!(
            validate_snippet(&snip("a", "", &"b".repeat(MAX_BODY_CHARS + 1))),
            Err(SnippetError::BodyTooLong)
        );
        // an empty keyword is fine, tabs and new lines in the text are fine
        assert!(validate_snippet(&snip("a", "", "line1\n\tline2")).is_ok());
    }

    #[test]
    fn upsert_assigns_ids_and_keeps_names_and_keywords_unique() {
        let mut s = Snippets::new();
        let a = s.upsert(snip("Sign off", ";sig", "Thanks")).unwrap();
        assert_eq!(a.id, "sn-sign-off");
        let b = s.upsert(snip("Sign-off!", ";sig2", "Best")).unwrap();
        assert_eq!(b.id, "sn-sign-off-2");
        assert_eq!(
            s.upsert(snip("SIGN OFF", "", "x")),
            Err(SnippetError::NameTaken)
        );
        assert_eq!(
            s.upsert(snip("Other", ";SIG", "x")),
            Err(SnippetError::TriggerTaken)
        );

        // editing keeps the id and may keep its own name and keyword
        let edited = s
            .upsert(Snippet {
                id: a.id.clone(),
                ..snip("Sign off", ";sig", "Many thanks")
            })
            .unwrap();
        assert_eq!(edited.id, a.id);
        assert_eq!(s.get(&a.id).unwrap().body, "Many thanks");
        assert_eq!(s.list().len(), 2);

        // an id that is not stored is treated as a new snippet, never trusted
        let c = s
            .upsert(Snippet {
                id: "evil".into(),
                ..snip("Third", "", "x")
            })
            .unwrap();
        assert_eq!(c.id, "sn-third");

        assert!(s.remove(&b.id));
        assert!(!s.remove(&b.id));
        assert_eq!(s.list().len(), 2);
    }

    #[test]
    fn the_list_is_capped() {
        let mut s = Snippets::new();
        for n in 0..MAX_SNIPPETS {
            s.upsert(snip(&format!("snippet {n}"), "", "x")).unwrap();
        }
        assert_eq!(
            s.upsert(snip("one more", "", "x")),
            Err(SnippetError::TooMany)
        );
    }

    #[test]
    fn loading_drops_bad_duplicate_and_idless_entries() {
        let s = Snippets::from_values(vec![
            json!({"id": "sn-a", "name": "A", "trigger": ";a", "body": "x"}),
            json!({"id": "sn-a", "name": "A again", "body": "x"}), // repeated id
            json!({"id": "sn-b", "name": "a", "body": "x"}),       // repeated name
            json!({"id": "sn-c", "name": "C", "trigger": ";A", "body": "x"}), // repeated keyword
            json!({"id": "", "name": "No id", "body": "x"}),
            json!({"id": "sn-d", "name": "", "body": "x"}), // invalid
            json!({"id": "sn-e", "name": "E"}),             // no body
            json!("not an object"),
            json!({"id": "sn-f", "name": "F", "body": "ok"}),
        ]);
        let ids: Vec<&str> = s.list().iter().map(|x| x.id.as_str()).collect();
        assert_eq!(ids, vec!["sn-a", "sn-f"]);
    }

    #[test]
    fn the_wire_shape_is_camel_case_json() {
        let v = serde_json::to_value(snip("A", ";a", "x")).unwrap();
        assert_eq!(
            v,
            json!({"id": "", "name": "A", "trigger": ";a", "body": "x"})
        );
    }
}
