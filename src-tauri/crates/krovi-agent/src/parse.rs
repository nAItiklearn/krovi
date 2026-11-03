//! The intent parser: a sentence in, [`Intent`]s out. Deterministic English, no LLM, no network.
//!
//! What it understands (see the tests for the exact phrasings):
//!
//! * open a site or app: "open youtube", "go to github.com", "open https://example.com",
//!   "launch notepad", "open spotify"
//! * search: "search google for rust", "google rust lifetimes", "search youtube for lofi",
//!   "play lofi on youtube", "look up borrow checker on wikipedia"
//! * timers: "set a timer for 10 minutes", "timer 90 seconds", "25 minute timer for pasta"
//! * reminders: "remind me in 20 minutes to stretch", "remind me at 5pm to call mom",
//!   "remind me tomorrow at 9 to pay rent"
//! * notes: "note buy milk", "take a note: call the dentist"
//! * email drafts: "email sam@x.com about the trip", "draft an email to sam@x.com in gmail"
//! * several steps in one sentence, joined by "and then" / "then" / "and" / ";"
//!
//! Anything else is [`Intent::Unknown`] with a few example commands.
//!
//! "open X" prefers an installed app only when its name is exactly X (so "open spotify" starts
//! the app if you have it and the web player if you do not, and "open youtube" never launches
//! "YouTube Music"). "launch"/"start"/"run" also accept a close app name; "go to"/"visit" are
//! always websites.

use std::cell::Cell;
use std::collections::HashSet;

use crate::civil::{hhmm, local_to_utc_ms, LocalClock};
use crate::intent::{AppEntry, AppMatch, Intent, MailProvider};
use crate::sites::{self, Site};
use crate::text::{capitalize, clean_payload, lows, parse_duration, preview, tokenize, Tok};
use crate::url;
use crate::when;

/// What the parser needs from the host: the current time, the local UTC offset, and the
/// installed apps ("open spotify" is looked up here).
pub struct ParseContext<'a> {
    pub now_utc_ms: i64,
    /// Minutes east of UTC (IST = 330).
    pub utc_offset_minutes: i32,
    pub apps: &'a [AppEntry],
}

const MAX_INPUT_CHARS: usize = 1000;
const MAX_TOKENS: usize = 150;
/// Step separators considered per sentence.
const MAX_CUTS: usize = 8;
/// Upper bound on single-step parses per sentence (guards pathological "and and and" input).
const PARSE_BUDGET: u32 = 1500;

struct Cx<'a> {
    src: &'a str,
    apps: &'a [AppEntry],
    clock: LocalClock,
    offset: i32,
    budget: Cell<u32>,
}

/// Parse a sentence into one [`Intent`] per step. Never empty: an unintelligible sentence is a
/// single [`Intent::Unknown`] with suggestions.
pub fn parse(input: &str, ctx: &ParseContext<'_>) -> Vec<Intent> {
    if input.chars().count() > MAX_INPUT_CHARS {
        return vec![too_long(input)];
    }
    let toks = tokenize(input);
    if toks.len() > MAX_TOKENS {
        return vec![too_long(input)];
    }
    if toks.is_empty() {
        return vec![Intent::Unknown {
            text: String::new(),
            suggestions: suggestions_for(""),
        }];
    }
    let cx = Cx {
        src: input,
        apps: ctx.apps,
        clock: LocalClock::from_utc_ms(ctx.now_utc_ms, ctx.utc_offset_minutes),
        offset: ctx.utc_offset_minutes,
        budget: Cell::new(PARSE_BUDGET),
    };
    let mut out = parse_sequence(&cx, &toks, true);
    merge_context(&mut out);
    out
}

fn too_long(input: &str) -> Intent {
    Intent::Unknown {
        text: preview(&clean_payload(input), 60),
        suggestions: vec![
            "That is too long for one command. Try one short sentence at a time.".into(),
        ],
    }
}

/// Example commands to offer when a sentence was not understood; tuned by its first word.
pub fn suggestions_for(first: &str) -> Vec<String> {
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    match first {
        "remind" | "reminder" => s(&[
            "remind me in 20 minutes to stretch",
            "remind me at 5pm to call mom",
            "remind me tomorrow at 9 to pay rent",
        ]),
        "note" | "jot" | "take" | "make" => s(&["note buy milk", "take a note: call the dentist"]),
        "email" | "mail" | "draft" | "write" | "compose" | "send" => s(&[
            "email sam@example.com about the trip",
            "draft an email to sam@example.com in gmail about lunch",
        ]),
        "timer" => s(&["set a timer for 10 minutes", "25 minute timer for pasta"]),
        "set" | "start" | "create" | "add" => s(&[
            "set a timer for 10 minutes",
            "remind me in 20 minutes to stretch",
            "note buy milk",
        ]),
        "search" | "google" | "look" | "find" | "play" | "watch" => s(&[
            "search google for rust lifetimes",
            "search youtube for lofi",
            "play lofi on youtube",
        ]),
        "open" | "launch" | "go" | "visit" | "run" => {
            s(&["open youtube", "go to github.com", "launch notepad"])
        }
        _ => s(&[
            "open youtube",
            "search google for rust",
            "set a timer for 10 minutes",
            "remind me in 20 minutes to stretch",
            "note buy milk",
        ]),
    }
}

// ---------------------------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------------------------

fn payload(cx: &Cx<'_>, toks: &[Tok]) -> String {
    match (toks.first(), toks.last()) {
        (Some(a), Some(b)) => clean_payload(&cx.src[a.start..b.end]),
        _ => String::new(),
    }
}

fn unknown(cx: &Cx<'_>, toks: &[Tok]) -> Intent {
    let first = toks.first().map(|t| t.low.as_str()).unwrap_or("");
    Intent::Unknown {
        text: payload(cx, toks),
        suggestions: suggestions_for(first),
    }
}

const PREFIXES: &[&[&str]] = &[
    &["hey", "krovi"],
    &["ok", "krovi"],
    &["okay", "krovi"],
    &["hi", "krovi"],
    &["i", "want", "you", "to"],
    &["i", "would", "like", "you", "to"],
    &["i", "would", "like", "to"],
    &["i'd", "like", "to"],
    &["i", "want", "to"],
    &["i", "need", "to"],
    &["can", "you"],
    &["could", "you"],
    &["would", "you"],
    &["will", "you"],
    &["go", "ahead", "and"],
    &["please"],
    &["krovi"],
    &["let's"],
    &["lets"],
];
const SUFFIXES: &[&[&str]] = &[&["for", "me"], &["thank", "you"], &["please"], &["thanks"]];

/// Drop politeness and addressing: "hey krovi, could you open youtube please".
fn strip_filler(toks: &[Tok]) -> &[Tok] {
    let mut s = toks;
    'outer: loop {
        for p in PREFIXES {
            if s.len() > p.len() && s.iter().zip(p.iter()).all(|(t, w)| t.low == *w) {
                s = &s[p.len()..];
                continue 'outer;
            }
        }
        for q in SUFFIXES {
            let tail = &s[s.len().saturating_sub(q.len())..];
            if s.len() > q.len() && tail.iter().zip(q.iter()).all(|(t, w)| t.low == *w) {
                s = &s[..s.len() - q.len()];
                continue 'outer;
            }
        }
        return s;
    }
}

// ---------------------------------------------------------------------------------------------
// several steps in one sentence
// ---------------------------------------------------------------------------------------------

const VERBS: &[&str] = &[
    "open",
    "launch",
    "start",
    "run",
    "go",
    "goto",
    "visit",
    "browse",
    "navigate",
    "take",
    "pull",
    "bring",
    "load",
    "fire",
    "search",
    "google",
    "bing",
    "duckduckgo",
    "youtube",
    "wikipedia",
    "look",
    "lookup",
    "play",
    "watch",
    "find",
    "set",
    "timer",
    "remind",
    "reminder",
    "note",
    "jot",
    "email",
    "mail",
    "draft",
    "write",
    "compose",
    "send",
    "create",
    "add",
    "make",
    "new",
    "save",
    "schedule",
];

struct Cut {
    left_end: usize,
    right_start: usize,
}

fn cuts(toks: &[Tok]) -> Vec<Cut> {
    let l = lows(toks);
    let mut out = Vec::new();
    for i in 1..toks.len() {
        if out.len() >= MAX_CUTS {
            break;
        }
        let two = |a: &str, b: &str| l[i] == a && l.get(i + 1) == Some(&b);
        let connector = if two("and", "then") || two("after", "that") {
            Some(2)
        } else if (l[i] == "then" && l[i - 1] != "and")
            || (l[i] == "and" && l.get(i + 1) != Some(&"then"))
        {
            Some(1)
        } else {
            None
        };
        match connector {
            Some(len) if i + len < toks.len() => out.push(Cut {
                left_end: i,
                right_start: i + len,
            }),
            Some(_) => {}
            None if toks[i - 1].semi => out.push(Cut {
                left_end: i,
                right_start: i,
            }),
            None => {}
        }
    }
    out
}

