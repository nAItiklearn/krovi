//! Intents -> a plan of steps, each with a human summary and a permission level.
//!
//! The permission policy, in one place:
//!
//! | step                                                      | risk      | why |
//! |-----------------------------------------------------------|-----------|-----|
//! | open a site from the built-in table                       | `Safe`    | the address comes from our table, not from the user's words |
//! | search a built-in site                                    | `Safe`    | fixed site; the words are percent-encoded into one parameter |
//! | open an installed app                                     | `Safe`    | only an entry of the host's Start-menu list; same trust as clicking it in Start |
//! | start a timer, save a note                                | `Safe`    | local, visible, undoable |
//! | open an address the user typed or said                    | `Confirm` | the destination is not vouched for; the card shows the full address |
//! | add a reminder                                            | `Confirm` | a wrong time silently misses the thing; the card shows the exact date and time |
//! | open an email draft                                       | `Confirm` | the card shows recipient, subject and body; the user presses Send in their own mail app |
//! | invalid address, unknown or ambiguous app, bad time/range, a sentence we did not understand | `Blocked` | never runs |

use serde::Serialize;

use crate::civil::{clock_label, Date};
use crate::intent::{AppMatch, Intent, MailProvider};
use crate::mail::{self, EmailDraft};
use crate::sites;
use crate::text::{fmt_duration, preview};
use crate::url;

/// Most steps one sentence may produce.
pub const MAX_STEPS: usize = 8;
pub const MAX_TIMER_SECS: u64 = 24 * 3600;
pub const MAX_NOTE_CHARS: usize = 5000;
pub const MAX_TITLE_CHARS: usize = 200;
pub const MAX_LABEL_CHARS: usize = 80;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "level", content = "reason", rename_all = "kebab-case")]
pub enum Risk {
    /// Runs as soon as the plan is run.
    Safe,
    /// Runs only after the user confirms this step on its card.
    Confirm,
    /// Never runs; the reason is shown.
    Blocked(String),
}

