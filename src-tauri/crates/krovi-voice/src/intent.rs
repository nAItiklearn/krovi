//! The full command parser: text (+ the captured context, + a clock) -> [`Parsed`].
//!
//! Layered in front of the original `parser` matchers. Order matters and is deliberate:
//! reminders, then notes (including "save this as a note"), then context actions (shelf, phone),
//! then timer control and media, and finally the original collapse / timer / pomodoro / open-tool
//! matchers. Everything is pure: pass a fixed [`Env`] and the result is deterministic.

use std::sync::Arc;

use crate::catalog::Catalog;
use crate::civil::{hhmm, LocalNow};
use crate::osintent::{self, OsOpen};
use crate::parser::{
    capitalize_first, is_tool_name, strip_head_fillers, strip_tail_fillers, tokenize, try_collapse,
    try_new_note, try_open, try_pomodoro, try_timer, NoteOutcome, Tok,
};
use crate::types::{
    MediaCommand, ResultItem, TimerAction, TimerTarget, ToolId, VoiceCommand, VoiceContext,
};
use crate::when;

/// Everything the parser needs besides the words.
#[derive(Debug, Clone)]
pub struct Env {
    pub now: LocalNow,
    /// What "this" / "it" refers to, captured when the session started.
    pub context: Option<VoiceContext>,
    /// Tidy SPOKEN note bodies and reminder titles (fillers, stutters). Text that comes from the
    /// captured selection is never touched.
    pub clean: bool,
    /// The folders and apps "open ..." / "what's in ..." may name (Voice v3). `None`: those
    /// phrasings are not understood.
    pub catalog: Option<Arc<Catalog>>,
}

impl Env {
    /// The real local clock.
    pub fn now(context: Option<VoiceContext>) -> Env {
        Env::at(LocalNow::system(), context)
    }

    pub fn at(now: LocalNow, context: Option<VoiceContext>) -> Env {
        Env {
            now,
            context,
            clean: false,
            catalog: None,
        }
    }

    pub fn with_clean(mut self, clean: bool) -> Env {
        self.clean = clean;
        self
    }

    pub fn with_catalog(mut self, catalog: Option<Arc<Catalog>>) -> Env {
        self.catalog = catalog;
        self
    }

    fn spoken(&self, text: String) -> String {
        if self.clean {
            let cleaned = crate::clean::clean_fragment(&text);
            if cleaned.is_empty() {
                text
            } else {
                cleaned
            }
        } else {
            text
        }
    }
}

/// An action that needs a selection ("this") but none was captured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextAction {
    SaveAsNote,
    AddToShelf,
    SendToPhone,
}