fn starts_with_verb(toks: &[Tok]) -> bool {
    let t = strip_filler(toks);
    t.first().is_some_and(|x| VERBS.contains(&x.low.as_str()))
}

/// Free-text intents swallow "and": a note "buy milk and eggs" is one note.
fn is_free_text(i: &Intent) -> bool {
    matches!(
        i,
        Intent::Note { .. }
            | Intent::Reminder { .. }
            | Intent::Email { .. }
            | Intent::Search { .. }
    )
}

/// Not something that can run: a sentence we did not understand, or an app that was not found
/// (or is ambiguous). After free text, such a "follow-up" is taken to be more of the text.
fn is_failed(i: &Intent) -> bool {
    i.is_unknown()
        || matches!(
            i,
            Intent::OpenApp {
                app: AppMatch::NotFound | AppMatch::Ambiguous { .. },
                ..
            }
        )
}

fn parse_sequence(cx: &Cx<'_>, toks: &[Tok], bare_ok: bool) -> Vec<Intent> {
    for cut in cuts(toks) {
        if cx.budget.get() == 0 {
            break;
        }
        let (left, right) = (&toks[..cut.left_end], &toks[cut.right_start..]);
        if left.is_empty() || right.is_empty() {
            continue;
        }
        cx.budget.set(cx.budget.get() - 1);
        let first = parse_single(cx, left, false);
        if first.is_unknown() {
            continue;
        }
        let rest = parse_sequence(cx, right, false);
        let rest_failed = rest.iter().any(is_failed);
        // A clearly separate command that we could not follow is reported as its own failed
        // step, unless the left side is free text that may legitimately contain that "and".
        if !rest_failed || (starts_with_verb(right) && !is_free_text(&first)) {
            let mut out = vec![first];
            out.extend(rest);
            return out;
        }
    }
    vec![parse_single(cx, toks, bare_ok)]
}

/// "open youtube and search for lofi" means search YouTube: a search with no engine of its own
/// right after opening a site that has a search page inherits that site (and replaces the open).
fn merge_context(v: &mut Vec<Intent>) {
    let mut i = 0;
    while i + 1 < v.len() {
        let inherited = match (&v[i], &v[i + 1]) {
            (
                Intent::OpenSite { site },
                Intent::Search {
                    defaulted: true, ..
                },
            ) if sites::get(site).is_some_and(|s| s.search.is_some()) => Some(site.clone()),
            _ => None,
        };
        if let Some(key) = inherited {
            if let Intent::Search {
                site, defaulted, ..
            } = &mut v[i + 1]
            {
                *site = key;
                *defaulted = false;
            }
            v.remove(i);
        } else {
            i += 1;
        }
    }
}

// ---------------------------------------------------------------------------------------------
// one step
// ---------------------------------------------------------------------------------------------

fn parse_single(cx: &Cx<'_>, original: &[Tok], allow_bare: bool) -> Intent {
    let toks = strip_filler(original);
    let l = lows(toks);
    parse_reminder(cx, toks, &l)
        .or_else(|| parse_note(cx, toks, &l))
        .or_else(|| parse_email(cx, toks, &l))
        .or_else(|| parse_search(cx, toks, &l))
        .or_else(|| parse_timer(cx, toks, &l))
        .or_else(|| parse_open(cx, toks, &l, allow_bare))
        .unwrap_or_else(|| unknown(cx, original))
}

fn is_article(w: &str) -> bool {
    matches!(
        w,
        "a" | "an" | "the" | "new" | "quick" | "short" | "little" | "my"
    )
}

// ---- reminders ------------------------------------------------------------------------------

fn reminder_prefix(l: &[&str]) -> Option<usize> {
    match l {
        ["remind", "me", ..] => Some(2),
        ["reminder", ..] => Some(1),
        [verb, rest @ ..]
            if matches!(
                *verb,
                "set" | "add" | "create" | "make" | "schedule" | "new"
            ) =>
        {
            let i = rest.iter().take_while(|w| is_article(w)).count();
            (rest.get(i) == Some(&"reminder")).then_some(1 + i + 1)
        }
        _ => None,
    }
}

fn finish_reminder(cx: &Cx<'_>, title: String, r: when::Resolved) -> Option<Intent> {
    let title = if title.is_empty() {
        "Reminder".to_string()
    } else {
        capitalize(&title)
    };
    Some(Intent::Reminder {
        title,
        date: r.date.iso(),
        time: hhmm(r.minutes),
        due_utc_ms: local_to_utc_ms(r.date, r.minutes, cx.offset),
        default_time: r.default_time,
    })
}

fn parse_reminder(cx: &Cx<'_>, toks: &[Tok], l: &[&str]) -> Option<Intent> {
    let n = reminder_prefix(l)?;
    let (mut rt, mut rl) = (&toks[n..], &l[n..]);
    // "set a reminder for me to ...", "... for 5pm to ...", "remind me me ..."
    let skip = if rl.starts_with(&["for", "me"]) {
        2
    } else if matches!(rl.first(), Some(&"me") | Some(&"for")) {
        1
    } else {
        0
    };
    rt = &rt[skip.min(rt.len())..];
    rl = &rl[skip.min(rl.len())..];
    if rl.is_empty() {
        return None;
    }
    let marker = |s: &str| matches!(s, "to" | "that" | "about");

    if marker(rl[0]) {
        // title first: "to call mom at 5pm"
        let (body_t, body_l) = (&rt[1..], &rl[1..]);
        for k in 1..body_l.len() {
            if let Some(r) = when::parse_when(&body_l[k..], &cx.clock) {
                return finish_reminder(cx, payload(cx, &body_t[..k]), r);
            }
        }
        return None;
    }
    // just a time: "remind me in 5 minutes"
    if let Some(r) = when::parse_when(rl, &cx.clock) {
        return finish_reminder(cx, String::new(), r);
    }
    // time first: "at 5pm to call mom"
    for m in 1..rl.len().saturating_sub(1) {
        if marker(rl[m]) {
            if let Some(r) = when::parse_when(&rl[..m], &cx.clock) {
                return finish_reminder(cx, payload(cx, &rt[m + 1..]), r);
            }
        }
    }
    // no "to": "reminder: call mom at 5pm"
    for k in 1..rl.len() {
        if let Some(r) = when::parse_when(&rl[k..], &cx.clock) {
            return finish_reminder(cx, payload(cx, &rt[..k]), r);
        }
    }
    None
}

// ---- notes ----------------------------------------------------------------------------------

fn note_prefix(l: &[&str]) -> Option<usize> {
    match l {
        ["note", ..] => Some(1),
        [verb, rest @ ..]
            if matches!(
                *verb,
                "take" | "make" | "add" | "create" | "save" | "write" | "jot" | "new"
            ) =>
        {
            if matches!(*verb, "jot" | "write") && rest.first() == Some(&"down") {
                return Some(2);
            }
            let i = rest.iter().take_while(|w| is_article(w)).count();
            (rest.get(i) == Some(&"note")).then_some(1 + i + 1)
        }
        _ => None,
    }
}

fn parse_note(cx: &Cx<'_>, toks: &[Tok], l: &[&str]) -> Option<Intent> {
    let mut n = note_prefix(l)?;
    // "note to self buy milk", "note that ...", "make a note of ...", "note saying ..."
    if l[n..].starts_with(&["to", "self"]) {
        if l.len() == n + 2 {
            return None;
        }
        n += 2;
    } else if matches!(
        l.get(n),
        Some(&"that") | Some(&"saying") | Some(&"of") | Some(&"about")
    ) {
        if l.len() == n + 1 {
            return None; // "note that" with nothing after it
        }
        n += 1;
    }
    let body = payload(cx, &toks[n.min(toks.len())..]);
    if body.is_empty() {
        return None;
    }
    Some(Intent::Note {
        body: capitalize(&body),
    })
}

// ---- email ----------------------------------------------------------------------------------

/// (prefix length, true if the phrase can stand alone: "draft an email")
fn email_prefix(l: &[&str]) -> Option<(usize, bool)> {
    match l {
        ["email", ..] | ["e-mail", ..] | ["mail", ..] => Some((1, false)),
        [verb, rest @ ..]
            if matches!(
                *verb,
                "draft" | "write" | "compose" | "create" | "prepare" | "send" | "make" | "new"
            ) =>
        {
            let mut i = rest.iter().take_while(|w| is_article(w)).count();
            let provider = matches!(rest.get(i), Some(&"gmail") | Some(&"outlook"));
            if provider {
                i += 1;
            }
            match rest.get(i) {
                Some(&"email") | Some(&"e-mail") | Some(&"mail") => Some((1 + i + 1, true)),
                _ if provider => Some((1 + i, true)),
                _ => None,
            }
        }
        _ => None,
    }
}

