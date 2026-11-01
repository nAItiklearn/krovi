//! Carrying out the Voice v3 commands (open a folder, launch an app, search the index, list a
//! folder). The OS side effects sit behind [`Host`] so the logic - what is checked, what the user
//! is told - is unit-tested with a fake; `os::WinHost` is the real one.
//!
//! This module only ever acts on a [`VoiceCommand`] the parser built from the closed catalog (or
//! a search term). It is not exposed as "run this path" to anything else.

use std::path::Path;

use crate::civil::{clock_label, Date};
use crate::listing::{list_folder, rows, summarize};
use crate::types::{ResultItem, ResultKind, VoiceCommand};

/// Most search hits shown.
pub const SEARCH_LIMIT: usize = 20;

/// The side effects. Implementations must not touch the network.
pub trait Host {
    /// Show the folder in an Explorer window.
    fn open_folder(&self, path: &str) -> Result<(), String>;
    /// Launch an app by its catalog launch target (`shell:AppsFolder\<AUMID>` or a `.lnk`).
    fn launch_app(&self, target: &str) -> Result<(), String>;
    /// Query the Windows Search index by file name; at most `limit` rows, newest first.
    fn search_files(&self, query: &str, limit: usize) -> Result<Vec<ResultItem>, String>;
}

/// What happened, in the user's terms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// The action was really carried out (folder opened, app launched, search/listing done).
    pub executed: bool,
    pub feedback: String,
    pub results: Vec<ResultItem>,
}

impl Outcome {
    fn failed(feedback: impl Into<String>) -> Outcome {
        Outcome {
            executed: false,
            feedback: feedback.into(),
            results: Vec::new(),
        }
    }
}

/// Carry out `result.command` if it is one of the OS actions, filling in `executed`, `feedback`
/// and `results`. Any other result is left untouched.
pub fn apply(result: &mut crate::types::VoiceResult, host: &dyn Host) {
    if let Some(outcome) = result.command.as_ref().and_then(|c| run(c, host)) {
        result.executed = outcome.executed;
        result.feedback = Some(outcome.feedback);
        result.results = outcome.results;
    }
}

/// Whether `cmd` is one of the four OS actions.
pub fn is_os_command(cmd: &VoiceCommand) -> bool {
    matches!(
        cmd,
        VoiceCommand::OpenFolder { .. }
            | VoiceCommand::OpenApp { .. }
            | VoiceCommand::SearchFiles { .. }
            | VoiceCommand::ListFolder { .. }
    )
}

/// Run an OS action. `None` for any other command.
pub fn run(cmd: &VoiceCommand, host: &dyn Host) -> Option<Outcome> {
    Some(match cmd {
        VoiceCommand::OpenFolder { name, path } => {
            if !Path::new(path).is_dir() {
                return Some(Outcome::failed(missing_folder(name, path)));
            }
            match host.open_folder(path) {
                Ok(()) => Outcome {
                    executed: true,
                    feedback: format!("Opened {name} ({path})."),
                    results: vec![ResultItem::new(ResultKind::Folder, name, path)],
                },
                Err(e) => Outcome::failed(format!("Could not open {name}: {e}")),
            }
        }
        VoiceCommand::OpenApp { name, path } => match host.launch_app(path) {
            Ok(()) => Outcome {
                executed: true,
                feedback: format!("Launched {name}."),
                results: vec![ResultItem::new(ResultKind::App, name, path)],
            },
            Err(e) => Outcome::failed(format!("Could not launch {name}: {e}")),
        },
        VoiceCommand::SearchFiles { query } => match host.search_files(query, SEARCH_LIMIT) {
            Ok(results) if results.is_empty() => Outcome {
                executed: true,
                feedback: format!(
                    "Nothing in the Windows Search index is named like \"{query}\". Only indexed locations are searched."
                ),
                results,
            },
            Ok(results) => Outcome {
                executed: true,
                feedback: format!(
                    "Found {} for \"{query}\"{}.",
                    if results.len() == 1 {
                        "1 match".to_string()
                    } else {
                        format!("{} matches", results.len())
                    },
                    if results.len() >= SEARCH_LIMIT {
                        " (showing the first 20)"
                    } else {
                        ""
                    }
                ),
                results,
            },
            Err(e) => Outcome::failed(format!("File search is not available: {e}")),
        },
        VoiceCommand::ListFolder { name, path } => match list_folder(Path::new(path)) {
            Ok(listing) => Outcome {
                executed: true,
                feedback: summarize(name, &listing),
                results: rows(&listing),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Outcome::failed(missing_folder(name, path))
            }
            Err(e) => Outcome::failed(format!("Could not read {name}: {e}")),
        },
        _ => return None,
    })
}

