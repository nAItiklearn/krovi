//! Voice v3 end to end with synthesized speech through the REAL SAPI recognizer, using the REAL
//! folder / app catalog of this machine: "open [folder]", "open [app]", "what's in [folder]",
//! "find [name]" and the spoken reminder time phrases. Nothing is executed here (no window is
//! opened, no app launched): the test checks what the recognizer heard and what it parsed to.
//!
//!   cargo test -p krovi-voice --features native --test wav_v3 -- --nocapture
//!
//! Expectation for every phrase = the parser's reading of the text that was SAID, so a pass means
//! "the recognizer heard it well enough to reach the same command". Skips (passes) when the
//! recognizer, PowerShell or the TTS voices are missing.
#![cfg(all(windows, feature = "native"))]

use krovi_voice::os::scan_catalog;
use krovi_voice::{
    parse_intent, transcribe_wav_with, Catalog, Env, LocalNow, Mode, Parsed, StartOptions,
    VoiceCommand,
};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

struct Case {
    slug: &'static str,
    said: &'static str,
    group: Group,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Group {
    Folder,
    List,
    App,
    Search,
    Reminder,
    /// ordinary speech that must NOT become an action
    Chatter,
}

use Group::*;

fn cases() -> Vec<Case> {
    let c = |slug, said, group| Case { slug, said, group };
    vec![
        c("f-downloads", "open downloads", Folder),
        c("f-documents", "open my documents folder", Folder),
        c("f-pictures", "show my pictures", Folder),
        c("f-desktop", "go to desktop", Folder),
        c("f-krovi-dl", "open krovi downloads", Folder),
        c("f-music", "open the music folder", Folder),
        c("f-videos", "browse videos", Folder),
        c("f-take-me", "take me to downloads", Folder),
        c("l-downloads", "what's in downloads", List),
        c("l-documents", "what's in my documents folder", List),
        c("l-desktop", "tell me about the desktop", List),
        c("l-pictures", "list the contents of pictures", List),
        c("a-notepad", "open notepad", App),
        c("a-calculator", "launch calculator", App),
        c("a-chrome", "open google chrome", App),
        c("a-firefox", "start firefox", App),
        c("a-spotify", "launch spotify", App),
        c("a-paint", "open paint", App),
        c("a-vscode", "launch visual studio code", App),
        c("a-taskmgr", "launch task manager", App),
        c("a-snip", "open snipping tool", App),
        c("a-edge", "open microsoft edge", App),
        c("s-invoice", "search for invoice", Search),
        c("s-tax", "search for tax return", Search),
        c("s-boarding", "find me boarding pass", Search),
        c("s-budget", "find the file budget", Search),
        c("s-resume", "find file resume", Search),
        c("r-5pm", "remind me at five pm to call mom", Reminder),
        c(
            "r-530",
            "remind me at five thirty in the evening to take the pills",
            Reminder,
        ),
        c(
            "r-20min",
            "remind me in twenty minutes to check the oven",
            Reminder,
        ),
        c("r-hour", "remind me in an hour to stretch", Reminder),
        c(
            "r-tom9",
            "remind me tomorrow at nine to call the bank",
            Reminder,
        ),
        c(
            "r-fri-noon",
            "remind me on friday at noon to send the report",
            Reminder,
        ),
        c(
            "r-tonight8",
            "remind me tonight at eight to water the plants",
            Reminder,
        ),
        c(
            "r-tom-morning",
            "set a reminder for tomorrow at nine thirty in the morning to renew the passport",
            Reminder,
        ),
        c("x-door", "open the door", Chatter),
        c("x-window", "can you open the window please", Chatter),
        c("x-fridge", "what is in the fridge", Chatter),
        c("x-money", "show me the money", Chatter),
        c("x-store", "go to the store", Chatter),
        c("x-day", "tell me about your day", Chatter),
        c("x-car", "start the car", Chatter),
        c("x-rocket", "launch the rocket", Chatter),
    ]
}

fn synthesize(dir: &Path, voice: &str, prefix: &str, items: &[(&str, &str)]) -> bool {
    let manifest = dir.join(format!("{prefix}manifest.tsv"));
    let lines: Vec<String> = items
        .iter()
        .map(|(slug, said)| format!("{slug}\t{said}"))
        .collect();
    std::fs::write(&manifest, lines.join("\n")).unwrap();
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("testdata")
        .join("gen-wavs.ps1");
    let out = Command::new("powershell")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(&script)
        .arg("-OutDir")
        .arg(dir)
        .arg("-Manifest")
        .arg(&manifest)
        .args(["-Voice", voice, "-Rate", "16000", "-Prefix", prefix])
        .output();
    matches!(out, Ok(o) if o.status.success())
}

/// What the parser makes of exactly the words that were said (the answer key).
fn answer_key(said: &str, env: &Env) -> Option<VoiceCommand> {
    match parse_intent(said, env) {
        Parsed::Command(c) => Some(c),
        _ => None,
    }
}

fn is_os_action(c: &VoiceCommand) -> bool {
    krovi_voice::actions::is_os_command(c)
}

#[derive(Default, Clone, Copy)]
struct Tally {
    total: usize,
    ok: usize,
    /// reminders: the date and time (not the title) matched
    when_ok: usize,
}

fn run_voice(
    dir: &Path,
    label: &str,
    voice: &str,
    catalog: &Arc<Catalog>,
) -> Option<Vec<(Group, Tally)>> {
    let prefix = format!("{label}-");
    let cs = cases();
    let items: Vec<(&str, &str)> = cs.iter().map(|c| (c.slug, c.said)).collect();
    let cached = dir.join(format!("{prefix}{}.wav", items[0].0)).exists();
    if !cached && !synthesize(dir, voice, &prefix, &items) {
        eprintln!("[skip] could not synthesise with voice {voice:?}");
        return None;
    }
    // The clock moves while the suite runs (relative phrases like "in twenty minutes" depend on it):
    // the answer key is computed from the clock just before AND just after each recognition.
    let env_now = || Env::at(LocalNow::system(), None).with_catalog(Some(catalog.clone()));
    println!("\n=== {label}: voice={voice:?} ===");
    let mut tallies: Vec<(Group, Tally)> = [Folder, List, App, Search, Reminder, Chatter]
        .into_iter()
        .map(|g| (g, Tally::default()))
        .collect();
    for c in &cs {
        // skip phrases about apps/folders this machine does not have
        let key = answer_key(c.said, &env_now());
        if c.group != Chatter && c.group != Reminder && key.is_none() {
            println!("SKIP {:<14} {:<62} (not on this machine)", c.slug, c.said);
            continue;
        }
        let wav = dir.join(format!("{prefix}{}.wav", c.slug));
        let opts = StartOptions {
            catalog: Some(catalog.clone()),
            ..Default::default()
        };
        let r = transcribe_wav_with(&wav, Mode::Command, opts)
            .unwrap_or_else(|e| panic!("{} failed: {e}", c.slug));
        let key_after = answer_key(c.said, &env_now());
        let (good, when_good) = match c.group {
            Chatter => (r.command.is_none(), false),
            Reminder => {
                let same_when = |k: &Option<VoiceCommand>| match (k, &r.command) {
                    (
                        Some(VoiceCommand::AddReminder {
                            date: d1, time: t1, ..
                        }),
                        Some(VoiceCommand::AddReminder {
                            date: d2, time: t2, ..
                        }),
                    ) => d1 == d2 && t1 == t2,
                    _ => false,
                };
                let when_good = same_when(&key) || same_when(&key_after);
                let title_ok = match (&key, &r.command) {
                    (
                        Some(VoiceCommand::AddReminder { title: a, .. }),
                        Some(VoiceCommand::AddReminder { title: b, .. }),
                    ) => a.eq_ignore_ascii_case(b),
                    _ => false,
                };
                (when_good && title_ok, when_good)
            }
            Search => (
                match (&key, &r.command) {
                    (
                        Some(VoiceCommand::SearchFiles { query: a }),
                        Some(VoiceCommand::SearchFiles { query: b }),
                    ) => a.eq_ignore_ascii_case(b),
                    _ => false,
                },
                false,
            ),
            _ => (r.command == key, false),
        };
        let t = &mut tallies.iter_mut().find(|(g, _)| *g == c.group).unwrap().1;
        t.total += 1;
        t.ok += usize::from(good);
        t.when_ok += usize::from(when_good);
        println!(
            "{} {:<14} said={:<62} heard={:<64} -> {}",
            if good { "PASS" } else { "FAIL" },
            c.slug,
            c.said,
            r.text,
            match (&r.command, &r.feedback) {
                (Some(cmd), _) => format!("{cmd:?}"),
                (None, Some(f)) => format!("(none) feedback={f:?}"),
                (None, None) => "(none)".into(),
            }
        );
        if c.group == Chatter {
            if let Some(cmd) = &r.command {
                println!("     ^ false accept (os action: {})", is_os_action(cmd));
            }
        }
    }
    println!("--- {label} summary");
    for (g, t) in &tallies {
        if *g == Reminder {
            println!(
                "  {:<9} {}/{} fully correct, {}/{} date+time correct",
                format!("{g:?}"),
                t.ok,
                t.total,
                t.when_ok,
                t.total
            );
        } else if *g == Chatter {
            println!(
                "  {:<9} {}/{} correctly ignored ({} false accepts)",
                format!("{g:?}"),
                t.ok,
                t.total,
                t.total - t.ok
            );
        } else {
            println!("  {:<9} {}/{}", format!("{g:?}"), t.ok, t.total);
        }
    }
    Some(tallies)
}

#[test]
fn v3_intents_through_the_real_recognizer() {
    if !krovi_voice::available() {
        eprintln!("[skip] no on-device recognizer on this machine");
        return;
    }
    let keep = std::env::var_os("KROVI_TEST_WAV_DIR").map(PathBuf::from);
    let dir = keep.clone().unwrap_or_else(|| {
        std::env::temp_dir().join(format!("krovi-voice-v3-{}", std::process::id()))
    });
    std::fs::create_dir_all(&dir).unwrap();

    let catalog = Arc::new(scan_catalog());
    println!(
        "catalog: {} folders, {} apps",
        catalog.folders.len(),
        catalog.apps.len()
    );
    assert!(catalog.folders.len() >= 5 && !catalog.apps.is_empty());

    let david = run_voice(&dir, "v3-david", "Microsoft David Desktop", &catalog);
    let zira = run_voice(&dir, "v3-zira", "Microsoft Zira Desktop", &catalog);
    if keep.is_none() {
        let _ = std::fs::remove_dir_all(&dir);
    }

    let score = |t: &[(Group, Tally)]| {
        let (ok, total) = t
            .iter()
            .filter(|(g, _)| matches!(g, Folder | List | App))
            .fold((0, 0), |(o, n), (_, t)| (o + t.ok, n + t.total));
        (ok, total)
    };
    if let Some(t) = &david {
        let (ok, total) = score(t);
        assert!(ok * 100 >= total * 75, "David open/list {ok}/{total}");
    }
    if let Some(t) = &zira {
        let (ok, total) = score(t);
        assert!(ok * 100 >= total * 50, "Zira open/list {ok}/{total}");
    }
}