/// "sam at x dot com" -> ("sam@x.com", 5 words). Only the strict shape
/// `local [dot local]* at host (dot host)+` with a letters-only last part.
fn spoken_email(l: &[&str]) -> Option<(String, usize)> {
    let local_byte = |b: u8| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'+');
    let host_byte = |b: u8| b.is_ascii_alphanumeric() || b == b'-';
    let (mut i, mut local) = (0usize, String::new());
    loop {
        let w = *l.get(i)?;
        if matches!(w, "at" | "dot" | "me") || w.is_empty() || !w.bytes().all(local_byte) {
            return None;
        }
        if !local.is_empty() {
            local.push('.');
        }
        local.push_str(w);
        i += 1;
        if l.get(i) == Some(&"dot") && l.get(i + 1).is_some() {
            i += 1;
        } else {
            break;
        }
    }
    if l.get(i) != Some(&"at") {
        return None;
    }
    i += 1;
    let mut parts: Vec<&str> = Vec::new();
    while let Some(&w) = l.get(i) {
        if w == "dot" || w.is_empty() || !w.bytes().all(host_byte) {
            break;
        }
        parts.push(w);
        i += 1;
        if l.get(i) == Some(&"dot") && l.get(i + 1).is_some() {
            i += 1;
        } else {
            break;
        }
    }
    let tld = *parts.last()?;
    if parts.len() < 2 || tld.len() < 2 || !tld.bytes().all(|b| b.is_ascii_alphabetic()) {
        return None;
    }
    Some((format!("{local}@{}", parts.join(".")), i))
}

const MAIL_NOISE: &[&str] = &[
    "to", "a", "an", "the", "new", "quick", "short", "email", "e-mail", "mail", "in", "on", "with",
    "using", "via", "through", "my", "please", "for", "and", "from", "gmail", "outlook", "default",
    "app", "client", "send", "draft", "also", "cc", "address",
];

enum Unit {
    Addr(String),
    Word(usize),
}

fn parse_email(cx: &Cx<'_>, toks: &[Tok], l: &[&str]) -> Option<Intent> {
    let (n, standalone) = email_prefix(l)?;
    if !standalone && l.len() <= n {
        return None;
    }
    let (rt, rl) = (&toks[n..], &l[n..]);

    // subject / body markers: header [about SUBJECT] [saying BODY]; the body runs to the end
    let (mut subj_at, mut body_at): (Option<usize>, Option<(usize, usize)>) = (None, None);
    for i in 0..rl.len() {
        let body_marker = match rl[i] {
            "saying" | "says" | "body" | "telling" => Some(1),
            "to" | "and" if rl.get(i + 1) == Some(&"say") => Some(2),
            "that" if matches!(rl.get(i + 1), Some(&"says") | Some(&"say")) => Some(2),
            _ => None,
        };
        if let Some(len) = body_marker {
            body_at = Some((i, len));
            break;
        }
        if subj_at.is_none() && matches!(rl[i], "about" | "regarding" | "re" | "subject") {
            subj_at = Some(i);
        }
    }
    let body_start = body_at.map(|(i, len)| i + len);
    let header_end = subj_at.or(body_at.map(|(i, _)| i)).unwrap_or(rl.len());
    let subject_range = subj_at.map(|i| {
        let mut s = i + 1;
        if rl.get(s) == Some(&"is") && s + 1 < body_at.map_or(rl.len(), |(b, _)| b) {
            s += 1;
        }
        (s, body_at.map_or(rl.len(), |(b, _)| b))
    });

    let provider = l[..n + header_end]
        .iter()
        .find_map(|w| match *w {
            "gmail" => Some(MailProvider::Gmail),
            "outlook" => Some(MailProvider::Outlook),
            _ => None,
        })
        .unwrap_or(MailProvider::SystemDefault);

    // recipients: every address-looking word, plus "sam at x dot com"
    let (header_t, header_l) = (&rt[..header_end], &rl[..header_end]);
    let mut units = Vec::new();
    let mut j = 0;
    while j < header_l.len() {
        if header_t[j].txt.contains('@') {
            units.push(Unit::Addr(header_t[j].txt.clone()));
            j += 1;
        } else if let Some((addr, used)) = spoken_email(&header_l[j..]) {
            units.push(Unit::Addr(addr));
            j += used;
        } else {
            units.push(Unit::Word(j));
            j += 1;
        }
    }
    let to: Vec<String> = units
        .iter()
        .filter_map(|u| match u {
            Unit::Addr(a) => Some(a.clone()),
            Unit::Word(_) => None,
        })
        .collect();
    let leftover: Vec<&str> = units
        .iter()
        .filter_map(|u| match u {
            Unit::Word(i) if !MAIL_NOISE.contains(&header_l[*i]) => Some(header_t[*i].txt.as_str()),
            _ => None,
        })
        .collect();

    let mut subject = subject_range
        .map(|(s, e)| payload(cx, &rt[s.min(e)..e]))
        .unwrap_or_default();
    let body = body_start
        .map(|s| payload(cx, &rt[s.min(rt.len())..]))
        .unwrap_or_default();
    let mut bad_recipient = None;
    if to.is_empty() {
        if !leftover.is_empty() {
            bad_recipient = Some(leftover.join(" "));
        }
    } else if subject.is_empty() && !leftover.is_empty() {
        subject = leftover.join(" ");
    }
    Some(Intent::Email {
        provider,
        to,
        subject: capitalize(&subject),
        body: capitalize(&body),
        bad_recipient,
    })
}

// ---- search ---------------------------------------------------------------------------------

const PREPS: &[&str] = &["on", "in", "using", "with", "via"];

/// "... on youtube" at the end of `l`: the engine and where the preposition starts.
fn engine_at_end(l: &[&str]) -> Option<(&'static Site, usize)> {
    for p in (1..l.len()).rev() {
        if PREPS.contains(&l[p]) {
            if let Some((site, used)) = sites::find_alias(&l[p + 1..]) {
                if p + 1 + used == l.len() && site.search.is_some() {
                    return Some((site, p));
                }
            }
        }
    }
    None
}

fn search_intent(site: &Site, query: String, defaulted: bool) -> Option<Intent> {
    (!query.is_empty()).then(|| Intent::Search {
        site: site.key.to_string(),
        query,
        defaulted,
    })
}

fn generic_search(
    cx: &Cx<'_>,
    toks: &[Tok],
    l: &[&str],
    start: usize,
    engine_required: bool,
) -> Option<Intent> {
    let (rt, rl) = (toks.get(start..)?, l.get(start..)?);
    if rl.is_empty() {
        return None;
    }
    let google = sites::get("google")?;

    if !engine_required {
        // the engine named first: "search youtube for lofi", "search on github for krovi"
        let k = usize::from(PREPS.contains(&rl[0]) && rl.len() > 1);
        if let Some((site, used)) = sites::find_alias(&rl[k..]) {
            if site.search.is_some() {
                if k + used == rl.len() {
                    return None; // "search google": an engine but nothing to search for
                }
                let mut q = k + used;
                let joined = matches!(rl.get(q), Some(&"for") | Some(&"about") | Some(&"up"));
                if joined {
                    q += 1;
                }
                if (joined || site.verb_search) && q < rl.len() {
                    return search_intent(site, payload(cx, &rt[q..]), false);
                }
            }
        }
        // "search the web for cats", "search online for cats"
        for web in [
            &["the", "web"][..],
            &["the", "internet"],
            &["the", "net"],
            &["web"],
            &["online"],
            &["internet"],
        ] {
            if rl.starts_with(web) {
                let mut q = web.len();
                if matches!(rl.get(q), Some(&"for") | Some(&"about")) {
                    q += 1;
                }
                if q < rl.len() {
                    return search_intent(google, payload(cx, &rt[q..]), false);
                }
            }
        }
    }

    let q0 = usize::from(matches!(rl[0], "for" | "about") && !engine_required);
    if q0 >= rl.len() {
        return None;
    }
    let (body_t, body_l) = (&rt[q0..], &rl[q0..]);
    if let Some((site, p)) = engine_at_end(body_l) {
        return search_intent(site, payload(cx, &body_t[..p]), false);
    }
    if engine_required {
        return None;
    }
    search_intent(google, payload(cx, body_t), true)
}

