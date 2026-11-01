//! The closed command grammar, as data. The SAPI layer turns these templates into a dynamic
//! context-free grammar so short commands are matched against a few hundred phrases (which a
//! desktop recognizer does far better than open dictation). Because both this table and the
//! parser derive tool names from `parser::TOOL_PHRASES`, and a test expands every template and
//! feeds it to `parse_command`, the two cannot drift apart.

use crate::catalog::Catalog;
use crate::osintent::{
    APP_GRAMMAR_VERBS, FOLDER_GRAMMAR_VERBS, LIST_GRAMMAR_PREFIXES, SEARCH_GRAMMAR_PREFIXES,
};
use crate::parser::TOOL_PHRASES;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Slot {
    /// Exactly one of these alternatives (each may be several words).
    Any(Vec<String>),
    /// Zero or one of these alternatives.
    Opt(Vec<String>),
    /// A number from one to ninety-nine, spoken as words ("five", "twenty five").
    Num,
    /// One or more words of free dictation embedded in the grammar (SAPI's `*+`): the body of
    /// "new note ...". Everything else in a template is a closed phrase.
    Dictation,
}

pub(crate) type Template = Vec<Slot>;

fn any(words: &[&str]) -> Slot {
    Slot::Any(words.iter().map(|w| w.to_string()).collect())
}

fn opt(words: &[&str]) -> Slot {
    Slot::Opt(words.iter().map(|w| w.to_string()).collect())
}

/// Every phrase for every tool, deduplicated, in table order.
pub(crate) fn tool_names() -> Vec<String> {
    let mut seen = Vec::new();
    for (_, phrases) in TOOL_PHRASES {
        for p in *phrases {
            let p = p.to_string();
            if !seen.contains(&p) {
                seen.push(p);
            }
        }
    }
    seen
}

const NOTE_PREFIX_PHRASES: &[&str] = &[
    "new note",
    "take a note",
    "make a note",
    "add a note",
    "create a note",
    "note to self",
];

pub(crate) const UNITS_PLURAL: &[&str] = &["seconds", "minutes", "hours"];
pub(crate) const UNITS_SINGULAR: &[&str] = &["second", "minute", "hour"];

/// The whole command grammar. `catalog` supplies the folder and app names that "open ..." and
/// "what's in ..." may name (Voice v3); with an empty catalog those templates are simply absent.
pub(crate) fn command_templates(catalog: &Catalog) -> Vec<Template> {
    let tools = Slot::Any(tool_names());
    let start = any(&["start", "set", "begin"]);
    let det = opt(&["a", "an", "the"]);
    let units_any = Slot::Any(
        UNITS_PLURAL
            .iter()
            .chain(UNITS_SINGULAR)
            .map(|s| s.to_string())
            .collect(),
    );
    let units_one = any(UNITS_SINGULAR);

    let mut templates = vec![
        // open timers / go to the calendar / show me my notes
        // A verb is REQUIRED: bare one-word names ("timer", "notebook") match stray room noise and
        // chatter far too easily (measured: up to 4/7 false accepts). The text parser still accepts
        // bare names when they arrive as typed/dictated text.
        vec![
            any(&[
                "open",
                "show",
                "show me",
                "go to",
                "switch to",
                "launch",
                "take me to",
                "bring up",
                "view",
            ]),
            opt(&["the", "my"]),
            tools,
        ],
        // start pomodoro / begin a focus session
        vec![
            any(&["start", "begin", "run"]),
            opt(&["a", "the"]),
            any(&["pomodoro", "focus"]),
            opt(&["timer", "session", "mode"]),
        ],
        // start a timer for five minutes / set a timer for 25 minutes
        vec![
            start.clone(),
            det.clone(),
            any(&["timer"]),
            any(&["for"]),
            Slot::Num,
            units_any.clone(),
        ],
        // timer for ten minutes
        vec![any(&["timer"]), any(&["for"]), Slot::Num, units_any],
        // start a five minute timer
        vec![
            start.clone(),
            det.clone(),
            Slot::Num,
            units_one,
            any(&["timer"]),
        ],
        // start a timer for an hour / half an hour / a minute
        vec![
            start.clone(),
            det.clone(),
            any(&["timer"]),
            any(&["for"]),
            any(&[
                "an hour",
                "half an hour",
                "a minute",
                "a quarter of an hour",
            ]),
        ],
        // start a timer  (no length: opens the Timers tool)
        vec![start, det, any(&["timer"])],
        // new note buy milk / add a reminder for tomorrow at nine to call mom / find invoice
        // march: a closed prefix, then free dictation for the body (the parser resolves dates,
        // times and search terms, not the recognizer). All prefixes share ONE dictation slot:
        // every embedded dictation word costs the recognizer a language-model load.
        vec![any(&dictation_prefixes()), Slot::Dictation],
        // new note / add a reminder  (no body: opens the Notes / Calendar tool)
        vec![any(&prefix_phrases())],
        // close / hide the panel / go away
        vec![
            any(&["close", "hide", "dismiss", "collapse", "minimize"]),
            opt(&["the"]),
            opt(&["panel", "krovi", "window", "shelf"]),
        ],
        vec![any(&["go away", "never mind", "that is all"])],
        // ---- Voice v2 ----
        // act on the selection ("this")
        vec![any(CONTEXT_NOTE_PHRASES)],
        vec![any(SHELF_PHRASES)],
        vec![any(PHONE_PHRASES)],
        // media transport and the "what's playing" question
        vec![any(MEDIA_PHRASES)],
        // pause / resume / stop the timer or pomodoro
        vec![
            any(&["pause", "resume", "stop", "cancel"]),
            opt(&["the"]),
            any(&["timer", "pomodoro", "stopwatch", "countdown"]),
        ],
    ];

    // ---- Voice v3: the closed folder / app sets, built from the OS enumeration ----
    let folders = catalog.folder_phrases();
    if !folders.is_empty() {
        // open downloads / show my pictures folder / go to krovi downloads
        templates.push(vec![
            any(FOLDER_GRAMMAR_VERBS),
            opt(&["the", "my"]),
            Slot::Any(folders.clone()),
            opt(&["folder"]),
        ]);
        // what's in downloads / tell me about my documents folder
        templates.push(vec![
            any(LIST_GRAMMAR_PREFIXES),
            opt(&["the", "my"]),
            Slot::Any(folders),
            opt(&["folder"]),
        ]);
    }
    let apps = catalog.app_phrases();
    if !apps.is_empty() {
        // open spotify / launch vs code / start the calculator
        templates.push(vec![any(APP_GRAMMAR_VERBS), opt(&["the"]), Slot::Any(apps)]);
    }
    templates
}

