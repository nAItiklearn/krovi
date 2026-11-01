//! Voice v2 end to end with synthesized speech: reminders, "this" actions (shelf, phone, note),
//! timers and media, and the clean-transcription pass, all through the REAL SAPI recognizer.
//! Run with `-- --nocapture` for the table of what was said, heard and understood:
//!
//!   cargo test -p krovi-voice --features native --test wav_v2 -- --nocapture
//!
//! Skips (passes) when the recognizer, PowerShell or the TTS voices are missing.
#![cfg(all(windows, feature = "native"))]

use krovi_voice::{
    transcribe_wav_with, Date, LocalNow, MediaCommand, Mode, StartOptions, TimerAction,
    TimerTarget, ToolId, VoiceCommand, VoiceContext,
};
use std::path::{Path, PathBuf};
use std::process::Command;

use VoiceCommand::*;

struct Case {
    slug: &'static str,
    said: &'static str,
    context: Option<VoiceContext>,
    expect: Expect,
}

enum Expect {
    Cmd(VoiceCommand),
    /// Understood but the selection is missing: no command, and a feedback line.
    NeedsSelection,
}

fn text_ctx() -> Option<VoiceContext> {
    Some(VoiceContext::Text {
        value: "Call the dentist at 3".into(),
        app: Some("notepad".into()),
    })
}

fn files_ctx() -> Option<VoiceContext> {
    Some(VoiceContext::Files {
        value: vec![r"C:\docs\report.pdf".into(), r"C:\docs\photo.jpg".into()],
        app: Some("explorer".into()),
    })
}

fn weekday_after_today(now: &LocalNow, target_monday0: u32) -> Date {
    let ahead = (target_monday0 + 7 - now.date.weekday()) % 7;
    now.date
        .add_days(if ahead == 0 { 7 } else { i64::from(ahead) })
}

fn cases(now: &LocalNow) -> Vec<Case> {
    let tomorrow = now.date.add_days(1).iso();
    let friday = weekday_after_today(now, 4).iso();
    vec![
        Case {
            slug: "rem-tomorrow",
            said: "add a reminder for tomorrow at nine to call mom",
            context: None,
            expect: Expect::Cmd(AddReminder {
                date: tomorrow,
                time: Some("09:00".into()),
                title: "Call mom".into(),
            }),
        },
        Case {
            slug: "rem-friday",
            said: "remind me friday to submit the report",
            context: None,
            expect: Expect::Cmd(AddReminder {
                date: friday,
                time: None,
                title: "Submit the report".into(),
            }),
        },
        Case {
            slug: "note-from-selection",
            said: "save this as a note",
            context: text_ctx(),
            expect: Expect::Cmd(NewNote {
                body: "Call the dentist at 3".into(),
            }),
        },
        Case {
            slug: "shelf-files",
            said: "copy this to the file shelf",
            context: files_ctx(),
            expect: Expect::Cmd(ShelfAdd {
                paths: vec![r"C:\docs\report.pdf".into(), r"C:\docs\photo.jpg".into()],
                text: None,
            }),
        },
        Case {
            slug: "shelf-text",
            said: "put this on the shelf",
            context: text_ctx(),
            expect: Expect::Cmd(ShelfAdd {
                paths: vec![],
                text: Some("Call the dentist at 3".into()),
            }),
        },
        Case {
            slug: "phone-files",
            said: "send this to my phone",
            context: files_ctx(),
            expect: Expect::Cmd(TransferSend {
                paths: vec![r"C:\docs\report.pdf".into(), r"C:\docs\photo.jpg".into()],
                text: None,
            }),
        },
        Case {
            slug: "phone-nothing",
            said: "send this to my phone",
            context: None,
            expect: Expect::NeedsSelection,
        },
        Case {
            slug: "timer-10",
            said: "start a ten minute timer",
            context: None,
            expect: Expect::Cmd(StartTimer { seconds: 600 }),
        },
        Case {
            slug: "pomodoro",
            said: "start a pomodoro",
            context: None,
            expect: Expect::Cmd(StartPomodoro),
        },
        Case {
            slug: "pause-timer",
            said: "pause the timer",
            context: None,
            expect: Expect::Cmd(TimerControl {
                action: TimerAction::Pause,
                target: TimerTarget::Timer,
            }),
        },
        Case {
            slug: "next-track",
            said: "next track",
            context: None,
            expect: Expect::Cmd(Media {
                command: MediaCommand::Next,
            }),
        },
        Case {
            slug: "whats-playing",
            said: "what's playing",
            context: None,
            expect: Expect::Cmd(Media {
                command: MediaCommand::WhatsPlaying,
            }),
        },
        Case {
            slug: "pause-music",
            said: "pause the music",
            context: None,
            expect: Expect::Cmd(Media {
                command: MediaCommand::Pause,
            }),
        },
        Case {
            slug: "open-calendar",
            said: "open the calendar",
            context: None,
            expect: Expect::Cmd(OpenTool {
                tool: ToolId::Calendar,
            }),
        },
        Case {
            slug: "close",
            said: "close",
            context: None,
            expect: Expect::Cmd(Collapse),
        },
    ]
}