fn verb_search(cx: &Cx<'_>, toks: &[Tok], l: &[&str]) -> Option<Intent> {
    let (mut site, mut used) = sites::find_alias(l)?;
    if !site.verb_search {
        if used == l.len() {
            return None; // just a site name ("google drive"): that is "open", not a search
        }
        let (first, n) = sites::find_alias(&l[..1])?;
        if !first.verb_search {
            return None;
        }
        site = first;
        used = n;
    }
    let mut q = used;
    if l.get(q) == Some(&"search") {
        q += 1;
    }
    if matches!(l.get(q), Some(&"for") | Some(&"about")) {
        q += 1;
    }
    if q >= l.len() {
        return None;
    }
    search_intent(site, payload(cx, &toks[q..]), false)
}

fn parse_search(cx: &Cx<'_>, toks: &[Tok], l: &[&str]) -> Option<Intent> {
    match *l.first()? {
        "search" | "lookup" => generic_search(cx, toks, l, 1, false),
        "look" if l.get(1) == Some(&"up") => generic_search(cx, toks, l, 2, false),
        "look" if l.get(1) == Some(&"for") => generic_search(cx, toks, l, 2, false),
        "play" | "watch" | "find" => generic_search(cx, toks, l, 1, true),
        _ => verb_search(cx, toks, l),
    }
}

// ---- timers ---------------------------------------------------------------------------------

fn parse_timer(cx: &Cx<'_>, toks: &[Tok], l: &[&str]) -> Option<Intent> {
    let tpos = l.iter().position(|w| matches!(*w, "timer" | "countdown"))?;
    let pre = &l[..tpos];
    let mut i = usize::from(pre.first().is_some_and(|v| {
        matches!(
            *v,
            "set" | "start" | "create" | "add" | "begin" | "make" | "run"
        )
    }));
    while i < pre.len() && is_article(pre[i]) {
        i += 1;
    }
    let mut seconds: Option<u64> = None;
    if i < pre.len() {
        // "25 minute timer": the words before "timer" must be exactly a duration
        match parse_duration(&pre[i..]) {
            Some((s, used)) if i + used == pre.len() => seconds = Some(s),
            _ => return None,
        }
    }
    let suffix = &l[tpos + 1..];
    let mut s = 0;
    if seconds.is_none() {
        while s < suffix.len() && matches!(suffix[s], "for" | "of" | "to" | "at") {
            s += 1;
        }
        if let Some((secs, used)) = parse_duration(&suffix[s..]) {
            seconds = Some(secs);
            s += used;
        } else if let Some(f) = (s + 1..suffix.len()).find(|&f| {
            // "timer for pasta for 10 minutes": label first
            suffix[f] == "for"
                && parse_duration(&suffix[f + 1..]).is_some_and(|(_, u)| f + 1 + u == suffix.len())
        }) {
            let (secs, _) = parse_duration(&suffix[f + 1..])?;
            let label = payload(cx, &toks[tpos + 1 + s..tpos + 1 + f]);
            return Some(Intent::Timer {
                seconds: secs,
                label: (!label.is_empty()).then_some(label),
            });
        }
    }
    let seconds = seconds?;
    // an optional label: "called tea", "for pasta", "to boil eggs"
    let rest = &suffix[s.min(suffix.len())..];
    let label = match rest.first() {
        None => None,
        Some(m)
            if matches!(
                *m,
                "called" | "named" | "labeled" | "labelled" | "titled" | "for" | "to" | "as"
            ) && rest.len() > 1 =>
        {
            let from = tpos + 1 + s + 1;
            Some(payload(cx, &toks[from..])).filter(|x| !x.is_empty())
        }
        Some(_) => return None,
    };
    Some(Intent::Timer { seconds, label })
}

// ---- open ------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Verb {
    Open,
    Launch,
    Visit,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Force {
    None,
    App,
    Site,
}

fn open_prefix(l: &[&str]) -> Option<(usize, Verb)> {
    Some(match l {
        ["open", ..] => (1, Verb::Open),
        ["launch", ..] | ["start", ..] | ["run", ..] => (1, Verb::Launch),
        ["fire", "up", ..] => (2, Verb::Launch),
        ["go", "to", ..] | ["browse", "to", ..] | ["navigate", "to", ..] => (2, Verb::Visit),
        ["take", "me", "to", ..] => (3, Verb::Visit),
        ["goto", ..] | ["visit", ..] | ["browse", ..] => (1, Verb::Visit),
        ["pull", "up", ..] | ["bring", "up", ..] => (2, Verb::Open),
        ["load", ..] => (1, Verb::Open),
        _ => return None,
    })
}

const BROWSERS: &[&str] = &["browser", "chrome", "edge", "firefox", "brave", "opera"];

/// How many trailing words are "in my browser" / "in chrome".
fn trailing_browser(l: &[&str]) -> usize {
    let n = l.len();
    if n >= 3
        && l[n - 3] == "in"
        && matches!(l[n - 2], "my" | "the" | "a")
        && BROWSERS.contains(&l[n - 1])
    {
        3
    } else if n >= 2 && l[n - 2] == "in" && BROWSERS.contains(&l[n - 1]) {
        2
    } else {
        0
    }
}

