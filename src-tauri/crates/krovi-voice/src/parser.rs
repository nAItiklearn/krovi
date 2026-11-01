//! Pure text -> [`VoiceCommand`] parser. No audio, no COM, no I/O: it runs on whatever text the
//! recognizer produced, whether that came from the command grammar ("start a timer for five
//! minutes") or from free dictation ("Start the timer for 5 minutes."), so it is deliberately
//! tolerant of filler, casing, punctuation, digits-vs-words and plural/synonym variants, and
//! returns `None` for anything it does not recognise.

use crate::types::{ToolId, VoiceCommand};

/// Spoken names for every tool. These are what the command grammar listens for, so keep them to
/// real, easily pronounced English words. Parser-only variants live in [`TOOL_ALIASES`].
pub(crate) const TOOL_PHRASES: &[(ToolId, &[&str])] = &[
    (
        ToolId::Home,
        &["home", "home screen", "dashboard", "overview"],
    ),
    (
        ToolId::Clipboard,
        &["clipboard", "clipboard history", "copy history", "clips"],
    ),
    (ToolId::Paste, &["paste stack", "paste", "paste slots"]),
    (
        ToolId::Timers,
        &["timers", "timer", "stopwatch", "countdown", "countdowns"],
    ),
    (
        ToolId::Pomodoro,
        &[
            "pomodoro",
            "pomodoros",
            "pomodoro timer",
            "focus",
            "focus timer",
            "focus mode",
            "focus session",
        ],
    ),
    (ToolId::Shelf, &["shelf", "file shelf", "files"]),
    (
        ToolId::Media,
        &["media", "music", "now playing", "player", "media controls"],
    ),
    (
        ToolId::Calendar,
        &["calendar", "agenda", "schedule", "events"],
    ),
    (ToolId::Weather, &["weather", "forecast"]),
    // "notepad" is deliberately NOT a synonym: it is the name of a Windows app (Voice v3 launches it).
    (ToolId::Notes, &["notes", "note", "notebook"]),
    (
        ToolId::Clocks,
        &[
            "clocks",
            "clock",
            "world clocks",
            "world clock",
            "time zones",
            "time zone",
        ],
    ),
    (
        ToolId::Convert,
        &[
            "convert",
            "converter",
            "conversion",
            "currency",
            "currency converter",
            "unit converter",
            "units",
            "exchange rates",
        ],
    ),
    (ToolId::Calculator, &["calculator", "calc"]),
    (
        ToolId::Screentime,
        &["screen time", "screentime", "app usage", "usage"],
    ),
    // Quick Transfer. "send to phone" is exactly the tool; "send THIS to my phone" (a reference word
    // plus a selection) stays the `transfer-send` intent - see `intent::is_phone_request`.
    (
        ToolId::Transfer,
        &[
            "transfer",
            "quick transfer",
            "send to phone",
            "phone transfer",
            "file transfer",
        ],
    ),
    // On-device summarizer + read-aloud. No other intent uses these words (checked by the tests).
    (
        ToolId::Explain,
        &[
            "explain",
            "explainer",
            "summarize",
            "summarizer",
            "summary",
            "read aloud",
        ],
    ),
    (
        ToolId::Settings,
        &["settings", "preferences", "options", "configuration"],
    ),
];

/// Extra spellings the parser accepts (typical dictation mis-hearings) but the grammar does not
/// list: out-of-vocabulary words like "pomodoro" are what free dictation gets wrong.
pub(crate) const TOOL_ALIASES: &[(ToolId, &[&str])] = &[
    (
        ToolId::Pomodoro,
        &[
            "pomo",
            "pomo doro",
            "poma doro",
            "pomadoro",
            "pomodora",
            "pomodoros timer",
            "pomodoro session",
            "pomodoro focus",
        ],
    ),
    (ToolId::Calendar, &["calender", "calendars"]),
    (ToolId::Clipboard, &["clip board", "clipboards"]),
    (ToolId::Shelf, &["file", "shelves", "file shelves"]),
    (ToolId::Timers, &["stop watch", "time", "timer s"]),
    (ToolId::Settings, &["setting", "preference"]),
    (ToolId::Notes, &["my notes", "new note", "new notes"]),
    (ToolId::Calendar, &["reminder", "reminders", "my reminders"]),
];

// ---------------------------------------------------------------------------------------------
// Tokenising
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub(crate) struct Tok {
    /// Lower-case alphanumeric word (digits kept, apostrophes dropped).
    pub(crate) norm: String,
    /// Byte offset in the original text where the whitespace-delimited chunk began.
    pub(crate) start: usize,
}

/// Just the normalized words of `text` (what the matchers see).
pub(crate) fn norm_words(text: &str) -> Vec<String> {
    tokenize(text).into_iter().map(|t| t.norm).collect()
}