/// (slug, spoken text, what clean transcription should make of the recognised words)
fn dictation() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "dict-fillers",
            "um remember to uh call the dentist comma tomorrow morning period",
        ),
        (
            "dict-stutter",
            "the the meeting is is moved to three thirty",
        ),
        ("dict-restart", "i want to i want to buy milk and eggs"),
        ("dict-question", "what time is the meeting"),
        ("dict-youknow", "it was you know really good"),
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

fn run_voice(dir: &Path, label: &str, voice: &str, now: &LocalNow) -> Option<(usize, usize)> {
    let prefix = format!("{label}-");
    let cs = cases(now);
    let mut items: Vec<(&str, &str)> = cs.iter().map(|c| (c.slug, c.said)).collect();
    items.extend(dictation());
    let cached = dir.join(format!("{prefix}{}.wav", items[0].0)).exists();
    if !cached && !synthesize(dir, voice, &prefix, &items) {
        eprintln!("[skip] could not synthesise with voice {voice:?}");
        return None;
    }

    println!(
        "\n=== {label}: voice={voice:?} (today is {} {}) ===",
        now.date.short_label(),
        now.date.iso()
    );
    let (mut ok, mut total) = (0, 0);
    for c in &cs {
        let wav = dir.join(format!("{prefix}{}.wav", c.slug));
        let opts = StartOptions {
            context: c.context.clone(),
            ..Default::default()
        };
        let r = transcribe_wav_with(&wav, Mode::Command, opts)
            .unwrap_or_else(|e| panic!("{} failed: {e}", c.slug));
        total += 1;
        let good = match &c.expect {
            Expect::Cmd(want) => r.command.as_ref() == Some(want),
            Expect::NeedsSelection => {
                r.command.is_none()
                    && r.feedback
                        .as_deref()
                        .is_some_and(|f| f.contains("Nothing is selected"))
            }
        };
        ok += usize::from(good);
        println!(
            "{} said={:<48} heard={:<52} -> {:?}{}",
            if good { "PASS" } else { "FAIL" },
            c.said,
            r.text,
            r.command,
            r.feedback
                .as_ref()
                .map(|f| format!("  feedback={f:?}"))
                .unwrap_or_default()
        );
    }

    println!(
        "--- clean transcription (before = what the recogniser wrote, after = Krovi's cleanup)"
    );
    for (slug, said) in dictation() {
        let wav = dir.join(format!("{prefix}{slug}.wav"));
        let raw = transcribe_wav_with(
            &wav,
            Mode::Note,
            StartOptions {
                clean_transcription: false,
                ..Default::default()
            },
        )
        .unwrap();
        let clean = transcribe_wav_with(&wav, Mode::Note, StartOptions::default()).unwrap();
        println!(
            "said : {said}\n  before: {:?}\n  after : {:?}",
            raw.text, clean.text
        );
    }
    println!("--- {label}: v2 intents {ok}/{total} correct");
    Some((ok, total))
}

#[test]
fn v2_intents_through_the_real_recognizer() {
    if !krovi_voice::available() {
        eprintln!("[skip] no on-device recognizer on this machine");
        return;
    }
    let keep = std::env::var_os("KROVI_TEST_WAV_DIR").map(PathBuf::from);
    let dir = keep.clone().unwrap_or_else(|| {
        std::env::temp_dir().join(format!("krovi-voice-v2-{}", std::process::id()))
    });
    std::fs::create_dir_all(&dir).unwrap();
    let now = LocalNow::system();

    let david = run_voice(&dir, "v2-david", "Microsoft David Desktop", &now);
    let zira = run_voice(&dir, "v2-zira", "Microsoft Zira Desktop", &now);
    if keep.is_none() {
        let _ = std::fs::remove_dir_all(&dir);
    }

    if let Some((ok, total)) = david {
        assert!(ok * 100 >= total * 80, "David v2 intents {ok}/{total}");
    }
    if let Some((ok, total)) = zira {
        assert!(ok * 100 >= total * 55, "Zira v2 intents {ok}/{total}");
    }
}