impl ContextAction {
    /// What the user was trying to do, for the "nothing selected" feedback.
    pub fn describe(self) -> &'static str {
        match self {
            ContextAction::SaveAsNote => "save as a note",
            ContextAction::AddToShelf => "put on the file shelf",
            ContextAction::SendToPhone => "send to your phone",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Parsed {
    Command(VoiceCommand),
    /// Understood, but it refers to a selection that was not captured.
    NeedsContext(ContextAction),
    /// Understood as an "open / what's in / find" request, but it names nothing in the catalog (or
    /// several things equally): nothing is done, and this is what to tell the user.
    Unresolved {
        feedback: String,
        results: Vec<ResultItem>,
    },
    Unknown,
}

impl Parsed {
    pub fn command(self) -> Option<VoiceCommand> {
        match self {
            Parsed::Command(c) => Some(c),
            _ => None,
        }
    }
}

const REF_WORDS: &[&str] = &[
    "this",
    "that",
    "it",
    "these",
    "those",
    "them",
    "selection",
    "selected",
];

fn has_ref(w: &[&str]) -> bool {
    w.iter().any(|t| REF_WORDS.contains(t))
}

fn contains(w: &[&str], word: &str) -> bool {
    w.contains(&word)
}

// ---------------------------------------------------------------------------------------------
// entry point
// ---------------------------------------------------------------------------------------------

pub fn parse(text: &str, env: &Env) -> Parsed {
    let mut toks = tokenize(text);
    strip_head_fillers(&mut toks);
    if toks.is_empty() {
        return Parsed::Unknown;
    }

    if let Some(p) = try_reminder(&toks, text, env) {
        return p;
    }

    // "save this as a note" / "note this" / "write that down" point at the selection; this must be
    // decided before the note-prefix matcher, which would take "that down" for a note body.
    let mut trimmed = toks.clone();
    strip_tail_fillers(&mut trimmed);
    let early: Vec<&str> = trimmed.iter().map(|t| t.norm.as_str()).collect();
    if !early.is_empty() && is_context_note(&early) {
        return from_context(env, ContextAction::SaveAsNote);
    }

    if let Some(outcome) = try_new_note(&toks, text) {
        return match outcome {
            NoteOutcome::Body(body) if is_context_reference(&body) => {
                from_context(env, ContextAction::SaveAsNote)
            }
            NoteOutcome::Body(body) => Parsed::Command(VoiceCommand::NewNote {
                body: env.spoken(body),
            }),
            NoteOutcome::OpenNotes => Parsed::Command(VoiceCommand::OpenTool {
                tool: ToolId::Notes,
            }),
        };
    }

    strip_tail_fillers(&mut toks);
    if toks.is_empty() {
        return Parsed::Unknown;
    }
    let words: Vec<&str> = toks.iter().map(|t| t.norm.as_str()).collect();

    if is_shelf_request(&words) {
        return from_context(env, ContextAction::AddToShelf);
    }
    if is_phone_request(&words) {
        return from_context(env, ContextAction::SendToPhone);
    }
    if let Some(c) = try_timer_control(&words).or_else(|| try_media(&words)) {
        return Parsed::Command(c);
    }

    // Voice v3: find a file by name / say what is in a known folder.
    if let Some(p) = osintent::try_search(&toks, text) {
        return p;
    }
    if let Some(p) = osintent::try_list(&words, env) {
        return p;
    }

    if let Some(c) = try_collapse(&words)
        .or_else(|| try_timer(&words))
        .or_else(|| try_pomodoro(&words))
    {
        return Parsed::Command(c);
    }

    // "open downloads" / "launch spotify": folders and apps from the catalog. A Krovi tool called
    // exactly the same wins for "open"; "launch" prefers the installed app (see osintent).
    let os = osintent::try_open(&words, env);
    if let OsOpen::Resolved(p) = os {
        return p;
    }
    match try_open(&words) {
        Some(c) => Parsed::Command(c),
        None => match os {
            OsOpen::Miss(feedback) => Parsed::Unresolved {
                feedback,
                results: vec![],
            },
            _ => Parsed::Unknown,
        },
    }
}

// ---------------------------------------------------------------------------------------------
// context-dependent actions
// ---------------------------------------------------------------------------------------------

/// "this", "that down", "of this", ... : a body that only points at the selection.
fn is_context_reference(body: &str) -> bool {
    let words: Vec<String> = crate::parser::norm_words(body);
    let w: Vec<&str> = words.iter().map(String::as_str).collect();
    matches!(
        w.as_slice(),
        ["this"]
            | ["that"]
            | ["it"]
            | ["this", "down"]
            | ["that", "down"]
            | ["it", "down"]
            | ["of", "this"]
            | ["of", "that"]
            | ["about", "this"]
            | ["about", "that"]
            | ["this", "text"]
            | ["this", "selection"]
            | ["the", "selection"]
            | ["this", "one"]
    )
}

fn from_context(env: &Env, action: ContextAction) -> Parsed {
    use ContextAction::*;
    match (&env.context, action) {
        (Some(VoiceContext::Text { value, .. }), SaveAsNote) => {
            Parsed::Command(VoiceCommand::NewNote {
                body: value.clone(),
            })
        }
        (Some(VoiceContext::Files { value, .. }), SaveAsNote) => {
            Parsed::Command(VoiceCommand::NewNote {
                body: value.join("\n"),
            })
        }
        (Some(VoiceContext::Text { value, .. }), AddToShelf) => {
            Parsed::Command(VoiceCommand::ShelfAdd {
                paths: vec![],
                text: Some(value.clone()),
            })
        }
        (Some(VoiceContext::Files { value, .. }), AddToShelf) => {
            Parsed::Command(VoiceCommand::ShelfAdd {
                paths: value.clone(),
                text: None,
            })
        }
        (Some(VoiceContext::Text { value, .. }), SendToPhone) => {
            Parsed::Command(VoiceCommand::TransferSend {
                paths: vec![],
                text: Some(value.clone()),
            })
        }
        (Some(VoiceContext::Files { value, .. }), SendToPhone) => {
            Parsed::Command(VoiceCommand::TransferSend {
                paths: value.clone(),
                text: None,
            })
        }
        // app-only context or none at all: nothing to act on
        _ => Parsed::NeedsContext(action),
    }
}

/// "save this as a note", "make this a note", "add this to my notes", "write that down".
fn is_context_note(w: &[&str]) -> bool {
    const FIRST: &[&str] = &[
        "save", "make", "turn", "put", "store", "add", "copy", "keep", "write", "jot", "convert",
        "create", "note", "take",
    ];
    const ALLOWED: &[&str] = &[
        "save",
        "make",
        "turn",
        "put",
        "store",
        "add",
        "copy",
        "keep",
        "write",
        "jot",
        "convert",
        "create",
        "this",
        "that",
        "it",
        "these",
        "selection",
        "selected",
        "text",
        "as",
        "a",
        "an",
        "the",
        "my",
        "to",
        "into",
        "in",
        "note",
        "notes",
        "new",
        "down",
        "of",
        "onto",
        "on",
        "for",
        "later",
    ];
    FIRST.contains(&w[0])
        && has_ref(w)
        && w.iter().all(|t| ALLOWED.contains(t))
        && (contains(w, "note") || contains(w, "notes") || contains(w, "down"))
}

const OPENING: &[&str] = &[
    "open", "show", "go", "switch", "launch", "take", "bring", "pull", "view", "see", "check",
    "start", "display", "look", "navigate", "goto", "run", "whats", "what", "how", "hows",
];

/// "copy this to the file shelf", "put this on the shelf", "shelve this".
fn is_shelf_request(w: &[&str]) -> bool {
    const SHELF_WORDS: &[&str] = &["shelf", "shelves", "shelve", "shelved", "shelving"];
    const VERBS: &[&str] = &[
        "copy", "put", "add", "save", "drop", "send", "move", "store", "park", "place", "stick",
        "throw", "pin", "shelve", "keep", "stash", "toss", "push", "file", "attach", "pop",
    ];
    w.iter().any(|t| SHELF_WORDS.contains(t))
        && !OPENING.contains(&w[0])
        && (VERBS.contains(&w[0]) || has_ref(w))
}

/// "send this to my phone", "share it with my phone".
fn is_phone_request(w: &[&str]) -> bool {
    const PHONE: &[&str] = &[
        "phone",
        "phones",
        "mobile",
        "cellphone",
        "smartphone",
        "iphone",
        "android",
    ];
    const VERBS: &[&str] = &[
        "send", "share", "transfer", "beam", "push", "move", "put", "copy", "text", "pass",
        "throw", "get", "airdrop",
    ];
    // "send to phone" on its own is the Quick Transfer TOOL; anything else with a send verb and a
    // phone word ("send this to my phone", "send to my phone") is the selection action.
    w.iter().any(|t| PHONE.contains(t)) && VERBS.contains(&w[0]) && !is_tool_name(w)
}

// ---------------------------------------------------------------------------------------------
// timer control and media
// ---------------------------------------------------------------------------------------------

/// "pause the timer", "resume pomodoro", "cancel the timer".
fn try_timer_control(w: &[&str]) -> Option<VoiceCommand> {
    const TARGETS: &[&str] = &[
        "timer",
        "timers",
        "countdown",
        "stopwatch",
        "pomodoro",
        "pomodoros",
        "pomo",
        "focus",
        "session",
    ];
    const FILLER: &[&str] = &[
        "the", "my", "a", "current", "running", "active", "this", "that", "it",
    ];
    let action = match w[0] {
        "pause" | "hold" => TimerAction::Pause,
        "resume" | "continue" | "unpause" | "restart" => TimerAction::Resume,
        "stop" | "cancel" | "end" | "kill" | "reset" | "clear" | "abort" => TimerAction::Stop,
        _ => return None,
    };
    let rest = &w[1..];
    if rest.is_empty()
        || !rest
            .iter()
            .all(|t| TARGETS.contains(t) || FILLER.contains(t))
        || !rest.iter().any(|t| TARGETS.contains(t))
    {
        return None;
    }
    let target = if rest
        .iter()
        .any(|t| matches!(*t, "pomodoro" | "pomodoros" | "pomo" | "focus" | "session"))
    {
        TimerTarget::Pomodoro
    } else {
        TimerTarget::Timer
    };
    Some(VoiceCommand::TimerControl { action, target })
}

/// Transport words and "what's playing". Whole-utterance matches only, so ordinary sentences
/// containing "next" or "play" are never hijacked.
fn try_media(w: &[&str]) -> Option<VoiceCommand> {
    use MediaCommand::*;
    let joined = w.join(" ");
    if matches!(
        joined.as_str(),
        "whats playing"
            | "what is playing"
            | "whats playing right now"
            | "whats currently playing"
            | "what is currently playing"
            | "what song is this"
            | "what song is that"
            | "what song is playing"
            | "what track is this"
            | "whats this song"
            | "what is this song"
            | "whats the song"
            | "what am i listening to"
            | "whats on"
    ) {
        return Some(VoiceCommand::Media {
            command: WhatsPlaying,
        });
    }

    // drop articles / demonstratives, then a leading "play" / "go to" before next/previous
    let mut f: Vec<&str> = w
        .iter()
        .copied()
        .filter(|t| !matches!(*t, "the" | "a" | "an" | "this" | "that" | "my"))
        .collect();
    if matches!(f.as_slice(), ["play" | "go" | "jump" | "switch", ..]) && f.len() > 1 {
        let skip = if f[0] == "play" {
            1
        } else if f.get(1) == Some(&"to") {
            2
        } else {
            0
        };
        if skip > 0
            && matches!(
                f.get(skip),
                Some(&"next") | Some(&"previous") | Some(&"last")
            )
        {
            f.drain(..skip);
        }
    }
    let f = f.join(" ");
    let command = match f.as_str() {
        "next" | "next track" | "next song" | "next one" | "skip" | "skip track" | "skip song"
        | "skip it" | "skip one" | "skip ahead" => Next,
        "previous" | "previous track" | "previous song" | "previous one" | "last track"
        | "last song" | "go back a track" | "go back a song" => Previous,
        "pause" | "pause music" | "pause song" | "pause track" | "pause playback"
        | "pause playing" | "stop music" | "stop playing" | "stop playback" => Pause,
        "play" | "play music" | "play song" | "play track" | "resume" | "resume music"
        | "resume playing" | "resume playback" | "unpause" | "continue playing"
        | "continue music" => Play,
        "toggle music" | "toggle playback" | "play pause" | "play or pause" => PlayPause,
        _ => return None,
    };
    Some(VoiceCommand::Media { command })
}

// ---------------------------------------------------------------------------------------------
// reminders
// ---------------------------------------------------------------------------------------------

const REMINDER_PREFIXES: &[&[&str]] = &[
    &["add", "a", "new", "reminder"],
    &["set", "a", "new", "reminder"],
    &["create", "a", "new", "reminder"],
    &["add", "a", "reminder"],
    &["set", "a", "reminder"],
    &["create", "a", "reminder"],
    &["make", "a", "reminder"],
    &["schedule", "a", "reminder"],
    &["add", "reminder"],
    &["set", "reminder"],
    &["create", "reminder"],
    &["make", "reminder"],
    &["new", "reminder"],
    &["remind", "me"],
    &["reminder"],
];

const TITLE_LEAD: &[&str] = &["to", "that", "about", "for", "and", "of", "saying", "so"];
const TITLE_TRAIL: &[&str] = &[
    "on", "at", "by", "in", "for", "due", "until", "till", "to", "and", "please", "then",
];

fn try_reminder(toks: &[Tok], text: &str, env: &Env) -> Option<Parsed> {
    let words: Vec<&str> = toks.iter().map(|t| t.norm.as_str()).collect();
    let plen = REMINDER_PREFIXES
        .iter()
        .filter(|p| words.len() >= p.len() && words[..p.len()] == ***p)
        .map(|p| p.len())
        .max()?;
    let open_calendar = || {
        Some(Parsed::Command(VoiceCommand::OpenTool {
            tool: ToolId::Calendar,
        }))
    };

    let rest_words: Vec<&str> = words[plen..].to_vec();
    if rest_words.is_empty() {
        return open_calendar();
    }
    let found = when::extract(&rest_words, &env.now);

    // Rebuild the title from the ORIGINAL spelling of the chunks that carry no date/time words.
    let chunk_end = |idx: usize| -> usize {
        let start = toks[idx].start;
        toks[idx..]
            .iter()
            .find(|t| t.start != start)
            .map_or(text.len(), |t| t.start)
    };
    let mut kept: Vec<(String, String)> = Vec::new(); // (normalized first word, raw chunk)
    let mut j = 0;
    while j < rest_words.len() {
        let idx = plen + j;
        let start = toks[idx].start;
        let mut k = j;
        while k < rest_words.len() && toks[plen + k].start == start {
            k += 1;
        }
        if found.consumed[j..k].iter().all(|c| !c) {
            let raw = text[start..chunk_end(plen + k - 1)].trim().to_string();
            kept.push((toks[idx].norm.clone(), raw));
        }
        j = k;
    }
    while kept
        .first()
        .is_some_and(|(n, _)| TITLE_LEAD.contains(&n.as_str()))
    {
        kept.remove(0);
    }
    while kept
        .last()
        .is_some_and(|(n, _)| TITLE_TRAIL.contains(&n.as_str()))
    {
        kept.pop();
    }
    let title = kept
        .iter()
        .map(|(_, raw)| raw.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let title = title
        .trim()
        .trim_matches(|c: char| matches!(c, ',' | ';' | ':' | '-' | '.'))
        .trim()
        .to_string();
    if title.is_empty() {
        return open_calendar();
    }

    let date = found.date.unwrap_or(env.now.date);
    Some(Parsed::Command(VoiceCommand::AddReminder {
        date: date.iso(),
        time: found.time.map(hhmm),
        title: capitalize_first(&env.spoken(title)),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::civil::Date;
    use ContextAction::*;
    use MediaCommand as M;
    use VoiceCommand::*;

    /// Sunday 2026-11-01, 10:30.
    fn env() -> Env {
        Env::at(LocalNow::new(Date::new(2026, 11, 1).unwrap(), 10, 30), None)
    }

    fn with(ctx: VoiceContext) -> Env {
        Env {
            context: Some(ctx),
            ..env()
        }
    }

    #[test]
    fn spoken_bodies_are_cleaned_but_the_selection_never_is() {
        let spoken = parse("new note um buy the the milk", &env().with_clean(true)).command();
        assert_eq!(
            spoken,
            Some(NewNote {
                body: "Buy the milk".into()
            })
        );
        let raw = parse("new note um buy the the milk", &env()).command();
        assert_eq!(
            raw,
            Some(NewNote {
                body: "Um buy the the milk".into()
            })
        );
        let selected = VoiceContext::Text {
            value: "um the the original  text".into(),
            app: None,
        };
        let from_selection = parse(
            "save this as a note",
            &Env {
                context: Some(selected),
                ..env()
            }
            .with_clean(true),
        )
        .command();
        assert_eq!(
            from_selection,
            Some(NewNote {
                body: "um the the original  text".into()
            }),
            "selection must be stored exactly as selected"
        );
        let r = parse(
            "remind me tomorrow to uh call the the bank",
            &env().with_clean(true),
        )
        .command();
        assert_eq!(r, reminder("2026-11-02", None, "Call the bank"));
    }

    fn cmd(s: &str) -> Option<VoiceCommand> {
        parse(s, &env()).command()
    }

    fn reminder(date: &str, time: Option<&str>, title: &str) -> Option<VoiceCommand> {
        Some(AddReminder {
            date: date.into(),
            time: time.map(Into::into),
            title: title.into(),
        })
    }

    #[test]
    fn reminders_with_dates_times_and_titles() {
        assert_eq!(
            cmd("add a reminder for tomorrow at 9 to call mom"),
            reminder("2026-11-02", Some("09:00"), "Call mom")
        );
        assert_eq!(
            cmd("Add a reminder for tomorrow at nine to call mom."),
            reminder("2026-11-02", Some("09:00"), "Call mom")
        );
        assert_eq!(
            cmd("remind me friday to submit the report"),
            reminder("2026-11-06", None, "Submit the report")
        );
        assert_eq!(
            cmd("remind me to pay rent on the 5th"),
            reminder("2026-11-05", None, "Pay rent")
        );
        assert_eq!(
            cmd("remind me to call mom tomorrow at 9"),
            reminder("2026-11-02", Some("09:00"), "Call mom")
        );
        assert_eq!(
            cmd("remind me to take the pills at 5 pm"),
            reminder("2026-11-01", Some("17:00"), "Take the pills")
        );
        assert_eq!(
            cmd("set a reminder for next friday at half past three to renew the passport"),
            reminder("2026-11-06", Some("15:30"), "Renew the passport")
        );
        assert_eq!(
            cmd("remind me in 30 minutes to check the oven"),
            reminder("2026-11-01", Some("11:00"), "Check the oven")
        );
        assert_eq!(
            cmd("remind me in two days about the dentist"),
            reminder("2026-11-03", None, "The dentist")
        );
        assert_eq!(
            cmd("create a reminder for december twenty fifth to buy gifts"),
            reminder("2026-12-25", None, "Buy gifts")
        );
        assert_eq!(
            cmd("remind me tomorrow morning to water the plants"),
            reminder("2026-11-02", Some("09:00"), "Water the plants")
        );
        assert_eq!(
            cmd("can you please remind me on friday at 2 pm to send the invoice"),
            reminder("2026-11-06", Some("14:00"), "Send the invoice")
        );
    }

    /// The exact time phrases Voice v3 promises (SPEC 11.13), at Sunday 2026-11-01 10:30.
    #[test]
    fn reminder_time_phrases_resolve_exactly() {
        let table: &[(&str, &str, Option<&str>)] = &[
            ("remind me to call mom at 5pm", "2026-11-01", Some("17:00")),
            (
                "remind me to call mom at 5 p m",
                "2026-11-01",
                Some("17:00"),
            ),
            (
                "remind me to call mom at 5:30 in the evening",
                "2026-11-01",
                Some("17:30"),
            ),
            (
                "remind me to call mom at 5:30 pm",
                "2026-11-01",
                Some("17:30"),
            ),
            (
                "remind me to call mom in 20 minutes",
                "2026-11-01",
                Some("10:50"),
            ),
            (
                "remind me to call mom in twenty minutes",
                "2026-11-01",
                Some("10:50"),
            ),
            (
                "remind me to call mom in an hour",
                "2026-11-01",
                Some("11:30"),
            ),
            (
                "remind me to call mom in two hours",
                "2026-11-01",
                Some("12:30"),
            ),
            (
                "remind me to call mom in half an hour",
                "2026-11-01",
                Some("11:00"),
            ),
            (
                "remind me to call mom in an hour and a half",
                "2026-11-01",
                Some("12:00"),
            ),
            (
                "remind me to call mom in a quarter of an hour",
                "2026-11-01",
                Some("10:45"),
            ),
            (
                "remind me tomorrow at 9 to call mom",
                "2026-11-02",
                Some("09:00"),
            ),
            (
                "remind me to call mom tomorrow at 9",
                "2026-11-02",
                Some("09:00"),
            ),
            (
                "remind me to call mom tomorrow at 9 am",
                "2026-11-02",
                Some("09:00"),
            ),
            (
                "remind me to call mom tomorrow at 9:15 in the morning",
                "2026-11-02",
                Some("09:15"),
            ),
            (
                "remind me to call mom on friday at noon",
                "2026-11-06",
                Some("12:00"),
            ),
            (
                "remind me to call mom friday at noon",
                "2026-11-06",
                Some("12:00"),
            ),
            (
                "remind me to call mom on friday at midnight",
                "2026-11-06",
                Some("00:00"),
            ),
            (
                "remind me to call mom tonight at 8",
                "2026-11-01",
                Some("20:00"),
            ),
            (
                "remind me to call mom tonight at 8 30",
                "2026-11-01",
                Some("20:30"),
            ),
            (
                "remind me to call mom this evening at 7",
                "2026-11-01",
                Some("19:00"),
            ),
            ("remind me to call mom at noon", "2026-11-01", Some("12:00")),
            (
                "remind me to call mom at 6 in the morning",
                "2026-11-02",
                Some("06:00"),
            ),
            // "next X" = X in the following Monday-Sunday week; today is Sunday, so that is tomorrow
            (
                "remind me to call mom next monday at 3 pm",
                "2026-11-02",
                Some("15:00"),
            ),
            (
                "remind me to call mom next friday at 3 pm",
                "2026-11-06",
                Some("15:00"),
            ),
            (
                "remind me to call mom friday morning at 8",
                "2026-11-06",
                Some("08:00"),
            ),
            (
                "remind me to call mom friday afternoon at 3",
                "2026-11-06",
                Some("15:00"),
            ),
            (
                "remind me to call mom tomorrow morning at 3",
                "2026-11-02",
                Some("03:00"),
            ),
            (
                "remind me to call mom at quarter past four",
                "2026-11-01",
                Some("16:15"),
            ),
        ];
        for (phrase, date, time) in table {
            assert_eq!(cmd(phrase), reminder(date, *time, "Call mom"), "{phrase}");
        }
    }

    /// Every example in docs/SPEC.md 11.13's reminder table, so the documentation cannot drift.
    #[test]
    fn the_documented_reminder_examples_all_resolve() {
        let table: &[(&str, &str, Option<&str>)] = &[
            (
                "remind me to call mom at 5 p m",
                "2026-11-01",
                Some("17:00"),
            ),
            (
                "remind me to call mom at 5:30 pm",
                "2026-11-01",
                Some("17:30"),
            ),
            (
                "remind me to call mom at five thirty",
                "2026-11-01",
                Some("17:30"),
            ),
            (
                "remind me to call mom at 17:00",
                "2026-11-01",
                Some("17:00"),
            ),
            (
                "remind me to call mom at 6 in the morning",
                "2026-11-02",
                Some("06:00"),
            ),
            (
                "remind me to call mom at 9 at night",
                "2026-11-01",
                Some("21:00"),
            ),
            (
                "remind me to call mom at 3 o'clock",
                "2026-11-01",
                Some("15:00"),
            ),
            (
                "remind me to call mom at midnight",
                "2026-11-02",
                Some("00:00"),
            ),
            (
                "remind me to call mom at half past three",
                "2026-11-01",
                Some("15:30"),
            ),
            (
                "remind me to call mom at quarter past four",
                "2026-11-01",
                Some("16:15"),
            ),
            (
                "remind me to call mom at quarter to five",
                "2026-11-01",
                Some("16:45"),
            ),
            ("remind me to call mom in 3 days", "2026-11-04", None),
            ("remind me to call mom in two weeks", "2026-11-15", None),
            ("remind me to call mom in a month", "2026-12-01", None),
            (
                "remind me to call mom the day after tomorrow",
                "2026-11-03",
                None,
            ),
            (
                "remind me to call mom this evening at 7",
                "2026-11-01",
                Some("19:00"),
            ),
            (
                "remind me to call mom tomorrow morning",
                "2026-11-02",
                Some("09:00"),
            ),
            ("remind me to call mom tonight", "2026-11-01", Some("20:00")),
            (
                "remind me to call mom on december twenty fifth",
                "2026-12-25",
                None,
            ),
            ("remind me to call mom on march 3rd", "2027-03-03", None),
            ("remind me to call mom on the 5th", "2026-11-05", None),
            ("remind me to call mom this friday", "2026-11-06", None),
        ];
        for (phrase, date, time) in table {
            assert_eq!(cmd(phrase), reminder(date, *time, "Call mom"), "{phrase}");
        }
    }

    /// The whole path ("remind me in two minutes ...") with a clock that has seconds: never early.
    /// The Quick Transfer TOOL versus the `transfer-send` selection action, and the Shelf tool.
    #[test]
    fn the_transfer_tool_does_not_collide_with_send_this_to_my_phone_or_the_shelf() {
        let tool = |t| Some(OpenTool { tool: t });
        let files = VoiceContext::Files {
            value: vec![r"C:\a\b.txt".into()],
            app: Some("explorer".into()),
        };
        // the tool, by every synonym, with and without a selection present
        for s in [
            "transfer",
            "quick transfer",
            "send to phone",
            "phone transfer",
            "file transfer",
            "open quick transfer",
            "open file transfer",
            "show me the phone transfer",
            "go to transfer",
            "launch quick transfer",
            "open send to phone",
        ] {
            assert_eq!(cmd(s), tool(ToolId::Transfer), "{s}");
            assert_eq!(
                parse(s, &with(files.clone())).command(),
                tool(ToolId::Transfer),
                "{s} (with a selection)"
            );
        }
        // "this/it/that" + a selection is still the transfer-send action
        for s in [
            "send this to my phone",
            "send it to my phone",
            "send this to phone",
            "send that to the phone",
            "transfer this to my phone",
            "share this with my phone",
        ] {
            assert_eq!(
                parse(s, &with(files.clone())).command(),
                Some(TransferSend {
                    paths: vec![r"C:\a\b.txt".into()],
                    text: None
                }),
                "{s}"
            );
            assert_eq!(
                parse(s, &env()),
                Parsed::NeedsContext(SendToPhone),
                "{s} without a selection"
            );
        }
        // no reference word and not the exact tool name: still the selection action (asks for one)
        assert_eq!(
            parse("send to my phone", &env()),
            Parsed::NeedsContext(SendToPhone)
        );
        // the shelf keeps its own words
        assert_eq!(cmd("open file shelf"), tool(ToolId::Shelf));
        assert_eq!(cmd("open files"), tool(ToolId::Shelf));
        assert_eq!(cmd("files"), tool(ToolId::Shelf));
        assert_eq!(cmd("show me the shelf"), tool(ToolId::Shelf));
        assert_eq!(cmd("open the file shelf"), tool(ToolId::Shelf));
        let text = VoiceContext::Text {
            value: "x".into(),
            app: None,
        };
        assert_eq!(
            parse("put this on the shelf", &with(text)).command(),
            Some(ShelfAdd {
                paths: vec![],
                text: Some("x".into())
            })
        );
    }

    #[test]
    fn a_spoken_relative_reminder_is_never_early() {
        let day = Date::new(2026, 11, 1).unwrap();
        let env_at = |h, m, s| Env::at(LocalNow::new(day, h, m).with_secs(s), None);
        let run = |h, m, s, phrase: &str| parse(phrase, &env_at(h, m, s)).command();
        assert_eq!(
            run(18, 31, 57, "remind me in two minutes to check the oven"),
            reminder("2026-11-01", Some("18:34"), "Check the oven")
        );
        assert_eq!(
            run(18, 31, 0, "remind me in two minutes to check the oven"),
            reminder("2026-11-01", Some("18:33"), "Check the oven")
        );
        assert_eq!(
            run(23, 59, 30, "set a reminder in a minute to lock up"),
            reminder("2026-11-02", Some("00:01"), "Lock up")
        );
        // absolute times keep their exact minute
        assert_eq!(
            run(18, 31, 57, "remind me at 18:32 to check the oven"),
            reminder("2026-11-01", Some("18:32"), "Check the oven")
        );
    }

    #[test]
    fn reminder_without_a_date_defaults_to_today() {
        assert_eq!(
            cmd("remind me to buy milk"),
            reminder("2026-11-01", None, "Buy milk")
        );
        assert_eq!(
            cmd("remind me at 9 to call the bank"),
            reminder("2026-11-02", Some("09:00"), "Call the bank"),
            "09:00 already passed today"
        );
    }

    #[test]
    fn reminder_keeps_the_users_spelling_and_punctuation() {
        assert_eq!(
            cmd("remind me tomorrow to email Dr. Patel, then Sam"),
            reminder("2026-11-02", None, "Email Dr. Patel, then Sam")
        );
    }

    #[test]
    fn reminder_without_a_title_opens_the_calendar() {
        let cal = Some(OpenTool {
            tool: ToolId::Calendar,
        });
        assert_eq!(cmd("add a reminder"), cal);
        assert_eq!(cmd("add a reminder for tomorrow at 9"), cal);
        assert_eq!(cmd("remind me"), cal);
    }

    #[test]
    fn save_this_as_a_note_uses_the_selection() {
        let text = VoiceContext::Text {
            value: "Call the dentist at 3".into(),
            app: Some("notepad".into()),
        };
        for s in [
            "save this as a note",
            "make this a note",
            "turn this into a note",
            "save it as a note",
            "add this to my notes",
            "copy this to a new note",
            "put that in a note",
            "write this down",
            "jot that down",
            "note this",
            "note this down",
            "make a note of this",
            "take a note of that",
        ] {
            assert_eq!(
                parse(s, &with(text.clone())),
                Parsed::Command(NewNote {
                    body: "Call the dentist at 3".into()
                }),
                "{s}"
            );
        }
    }

    #[test]
    fn save_this_as_a_note_with_files_lists_the_paths() {
        let files = VoiceContext::Files {
            value: vec![r"C:\a\b.txt".into(), r"C:\a\c.png".into()],
            app: Some("explorer".into()),
        };
        assert_eq!(
            parse("save this as a note", &with(files)),
            Parsed::Command(NewNote {
                body: "C:\\a\\b.txt\nC:\\a\\c.png".into()
            })
        );
    }

    #[test]
    fn context_actions_without_a_selection_ask_for_one() {
        assert_eq!(
            parse("save this as a note", &env()),
            Parsed::NeedsContext(SaveAsNote)
        );
        assert_eq!(
            parse("put this on the shelf", &env()),
            Parsed::NeedsContext(AddToShelf)
        );
        assert_eq!(
            parse("send this to my phone", &env()),
            Parsed::NeedsContext(SendToPhone)
        );
        let app_only = VoiceContext::App {
            value: "chrome".into(),
        };
        assert_eq!(
            parse("send this to my phone", &with(app_only)),
            Parsed::NeedsContext(SendToPhone)
        );
        assert_eq!(cmd("send this to my phone"), None);
    }

    #[test]
    fn shelf_requests() {
        let files = VoiceContext::Files {
            value: vec![r"C:\docs\report.pdf".into()],
            app: Some("explorer".into()),
        };
        let text = VoiceContext::Text {
            value: "snippet".into(),
            app: None,
        };
        for s in [
            "copy this to the file shelf",
            "put this on the shelf",
            "add this to the shelf",
            "save this to the shelf",
            "shelve this",
            "put it on the file shelf",
            "drop that on the shelf",
            "move this to the shelf",
        ] {
            assert_eq!(
                parse(s, &with(files.clone())),
                Parsed::Command(ShelfAdd {
                    paths: vec![r"C:\docs\report.pdf".into()],
                    text: None
                }),
                "{s}"
            );
            assert_eq!(
                parse(s, &with(text.clone())),
                Parsed::Command(ShelfAdd {
                    paths: vec![],
                    text: Some("snippet".into())
                }),
                "{s}"
            );
        }
    }

    #[test]
    fn opening_the_shelf_is_not_adding_to_it() {
        assert_eq!(
            cmd("open the file shelf"),
            Some(OpenTool {
                tool: ToolId::Shelf
            })
        );
        assert_eq!(
            cmd("show me the shelf"),
            Some(OpenTool {
                tool: ToolId::Shelf
            })
        );
        assert_eq!(
            cmd("go to the shelf"),
            Some(OpenTool {
                tool: ToolId::Shelf
            })
        );
    }

    #[test]
    fn transfer_to_phone() {
        let files = VoiceContext::Files {
            value: vec![r"C:\p\a.jpg".into(), r"C:\p\b.jpg".into()],
            app: None,
        };
        let text = VoiceContext::Text {
            value: "https://example.com".into(),
            app: Some("msedge".into()),
        };
        for s in [
            "send this to my phone",
            "send it to my phone",
            "send that to the phone",
            "share this with my phone",
            "transfer this to my phone",
            "beam this to my phone",
            "send this to phone",
        ] {
            assert_eq!(
                parse(s, &with(files.clone())),
                Parsed::Command(TransferSend {
                    paths: vec![r"C:\p\a.jpg".into(), r"C:\p\b.jpg".into()],
                    text: None
                }),
                "{s}"
            );
            assert_eq!(
                parse(s, &with(text.clone())),
                Parsed::Command(TransferSend {
                    paths: vec![],
                    text: Some("https://example.com".into())
                }),
                "{s}"
            );
        }
    }

    #[test]
    fn media_commands() {
        let m = |c| Some(Media { command: c });
        assert_eq!(cmd("next track"), m(M::Next));
        assert_eq!(cmd("next song"), m(M::Next));
        assert_eq!(cmd("skip"), m(M::Next));
        assert_eq!(cmd("skip this song"), m(M::Next));
        assert_eq!(cmd("play the next track"), m(M::Next));
        assert_eq!(cmd("previous track"), m(M::Previous));
        assert_eq!(cmd("go to the previous song"), m(M::Previous));
        assert_eq!(cmd("pause music"), m(M::Pause));
        assert_eq!(cmd("pause the music"), m(M::Pause));
        assert_eq!(cmd("pause"), m(M::Pause));
        assert_eq!(cmd("play music"), m(M::Play));
        assert_eq!(cmd("resume"), m(M::Play));
        assert_eq!(cmd("what's playing"), m(M::WhatsPlaying));
        assert_eq!(cmd("What is playing?"), m(M::WhatsPlaying));
        assert_eq!(cmd("what song is this"), m(M::WhatsPlaying));
        assert_eq!(cmd("what's playing right now"), m(M::WhatsPlaying));
        assert_eq!(cmd("please skip this track"), m(M::Next));
    }

    #[test]
    fn media_words_inside_other_sentences_are_left_alone() {
        assert_eq!(cmd("the next meeting is on friday"), None);
        assert_eq!(cmd("i want to play football tomorrow"), None);
        assert_eq!(cmd("pause the presentation while we eat"), None);
    }

    #[test]
    fn timer_and_pomodoro_control() {
        let tc = |a, t| {
            Some(TimerControl {
                action: a,
                target: t,
            })
        };
        assert_eq!(
            cmd("pause the timer"),
            tc(TimerAction::Pause, TimerTarget::Timer)
        );
        assert_eq!(
            cmd("pause timer"),
            tc(TimerAction::Pause, TimerTarget::Timer)
        );
        assert_eq!(
            cmd("resume the timer"),
            tc(TimerAction::Resume, TimerTarget::Timer)
        );
        assert_eq!(
            cmd("stop the timer"),
            tc(TimerAction::Stop, TimerTarget::Timer)
        );
        assert_eq!(
            cmd("cancel the timer"),
            tc(TimerAction::Stop, TimerTarget::Timer)
        );
        assert_eq!(
            cmd("pause the pomodoro"),
            tc(TimerAction::Pause, TimerTarget::Pomodoro)
        );
        assert_eq!(
            cmd("resume pomodoro"),
            tc(TimerAction::Resume, TimerTarget::Pomodoro)
        );
        assert_eq!(
            cmd("stop the stopwatch"),
            tc(TimerAction::Stop, TimerTarget::Timer)
        );
        // a bare "pause" is the music; "pause the timer" is the timer
        assert_eq!(cmd("pause"), Some(Media { command: M::Pause }));
    }

    #[test]
    fn existing_intents_still_work_with_the_new_layers() {
        assert_eq!(
            cmd("start a 10 minute timer"),
            Some(StartTimer { seconds: 600 })
        );
        assert_eq!(cmd("start a pomodoro"), Some(StartPomodoro));
        assert_eq!(
            cmd("open timers"),
            Some(OpenTool {
                tool: ToolId::Timers
            })
        );
        assert_eq!(cmd("close"), Some(Collapse));
        assert_eq!(
            cmd("new note buy milk"),
            Some(NewNote {
                body: "Buy milk".into()
            })
        );
        assert_eq!(
            cmd("open reminders"),
            Some(OpenTool {
                tool: ToolId::Calendar
            })
        );
    }

    #[test]
    fn chatter_is_still_unknown() {
        for s in [
            "the quick brown fox",
            "what time is it in tokyo",
            "hello there",
            "",
            "send the report to john",
            "put the milk in the fridge",
        ] {
            assert_eq!(parse(s, &env()), Parsed::Unknown, "{s:?}");
        }
    }
}