/// The confirmation for a saved reminder, stating the EXACT resolved date and time so there is no
/// ambiguity about what was understood: "Reminder set for today, Sun 1 Nov, 5:00 PM: Call mom".
/// `date` is `yyyy-mm-dd`, `time` is `HH:MM` (24h) when one was spoken.
pub fn reminder_feedback(date: &str, time: Option<&str>, title: &str, today: Date) -> String {
    let parsed = date
        .split('-')
        .map(|p| p.parse::<i32>().ok())
        .collect::<Vec<_>>();
    let day = match parsed.as_slice() {
        [Some(y), Some(m), Some(d)] if *m > 0 && *d > 0 => Date::new(*y, *m as u32, *d as u32),
        _ => None,
    };
    let when = match day {
        Some(d) => {
            let mut label = d.short_label();
            if d.y != today.y {
                label = format!("{label} {}", d.y);
            }
            if d == today {
                format!("today, {label}")
            } else if d == today.add_days(1) {
                format!("tomorrow, {label}")
            } else {
                label
            }
        }
        None => date.to_string(),
    };
    let at = time
        .and_then(|t| t.split_once(':'))
        .and_then(|(h, m)| Some(h.parse::<u32>().ok()? * 60 + m.parse::<u32>().ok()?))
        .map(clock_label)
        .unwrap_or_else(|| "no alert time".to_string());
    format!("Reminder set for {when}, {at}: {title}")
}