const REMINDER_PREFIX_PHRASES: &[&str] = &[
    "add a reminder",
    "set a reminder",
    "create a reminder",
    "make a reminder",
    "new reminder",
    "remind me",
];

/// "new note ..." and "add a reminder ..." prefixes: valid on their own (they open the tool).
fn prefix_phrases() -> Vec<&'static str> {
    NOTE_PREFIX_PHRASES
        .iter()
        .chain(REMINDER_PREFIX_PHRASES)
        .copied()
        .collect()
}

/// Every prefix whose body is free dictation: notes, reminders and file searches (see the shared
/// dictation slot above). A search prefix is never valid alone.
fn dictation_prefixes() -> Vec<&'static str> {
    let mut v = prefix_phrases();
    v.extend_from_slice(SEARCH_GRAMMAR_PREFIXES);
    v
}

const CONTEXT_NOTE_PHRASES: &[&str] = &[
    "save this as a note",
    "save that as a note",
    "save it as a note",
    "make this a note",
    "turn this into a note",
    "add this to my notes",
    "copy this to a new note",
    "put this in a note",
    "write this down",
    "note this down",
    "note this",
];

const SHELF_PHRASES: &[&str] = &[
    "copy this to the file shelf",
    "copy this to the shelf",
    "put this on the shelf",
    "put this on the file shelf",
    "put it on the shelf",
    "add this to the shelf",
    "add this to the file shelf",
    "save this to the shelf",
    "move this to the shelf",
    "drop this on the shelf",
    "shelve this",
];

const PHONE_PHRASES: &[&str] = &[
    "send this to my phone",
    "send it to my phone",
    "send that to my phone",
    "send this to the phone",
    "send this to phone",
    "share this with my phone",
    "transfer this to my phone",
];

/// Multi-word only: a lone "pause" or "skip" matches room noise too easily to put in the grammar
/// (the text parser still accepts them when typed or dictated). Only the short "what's playing" is
/// listed for the media question; "what song is this", "what is playing" and "what's playing
/// right now" are parsed but deliberately NOT in the grammar: measured, the recognizer heard
/// "what time is it in tokyo (right now)" as each of them and fired a media query on ordinary
/// questions.
const MEDIA_PHRASES: &[&str] = &[
    "next track",
    "next song",
    "skip track",
    "skip song",
    "skip this song",
    "skip this track",
    "play the next track",
    "previous track",
    "previous song",
    "last track",
    "last song",
    "pause music",
    "pause the music",
    "pause playback",
    "play music",
    "resume music",
    "resume playback",
    "toggle music",
    "what's playing",
];