/// `example.com`, `www.example.co.uk/path`, `localhost` is NOT (needs a dot and a letters TLD).
fn domain_like(raw: &str) -> bool {
    if raw.contains(char::is_whitespace) {
        return false;
    }
    let host_end = raw.find(['/', '?', '#']).unwrap_or(raw.len());
    let host = raw[..host_end].split(':').next().unwrap_or("");
    let labels: Vec<&str> = host.split('.').collect();
    labels.len() >= 2
        && labels
            .iter()
            .all(|l| !l.is_empty() && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'))
        && labels
            .last()
            .is_some_and(|t| t.len() >= 2 && t.bytes().all(|b| b.is_ascii_alphabetic()))
}

const EXEC_EXT: &[&str] = &["exe", "bat", "cmd", "msi", "lnk", "scr"];
const DOC_EXT: &[&str] = &[
    "txt", "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "png", "jpg", "jpeg", "gif", "zip",
    "rar", "7z", "iso", "md", "json", "log", "csv", "ps1", "vbs", "dll", "jar", "js", "py", "sh",
];

/// For "notepad.exe": the file extension, when `raw` is a bare file name (not a link).
fn file_ext(raw: &str) -> Option<String> {
    if raw.contains('/') || url::has_scheme(raw) {
        return None;
    }
    let (_, ext) = raw.rsplit_once('.')?;
    Some(ext.to_ascii_lowercase())
}

fn path_less(u: &str) -> bool {
    let after = u.split_once("://").map_or(u, |x| x.1);
    match after.find(['/', '?', '#']) {
        None => true,
        Some(i) => &after[i..] == "/",
    }
}

/// A single address-looking word: a known site, or an address to confirm.
fn address_intent(raw: &str) -> Intent {
    if let Ok(v) = url::validate(raw) {
        if path_less(&v.url) {
            if let Some(site) = sites::find_host(&v.host) {
                return Intent::OpenSite {
                    site: site.key.to_string(),
                };
            }
        }
    }
    Intent::OpenUrl {
        url: raw.to_string(),
    }
}

fn app_words(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect()
}

const VENDOR_WORDS: &[&str] = &[
    "microsoft",
    "google",
    "adobe",
    "windows",
    "apple",
    "mozilla",
    "jetbrains",
    "autodesk",
];
const TRAILING_NOISE: &[&str] = &["app", "desktop", "classic", "new", "application"];

fn score_app(q: &[String], app: &[String]) -> u32 {
    if q.is_empty() || app.is_empty() {
        return 0;
    }
    if q == app {
        return 100;
    }
    if q.iter().map(String::len).sum::<usize>() < 3 {
        return 0;
    }
    let mut trimmed = app;
    if trimmed.len() > 1 && VENDOR_WORDS.contains(&trimmed[0].as_str()) {
        trimmed = &trimmed[1..];
    }
    while trimmed.len() > 1 && TRAILING_NOISE.contains(&trimmed[trimmed.len() - 1].as_str()) {
        trimmed = &trimmed[..trimmed.len() - 1];
    }
    if q == trimmed {
        return 90;
    }
    if app.len() >= q.len() && app[..q.len()] == *q {
        return 80;
    }
    if trimmed.len() >= q.len() && trimmed[..q.len()] == *q {
        return 80;
    }
    if q.iter().all(|w| app.contains(w)) {
        return 60;
    }
    0
}

/// Look `query` up in the host's app list: the best score wins; a tie between different apps is
/// reported as ambiguous so nothing is launched by guesswork. Returns the match and its score.
pub(crate) fn resolve_app(query: &str, apps: &[AppEntry]) -> (AppMatch, u32) {
    let q = app_words(query);
    let mut scored: Vec<(u32, &AppEntry)> = apps
        .iter()
        .filter_map(|a| {
            let s = score_app(&q, &app_words(&a.name));
            (s > 0).then_some((s, a))
        })
        .collect();
    let best = scored.iter().map(|(s, _)| *s).max().unwrap_or(0);
    if best == 0 {
        return (AppMatch::NotFound, 0);
    }
    scored.retain(|(s, _)| *s == best);
    let mut seen = HashSet::new();
    let mut uniq: Vec<&AppEntry> = Vec::new();
    for (_, a) in scored {
        if seen.insert(a.name.to_lowercase()) {
            uniq.push(a);
        }
    }
    if let [only] = uniq[..] {
        return (
            AppMatch::Found {
                name: only.name.clone(),
                target: only.target.clone(),
            },
            best,
        );
    }
    let mut names: Vec<String> = uniq.iter().map(|a| a.name.clone()).collect();
    names.sort();
    names.truncate(5);
    (AppMatch::Ambiguous { names }, best)
}

fn parse_open(cx: &Cx<'_>, toks: &[Tok], l: &[&str], allow_bare: bool) -> Option<Intent> {
    let (verb, n) = match open_prefix(l) {
        Some((n, v)) => (Some(v), n),
        None => (None, 0),
    };
    if verb.is_none() && !allow_bare {
        return None;
    }
    let mut t = &toks[n..];
    if t.len() > 1 && t[0].low == "the" {
        t = &t[1..];
    }
    let mut force = Force::None;
    loop {
        let tl = lows(t);
        let len = tl.len();
        let browser = trailing_browser(&tl);
        if browser > 0 && len > browser {
            force = Force::Site;
            t = &t[..len - browser];
        } else if len > 1 && matches!(tl[len - 1], "app" | "application" | "program") {
            force = Force::App;
            t = &t[..len - 1];
        } else if len > 1 && matches!(tl[len - 1], "website" | "site" | "page" | "homepage") {
            force = Force::Site;
            let cut = if len > 2 && tl[len - 2] == "web" {
                2
            } else {
                1
            };
            t = &t[..len - cut];
        } else {
            break;
        }
    }
    if t.is_empty() {
        return None;
    }
    let tl = lows(t);
    let mut name = payload(cx, t);

    // 1. an address
    if t.len() == 1 {
        let raw = t[0].txt.as_str();
        if url::has_scheme(raw) || domain_like(raw) {
            match file_ext(raw).as_deref() {
                Some(ext) if EXEC_EXT.contains(&ext) => {
                    // "open notepad.exe": the app called notepad
                    name = raw[..raw.len() - ext.len() - 1].to_string();
                }
                Some(ext) if DOC_EXT.contains(&ext) => return None,
                _ => return Some(address_intent(raw)),
            }
        }
    }

    // 2. a site from the table
    let site = sites::find_alias(&tl)
        .filter(|(_, used)| *used == tl.len())
        .map(|(s, _)| s);
    let site_intent = |s: &Site| Intent::OpenSite {
        site: s.key.to_string(),
    };
    match verb {
        None | Some(Verb::Visit) => site.map(site_intent),
        Some(_) if force == Force::Site => site.map(site_intent),
        Some(v) => {
            let (app, score) = resolve_app(&name, cx.apps);
            let found = matches!(app, AppMatch::Found { .. });
            let wants_app = match (site, force) {
                (_, Force::App) | (None, _) => true,
                (Some(_), _) => found && score >= if v == Verb::Launch { 90 } else { 100 },
            };
            if wants_app {
                Some(Intent::OpenApp { query: name, app })
            } else {
                site.map(site_intent)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::civil::Date;

    /// Monday 2026-11-02, 10:00:30 UTC.
    fn now_ms() -> i64 {
        Date::new(2026, 11, 2).unwrap().days() * 86_400_000 + (10 * 3600 + 30) * 1000
    }

    fn app(name: &str) -> AppEntry {
        AppEntry {
            name: name.into(),
            target: format!("shell:AppsFolder\\{name}"),
        }
    }

    fn std_apps() -> Vec<AppEntry> {
        [
            "Spotify",
            "Notepad",
            "Visual Studio Code",
            "Calculator",
            "Microsoft Edge",
            "Google Chrome",
            "YouTube Music",
            "Paint",
            "Paint 3D",
            "Settings",
        ]
        .iter()
        .map(|n| app(n))
        .collect()
    }

    fn run_with(input: &str, apps: &[AppEntry], offset: i32) -> Vec<Intent> {
        parse(
            input,
            &ParseContext {
                now_utc_ms: now_ms(),
                utc_offset_minutes: offset,
                apps,
            },
        )
    }

    fn run(input: &str) -> Vec<Intent> {
        run_with(input, &std_apps(), 0)
    }

    fn one(input: &str) -> Intent {
        let mut v = run(input);
        assert_eq!(v.len(), 1, "{input:?} -> {v:?}");
        v.remove(0)
    }

    fn site(key: &str) -> Intent {
        Intent::OpenSite { site: key.into() }
    }
    fn search(key: &str, q: &str, defaulted: bool) -> Intent {
        Intent::Search {
            site: key.into(),
            query: q.into(),
            defaulted,
        }
    }
    /// Check an OpenApp intent resolved to `name`, ignoring the echoed query.
    fn assert_app(got: &Intent, name: &str) {
        match got {
            Intent::OpenApp {
                app: AppMatch::Found { name: n, target },
                ..
            } => {
                assert_eq!(n, name);
                assert_eq!(target, &format!("shell:AppsFolder\\{name}"));
            }
            other => panic!("expected app {name}, got {other:?}"),
        }
    }
    fn assert_unknown(input: &str) -> Vec<String> {
        match one(input) {
            Intent::Unknown { suggestions, .. } => {
                assert!(!suggestions.is_empty());
                suggestions
            }
            other => panic!("{input:?} should be unknown, got {other:?}"),
        }
    }

    // ---- websites ----

    #[test]
    fn open_known_sites() {
        for (input, key) in [
            ("open youtube", "youtube"),
            ("Open YouTube", "youtube"),
            ("OPEN YOUTUBE!", "youtube"),
            ("open you tube", "youtube"),
            ("go to github.com", "github"),
            ("go to github", "github"),
            ("visit reddit", "reddit"),
            ("take me to wikipedia", "wikipedia"),
            ("open https://github.com", "github"),
            ("open https://www.github.com/", "github"),
            ("open www.github.com", "github"),
            ("open the github website", "github"),
            ("open github in my browser", "github"),
            ("open gmail in the browser", "gmail"),
            ("open google maps", "maps"),
            ("open twitter", "x"),
            ("open x.com", "x"),
            ("browse to stackoverflow.com", "stackoverflow"),
            ("pull up netflix", "netflix"),
            ("navigate to linkedin", "linkedin"),
            ("open chatgpt", "chatgpt"),
            ("youtube", "youtube"),
            ("github.com", "github"),
            ("the youtube", "youtube"),
            ("open youtube website", "youtube"),
            ("open the youtube web site", "youtube"),
            ("launch youtube", "youtube"),
            ("open google drive", "drive"),
        ] {
            assert_eq!(one(input), site(key), "{input}");
        }
    }

    #[test]
    fn typed_addresses_stay_unverified() {
        for (input, url) in [
            ("open example.org", "example.org"),
            ("go to example.com/path", "example.com/path"),
            (
                "open https://example.com/a?b=1&c=2",
                "https://example.com/a?b=1&c=2",
            ),
            (
                "open github.com/rust-lang/rust",
                "github.com/rust-lang/rust",
            ),
            ("visit http://example.com", "http://example.com"),
            ("example.com", "example.com"),
            (
                "https://example.com/some/page",
                "https://example.com/some/page",
            ),
            ("go to javascript:alert(1)", "javascript:alert(1)"),
            (
                "open file:///C:/Windows/System32/calc.exe",
                "file:///C:/Windows/System32/calc.exe",
            ),
            ("open data:text/html,hello", "data:text/html,hello"),
            (
                "open https://user:pass@example.com",
                "https://user:pass@example.com",
            ),
            ("open ftp://example.com/x", "ftp://example.com/x"),
        ] {
            assert_eq!(one(input), Intent::OpenUrl { url: url.into() }, "{input}");
        }
    }

    #[test]
    fn go_to_something_that_is_not_a_site_is_unknown() {
        for input in [
            "go to the kitchen",
            "go to bed",
            "visit mom",
            "go to sleep",
            "go to",
        ] {
            assert_unknown(input);
        }
    }

    #[test]
    fn documents_are_not_websites_or_apps() {
        for input in [
            "open report.pdf",
            "open notes.txt",
            "open archive.zip",
            "open script.js",
        ] {
            assert_unknown(input);
        }
    }

    // ---- apps ----

    #[test]
    fn open_installed_apps() {
        for (input, name) in [
            ("open spotify", "Spotify"),
            ("launch notepad", "Notepad"),
            ("start calculator", "Calculator"),
            ("run notepad", "Notepad"),
            ("fire up spotify", "Spotify"),
            ("open notepad.exe", "Notepad"),
            ("open visual studio", "Visual Studio Code"),
            ("open edge", "Microsoft Edge"),
            ("launch chrome", "Google Chrome"),
            ("open paint", "Paint"),
            ("open the calculator app", "Calculator"),
            ("open settings", "Settings"),
        ] {
            assert_app(&one(input), name);
        }
    }

    #[test]
    fn app_beats_site_only_on_an_exact_name() {
        // Spotify the app is installed -> app; no app installed -> the web player
        assert_app(&one("open spotify"), "Spotify");
        assert_eq!(run_with("open spotify", &[], 0), vec![site("spotify")]);
        // "YouTube Music" is not "YouTube": open goes to the site
        assert_eq!(one("open youtube"), site("youtube"));
        // launch accepts a close app name (>= 90), still not a partial one
        assert_eq!(one("launch youtube"), site("youtube"));
        let with_exact = [app("YouTube")];
        let got = run_with("open youtube", &with_exact, 0);
        assert_app(&got[0], "YouTube");
        // vendor prefix and trailing "(new)" are ignored for launch, not for open
        let outlook = [app("Outlook (new)")];
        assert_eq!(run_with("open outlook", &outlook, 0), vec![site("outlook")]);
        assert_app(&run_with("launch outlook", &outlook, 0)[0], "Outlook (new)");
    }

    #[test]
    fn unknown_and_ambiguous_apps_are_reported_not_guessed() {
        match one("open nonexistentapp") {
            Intent::OpenApp {
                app: AppMatch::NotFound,
                query,
            } => assert_eq!(query, "nonexistentapp"),
            other => panic!("{other:?}"),
        }
        let apps = [app("Adobe Reader"), app("Adobe Photoshop"), app("Notepad")];
        match run_with("open adobe", &apps, 0).remove(0) {
            Intent::OpenApp {
                app: AppMatch::Ambiguous { names },
                ..
            } => {
                assert_eq!(names, ["Adobe Photoshop", "Adobe Reader"]);
            }
            other => panic!("{other:?}"),
        }
        // with no app list at all nothing can be launched
        match run_with("open notepad", &[], 0).remove(0) {
            Intent::OpenApp {
                app: AppMatch::NotFound,
                ..
            } => {}
            other => panic!("{other:?}"),
        }
        // a one-letter query never fuzzy-matches
        match run_with("open q", &apps, 0).remove(0) {
            Intent::OpenApp {
                app: AppMatch::NotFound,
                ..
            } => {}
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn duplicate_app_entries_are_one_app() {
        let apps = [app("Notepad"), app("notepad")];
        assert_app(&run_with("open notepad", &apps, 0)[0], "Notepad");
    }

    // ---- search ----

    #[test]
    fn search_phrasings() {
        for (input, want) in [
            ("search google for rust", search("google", "rust", false)),
            (
                "Search Google for Rust Lifetimes",
                search("google", "Rust Lifetimes", false),
            ),
            (
                "google rust lifetimes",
                search("google", "rust lifetimes", false),
            ),
            ("google search for rust", search("google", "rust", false)),
            ("google for rust", search("google", "rust", false)),
            ("search youtube for lofi", search("youtube", "lofi", false)),
            (
                "search on youtube for lofi",
                search("youtube", "lofi", false),
            ),
            (
                "search youtube lofi beats",
                search("youtube", "lofi beats", false),
            ),
            ("youtube lofi beats", search("youtube", "lofi beats", false)),
            ("play lofi on youtube", search("youtube", "lofi", false)),
            (
                "play lofi beats on youtube",
                search("youtube", "lofi beats", false),
            ),
            (
                "watch cat videos on youtube",
                search("youtube", "cat videos", false),
            ),
            (
                "play daft punk on spotify",
                search("spotify", "daft punk", false),
            ),
            ("search for rust", search("google", "rust", true)),
            (
                "search rust lifetimes",
                search("google", "rust lifetimes", true),
            ),
            (
                "search rust lifetimes on github",
                search("github", "rust lifetimes", false),
            ),
            (
                "search for krovi on github",
                search("github", "krovi", false),
            ),
            ("search github for krovi", search("github", "krovi", false)),
            (
                "look up borrow checker",
                search("google", "borrow checker", true),
            ),
            (
                "look up borrow checker on wikipedia",
                search("wikipedia", "borrow checker", false),
            ),
            ("lookup tauri", search("google", "tauri", true)),
            ("look for tauri docs", search("google", "tauri docs", true)),
            ("search the web for cats", search("google", "cats", false)),
            ("search online for cats", search("google", "cats", false)),
            ("search maps for pizza", search("maps", "pizza", false)),
            (
                "search for pizza on google maps",
                search("maps", "pizza", false),
            ),
            (
                "search maps of india",
                search("google", "maps of india", true),
            ),
            (
                "search github krovi",
                search("google", "github krovi", true),
            ),
            ("find lofi on youtube", search("youtube", "lofi", false)),
            ("bing weather today", search("bing", "weather today", false)),
            (
                "duck duck go privacy tools",
                search("duckduckgo", "privacy tools", false),
            ),
            (
                "wikipedia ada lovelace",
                search("wikipedia", "ada lovelace", false),
            ),
            (
                "search for rust on my computer",
                search("google", "rust on my computer", true),
            ),
            (
                "search for \"quoted phrase\"",
                search("google", "quoted phrase", true),
            ),
            (
                "search google for c++ & rust",
                search("google", "c++ & rust", false),
            ),
        ] {
            assert_eq!(one(input), want, "{input}");
        }
    }

    #[test]
    fn google_drive_is_not_a_search_engine() {
        assert_eq!(one("google drive"), site("drive"));
        assert_eq!(
            one("google drive reports"),
            search("google", "drive reports", false)
        );
    }

    #[test]
    fn search_negatives() {
        for input in [
            "search",
            "search google",
            "search for",
            "play something",
            "play",
            "find my keys",
            "look up",
            "google for",
        ] {
            assert_unknown(input);
        }
    }

    // ---- timers ----

    #[test]
    fn timers() {
        for (input, secs, label) in [
            ("set a timer for 10 minutes", 600, None),
            ("Set a timer for 10 minutes.", 600, None),
            ("timer 90 seconds", 90, None),
            ("25 minute timer", 1500, None),
            ("set a 5 minute timer", 300, None),
            ("set a five minute timer", 300, None),
            ("start a timer for 1 hour 30 minutes", 5400, None),
            ("timer for half an hour", 1800, None),
            ("set a timer for an hour and a half", 5400, None),
            ("set a timer for 2h", 7200, None),
            ("set timer 45s", 45, None),
            ("create a timer for 3 minutes", 180, None),
            ("set a timer for ten minutes called tea", 600, Some("tea")),
            ("10 minute timer for pasta", 600, Some("pasta")),
            (
                "set a timer for 10 minutes for the pasta",
                600,
                Some("the pasta"),
            ),
            ("set a timer for pasta for 10 minutes", 600, Some("pasta")),
            (
                "set a timer for 20 minutes to boil eggs",
                1200,
                Some("boil eggs"),
            ),
            ("set a timer for 0 minutes", 0, None),
            ("set a timer for 1000 hours", 3_600_000, None),
        ] {
            assert_eq!(
                one(input),
                Intent::Timer {
                    seconds: secs,
                    label: label.map(String::from)
                },
                "{input}"
            );
        }
    }

    #[test]
    fn timer_negatives() {
        for input in [
            "set a timer",
            "timer",
            "set a timer for soon",
            "set a timer for ten",
            "start my pomodoro timer",
            "set a timer for 5 minutes and tell me jokes",
        ] {
            let v = run(input);
            assert!(
                !v.iter().any(|i| matches!(i, Intent::Timer { .. })),
                "{input} -> {v:?}"
            );
        }
        for input in [
            "set a timer",
            "timer",
            "set a timer for soon",
            "set a timer for ten",
        ] {
            assert_unknown(input);
        }
    }

    // ---- reminders ----

    fn reminder(title: &str, date: &str, time: &str, default_time: bool) -> Intent {
        let d = Date::parse_iso(date).unwrap();
        let (h, m) = (
            time[..2].parse::<u32>().unwrap(),
            time[3..].parse::<u32>().unwrap(),
        );
        Intent::Reminder {
            title: title.into(),
            date: date.into(),
            time: time.into(),
            due_utc_ms: local_to_utc_ms(d, h * 60 + m, 0),
            default_time,
        }
    }

    #[test]
    fn reminders() {
        for (input, want) in [
            (
                "remind me in 20 minutes to stretch",
                reminder("Stretch", "2026-11-02", "10:21", false),
            ),
            (
                "remind me at 5pm to call mom",
                reminder("Call mom", "2026-11-02", "17:00", false),
            ),
            (
                "remind me tomorrow at 9 to pay rent",
                reminder("Pay rent", "2026-11-03", "09:00", false),
            ),
            (
                "remind me to call mom at 5pm",
                reminder("Call mom", "2026-11-02", "17:00", false),
            ),
            (
                "remind me to pay rent tomorrow at 9",
                reminder("Pay rent", "2026-11-03", "09:00", false),
            ),
            (
                "remind me to buy milk on friday at 3pm",
                reminder("Buy milk", "2026-11-06", "15:00", false),
            ),
            (
                "remind me to stretch in 20 minutes",
                reminder("Stretch", "2026-11-02", "10:21", false),
            ),
            (
                "remind me in an hour to check the oven",
                reminder("Check the oven", "2026-11-02", "11:01", false),
            ),
            (
                "remind me tomorrow to pay rent",
                reminder("Pay rent", "2026-11-03", "09:00", true),
            ),
            (
                "remind me on friday to submit the report",
                reminder("Submit the report", "2026-11-06", "09:00", true),
            ),
            (
                "remind me next monday at 10 to plan the week",
                reminder("Plan the week", "2026-11-09", "10:00", false),
            ),
            (
                "set a reminder for 5pm to call mom",
                reminder("Call mom", "2026-11-02", "17:00", false),
            ),
            (
                "set a reminder to call mom at 5pm",
                reminder("Call mom", "2026-11-02", "17:00", false),
            ),
            (
                "add a reminder tomorrow at 8am to take medicine",
                reminder("Take medicine", "2026-11-03", "08:00", false),
            ),
            (
                "reminder: call mom at 5pm",
                reminder("Call mom", "2026-11-02", "17:00", false),
            ),
            (
                "remind me call mom at 5pm",
                reminder("Call mom", "2026-11-02", "17:00", false),
            ),
            (
                "remind me tonight at 8 that dinner is at nine",
                reminder("Dinner is at nine", "2026-11-02", "20:00", false),
            ),
            (
                "remind me to catch the bus at 5",
                reminder("Catch the bus", "2026-11-02", "17:00", false),
            ),
            (
                "remind me in 5 minutes",
                reminder("Reminder", "2026-11-02", "10:06", false),
            ),
            (
                "remind me about the meeting at noon",
                reminder("The meeting", "2026-11-02", "12:00", false),
            ),
            (
                "remind me to call the 5th avenue office tomorrow at 9",
                reminder("Call the 5th avenue office", "2026-11-03", "09:00", false),
            ),
            (
                "remind me on march 15th to renew the passport",
                reminder("Renew the passport", "2027-03-15", "09:00", true),
            ),
        ] {
            assert_eq!(one(input), want, "{input}");
        }
    }

    #[test]
    fn reminder_times_use_the_utc_offset() {
        // 10:00:30 UTC is 15:30:30 in IST (+330); "at 5pm" is 17:00 local = 11:30 UTC
        let v = run_with("remind me at 5pm to call mom", &[], 330);
        let want_utc =
            Date::new(2026, 11, 2).unwrap().days() * 86_400_000 + (11 * 3600 + 30 * 60) * 1000;
        assert_eq!(
            v,
            vec![Intent::Reminder {
                title: "Call mom".into(),
                date: "2026-11-02".into(),
                time: "17:00".into(),
                due_utc_ms: want_utc,
                default_time: false,
            }]
        );
        // and a relative reminder keeps the local wall clock: 15:30:30 + 20 min -> 15:51
        match run_with("remind me in 20 minutes to stretch", &[], 330).remove(0) {
            Intent::Reminder { time, date, .. } => {
                assert_eq!((date.as_str(), time.as_str()), ("2026-11-02", "15:51"))
            }
            other => panic!("{other:?}"),
        }
        // west of UTC the local date can differ from the UTC date
        match run_with("remind me tomorrow at 9 to pay rent", &[], -660).remove(0) {
            Intent::Reminder { date, time, .. } => {
                assert_eq!((date.as_str(), time.as_str()), ("2026-11-02", "09:00"))
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn reminder_negatives() {
        for input in [
            "remind me",
            "remind me to stretch",
            "remind me to",
            "remind sam to call",
            "remind me to call mom sometime",
            "set a reminder",
            "remind me at",
        ] {
            let v = run(input);
            assert!(v.iter().all(Intent::is_unknown), "{input} -> {v:?}");
        }
        let s = assert_unknown("remind me to stretch");
        assert!(s.iter().any(|x| x.starts_with("remind me")));
    }

    // ---- notes ----

    #[test]
    fn notes() {
        for (input, body) in [
            ("note buy milk", "Buy milk"),
            ("Note: buy milk", "Buy milk"),
            ("take a note: call the dentist.", "Call the dentist"),
            ("take a note buy milk", "Buy milk"),
            ("make a note of the wifi password", "The wifi password"),
            ("add a note about the meeting", "The meeting"),
            ("new note: idea for the app", "Idea for the app"),
            ("create a note buy bread", "Buy bread"),
            ("jot down call the bank", "Call the bank"),
            ("write down the locker code 4521", "The locker code 4521"),
            ("note that the meeting moved to 3", "The meeting moved to 3"),
            ("note to self buy milk", "Buy milk"),
            ("note buy milk and eggs", "Buy milk and eggs"),
            ("note call mom, then dad", "Call mom, then dad"),
            ("take a note: \"quoted text\"", "Quoted text"),
            ("note search google for rust", "Search google for rust"),
            ("note mixed CASE Preserved", "Mixed CASE Preserved"),
            (
                "note caf\u{e9} \u{2014} cr\u{e8}me",
                "Caf\u{e9} \u{2014} cr\u{e8}me",
            ),
        ] {
            assert_eq!(one(input), Intent::Note { body: body.into() }, "{input}");
        }
    }

    #[test]
    fn note_negatives() {
        for input in [
            "note",
            "note:",
            "take a note",
            "make a note",
            "jot down",
            "note that",
        ] {
            let v = run(input);
            assert!(v.iter().all(Intent::is_unknown), "{input} -> {v:?}");
        }
    }

    // ---- email ----

    fn email(
        provider: MailProvider,
        to: &[&str],
        subject: &str,
        body: &str,
        bad: Option<&str>,
    ) -> Intent {
        Intent::Email {
            provider,
            to: to.iter().map(|s| s.to_string()).collect(),
            subject: subject.into(),
            body: body.into(),
            bad_recipient: bad.map(String::from),
        }
    }

    #[test]
    fn email_drafts() {
        use MailProvider::{Gmail, Outlook, SystemDefault as Def};
        for (input, want) in [
            (
                "email sam@x.com about the trip",
                email(Def, &["sam@x.com"], "The trip", "", None),
            ),
            (
                "draft an email to sam@x.com about the trip",
                email(Def, &["sam@x.com"], "The trip", "", None),
            ),
            (
                "draft an email to sam@x.com in gmail about the trip",
                email(Gmail, &["sam@x.com"], "The trip", "", None),
            ),
            (
                "draft an email to sam@x.com in outlook about the trip",
                email(Outlook, &["sam@x.com"], "The trip", "", None),
            ),
            (
                "write an email to sam@x.com using outlook saying see you at noon",
                email(Outlook, &["sam@x.com"], "", "See you at noon", None),
            ),
            (
                "compose a gmail to sam@x.com",
                email(Gmail, &["sam@x.com"], "", "", None),
            ),
            (
                "compose an outlook email to sam@x.com about lunch",
                email(Outlook, &["sam@x.com"], "Lunch", "", None),
            ),
            (
                "send an email to sam@x.com about the offsite",
                email(Def, &["sam@x.com"], "The offsite", "", None),
            ),
            ("email sam@x.com", email(Def, &["sam@x.com"], "", "", None)),
            (
                "Email Sam@X.com about Q3",
                email(Def, &["Sam@X.com"], "Q3", "", None),
            ),
            (
                "email sam@x.com about the trip saying i will be late",
                email(Def, &["sam@x.com"], "The trip", "I will be late", None),
            ),
            (
                "email sam@x.com subject lunch body see you there",
                email(Def, &["sam@x.com"], "Lunch", "See you there", None),
            ),
            (
                "email sam@x.com with subject is lunch",
                email(Def, &["sam@x.com"], "Lunch", "", None),
            ),
            (
                "email sam@x.com to say hi",
                email(Def, &["sam@x.com"], "", "Hi", None),
            ),
            (
                "email sam@x.com about the trip saying sorry about the delay",
                email(
                    Def,
                    &["sam@x.com"],
                    "The trip",
                    "Sorry about the delay",
                    None,
                ),
            ),
            (
                "email sam@x.com about the outlook for q3",
                email(Def, &["sam@x.com"], "The outlook for q3", "", None),
            ),
            (
                "email a@x.com and b@y.org about the offsite",
                email(Def, &["a@x.com", "b@y.org"], "The offsite", "", None),
            ),
            (
                "email sam at x dot com about lunch",
                email(Def, &["sam@x.com"], "Lunch", "", None),
            ),
            (
                "email sam dot smith at mail dot example dot org about lunch",
                email(Def, &["sam.smith@mail.example.org"], "Lunch", "", None),
            ),
            (
                "email sam@x.com lunch tomorrow",
                email(Def, &["sam@x.com"], "Lunch tomorrow", "", None),
            ),
            ("draft an email", email(Def, &[], "", "", None)),
            (
                "draft an email about the trip",
                email(Def, &[], "The trip", "", None),
            ),
            (
                "email mom about dinner",
                email(Def, &[], "Dinner", "", Some("mom")),
            ),
            (
                "draft an email to my boss about the offsite",
                email(Def, &[], "The offsite", "", Some("boss")),
            ),
            (
                "email notanaddress@ about x",
                email(Def, &["notanaddress@"], "X", "", None),
            ),
        ] {
            assert_eq!(one(input), want, "{input}");
        }
    }

    #[test]
    fn email_negatives() {
        for input in [
            "email",
            "mail",
            "send a note to sam",
            "write a letter to sam",
        ] {
            let v = run(input);
            assert!(
                !v.iter().any(|i| matches!(i, Intent::Email { .. })),
                "{input} -> {v:?}"
            );
        }
    }

    // ---- several steps ----

    #[test]
    fn multi_step_commands() {
        use MailProvider::SystemDefault as Def;
        assert_eq!(
            run("open youtube then open github"),
            vec![site("youtube"), site("github")]
        );
        assert_eq!(
            run("open youtube; open github"),
            vec![site("youtube"), site("github")]
        );
        assert_eq!(
            run("open youtube, and then open github, then open reddit"),
            vec![site("youtube"), site("github"), site("reddit")]
        );
        assert_eq!(
            run("open youtube after that open github"),
            vec![site("youtube"), site("github")]
        );
        assert_eq!(
            run("please open youtube and search github for krovi"),
            vec![site("youtube"), search("github", "krovi", false)]
        );
        let v = run("set a timer for 10 minutes and then open spotify");
        assert_eq!(v.len(), 2);
        assert_eq!(
            v[0],
            Intent::Timer {
                seconds: 600,
                label: None
            }
        );
        assert_app(&v[1], "Spotify");

        let v = run("note buy milk and open spotify");
        assert_eq!(
            v[0],
            Intent::Note {
                body: "Buy milk".into()
            }
        );
        assert_app(&v[1], "Spotify");

        assert_eq!(
            run("remind me in 20 minutes to stretch and then search google for stretches"),
            vec![
                reminder("Stretch", "2026-11-02", "10:21", false),
                search("google", "stretches", false)
            ]
        );
        assert_eq!(
            run("email sam@x.com about the trip and then open gmail"),
            vec![
                email(Def, &["sam@x.com"], "The trip", "", None),
                site("gmail")
            ]
        );
        assert_eq!(
            run("set a timer for 5 minutes and play lofi on youtube"),
            vec![
                Intent::Timer {
                    seconds: 300,
                    label: None
                },
                search("youtube", "lofi", false)
            ]
        );
    }

    #[test]
    fn opening_a_site_then_searching_it_is_one_search() {
        assert_eq!(
            run("open youtube and search for lofi"),
            vec![search("youtube", "lofi", false)]
        );
        assert_eq!(
            run("open google and search for rust"),
            vec![search("google", "rust", false)]
        );
        assert_eq!(
            run("open github then search for krovi"),
            vec![search("github", "krovi", false)]
        );
        // a site with no search page is not merged
        let v = run("open gmail and search for invoices");
        assert_eq!(v, vec![site("gmail"), search("google", "invoices", true)]);
        // an explicit engine is respected
        assert_eq!(
            run("open youtube and search google for lofi"),
            vec![site("youtube"), search("google", "lofi", false)]
        );
    }

    #[test]
    fn free_text_keeps_its_ands() {
        assert_eq!(
            one("note buy milk and eggs"),
            Intent::Note {
                body: "Buy milk and eggs".into()
            }
        );
        assert_eq!(
            one("search google for salt and pepper"),
            search("google", "salt and pepper", false)
        );
        assert_eq!(
            one("search for rust and go"),
            search("google", "rust and go", true)
        );
        assert_eq!(
            one("remind me to stretch and drink water in 20 minutes"),
            reminder("Stretch and drink water", "2026-11-02", "10:21", false)
        );
        assert_eq!(
            one("email sam@x.com about the trip and the budget"),
            email(
                MailProvider::SystemDefault,
                &["sam@x.com"],
                "The trip and the budget",
                "",
                None
            )
        );
        // free text that ends in something unparseable stays one note
        assert_eq!(
            one("note call mom and open the window"),
            Intent::Note {
                body: "Call mom and open the window".into()
            }
        );
    }

    #[test]
    fn a_follow_up_we_cannot_follow_is_its_own_failed_step() {
        let v = run("open youtube and take a nap");
        assert_eq!(v.len(), 2);
        assert_eq!(v[0], site("youtube"));
        assert!(v[1].is_unknown());
        let v = run("set a timer for 5 minutes and remind me to stretch");
        assert_eq!(v.len(), 2);
        assert_eq!(
            v[0],
            Intent::Timer {
                seconds: 300,
                label: None
            }
        );
        match &v[1] {
            Intent::Unknown { text, suggestions } => {
                assert_eq!(text, "remind me to stretch");
                assert!(suggestions.iter().any(|s| s.contains("remind me in")));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn plain_words_after_and_do_not_split() {
        match one("open youtube and foo bar") {
            Intent::OpenApp {
                app: AppMatch::NotFound,
                query,
            } => assert_eq!(query, "youtube and foo bar"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn app_names_with_and_are_not_split() {
        let apps = [app("Pencil and Paper")];
        assert_app(
            &run_with("open pencil and paper", &apps, 0)[0],
            "Pencil and Paper",
        );
    }

    // ---- politeness ----

    #[test]
    fn filler_is_ignored() {
        for input in [
            "please open youtube",
            "open youtube please",
            "can you open youtube",
            "could you please open youtube",
            "hey krovi, open youtube",
            "Krovi open youtube",
            "I want to open youtube",
            "I'd like to open youtube",
            "go ahead and open youtube",
            "open youtube for me",
            "open youtube, thanks",
            "okay krovi could you open youtube for me please",
        ] {
            assert_eq!(one(input), site("youtube"), "{input}");
        }
    }

    // ---- unknown ----

    #[test]
    fn unknown_sentences_get_suggestions() {
        for input in [
            "",
            "   ",
            "hello there",
            "what is the weather",
            "asdf",
            "???",
            "...",
            "dance",
        ] {
            assert_unknown(input);
        }
        let s = assert_unknown("tell me a joke");
        assert!(s.iter().any(|x| x.contains("open youtube")));
        assert!(assert_unknown("open").iter().any(|x| x.contains("youtube")));
    }

    #[test]
    fn suggestions_follow_the_first_word() {
        assert!(suggestions_for("remind")[0].starts_with("remind me"));
        assert!(suggestions_for("note")[0].starts_with("note"));
        assert!(suggestions_for("email")[0].starts_with("email"));
        assert!(suggestions_for("timer")[0].contains("timer"));
        assert!(suggestions_for("search")[0].starts_with("search"));
        assert!(suggestions_for("open")[0].starts_with("open"));
        assert!(suggestions_for("zzz").len() >= 4);
    }

    #[test]
    fn very_long_input_is_refused_not_parsed() {
        let long = "open youtube ".repeat(200);
        match one(&long) {
            Intent::Unknown { suggestions, .. } => assert!(suggestions[0].contains("too long")),
            other => panic!("{other:?}"),
        }
        let many_words = "a ".repeat(MAX_TOKENS + 1);
        assert!(matches!(one(&many_words), Intent::Unknown { .. }));
    }

    #[test]
    fn pathological_chains_terminate_quickly() {
        let chain = format!("note x {}", "and open a ".repeat(60));
        let started = std::time::Instant::now();
        let v = run(&chain);
        assert!(!v.is_empty());
        let chain2 = format!("open spotify {}zzz", "and open spotify ".repeat(40));
        let v2 = run(&chain2);
        assert!(!v2.is_empty());
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
    }

    #[test]
    fn script_injection_text_stays_inside_payloads() {
        assert_eq!(
            one("search google for <script>alert(1)</script> & calc.exe"),
            search("google", "<script>alert(1)</script> & calc.exe", false)
        );
        assert_eq!(
            one("note rm -rf / && shutdown /s"),
            Intent::Note {
                body: "Rm -rf / && shutdown /s".into()
            }
        );
    }
}