pub(crate) fn tokenize(text: &str) -> Vec<Tok> {
    let mut chunks: Vec<(usize, usize)> = Vec::new();
    let mut start: Option<usize> = None;
    for (i, c) in text.char_indices() {
        if c.is_whitespace() {
            if let Some(s) = start.take() {
                chunks.push((s, i));
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(s) = start {
        chunks.push((s, text.len()));
    }

    let mut out = Vec::new();
    for (s, e) in chunks {
        for norm in split_chunk(&text[s..e]) {
            out.push(Tok { norm, start: s });
        }
    }
    out
}

fn split_chunk(chunk: &str) -> Vec<String> {
    let chars: Vec<char> = chunk.chars().collect();
    let mut parts: Vec<String> = Vec::new();
    let mut cur = String::new();
    for (i, &c) in chars.iter().enumerate() {
        if c.is_alphanumeric() {
            cur.extend(c.to_lowercase());
        } else if c == '\'' || c == '\u{2019}' || c == '\u{2018}' {
            // "what's" -> "whats"
        } else if c == '.'
            && i > 0
            && i + 1 < chars.len()
            && chars[i - 1].is_ascii_digit()
            && chars[i + 1].is_ascii_digit()
        {
            cur.push('.');
        } else if !cur.is_empty() {
            parts.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        parts.push(cur);
    }
    parts.into_iter().flat_map(split_digit_letter).collect()
}

/// "25min" -> ["25", "min"], "1.5h" -> ["1.5", "h"].
fn split_digit_letter(s: String) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut prev: Option<bool> = None;
    for c in s.chars() {
        let numeric = c.is_ascii_digit() || c == '.';
        if let Some(p) = prev {
            if p != numeric && !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
        }
        cur.push(c);
        prev = Some(numeric);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Filler
// ---------------------------------------------------------------------------------------------

/// Longest first within a shared prefix; the stripper loops until nothing matches.
const HEAD_FILLERS: &[&[&str]] = &[
    &["i", "would", "like", "to"],
    &["go", "ahead", "and"],
    &["hey", "krovi"],
    &["ok", "krovi"],
    &["okay", "krovi"],
    &["hi", "krovi"],
    &["i", "want", "to"],
    &["i", "need", "to"],
    &["id", "like", "to"],
    &["can", "you"],
    &["could", "you"],
    &["would", "you"],
    &["will", "you"],
    &["i", "wanna"],
    &["let", "us"],
    &["you", "can"],
    &["go", "ahead"],
    &["krovi"],
    &["hey"],
    &["okay"],
    &["ok"],
    &["um"],
    &["uh"],
    &["er"],
    &["erm"],
    &["hmm"],
    &["so"],
    &["well"],
    &["please"],
    &["kindly"],
    &["just"],
    &["then"],
    &["lets"],
];

const TAIL_FILLERS: &[&[&str]] = &[
    &["thank", "you"],
    &["right", "now"],
    &["for", "me"],
    &["please"],
    &["thanks"],
    &["now"],
    &["ok"],
    &["okay"],
];

pub(crate) fn strip_head_fillers(toks: &mut Vec<Tok>) {
    'again: loop {
        for pat in HEAD_FILLERS {
            if toks.len() >= pat.len() && toks.iter().zip(pat.iter()).all(|(t, p)| t.norm == *p) {
                toks.drain(0..pat.len());
                continue 'again;
            }
        }
        break;
    }
}

pub(crate) fn strip_tail_fillers(toks: &mut Vec<Tok>) {
    'again: loop {
        for pat in TAIL_FILLERS {
            if toks.len() > pat.len() {
                let off = toks.len() - pat.len();
                if toks[off..]
                    .iter()
                    .zip(pat.iter())
                    .all(|(t, p)| t.norm == *p)
                {
                    toks.truncate(off);
                    continue 'again;
                }
            }
        }
        break;
    }
}

// ---------------------------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------------------------

/// Parse recognised text into a command using the real clock and no captured context.
/// Intents that need "this" (shelf, transfer, save-as-note) return `None` here; use
/// [`crate::intent::parse`] to supply context and a fixed clock.
pub fn parse_command(text: &str) -> Option<VoiceCommand> {
    crate::intent::parse(text, &crate::intent::Env::now(None)).command()
}

/// Tidy free dictation for storing as a note: collapse whitespace, upper-case the first letter.
pub fn tidy_text(text: &str) -> String {
    let joined = text.split_whitespace().collect::<Vec<_>>().join(" ");
    capitalize_first(joined.trim())
}

pub(crate) fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

// ---------------------------------------------------------------------------------------------
// new-note
// ---------------------------------------------------------------------------------------------

/// (prefix words, whether a following "to" belongs to the prefix rather than the body)
const NOTE_PREFIXES: &[(&[&str], bool)] = &[
    (&["new", "note"], false),
    (&["add", "a", "new", "note"], true),
    (&["add", "a", "note"], true),
    (&["add", "note"], true),
    (&["create", "a", "new", "note"], true),
    (&["create", "a", "note"], true),
    (&["create", "note"], true),
    (&["make", "a", "new", "note"], true),
    (&["make", "a", "note"], true),
    (&["make", "note"], true),
    (&["take", "a", "note"], true),
    (&["take", "note"], true),
    (&["write", "a", "note"], true),
    (&["write", "note"], true),
    (&["write", "down"], true),
    (&["jot", "down"], true),
    (&["note", "down"], true),
    (&["jot"], true),
    (&["note", "to", "self"], false),
    (&["note", "that"], false),
    (&["note"], true),
];

/// What a "new note ..." style prefix produced.
pub(crate) enum NoteOutcome {
    /// A body was spoken after the prefix.
    Body(String),
    /// The prefix named a note but nothing followed.
    OpenNotes,
}

pub(crate) fn try_new_note(toks: &[Tok], text: &str) -> Option<NoteOutcome> {
    let words: Vec<&str> = toks.iter().map(|t| t.norm.as_str()).collect();

    let mut best: Option<(usize, bool, bool)> = None;
    for (pat, to_ok) in NOTE_PREFIXES {
        if words.len() >= pat.len()
            && words[..pat.len()] == **pat
            && best.is_none_or(|(l, _, _)| pat.len() > l)
        {
            best = Some((pat.len(), *to_ok, pat.contains(&"note")));
        }
    }
    let (mut i, to_ok, names_a_note) = best?;

    // Connectors between the prefix and the body: "note that ...", "make a note to ...".
    loop {
        match words.get(i).copied() {
            Some("that") | Some("saying") | Some("says") | Some("reading") | Some("colon") => {
                i += 1
            }
            Some("to") if words.get(i + 1) == Some(&"self") => i += 2,
            Some("to") if to_ok && i + 1 < words.len() => i += 1,
            _ => break,
        }
    }

    // "take a note" / "new note" with nothing after it: the user wants the notes tool.
    let open_notes = || names_a_note.then_some(NoteOutcome::OpenNotes);
    let Some(first_body) = toks.get(i) else {
        return open_notes();
    };
    let body = text[first_body.start..]
        .trim()
        .trim_start_matches([':', ',', ';', '-', '\u{2013}', '\u{2014}', '.'])
        .trim();
    if body.is_empty() {
        return open_notes();
    }
    Some(NoteOutcome::Body(capitalize_first(body)))
}

// ---------------------------------------------------------------------------------------------
// collapse
// ---------------------------------------------------------------------------------------------

pub(crate) fn try_collapse(w: &[&str]) -> Option<VoiceCommand> {
    const VERBS: &[&str] = &[
        "close", "hide", "dismiss", "collapse", "minimize", "minimise", "shrink",
    ];
    const REST_OK: &[&str] = &[
        "the",
        "this",
        "that",
        "it",
        "panel",
        "krovi",
        "window",
        "shelf",
        "island",
        "up",
        "everything",
        "popup",
        "menu",
        "app",
        "pill",
        "bar",
        "down",
        "away",
    ];
    const PHRASES: &[&[&str]] = &[
        &["go", "away"],
        &["thats", "all"],
        &["that", "is", "all"],
        &["never", "mind"],
        &["nevermind"],
        &["all", "done"],
        &["im", "done"],
        &["we", "are", "done"],
        &["get", "out", "of", "the", "way"],
    ];
    if PHRASES.contains(&w) {
        return Some(VoiceCommand::Collapse);
    }
    if VERBS.contains(&w[0]) && w.len() <= 5 && w[1..].iter().all(|t| REST_OK.contains(t)) {
        return Some(VoiceCommand::Collapse);
    }
    None
}

// ---------------------------------------------------------------------------------------------
// numbers and durations
// ---------------------------------------------------------------------------------------------

fn unit_secs(w: &str) -> Option<f64> {
    match w {
        "second" | "seconds" | "sec" | "secs" | "s" => Some(1.0),
        "minute" | "minutes" | "min" | "mins" | "m" => Some(60.0),
        "hour" | "hours" | "hr" | "hrs" | "h" => Some(3600.0),
        _ => None,
    }
}

#[derive(Clone, Copy, PartialEq)]
enum NumKind {
    None,
    Ones,
    Teen,
    Tens,
    Hundred,
}

fn number_word(w: &str) -> Option<(u64, NumKind)> {
    Some(match w {
        "zero" => (0, NumKind::Ones),
        "one" => (1, NumKind::Ones),
        "two" => (2, NumKind::Ones),
        "three" => (3, NumKind::Ones),
        "four" => (4, NumKind::Ones),
        "five" => (5, NumKind::Ones),
        "six" => (6, NumKind::Ones),
        "seven" => (7, NumKind::Ones),
        "eight" => (8, NumKind::Ones),
        "nine" => (9, NumKind::Ones),
        "ten" => (10, NumKind::Teen),
        "eleven" => (11, NumKind::Teen),
        "twelve" => (12, NumKind::Teen),
        "thirteen" => (13, NumKind::Teen),
        "fourteen" => (14, NumKind::Teen),
        "fifteen" => (15, NumKind::Teen),
        "sixteen" => (16, NumKind::Teen),
        "seventeen" => (17, NumKind::Teen),
        "eighteen" => (18, NumKind::Teen),
        "nineteen" => (19, NumKind::Teen),
        "twenty" => (20, NumKind::Tens),
        "thirty" => (30, NumKind::Tens),
        "forty" | "fourty" => (40, NumKind::Tens),
        "fifty" => (50, NumKind::Tens),
        "sixty" => (60, NumKind::Tens),
        "seventy" => (70, NumKind::Tens),
        "eighty" => (80, NumKind::Tens),
        "ninety" => (90, NumKind::Tens),
        _ => return None,
    })
}

/// A quantity starting at `w[i]`: digits ("25", "1.5"), number words ("twenty five"),
/// "a"/"an" (only when a unit follows), "half" and "quarter". Returns (value, tokens used).
pub(crate) fn parse_quantity(w: &[&str], i: usize) -> Option<(f64, usize)> {
    let t = w[i];
    if t.starts_with(|c: char| c.is_ascii_digit()) {
        let n: f64 = t.parse().ok()?;
        return if n.is_finite() && n < 100_000.0 {
            Some((n, 1))
        } else {
            None
        };
    }
    match t {
        "half" => return Some((0.5, 1)),
        "quarter" => return Some((0.25, 1)),
        "a" | "an" => {
            return if w.get(i + 1).and_then(|n| unit_secs(n)).is_some() {
                Some((1.0, 1))
            } else {
                None
            };
        }
        _ => {}
    }

    let mut j = i;
    let mut cur = 0u64;
    let mut last = NumKind::None;
    while j < w.len() {
        let x = w[j];
        if let Some((v, kind)) = number_word(x) {
            let ok = match kind {
                NumKind::Ones => {
                    matches!(last, NumKind::None | NumKind::Hundred | NumKind::Tens)
                        && !(last == NumKind::Tens && v == 0)
                }
                NumKind::Teen | NumKind::Tens => matches!(last, NumKind::None | NumKind::Hundred),
                _ => false,
            };
            if !ok {
                break;
            }
            cur += v;
            last = kind;
            j += 1;
        } else if x == "hundred" && last == NumKind::Ones && (1..=9).contains(&cur) {
            cur *= 100;
            last = NumKind::Hundred;
            j += 1;
        } else if x == "and"
            && last == NumKind::Hundred
            && w.get(j + 1).and_then(|n| number_word(n)).is_some()
        {
            j += 1;
        } else {
            break;
        }
    }
    if j > i {
        Some((cur as f64, j - i))
    } else {
        None
    }
}

fn is_unit(w: Option<&&str>) -> Option<f64> {
    w.and_then(|u| unit_secs(u))
}

/// Longest timer we accept (100 hours): "ninety nine hours" is sayable, 500 hours is noise.
const MAX_TIMER_SECS: f64 = 360_000.0;

/// Total seconds mentioned in `w`. A lone number with no unit counts as minutes when
/// `allow_bare` ("timer for ten"). Returns `None` for zero or more than [`MAX_TIMER_SECS`].
fn parse_duration(w: &[&str], allow_bare: bool) -> Option<u64> {
    let mut total = 0.0f64;
    let mut found = false;
    let mut bare: Option<f64> = None;
    let mut i = 0;
    while i < w.len() {
        let Some((q, used)) = parse_quantity(w, i) else {
            i += 1;
            continue;
        };
        let mut j = i + used;
        let fractional_word = matches!(w[i], "half" | "quarter");
        if fractional_word {
            // "half an hour", "a quarter of an hour"
            while j < w.len() && matches!(w[j], "an" | "a" | "of") {
                j += 1;
            }
        }
        let mut qq = q;
        // "two and a half minutes"
        if w.get(j) == Some(&"and") {
            if w.get(j + 1) == Some(&"a")
                && w.get(j + 2) == Some(&"half")
                && is_unit(w.get(j + 3)).is_some()
            {
                qq += 0.5;
                j += 3;
            } else if w.get(j + 1) == Some(&"half") && is_unit(w.get(j + 2)).is_some() {
                qq += 0.5;
                j += 2;
            }
        }
        if let Some(mult) = is_unit(w.get(j)) {
            j += 1;
            // "an hour and a half"
            if w.get(j) == Some(&"and") {
                if w.get(j + 1) == Some(&"a") && w.get(j + 2) == Some(&"half") {
                    qq += 0.5;
                    j += 3;
                } else if w.get(j + 1) == Some(&"half") {
                    qq += 0.5;
                    j += 2;
                }
            }
            total += qq * mult;
            found = true;
            i = j;
            continue;
        }
        if bare.is_none() && !fractional_word && !matches!(w[i], "a" | "an") {
            bare = Some(q);
        }
        i += used.max(1);
    }
    if !found && allow_bare {
        if let Some(b) = bare {
            total = b * 60.0;
            found = true;
        }
    }
    if !found {
        return None;
    }
    let secs = total.round();
    if !(1.0..=MAX_TIMER_SECS).contains(&secs) {
        return None;
    }
    Some(secs as u64)
}

// ---------------------------------------------------------------------------------------------
// start-timer
// ---------------------------------------------------------------------------------------------

pub(crate) fn try_timer(w: &[&str]) -> Option<VoiceCommand> {
    if !w
        .iter()
        .any(|t| matches!(*t, "timer" | "timers" | "countdown"))
    {
        return None;
    }
    if matches!(
        w[0],
        "stop"
            | "cancel"
            | "pause"
            | "reset"
            | "delete"
            | "remove"
            | "clear"
            | "end"
            | "kill"
            | "turn"
            | "resume"
    ) {
        return None;
    }
    if w.iter().any(|t| is_pomodoro_word(t) || *t == "focus") {
        return None; // "pomodoro timer for 25 minutes" is a pomodoro, handled next
    }
    let seconds = parse_duration(w, true)?;
    Some(VoiceCommand::StartTimer { seconds })
}

// ---------------------------------------------------------------------------------------------
// start-pomodoro
// ---------------------------------------------------------------------------------------------

fn is_pomodoro_word(w: &str) -> bool {
    matches!(
        w,
        "pomodoro" | "pomodoros" | "pomo" | "pomodora" | "pomadoro"
    )
}

fn starts_with_start_verb(w: &[&str]) -> Option<usize> {
    match w {
        ["kick", "off", ..] | ["fire", "up", ..] => Some(2),
        [first, ..]
            if matches!(
                *first,
                "start" | "begin" | "run" | "launch" | "resume" | "new" | "do" | "go"
            ) =>
        {
            Some(1)
        }
        _ => None,
    }
}

pub(crate) fn try_pomodoro(w: &[&str]) -> Option<VoiceCommand> {
    let skip = starts_with_start_verb(w)?;
    let rest: Vec<&str> = w[skip..]
        .iter()
        .copied()
        .filter(|t| {
            !matches!(
                *t,
                "a" | "an" | "the" | "my" | "new" | "another" | "one" | "of"
            )
        })
        .collect();
    if rest.is_empty() {
        return None;
    }
    if rest.iter().any(|t| is_pomodoro_word(t))
        || (rest.len() >= 2 && rest[0] == "pomo" && rest[1] == "doro")
    {
        return Some(VoiceCommand::StartPomodoro);
    }
    let session_word = |t: &str| {
        matches!(
            t,
            "session" | "timer" | "mode" | "time" | "sprint" | "block" | "round" | "cycle"
        )
    };
    match rest.as_slice() {
        ["focus"] => Some(VoiceCommand::StartPomodoro),
        ["focus", s, ..] if session_word(s) => Some(VoiceCommand::StartPomodoro),
        ["work" | "study", s, ..] if session_word(s) => Some(VoiceCommand::StartPomodoro),
        _ => None,
    }
}

// ---------------------------------------------------------------------------------------------
// open-tool
// ---------------------------------------------------------------------------------------------

/// Verb phrases that mean "go to a tool". Longest first.
const OPEN_VERBS: &[&[&str]] = &[
    &["take", "me", "to"],
    &["go", "to"],
    &["switch", "to"],
    &["jump", "to"],
    &["bring", "up"],
    &["pull", "up"],
    &["look", "at"],
    &["navigate", "to"],
    &["open", "up"],
    &["show", "me"],
    &["what", "is"],
    &["how", "is"],
    &["whats"],
    &["hows"],
    &["open"],
    &["show"],
    &["display"],
    &["launch"],
    &["view"],
    &["see"],
    &["check"],
    &["start"],
    &["begin"],
    &["run"],
    &["goto"],
    &["go"],
    &["set"],
];

const DETERMINERS: &[&str] = &[
    "the", "a", "an", "my", "me", "our", "your", "this", "some", "up", "to", "for",
];
const TRAILING_NOUNS: &[&str] = &[
    "tool", "app", "panel", "view", "tab", "page", "section", "widget", "card", "please", "now",
    "for", "me", "screen",
];

fn collapsed(words: &[&str]) -> String {
    words.concat()
}

pub(crate) fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut cur = vec![i; b.len() + 1];
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        prev = cur;
    }
    prev[b.len()]
}

fn all_phrases() -> impl Iterator<Item = (ToolId, &'static str)> {
    TOOL_PHRASES
        .iter()
        .chain(TOOL_ALIASES.iter())
        .flat_map(|(t, ps)| ps.iter().map(move |p| (*t, *p)))
}

/// Map the words after the verb to a tool: exact phrase first, then (if `fuzzy`) a close spelling.
fn resolve_tool(rem: &[&str], fuzzy: bool) -> Option<ToolId> {
    if rem.is_empty() {
        return None;
    }
    let joined = rem.join(" ");
    if let Some((tool, _)) = all_phrases().find(|(_, p)| *p == joined) {
        return Some(tool);
    }
    let flat = collapsed(rem);
    if let Some((tool, _)) = all_phrases().find(|(_, p)| p.replace(' ', "") == flat) {
        return Some(tool);
    }
    if !fuzzy || flat.chars().count() < 5 {
        return None;
    }
    let max_dist = if flat.chars().count() >= 9 { 2 } else { 1 };
    let mut best: Option<(usize, ToolId)> = None;
    let mut tie = false;
    for (tool, p) in all_phrases() {
        let cand = p.replace(' ', "");
        if cand.chars().count() < 5 {
            continue;
        }
        let d = edit_distance(&flat, &cand);
        if d > max_dist {
            continue;
        }
        match best {
            Some((bd, bt)) if d == bd && bt != tool => tie = true,
            Some((bd, _)) if d < bd => {
                best = Some((d, tool));
                tie = false;
            }
            None => best = Some((d, tool)),
            _ => {}
        }
    }
    if tie {
        None
    } else {
        best.map(|(_, t)| t)
    }
}

/// Whether these words name one of Krovi's own tools exactly (no fuzzy matching).
pub(crate) fn is_tool_name(rest: &[&str]) -> bool {
    resolve_tool(rest, false).is_some()
}

pub(crate) fn try_open(w: &[&str]) -> Option<VoiceCommand> {
    let verb_len = OPEN_VERBS
        .iter()
        .filter(|v| w.len() > v.len() && w[..v.len()] == ***v)
        .map(|v| v.len())
        .max();

    let mut rest: &[&str] = &w[verb_len.unwrap_or(0)..];
    while let Some((first, tail)) = rest.split_first() {
        if DETERMINERS.contains(first) {
            rest = tail;
        } else {
            break;
        }
    }
    while let Some((last, head)) = rest.split_last() {
        if TRAILING_NOUNS.contains(last) && !head.is_empty() {
            rest = head;
        } else {
            break;
        }
    }
    if rest.is_empty() {
        return None;
    }

    if verb_len.is_some() {
        resolve_tool(rest, true).map(|tool| VoiceCommand::OpenTool { tool })
    } else if rest.len() <= 3 {
        // A bare tool name ("notes", "world clocks") is fine; arbitrary chatter is not.
        resolve_tool(rest, false).map(|tool| VoiceCommand::OpenTool { tool })
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ToolId::*;
    use VoiceCommand::*;

    fn open(t: ToolId) -> Option<VoiceCommand> {
        Some(OpenTool { tool: t })
    }
    fn timer(s: u64) -> Option<VoiceCommand> {
        Some(StartTimer { seconds: s })
    }
    fn note(b: &str) -> Option<VoiceCommand> {
        Some(NewNote {
            body: b.to_string(),
        })
    }

    fn check(cases: &[(&str, Option<VoiceCommand>)]) {
        for (text, want) in cases {
            assert_eq!(&parse_command(text), want, "input: {text:?}");
        }
    }

    #[test]
    fn open_tools_with_verbs_and_synonyms() {
        check(&[
            ("open timers", open(Timers)),
            ("Open Timers.", open(Timers)),
            ("open the timers", open(Timers)),
            ("open timer", open(Timers)),
            ("show me the clipboard", open(Clipboard)),
            ("show clipboard history", open(Clipboard)),
            ("go to notes", open(Notes)),
            ("go to the calendar", open(Calendar)),
            ("switch to weather", open(Weather)),
            ("take me to settings", open(Settings)),
            ("launch the calculator", open(Calculator)),
            ("open calculator app", open(Calculator)),
            ("bring up the paste stack", open(Paste)),
            ("open world clocks", open(Clocks)),
            ("open screen time", open(Screentime)),
            ("open the file shelf", open(Shelf)),
            ("show me the weather please", open(Weather)),
            ("what's the weather", open(Weather)),
            ("whats the forecast", open(Weather)),
            ("open currency converter", open(Convert)),
            ("go home", open(Home)),
            ("open the home screen", open(Home)),
            ("open media", open(Media)),
            ("show now playing", open(Media)),
            ("open pomodoro", open(Pomodoro)),
            ("open the pomodoro timer", open(Pomodoro)),
            ("could you please open the notes", open(Notes)),
            ("hey krovi open timers", open(Timers)),
            ("okay, open the settings now", open(Settings)),
        ]);
    }

    #[test]
    fn the_explain_tool_answers_to_its_synonyms() {
        for name in [
            "explain",
            "explainer",
            "summarize",
            "summarizer",
            "summary",
            "read aloud",
        ] {
            for said in [
                format!("open {name}"),
                format!("show me the {name}"),
                format!("go to {name}"),
            ] {
                assert_eq!(parse_command(&said), open(Explain), "input: {said:?}");
            }
        }
        check(&[
            ("launch the summarizer", open(Explain)),
            ("bring up explain", open(Explain)),
            // typed bare names, like every other tool
            ("summarize", open(Explain)),
            ("Explainer.", open(Explain)),
            ("read aloud", open(Explain)),
        ]);
    }

    #[test]
    fn the_transfer_tool_answers_to_its_synonyms_and_leaves_the_shelf_alone() {
        for name in [
            "transfer",
            "quick transfer",
            "send to phone",
            "phone transfer",
            "file transfer",
        ] {
            for said in [
                format!("open {name}"),
                format!("show me the {name}"),
                format!("go to {name}"),
                name.to_string(),
            ] {
                assert_eq!(parse_command(&said), open(Transfer), "input: {said:?}");
            }
        }
        check(&[
            ("launch quick transfer", open(Transfer)),
            ("open file transfer", open(Transfer)),
            ("open the file shelf", open(Shelf)),
            ("open files", open(Shelf)),
            ("files", open(Shelf)),
            ("show me the shelf", open(Shelf)),
            // sentences are not the tool
            ("send the files to my phone", None),
            ("I want to transfer the money", None),
        ]);
    }

    #[test]
    fn the_explain_words_do_not_hijack_other_intents() {
        // sentences that merely contain the words are not commands
        for s in [
            "summarize this meeting for me",
            "explain how this works",
            "read aloud the first paragraph",
            "I need a summary of the report",
        ] {
            assert_eq!(parse_command(s), None, "{s}");
        }
        // the neighbouring intents keep their meaning
        check(&[
            ("new note summary of the call", note("Summary of the call")),
            ("open notes", open(Notes)),
            ("open the calendar", open(Calendar)),
        ]);
    }

    #[test]
    fn bare_tool_names_open_the_tool() {
        check(&[
            ("timers", open(Timers)),
            ("notes", open(Notes)),
            ("clipboard", open(Clipboard)),
            ("world clocks", open(Clocks)),
            ("Weather.", open(Weather)),
        ]);
    }

    #[test]
    fn near_miss_spellings_still_resolve_to_long_tool_names() {
        check(&[
            ("open calender", open(Calendar)),
            ("open pomodoros", open(Pomodoro)),
            ("open the clip board", open(Clipboard)),
            ("open calculater", open(Calculator)),
        ]);
    }

    #[test]
    fn start_pomodoro() {
        check(&[
            ("start pomodoro", Some(StartPomodoro)),
            ("Start pomodoro.", Some(StartPomodoro)),
            ("start a pomodoro", Some(StartPomodoro)),
            ("start the pomodoro timer", Some(StartPomodoro)),
            ("begin a pomodoro session", Some(StartPomodoro)),
            ("start a 25 minute pomodoro", Some(StartPomodoro)),
            ("start focus", Some(StartPomodoro)),
            ("start a focus session", Some(StartPomodoro)),
            ("start focus timer", Some(StartPomodoro)),
            ("let's start a pomodoro", Some(StartPomodoro)),
            ("kick off a pomodoro", Some(StartPomodoro)),
            ("start pomo doro", Some(StartPomodoro)),
            ("please start the pomodoro", Some(StartPomodoro)),
        ]);
    }

    #[test]
    fn start_timer_with_durations() {
        check(&[
            ("start a timer for five minutes", timer(300)),
            ("Start a timer for 5 minutes.", timer(300)),
            ("start the timer for five minutes", timer(300)),
            ("set a timer for 25 minutes", timer(1500)),
            ("set a timer for 25 min", timer(1500)),
            ("timer for ten minutes", timer(600)),
            ("start a five minute timer", timer(300)),
            ("start a 25-minute timer", timer(1500)),
            ("5 minute timer", timer(300)),
            ("set a timer for ninety seconds", timer(90)),
            ("set a timer for 90 seconds", timer(90)),
            ("start a timer for twenty five minutes", timer(1500)),
            ("start a timer for one hour", timer(3600)),
            ("start a timer for an hour", timer(3600)),
            ("timer for half an hour", timer(1800)),
            ("set a timer for one hour and thirty minutes", timer(5400)),
            ("set a timer for 1 hour 30 minutes", timer(5400)),
            ("set a timer for an hour and a half", timer(5400)),
            ("set a timer for two and a half minutes", timer(150)),
            ("set a timer for a minute", timer(60)),
            ("set a timer for thirty seconds please", timer(30)),
            ("start a timer for 25mins", timer(1500)),
            ("start a timer for 2h", timer(7200)),
            ("could you set a timer for fifteen minutes", timer(900)),
            ("start a countdown for three minutes", timer(180)),
            ("set a timer for one hundred twenty seconds", timer(120)),
            ("start a timer for ten", timer(600)),
        ]);
    }

    #[test]
    fn timer_without_duration_just_opens_timers() {
        check(&[
            ("start a timer", open(Timers)),
            ("open the stopwatch", open(Timers)),
            ("start stopwatch", open(Timers)),
        ]);
    }

    #[test]
    fn absurd_or_zero_durations_are_rejected_not_clamped() {
        assert_eq!(parse_command("set a timer for zero minutes"), None);
        assert_eq!(parse_command("set a timer for 500 hours"), None);
        // Cancelling is not starting.
        assert_eq!(parse_command("cancel the five minute timer"), None);
        // ... and "stop the timer" is timer control (Voice v2), never a new timer.
        assert!(matches!(
            parse_command("stop the timer"),
            Some(VoiceCommand::TimerControl { .. })
        ));
    }

    #[test]
    fn new_note_keeps_the_body_verbatim() {
        check(&[
            ("new note buy milk", note("Buy milk")),
            ("New note: buy milk.", note("Buy milk.")),
            (
                "new note buy milk, eggs and bread",
                note("Buy milk, eggs and bread"),
            ),
            ("take a note call mom tomorrow", note("Call mom tomorrow")),
            ("make a note to call mom", note("Call mom")),
            (
                "add a note saying pick up the parcel",
                note("Pick up the parcel"),
            ),
            (
                "create a new note remember the charger",
                note("Remember the charger"),
            ),
            ("note to self buy milk", note("Buy milk")),
            (
                "note that the meeting moved to 3 pm",
                note("The meeting moved to 3 pm"),
            ),
            (
                "write down buy a birthday card",
                note("Buy a birthday card"),
            ),
            ("jot down tickets for friday", note("Tickets for friday")),
            ("please new note buy milk", note("Buy milk")),
            ("hey krovi, new note buy milk", note("Buy milk")),
            (
                "new note to do list for monday",
                note("To do list for monday"),
            ),
        ]);
    }

    #[test]
    fn empty_note_opens_notes() {
        check(&[
            ("new note", open(Notes)),
            ("open notes", open(Notes)),
            ("note", open(Notes)),
        ]);
    }

    #[test]
    fn collapse_commands() {
        check(&[
            ("close", Some(Collapse)),
            ("Close.", Some(Collapse)),
            ("close the panel", Some(Collapse)),
            ("close krovi", Some(Collapse)),
            ("please close it", Some(Collapse)),
            ("hide", Some(Collapse)),
            ("hide the panel", Some(Collapse)),
            ("dismiss", Some(Collapse)),
            ("collapse", Some(Collapse)),
            ("go away", Some(Collapse)),
            ("that's all", Some(Collapse)),
            ("never mind", Some(Collapse)),
        ]);
    }

    #[test]
    fn garbage_and_chatter_return_none() {
        check(&[
            ("", None),
            ("   ", None),
            ("...", None),
            ("hmm", None),
            ("um", None),
            ("the quick brown fox jumps over the lazy dog", None),
            ("open the pod bay doors", None),
            ("what time is it in tokyo", None),
            ("asdf qwer zxcv", None),
            ("start", None),
            ("open", None),
            ("close the deal on the house", None),
            ("pomodoro cake recipe for dinner tonight", None),
            ("1 2 3 4 5", None),
        ]);
    }

    #[test]
    fn durations_parse_directly() {
        let d = |s: &str| {
            let t = tokenize(s);
            let w: Vec<&str> = t.iter().map(|t| t.norm.as_str()).collect();
            parse_duration(&w, false)
        };
        assert_eq!(d("five minutes"), Some(300));
        assert_eq!(d("1.5 hours"), Some(5400));
        assert_eq!(d("a quarter of an hour"), Some(900));
        assert_eq!(d("two hours thirty minutes"), Some(9000));
        assert_eq!(d("one hundred and five seconds"), Some(105));
        assert_eq!(d("five"), None, "no unit and bare not allowed");
        assert_eq!(
            d("twenty twenty minutes"),
            Some(1200),
            "'twenty twenty' must not merge into forty"
        );
        assert_eq!(d("0 minutes"), None);
    }

    #[test]
    fn tokenizer_splits_hyphens_digits_and_apostrophes() {
        let t = |s: &str| tokenize(s).into_iter().map(|t| t.norm).collect::<Vec<_>>();
        assert_eq!(t("25-minute"), ["25", "minute"]);
        assert_eq!(t("25min"), ["25", "min"]);
        assert_eq!(t("What's the weather?"), ["whats", "the", "weather"]);
        assert_eq!(t("1.5h"), ["1.5", "h"]);
        assert_eq!(t("Twenty-five"), ["twenty", "five"]);
    }

    #[test]
    fn tidy_text_normalises_dictation() {
        assert_eq!(
            tidy_text("  remember   to call the dentist \n"),
            "Remember to call the dentist"
        );
        assert_eq!(tidy_text(""), "");
        assert_eq!(tidy_text("Already fine."), "Already fine.");
    }

    #[test]
    fn edit_distance_basics() {
        assert_eq!(edit_distance("kitten", "sitting"), 3);
        assert_eq!(edit_distance("same", "same"), 0);
        assert_eq!(edit_distance("", "abc"), 3);
    }
}