fn missing_folder(name: &str, path: &str) -> String {
    if name.eq_ignore_ascii_case("krovi downloads") {
        "The Krovi downloads folder does not exist yet; it is created when a phone sends a file to Quick Transfer.".to_string()
    } else {
        format!("{name} ({path}) is not available any more.")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    struct Fake {
        calls: RefCell<Vec<String>>,
        hits: Vec<ResultItem>,
        fail: bool,
    }

    impl Host for Fake {
        fn open_folder(&self, path: &str) -> Result<(), String> {
            self.calls.borrow_mut().push(format!("folder {path}"));
            if self.fail {
                Err("boom".into())
            } else {
                Ok(())
            }
        }
        fn launch_app(&self, target: &str) -> Result<(), String> {
            self.calls.borrow_mut().push(format!("app {target}"));
            if self.fail {
                Err("boom".into())
            } else {
                Ok(())
            }
        }
        fn search_files(&self, query: &str, limit: usize) -> Result<Vec<ResultItem>, String> {
            self.calls
                .borrow_mut()
                .push(format!("search {query} {limit}"));
            if self.fail {
                Err("index offline".into())
            } else {
                Ok(self.hits.clone())
            }
        }
    }

    fn temp_dir() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("krovi-actions-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn open_folder_checks_it_exists_then_opens_and_confirms() {
        let dir = temp_dir();
        let host = Fake::default();
        let cmd = VoiceCommand::OpenFolder {
            name: "Downloads".into(),
            path: dir.to_string_lossy().into_owned(),
        };
        let out = run(&cmd, &host).unwrap();
        assert!(out.executed);
        assert!(
            out.feedback.starts_with("Opened Downloads ("),
            "{}",
            out.feedback
        );
        assert_eq!(out.results.len(), 1);
        assert_eq!(out.results[0].kind, ResultKind::Folder);
        assert_eq!(host.calls.borrow().len(), 1);

        let gone = VoiceCommand::OpenFolder {
            name: "Projects".into(),
            path: r"Z:\definitely\not\here".into(),
        };
        let out = run(&gone, &host).unwrap();
        assert!(!out.executed);
        assert!(out.feedback.contains("not available any more"));
        assert_eq!(
            host.calls.borrow().len(),
            1,
            "a missing folder is never handed to the OS"
        );

        let krovi = VoiceCommand::OpenFolder {
            name: "Krovi downloads".into(),
            path: r"Z:\nope\Krovi".into(),
        };
        assert!(run(&krovi, &host)
            .unwrap()
            .feedback
            .contains("does not exist yet"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn launching_reports_the_app_or_the_failure() {
        let ok = Fake::default();
        let cmd = VoiceCommand::OpenApp {
            name: "Notepad".into(),
            path: r"shell:AppsFolder\Microsoft.WindowsNotepad_8wekyb3d8bbwe!App".into(),
        };
        let out = run(&cmd, &ok).unwrap();
        assert_eq!(
            (out.executed, out.feedback.as_str()),
            (true, "Launched Notepad.")
        );
        assert_eq!(out.results[0].kind, ResultKind::App);
        let bad = Fake {
            fail: true,
            ..Default::default()
        };
        let out = run(&cmd, &bad).unwrap();
        assert!(!out.executed);
        assert!(out.feedback.contains("Could not launch Notepad"));
    }

    #[test]
    fn search_reports_matches_none_and_errors() {
        let hit = |n: &str| ResultItem::new(ResultKind::File, n, format!(r"C:\Docs\{n}"));
        let host = Fake {
            hits: vec![hit("invoice-march.pdf"), hit("invoice-april.pdf")],
            ..Default::default()
        };
        let cmd = VoiceCommand::SearchFiles {
            query: "invoice".into(),
        };
        let out = run(&cmd, &host).unwrap();
        assert!(out.executed);
        assert_eq!(out.feedback, "Found 2 matches for \"invoice\".");
        assert_eq!(out.results.len(), 2);
        assert_eq!(host.calls.borrow()[0], "search invoice 20");

        let none = Fake::default();
        let out = run(&cmd, &none).unwrap();
        assert!(out.executed && out.results.is_empty());
        assert!(out.feedback.contains("Only indexed locations"));

        let down = Fake {
            fail: true,
            ..Default::default()
        };
        let out = run(&cmd, &down).unwrap();
        assert!(!out.executed);
        assert!(out.feedback.contains("index offline"));

        let one = Fake {
            hits: vec![hit("a.txt")],
            ..Default::default()
        };
        assert_eq!(
            run(&cmd, &one).unwrap().feedback,
            "Found 1 match for \"invoice\"."
        );
    }

    #[test]
    fn listing_a_folder_uses_the_filesystem_not_the_host() {
        let dir = std::env::temp_dir().join(format!("krovi-actions-list-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), "hello").unwrap();
        std::fs::create_dir(dir.join("sub")).unwrap();
        let host = Fake::default();
        let cmd = VoiceCommand::ListFolder {
            name: "Docs".into(),
            path: dir.to_string_lossy().into_owned(),
        };
        let out = run(&cmd, &host).unwrap();
        assert!(out.executed);
        assert!(
            out.feedback.starts_with("Docs: 2 items (1 file, 1 folder)"),
            "{}",
            out.feedback
        );
        assert_eq!(out.results.len(), 2);
        assert!(host.calls.borrow().is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn reminder_confirmations_state_the_exact_date_and_time() {
        let today = Date::new(2026, 11, 1).unwrap(); // a Sunday
        let f = |d: &str, t: Option<&str>| reminder_feedback(d, t, "Call mom", today);
        assert_eq!(
            f("2026-11-01", Some("17:00")),
            "Reminder set for today, Sun 1 Nov, 5:00 PM: Call mom"
        );
        assert_eq!(
            f("2026-11-01", Some("17:30")),
            "Reminder set for today, Sun 1 Nov, 5:30 PM: Call mom"
        );
        assert_eq!(
            f("2026-11-02", Some("09:00")),
            "Reminder set for tomorrow, Mon 2 Nov, 9:00 AM: Call mom"
        );
        assert_eq!(
            f("2026-11-06", Some("12:00")),
            "Reminder set for Fri 6 Nov, 12:00 PM: Call mom"
        );
        assert_eq!(
            f("2026-11-02", Some("00:05")),
            "Reminder set for tomorrow, Mon 2 Nov, 12:05 AM: Call mom"
        );
        assert_eq!(
            f("2026-11-05", None),
            "Reminder set for Thu 5 Nov, no alert time: Call mom"
        );
        assert_eq!(
            f("2027-01-02", Some("08:00")),
            "Reminder set for Sat 2 Jan 2027, 8:00 AM: Call mom"
        );
        // a malformed date is echoed, never hidden
        assert!(f("soon", None).contains("soon"));
    }

    #[test]
    fn other_commands_are_not_handled_here() {
        let host = Fake::default();
        assert!(run(&VoiceCommand::Collapse, &host).is_none());
        assert!(!is_os_command(&VoiceCommand::StartPomodoro));
        assert!(is_os_command(&VoiceCommand::SearchFiles {
            query: "x".into()
        }));
    }
}