/// Spoken number words for the `Num` sub-rule, as (first word alternatives, optional unit words).
/// 1-19 are single words; 20-90 are a tens word optionally followed by one of "one".."nine".
pub(crate) const ONES: &[&str] = &[
    "one", "two", "three", "four", "five", "six", "seven", "eight", "nine",
];
pub(crate) const TEENS: &[&str] = &[
    "ten",
    "eleven",
    "twelve",
    "thirteen",
    "fourteen",
    "fifteen",
    "sixteen",
    "seventeen",
    "eighteen",
    "nineteen",
];
pub(crate) const TENS: &[&str] = &[
    "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety",
];

#[cfg(test)]
pub(crate) fn expand(template: &Template, num_samples: &[&str]) -> Vec<String> {
    let mut out: Vec<String> = vec![String::new()];
    for slot in template {
        let alts: Vec<String> = match slot {
            Slot::Any(a) => a.clone(),
            Slot::Opt(a) => std::iter::once(String::new())
                .chain(a.iter().cloned())
                .collect(),
            Slot::Num => num_samples.iter().map(|s| s.to_string()).collect(),
            Slot::Dictation => vec!["buy milk and eggs".to_string()],
        };
        let mut next = Vec::with_capacity(out.len() * alts.len());
        for prefix in &out {
            for alt in &alts {
                next.push(if prefix.is_empty() {
                    alt.clone()
                } else if alt.is_empty() {
                    prefix.clone()
                } else {
                    format!("{prefix} {alt}")
                });
            }
        }
        out = next;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::civil::{Date, LocalNow};
    use crate::intent::{parse, Env};
    use crate::parser::parse_command;
    use crate::types::{VoiceCommand, VoiceContext};

    use crate::catalog::Entry;
    use std::sync::Arc;

    fn fixture_catalog() -> Arc<Catalog> {
        let app = |n: &str| Entry::new(n, format!("shell:AppsFolder\\{n}"));
        Arc::new(Catalog::new(
            vec![
                Entry::new("Desktop", r"C:\u\Desktop"),
                Entry::new("Documents", r"C:\u\Documents"),
                Entry::new("Downloads", r"C:\u\Downloads"),
                Entry::new("Pictures", r"C:\u\Pictures"),
                Entry::new("Music", r"C:\u\Music"),
                Entry::new("Videos", r"C:\u\Videos"),
                Entry::new("Krovi downloads", r"C:\u\Downloads\Krovi"),
                Entry::new("Projects", r"D:\Projects"),
            ],
            vec![
                app("Notepad"),
                app("Spotify"),
                app("Visual Studio Code"),
                app("Google Chrome"),
                app("Microsoft Edge"),
                app("Calculator"),
                app("Settings"),
                app("Notepad++"),
            ],
        ))
    }

    /// "this" phrases need a selection, so the cross-check runs with one present.
    fn env_with_selection() -> Env {
        Env::at(
            LocalNow::new(Date::new(2026, 11, 1).unwrap(), 10, 30),
            Some(VoiceContext::Text {
                value: "selected words".into(),
                app: Some("notepad".into()),
            }),
        )
        .with_catalog(Some(fixture_catalog()))
    }

    const SAMPLES: &[&str] = &[
        "one",
        "five",
        "twelve",
        "twenty",
        "twenty five",
        "ninety nine",
    ];

    #[test]
    fn every_grammar_phrase_parses_to_a_command() {
        let mut total = 0;
        let env = env_with_selection();
        for t in command_templates(&fixture_catalog()) {
            for phrase in expand(&t, SAMPLES) {
                total += 1;
                assert!(
                    parse(&phrase, &env).command().is_some(),
                    "grammar phrase does not parse: {phrase:?}"
                );
            }
        }
        println!("command grammar expands to {total} phrases with 6 sample numbers");
        assert!(total > 500, "grammar unexpectedly small: {total}");
    }

    #[test]
    fn timer_templates_produce_the_spoken_duration() {
        let templates = command_templates(&Catalog::default());
        let with_num: Vec<&Template> = templates
            .iter()
            .filter(|t| t.contains(&Slot::Num))
            .collect();
        assert!(with_num.len() >= 3);
        for t in with_num {
            for (num, n) in [
                ("five", 5u64),
                ("twenty five", 25),
                ("ninety nine", 99),
                ("one", 1),
            ] {
                for phrase in expand(t, &[num]) {
                    let unit = if phrase.contains("second") {
                        1
                    } else if phrase.contains("hour") {
                        3600
                    } else {
                        60
                    };
                    assert_eq!(
                        parse_command(&phrase),
                        Some(VoiceCommand::StartTimer { seconds: n * unit }),
                        "{phrase:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn number_vocabulary_is_complete() {
        assert_eq!(ONES.len(), 9);
        assert_eq!(TEENS.len(), 10);
        assert_eq!(TENS.len(), 8);
    }
}