/// What a step will do, already validated. This is what the confirmation card renders.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "type",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum Action {
    /// Open `url` in the default browser. `site` is the display name (or host) and `known`
    /// says the address came from the built-in site table.
    OpenUrl {
        url: String,
        site: String,
        known: bool,
    },
    Search {
        site: String,
        query: String,
        url: String,
    },
    /// Launch an installed app; `target` is the host's own launch target for it.
    OpenApp {
        name: String,
        target: String,
    },
    Timer {
        seconds: u64,
        label: String,
    },
    /// Local date `yyyy-mm-dd` and time `HH:MM`.
    Reminder {
        title: String,
        date: String,
        time: String,
    },
    Note {
        body: String,
    },
    /// A compose window in the user's mail app. Krovi never sends it. The built link is kept
    /// for the host and not sent to the UI.
    ComposeEmail {
        provider: MailProvider,
        to: Vec<String>,
        subject: String,
        body: String,
        #[serde(skip_serializing)]
        url: String,
    },
    /// Something that cannot be done (always `Blocked`); `text` is what was asked.
    Unsupported {
        text: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Step {
    /// Unique within the plan: "s1", "s2", ...
    pub id: String,
    pub action: Action,
    /// One plain sentence: "Open youtube.com in your browser".
    pub summary: String,
    pub risk: Risk,
}

impl Step {
    pub fn is_blocked(&self) -> bool {
        matches!(self.risk, Risk::Blocked(_))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub id: String,
    pub steps: Vec<Step>,
    /// Example commands to show when part of the sentence was not understood.
    pub suggestions: Vec<String>,
}

impl Plan {
    pub fn step(&self, id: &str) -> Option<&Step> {
        self.steps.iter().find(|s| s.id == id)
    }

    /// Steps that need the user's OK, in order.
    pub fn confirm_steps(&self) -> impl Iterator<Item = &Step> {
        self.steps.iter().filter(|s| s.risk == Risk::Confirm)
    }
}

type Built = (Action, String, Risk);

fn blocked(text: &str, summary: String, reason: impl Into<String>) -> Built {
    (
        Action::Unsupported {
            text: preview(text, 200),
        },
        summary,
        Risk::Blocked(reason.into()),
    )
}

/// Validate every intent and decide how much permission each step needs.
pub fn build_plan(plan_id: &str, intents: &[Intent], now_utc_ms: i64) -> Plan {
    let mut steps = Vec::new();
    let mut suggestions: Vec<String> = Vec::new();
    for (i, intent) in intents.iter().enumerate() {
        if i >= MAX_STEPS {
            let (action, summary, risk) = blocked(
                "",
                format!("{} more steps", intents.len() - MAX_STEPS),
                format!(
                    "Krovi runs up to {MAX_STEPS} steps at a time. Ask for the rest separately."
                ),
            );
            steps.push(Step {
                id: format!("s{}", i + 1),
                action,
                summary,
                risk,
            });
            break;
        }
        if let Intent::Unknown { suggestions: s, .. } = intent {
            for x in s {
                if suggestions.len() < 4 && !suggestions.contains(x) {
                    suggestions.push(x.clone());
                }
            }
        }
        let (action, summary, risk) = step_for(intent, now_utc_ms);
        steps.push(Step {
            id: format!("s{}", i + 1),
            action,
            summary,
            risk,
        });
    }
    Plan {
        id: plan_id.to_string(),
        steps,
        suggestions,
    }
}

fn step_for(intent: &Intent, now_utc_ms: i64) -> Built {
    match intent {
        Intent::OpenSite { site } => open_site(site),
        Intent::OpenUrl { url } => open_url(url),
        Intent::Search { site, query, .. } => search(site, query),
        Intent::OpenApp { query, app } => open_app(query, app),
        Intent::Timer { seconds, label } => timer(*seconds, label.as_deref()),
        Intent::Reminder {
            title,
            date,
            time,
            due_utc_ms,
            default_time,
        } => reminder(title, date, time, *due_utc_ms, *default_time, now_utc_ms),
        Intent::Note { body } => note(body),
        Intent::Email {
            provider,
            to,
            subject,
            body,
            bad_recipient,
        } => email(*provider, to, subject, body, bad_recipient.as_deref()),
        Intent::Unknown { text, .. } => {
            let summary = if text.is_empty() {
                "Nothing to do".to_string()
            } else {
                format!("Not understood: \"{}\"", preview(text, 80))
            };
            blocked(text, summary, "I didn't understand that.")
        }
    }
}

fn open_site(key: &str) -> Built {
    let Some(site) = sites::get(key) else {
        return blocked(
            key,
            format!("Open \"{key}\""),
            "That site is not on Krovi's list.",
        );
    };
    // the built-in table goes through the same gate as anything else
    match url::validate_full(site.url) {
        Ok(v) => (
            Action::OpenUrl {
                url: v.url,
                site: site.name.to_string(),
                known: true,
            },
            format!("Open {} in your browser", site.label),
            Risk::Safe,
        ),
        Err(e) => blocked(key, format!("Open {}", site.name), e.to_string()),
    }
}

fn open_url(typed: &str) -> Built {
    match url::validate(typed) {
        Ok(v) => {
            let unencrypted = if v.https { "" } else { " (not encrypted)" };
            (
                Action::OpenUrl {
                    url: v.url.clone(),
                    site: v.host.clone(),
                    known: false,
                },
                format!(
                    "Open {}{} in your browser",
                    preview(&v.url, 120),
                    unencrypted
                ),
                Risk::Confirm,
            )
        }
        Err(e) => blocked(
            typed,
            format!("Open \"{}\"", preview(typed, 80)),
            e.to_string(),
        ),
    }
}

fn search(key: &str, query: &str) -> Built {
    let Some(site) = sites::get(key) else {
        return blocked(
            query,
            format!("Search for \"{}\"", preview(query, 80)),
            "That site is not on Krovi's list.",
        );
    };
    let shown = url::normalize_query(query);
    let summary = format!("Search {} for \"{}\"", site.name, preview(&shown, 80));
    let Some(template) = site.search else {
        return blocked(
            query,
            summary,
            format!("{} has no search Krovi can use.", site.name),
        );
    };
    match url::search_url(template, query)
        .map_err(|e| e.to_string())
        .and_then(|u| {
            url::validate_full(&u)
                .map(|v| v.url)
                .map_err(|e| e.to_string())
        }) {
        Ok(u) => (
            Action::Search {
                site: site.name.to_string(),
                query: shown,
                url: u,
            },
            summary,
            Risk::Safe,
        ),
        Err(reason) => blocked(query, summary, reason),
    }
}

fn open_app(query: &str, app: &AppMatch) -> Built {
    match app {
        AppMatch::Found { name, target } => {
            if target.is_empty() || target.chars().any(char::is_control) {
                return blocked(
                    query,
                    format!("Open {name}"),
                    "That app has no usable launch target.",
                );
            }
            (
                Action::OpenApp {
                    name: name.clone(),
                    target: target.clone(),
                },
                format!("Open {name}"),
                Risk::Safe,
            )
        }
        AppMatch::Ambiguous { names } => blocked(
            query,
            format!("Open \"{}\"", preview(query, 60)),
            format!(
                "Several apps match \"{}\": {}. Say the full name.",
                preview(query, 60),
                names.join(", ")
            ),
        ),
        AppMatch::NotFound => blocked(
            query,
            format!("Open \"{}\"", preview(query, 60)),
            format!(
                "I couldn't find an app called \"{}\" in your Start menu.",
                preview(query, 60)
            ),
        ),
    }
}

fn timer(seconds: u64, label: Option<&str>) -> Built {
    let label = label.unwrap_or("");
    let dur = fmt_duration(seconds);
    let summary = if label.is_empty() {
        format!("Start a timer for {dur}")
    } else {
        format!("Start a timer for {dur} called \"{}\"", preview(label, 60))
    };
    if seconds == 0 {
        return blocked("", summary, "A timer needs at least one second.");
    }
    if seconds > MAX_TIMER_SECS {
        return blocked("", summary, "Timers are limited to 24 hours.");
    }
    if label.chars().count() > MAX_LABEL_CHARS {
        return blocked(
            label,
            summary,
            format!("That timer name is too long (over {MAX_LABEL_CHARS} characters)."),
        );
    }
    (
        Action::Timer {
            seconds,
            label: label.to_string(),
        },
        summary,
        Risk::Safe,
    )
}

fn parse_hhmm(s: &str) -> Option<u32> {
    let (h, m) = s.split_once(':')?;
    if h.len() != 2 || m.len() != 2 {
        return None;
    }
    let (h, m): (u32, u32) = (h.parse().ok()?, m.parse().ok()?);
    (h < 24 && m < 60).then_some(h * 60 + m)
}

fn reminder(
    title: &str,
    date: &str,
    time: &str,
    due_utc_ms: i64,
    default_time: bool,
    now_utc_ms: i64,
) -> Built {
    let (Some(d), Some(minutes)) = (Date::parse_iso(date), parse_hhmm(time)) else {
        return blocked(
            title,
            "Add a reminder".into(),
            "That date or time is not valid.",
        );
    };
    let mut summary = format!(
        "Remind you on {} at {}: \"{}\"",
        d.short_label(),
        clock_label(minutes),
        preview(title, 80)
    );
    if default_time {
        summary.push_str(" (no time was given, so 9:00 AM)");
    }
    if title.trim().is_empty() {
        return blocked(title, summary, "A reminder needs some text.");
    }
    if title.chars().count() > MAX_TITLE_CHARS {
        return blocked(
            title,
            summary,
            format!("That reminder is too long (over {MAX_TITLE_CHARS} characters)."),
        );
    }
    if due_utc_ms <= now_utc_ms {
        return blocked(title, summary, "That time has already passed.");
    }
    (
        Action::Reminder {
            title: title.to_string(),
            date: date.to_string(),
            time: time.to_string(),
        },
        summary,
        Risk::Confirm,
    )
}

fn note(body: &str) -> Built {
    let summary = format!("Save a note: \"{}\"", preview(body, 80));
    if body.trim().is_empty() {
        return blocked(
            body,
            "Save a note".into(),
            "There is nothing to put in the note.",
        );
    }
    if body.chars().count() > MAX_NOTE_CHARS {
        return blocked(
            body,
            summary,
            format!("That note is too long (over {MAX_NOTE_CHARS} characters)."),
        );
    }
    (
        Action::Note {
            body: body.to_string(),
        },
        summary,
        Risk::Safe,
    )
}

fn email(
    provider: MailProvider,
    to: &[String],
    subject: &str,
    body: &str,
    bad_recipient: Option<&str>,
) -> Built {
    if let Some(who) = bad_recipient {
        return blocked(
            who,
            format!("Draft an email to \"{}\"", preview(who, 40)),
            format!(
                "\"{}\" is not an email address. Krovi doesn't read your contacts, so say the address, for example name@example.com.",
                preview(who, 40)
            ),
        );
    }
    let normalised: Result<Vec<String>, _> = to.iter().map(|a| mail::validate_address(a)).collect();
    let to = match normalised {
        Ok(v) => v,
        Err(e) => return blocked("", "Draft an email".into(), e.to_string()),
    };
    let draft = EmailDraft {
        provider,
        to: to.clone(),
        subject: subject.to_string(),
        body: body.to_string(),
    };
    let link = match mail::compose_url(&draft) {
        Ok(u) => u,
        Err(e) => return blocked("", "Draft an email".into(), e.to_string()),
    };
    let mut summary = String::from("Open an email draft");
    if !to.is_empty() {
        summary.push_str(&format!(" to {}", to.join(", ")));
    }
    if !subject.is_empty() {
        summary.push_str(&format!(" about \"{}\"", preview(subject, 60)));
    }
    summary.push_str(&format!(
        " in {}. Krovi does not send it: you press Send.",
        provider.label()
    ));
    (
        Action::ComposeEmail {
            provider,
            to,
            subject: draft.subject,
            body: draft.body,
            url: link,
        },
        summary,
        Risk::Confirm,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intent::AppEntry;
    use crate::parse::{parse, ParseContext};
    use crate::url::{check_openable, Openable};

    /// Monday 2026-11-02, 10:00:30 UTC.
    fn now_ms() -> i64 {
        Date::new(2026, 11, 2).unwrap().days() * 86_400_000 + (10 * 3600 + 30) * 1000
    }

    fn apps() -> Vec<AppEntry> {
        ["Spotify", "Notepad"]
            .iter()
            .map(|n| AppEntry {
                name: (*n).into(),
                target: format!("shell:AppsFolder\\{n}"),
            })
            .collect()
    }

    fn plan(input: &str) -> Plan {
        let apps = apps();
        let ctx = ParseContext {
            now_utc_ms: now_ms(),
            utc_offset_minutes: 0,
            apps: &apps,
        };
        build_plan("p1", &parse(input, &ctx), now_ms())
    }

    fn only(input: &str) -> Step {
        let mut p = plan(input);
        assert_eq!(p.steps.len(), 1, "{input}: {:?}", p.steps);
        p.steps.remove(0)
    }

    fn blocked_reason(s: &Step) -> &str {
        match &s.risk {
            Risk::Blocked(r) => r,
            other => panic!("expected blocked, got {other:?} for {s:?}"),
        }
    }

    #[test]
    fn risk_policy_table() {
        for (input, risk) in [
            // known sites, search, apps, timers, notes: safe
            ("open youtube", "safe"),
            ("go to github.com", "safe"),
            ("search google for rust", "safe"),
            ("play lofi on youtube", "safe"),
            ("open spotify", "safe"),
            ("launch notepad", "safe"),
            ("set a timer for 10 minutes", "safe"),
            ("note buy milk", "safe"),
            // typed addresses, reminders, email: confirm
            ("go to example.org", "confirm"),
            ("open https://example.com/some/page", "confirm"),
            ("open github.com/rust-lang/rust", "confirm"),
            ("remind me at 5pm to call mom", "confirm"),
            ("remind me in 20 minutes to stretch", "confirm"),
            ("email sam@x.com about the trip", "confirm"),
            ("draft an email", "confirm"),
            // invalid: blocked
            ("go to javascript:alert(1)", "blocked"),
            ("open file:///C:/Windows/System32/calc.exe", "blocked"),
            ("open data:text/html,hi", "blocked"),
            ("open https://user:pass@example.com", "blocked"),
            ("open ftp://example.com", "blocked"),
            ("open http://exa mple.com", "blocked"),
            ("open nonexistentapp", "blocked"),
            ("set a timer for 0 minutes", "blocked"),
            ("set a timer for 25 hours", "blocked"),
            ("remind me today at 9am to stretch", "blocked"),
            ("email mom about dinner", "blocked"),
            ("email notanaddress@ about x", "blocked"),
            ("email sam@x.com,evil@y.com about x", "blocked"),
            ("tell me a joke", "blocked"),
        ] {
            let step = only(input);
            let got = match step.risk {
                Risk::Safe => "safe",
                Risk::Confirm => "confirm",
                Risk::Blocked(_) => "blocked",
            };
            assert_eq!(got, risk, "{input}: {step:?}");
        }
    }

    #[test]
    fn summaries_are_plain_sentences() {
        for (input, want) in [
            ("open youtube", "Open youtube.com in your browser"),
            ("open gmail", "Open mail.google.com in your browser"),
            (
                "search youtube for lofi beats",
                "Search YouTube for \"lofi beats\"",
            ),
            (
                "google rust lifetimes",
                "Search Google for \"rust lifetimes\"",
            ),
            ("open spotify", "Open Spotify"),
            ("set a timer for 10 minutes", "Start a timer for 10 minutes"),
            (
                "set a timer for 90 seconds called tea",
                "Start a timer for 1 minute 30 seconds called \"tea\"",
            ),
            ("note buy milk", "Save a note: \"Buy milk\""),
            (
                "go to example.org",
                "Open https://example.org in your browser",
            ),
            (
                "go to http://example.org/x",
                "Open http://example.org/x (not encrypted) in your browser",
            ),
            (
                "remind me at 5pm to call mom",
                "Remind you on Mon 2 Nov at 5:00 PM: \"Call mom\"",
            ),
            (
                "remind me tomorrow to pay rent",
                "Remind you on Tue 3 Nov at 9:00 AM: \"Pay rent\" (no time was given, so 9:00 AM)",
            ),
        ] {
            assert_eq!(only(input).summary, want, "{input}");
        }
    }

    #[test]
    fn timer_summary_reads_naturally_for_one_unit() {
        assert_eq!(
            only("set a timer for 1 minute").summary,
            "Start a timer for 1 minute"
        );
        assert_eq!(
            only("set a timer for 1 hour").summary,
            "Start a timer for 1 hour"
        );
    }

    #[test]
    fn email_card_has_everything() {
        let step =
            only("draft an email to Sam@X.com in gmail about the trip saying see you at noon");
        assert_eq!(step.risk, Risk::Confirm);
        match &step.action {
            Action::ComposeEmail {
                provider,
                to,
                subject,
                body,
                url,
            } => {
                assert_eq!(*provider, MailProvider::Gmail);
                assert_eq!(to, &["Sam@x.com"]);
                assert_eq!(subject, "The trip");
                assert_eq!(body, "See you at noon");
                assert_eq!(
                    url,
                    "https://mail.google.com/mail/?view=cm&fs=1&to=Sam%40x.com&su=The%20trip&body=See%20you%20at%20noon"
                );
            }
            other => panic!("{other:?}"),
        }
        assert!(step.summary.contains("Sam@x.com") && step.summary.contains("Gmail"));
        assert!(step.summary.contains("does not send"));
    }

    #[test]
    fn email_never_leaves_the_compose_link_in_the_ui_payload() {
        let step = only("email sam@x.com about the trip");
        let json = serde_json::to_value(&step).unwrap();
        assert_eq!(json["action"]["type"], "compose-email");
        assert!(json["action"].get("url").is_none(), "{json}");
        assert_eq!(json["action"]["to"][0], "sam@x.com");
        assert_eq!(json["action"]["provider"], "default");
    }

    #[test]
    fn json_shape_matches_the_typescript_bridge() {
        let p =
            plan("open youtube then go to example.org then open nonexistentapp then note buy milk");
        let json = serde_json::to_value(&p).unwrap();
        assert_eq!(json["id"], "p1");
        let steps = json["steps"].as_array().unwrap();
        assert_eq!(steps.len(), 4, "{json}");
        assert_eq!(steps[0]["id"], "s1");
        assert_eq!(steps[0]["risk"], serde_json::json!({ "level": "safe" }));
        assert_eq!(
            steps[0]["action"],
            serde_json::json!({ "type": "open-url", "url": "https://www.youtube.com/", "site": "YouTube", "known": true })
        );
        assert_eq!(steps[1]["risk"], serde_json::json!({ "level": "confirm" }));
        assert_eq!(
            steps[1]["action"],
            serde_json::json!({ "type": "open-url", "url": "https://example.org", "site": "example.org", "known": false })
        );
        assert_eq!(steps[2]["risk"]["level"], "blocked");
        assert!(steps[2]["risk"]["reason"]
            .as_str()
            .unwrap()
            .contains("couldn't find"));
        assert_eq!(steps[2]["action"]["type"], "unsupported");
        assert_eq!(
            steps[3]["action"],
            serde_json::json!({ "type": "note", "body": "Buy milk" })
        );
        assert!(json["suggestions"].as_array().unwrap().is_empty());
    }

    #[test]
    fn an_unintelligible_sentence_returns_suggestions_and_a_blocked_step() {
        let p = plan("tell me a joke");
        let json = serde_json::to_value(&p).unwrap();
        assert_eq!(json["steps"][0]["risk"]["level"], "blocked");
        assert!(json["steps"][0]["risk"]["reason"]
            .as_str()
            .unwrap()
            .contains("didn't understand"));
        assert_eq!(
            json["steps"][0]["action"],
            serde_json::json!({ "type": "unsupported", "text": "tell me a joke" })
        );
        assert!(json["suggestions"].as_array().unwrap().len() >= 3);
    }

    #[test]
    fn reminder_action_carries_local_date_and_time() {
        match only("remind me at 5pm to call mom").action {
            Action::Reminder { title, date, time } => {
                assert_eq!(
                    (title.as_str(), date.as_str(), time.as_str()),
                    ("Call mom", "2026-11-02", "17:00")
                );
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn blocked_steps_say_why() {
        assert!(blocked_reason(&only("go to javascript:alert(1)")).contains("javascript"));
        assert!(blocked_reason(&only("open https://user:pass@example.com")).contains("password"));
        assert!(blocked_reason(&only("open nonexistentapp")).contains("couldn't find"));
        assert!(blocked_reason(&only("set a timer for 25 hours")).contains("24 hours"));
        assert!(blocked_reason(&only("set a timer for 0 minutes")).contains("at least"));
        assert!(blocked_reason(&only("remind me today at 9am to stretch")).contains("passed"));
        assert!(blocked_reason(&only("email mom about dinner")).contains("contacts"));
        assert!(blocked_reason(&only("email x@@y.com about it"))
            .to_lowercase()
            .contains("email address"));
        assert!(blocked_reason(&only("go to example.org:99999")).contains("port"));
    }

    #[test]
    fn ambiguous_app_lists_the_candidates() {
        let apps = vec![
            AppEntry {
                name: "Adobe Reader".into(),
                target: "a".into(),
            },
            AppEntry {
                name: "Adobe Photoshop".into(),
                target: "b".into(),
            },
        ];
        let ctx = ParseContext {
            now_utc_ms: now_ms(),
            utc_offset_minutes: 0,
            apps: &apps,
        };
        let p = build_plan("p", &parse("open adobe", &ctx), now_ms());
        let reason = blocked_reason(&p.steps[0]).to_string();
        assert!(
            reason.contains("Adobe Photoshop")
                && reason.contains("Adobe Reader")
                && reason.contains("full name"),
            "{reason}"
        );
    }

    #[test]
    fn long_and_odd_inputs_are_blocked_not_run() {
        // (the sentence parser caps input well below this; the planner enforces it on its own)
        let long_note = Intent::Note {
            body: "x".repeat(MAX_NOTE_CHARS + 1),
        };
        let p = build_plan("p", &[long_note], now_ms());
        assert!(blocked_reason(&p.steps[0]).contains("too long"));
        let empty_note = Intent::Note { body: "  ".into() };
        assert!(build_plan("p", &[empty_note], now_ms()).steps[0].is_blocked());
        let long_search = format!("search google for {}", "x".repeat(400));
        assert!(blocked_reason(&only(&long_search)).contains("too long"));
        let long_timer_label = format!("set a timer for 5 minutes called {}", "x".repeat(100));
        assert!(blocked_reason(&only(&long_timer_label)).contains("too long"));
        let long_subject = format!("email sam@x.com about {}", "s".repeat(300));
        assert!(blocked_reason(&only(&long_subject)).contains("subject"));
    }

    #[test]
    fn step_ids_are_unique_and_ordered() {
        let p = plan("open youtube then open github then open reddit then note hi");
        let ids: Vec<&str> = p.steps.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["s1", "s2", "s3", "s4"]);
        assert_eq!(
            p.step("s2").unwrap().summary,
            "Open github.com in your browser"
        );
        assert!(p.step("s9").is_none());
        assert_eq!(p.confirm_steps().count(), 0);
    }

    #[test]
    fn too_many_steps_are_cut_off_with_a_blocked_marker() {
        let sentence = ["open youtube"; 12].join(" then ");
        let apps = apps();
        let ctx = ParseContext {
            now_utc_ms: now_ms(),
            utc_offset_minutes: 0,
            apps: &apps,
        };
        let intents = parse(&sentence, &ctx);
        let p = build_plan("p", &intents, now_ms());
        assert!(p.steps.len() <= MAX_STEPS + 1);
        if intents.len() > MAX_STEPS {
            assert!(p.steps.last().unwrap().is_blocked());
        }
    }

    #[test]
    fn suggestions_are_deduplicated_and_capped() {
        let p = plan("tell me a joke then tell me another then tell me a third");
        assert!(p.suggestions.len() <= 4);
        let mut seen = std::collections::HashSet::new();
        assert!(p.suggestions.iter().all(|s| seen.insert(s.clone())));
    }

    #[test]
    fn every_runnable_step_with_a_link_passes_the_open_gate() {
        let inputs = [
            "open youtube",
            "go to example.org",
            "open example.com/a?b=c&d=e#f",
            "search google for <script>alert(1)</script> & \"quotes\" 100%",
            "search youtube for caf\u{e9} \u{1f600}",
            "play lo-fi on youtube",
            "search github for a/b?c=d#e",
            "email sam@x.com about Q3 <numbers> & more saying line one",
            "email sam@x.com in outlook about x",
            "email sam@x.com in gmail about x",
        ];
        for input in inputs {
            for step in plan(input).steps {
                if step.is_blocked() {
                    continue;
                }
                let link = match &step.action {
                    Action::OpenUrl { url, .. } | Action::Search { url, .. } => url.clone(),
                    Action::ComposeEmail { url, .. } => url.clone(),
                    _ => continue,
                };
                assert!(check_openable(&link).is_ok(), "{input}: {link}");
                assert!(matches!(
                    check_openable(&link),
                    Ok(Openable::Web | Openable::Mailto)
                ));
            }
        }
    }

    #[test]
    fn odd_input_never_panics_and_always_yields_a_plan() {
        let sentences = [
            "open youtube and then search for caf\u{e9} \u{1f600} lofi",
            "remind me tomorrow at 9:30 am to call m\u{f6}m; note \u{65e5}\u{672c}\u{8a9e} \u{30c6}\u{30b9}\u{30c8}",
            "email s\u{e4}m@x.com about the tr\u{ee}p saying h\u{e9}llo",
            "set a timer for 1h30m called t\u{e9}a then go to example.com/\u{e9}",
            "play \u{1f3b5} on youtube, then take a note: \u{201c}quoted\u{201d} text",
            "  \t\n open\u{a0}youtube\u{2003}now  ",
            "((( ))) ... ;;; &&& ||| \"\"\" ''' <<< >>> {{{ }}} [[[ ]]]",
            "and and then then ; ; and then ;",
            "remind me at 25:61 to \u{0}\u{1} control",
            "open https://",
            "open http://[::1]:80/\u{7f}",
            "note \u{202e}rtl override\u{202c} text",
        ];
        for s in sentences {
            let chars: Vec<char> = s.chars().collect();
            for end in 0..=chars.len() {
                let prefix: String = chars[..end].iter().collect();
                let suffix: String = chars[end..].iter().collect();
                for input in [prefix, suffix] {
                    let p = plan(&input);
                    assert!(!p.steps.is_empty(), "{input:?}");
                    for step in &p.steps {
                        // a plan never runs something it did not validate
                        if let Action::OpenUrl { url, .. } | Action::Search { url, .. } =
                            &step.action
                        {
                            if !step.is_blocked() {
                                assert!(url::validate_full(url).is_ok(), "{input:?}: {url}");
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn nothing_dangerous_is_ever_runnable() {
        let nasty = [
            "open javascript:alert(1)",
            "go to JaVaScRiPt:alert(1)",
            "open file:///C:/Windows/System32/cmd.exe",
            "open \\\\evil\\share\\x.exe",
            "open //evil.example/x",
            "go to https://evil.example@good.example/",
            "open https://good.example\\@evil.example",
            "open vbscript:msgbox(1)",
            "open data:text/html;base64,PHNjcmlwdD4=",
            "open ms-settings:network",
            "open ms-msdt:/id PCWDiagnostic",
            "open calc.exe",
            "open cmd",
            "launch cmd /c calc",
            "open http://localhost:99999",
            "open http://0x7f.1/",
        ];
        for input in nasty {
            for step in plan(input).steps {
                let runnable = !step.is_blocked();
                if !runnable {
                    continue;
                }
                match &step.action {
                    Action::OpenUrl { url, .. } => {
                        assert!(url::validate_full(url).is_ok(), "{input}: {url}");
                        assert_eq!(
                            step.risk,
                            Risk::Confirm,
                            "{input}: a typed address must be confirmed"
                        );
                    }
                    Action::OpenApp { target, .. } => {
                        // only a target from the supplied list
                        assert!(target.starts_with("shell:AppsFolder\\"), "{input}");
                    }
                    other => panic!("{input}: unexpected runnable action {other:?}"),
                }
            }
        }
    }
}
