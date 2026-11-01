//! End-to-end check of the real recognizer, with no microphone and no network: Windows' own TTS
//! (System.Speech, via PowerShell) synthesises WAVs, `transcribe_wav` pushes them through the SAME
//! SAPI recognizer / grammars / command parser the live-mic path uses, and the results are
//! scored against expectations. Run with `-- --nocapture` for the full accuracy table:
//!
//!   cargo test -p krovi-voice --features native --test wav_roundtrip -- --nocapture
//!
//! Skips (passes) when the recognizer, PowerShell or the TTS voices are missing, so CI on a
//! stripped-down image does not fail for lack of speech components.
#![cfg(all(windows, feature = "native"))]

use krovi_voice::{transcribe_wav, Mode, ToolId, VoiceCommand};
use std::path::{Path, PathBuf};
use std::process::Command;

use ToolId::*;
use VoiceCommand::*;

struct Case {
    slug: &'static str,
    said: &'static str,
    mode: Mode,
    /// `None` = must NOT be turned into any command (distractor).
    expect: Option<VoiceCommand>,
}

fn cmd(slug: &'static str, said: &'static str, expect: VoiceCommand) -> Case {
    Case {
        slug,
        said,
        mode: Mode::Command,
        expect: Some(expect),
    }
}

fn chatter(slug: &'static str, said: &'static str) -> Case {
    Case {
        slug,
        said,
        mode: Mode::Command,
        expect: None,
    }
}

fn note(slug: &'static str, said: &'static str) -> Case {
    Case {
        slug,
        said,
        mode: Mode::Note,
        expect: None,
    }
}

fn open(t: ToolId) -> VoiceCommand {
    OpenTool { tool: t }
}

fn cases() -> Vec<Case> {
    vec![
        cmd("cmd-open-timers", "open timers", open(Timers)),
        cmd("cmd-start-pomodoro", "start pomodoro", StartPomodoro),
        cmd(
            "cmd-timer-five",
            "start a timer for five minutes",
            StartTimer { seconds: 300 },
        ),
        cmd(
            "cmd-new-note",
            "new note buy milk",
            NewNote {
                body: "Buy milk".into(),
            },
        ),
        cmd("cmd-close", "close", Collapse),
        cmd("cmd-open-notes", "open notes", open(Notes)),
        cmd(
            "cmd-open-clipboard",
            "show me the clipboard",
            open(Clipboard),
        ),
        cmd("cmd-go-weather", "go to the weather", open(Weather)),
        cmd(
            "cmd-timer-25",
            "set a timer for twenty five minutes",
            StartTimer { seconds: 1500 },
        ),
        cmd("cmd-open-settings", "open settings", open(Settings)),
        cmd("cmd-open-calendar", "open the calendar", open(Calendar)),
        cmd("cmd-hide", "hide the panel", Collapse),
        cmd(
            "cmd-pomodoro-session",
            "start a pomodoro session",
            StartPomodoro,
        ),
        cmd(
            "cmd-timer-hour",
            "set a timer for one hour",
            StartTimer { seconds: 3600 },
        ),
        cmd("cmd-stopwatch", "open the stopwatch", open(Timers)),
        cmd(
            "cmd-timer-90s",
            "start a timer for ninety seconds",
            StartTimer { seconds: 90 },
        ),
        cmd("cmd-calculator", "launch the calculator", open(Calculator)),
        cmd("cmd-media", "open media", open(Media)),
        chatter("chat-fox", "the quick brown fox jumps over the lazy dog"),
        chatter("chat-time", "what time is it in tokyo right now"),
        chatter("chat-there", "I will be there in ten minutes see you soon"),
        chatter("chat-joke", "tell me a joke about computers"),
        chatter("chat-lunch", "let us have lunch at noon tomorrow"),
        chatter("chat-oven", "the timer on the oven is broken again"),
        chatter(
            "chat-lecture",
            "my notes from the lecture are in the notebook",
        ),
        note(
            "note-dentist",
            "remember to call the dentist tomorrow morning",
        ),
        note("note-groceries", "buy milk eggs and bread on the way home"),
        note(
            "note-meeting",
            "the meeting with the design team moved to the afternoon",
        ),
        note(
            "note-idea",
            "idea for the weekend project build a small weather widget",
        ),
    ]
}

fn words(s: &str) -> Vec<String> {
    s.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(String::from)
        .collect()
}

/// Word error rate: word-level Levenshtein distance / reference length.
fn wer(reference: &str, hypothesis: &str) -> f64 {
    let (r, h) = (words(reference), words(hypothesis));
    let mut prev: Vec<usize> = (0..=h.len()).collect();
    for i in 1..=r.len() {
        let mut cur = vec![i; h.len() + 1];
        for j in 1..=h.len() {
            let cost = usize::from(r[i - 1] != h[j - 1]);
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        prev = cur;
    }
    prev[h.len()] as f64 / r.len().max(1) as f64
}

fn same_command(want: &VoiceCommand, got: &Option<VoiceCommand>) -> bool {
    match (want, got) {
        (NewNote { body: a }, Some(NewNote { body: b })) => a.eq_ignore_ascii_case(b),
        (a, Some(b)) => a == b,
        _ => false,
    }
}

fn synthesize(dir: &Path, voice: &str, rate: u32, noise: f64, prefix: &str) -> bool {
    let manifest = dir.join(format!("{prefix}manifest.tsv"));
    let lines: Vec<String> = cases()
        .iter()
        .map(|c| format!("{}\t{}", c.slug, c.said))
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
        .args([
            "-Voice",
            voice,
            "-Rate",
            &rate.to_string(),
            "-Prefix",
            prefix,
            "-Noise",
            &noise.to_string(),
        ])
        .output();
    matches!(out, Ok(o) if o.status.success())
}

struct Score {
    cmd_ok: usize,
    cmd_total: usize,
    false_accepts: usize,
    chatter_total: usize,
    wer_sum: f64,
    wer_n: usize,
}

fn run_voice(dir: &Path, label: &str, voice: &str, rate: u32, noise: f64) -> Option<Score> {
    // KROVI_TEST_SCENARIOS=david,zira limits the run; KROVI_TEST_WAV_DIR reuses synthesised audio.
    if let Ok(only) = std::env::var("KROVI_TEST_SCENARIOS") {
        if !only.split(',').any(|s| s.trim() == label) {
            return None;
        }
    }
    let prefix = format!("{}-", label);
    let cached = dir
        .join(format!("{prefix}{}.wav", cases()[0].slug))
        .exists();
    if !cached && !synthesize(dir, voice, rate, noise, &prefix) {
        eprintln!("[skip] could not synthesise with voice {voice:?}");
        return None;
    }
    let mut s = Score {
        cmd_ok: 0,
        cmd_total: 0,
        false_accepts: 0,
        chatter_total: 0,
        wer_sum: 0.0,
        wer_n: 0,
    };
    println!("\n=== {label}: voice={voice:?} {rate} Hz noise={noise} ===");
    for c in cases() {
        let wav = dir.join(format!("{prefix}{}.wav", c.slug));
        let r = transcribe_wav(&wav, c.mode).unwrap_or_else(|e| panic!("{} failed: {e}", c.slug));
        match (&c.expect, c.mode) {
            (Some(want), _) => {
                s.cmd_total += 1;
                let ok = same_command(want, &r.command);
                s.cmd_ok += usize::from(ok);
                println!(
                    "{} cmd   said={:<42} heard={:<42} -> {:?}",
                    if ok { "PASS" } else { "FAIL" },
                    c.said,
                    r.text,
                    r.command
                );
            }
            (None, Mode::Command) => {
                s.chatter_total += 1;
                let bad = r.command.is_some();
                s.false_accepts += usize::from(bad);
                println!(
                    "{} chat  said={:<42} heard={:<42} -> {:?}",
                    if bad { "FAIL" } else { "PASS" },
                    c.said,
                    r.text,
                    r.command
                );
            }
            (None, Mode::Note) => {
                let e = wer(c.said, &r.text);
                s.wer_sum += e;
                s.wer_n += 1;
                println!(
                    "WER {:>4.0}%  said={:<52} heard={}",
                    e * 100.0,
                    c.said,
                    r.text
                );
            }
        }
    }
    println!(
        "--- {label}: commands {}/{} correct, false accepts on chatter {}/{}, dictation WER {:.0}%",
        s.cmd_ok,
        s.cmd_total,
        s.false_accepts,
        s.chatter_total,
        100.0 * s.wer_sum / s.wer_n.max(1) as f64
    );
    Some(s)
}

/// 16 kHz mono 16-bit PCM WAV bytes.
fn wav_bytes(samples: &[i16]) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut v = Vec::with_capacity(44 + samples.len() * 2);
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&(36 + data_len).to_le_bytes());
    v.extend_from_slice(b"WAVEfmt ");
    v.extend_from_slice(&16u32.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes()); // PCM
    v.extend_from_slice(&1u16.to_le_bytes()); // mono
    v.extend_from_slice(&16000u32.to_le_bytes());
    v.extend_from_slice(&32000u32.to_le_bytes());
    v.extend_from_slice(&2u16.to_le_bytes());
    v.extend_from_slice(&16u16.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        v.extend_from_slice(&s.to_le_bytes());
    }
    v
}

/// Silence and noise: what a live microphone hears when nobody is talking. Nothing here may become
/// a command, and note mode must not invent a note.
fn non_speech(dir: &Path) -> (usize, usize) {
    let mut seed = 0x1234_5678u32;
    let mut noise = |amp: f64, n: usize| -> Vec<i16> {
        (0..n)
            .map(|_| {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                (((seed >> 8) as f64 / 8_388_608.0 - 1.0) * 32767.0 * amp) as i16
            })
            .collect()
    };
    let mut burst = noise(0.0, 8000);
    burst.extend(noise(0.6, 4000)); // loud 0.25 s thump
    burst.extend(noise(0.0, 12000));
    let clips: Vec<(&str, Vec<i16>)> = vec![
        ("silence", vec![0; 32000]),
        ("room-noise", noise(0.01, 48000)),
        ("loud-noise", noise(0.2, 48000)),
        ("burst", burst),
    ];

    let (mut bad, mut total) = (0, 0);
    for (name, samples) in clips {
        let wav = dir.join(format!("nonspeech-{name}.wav"));
        std::fs::write(&wav, wav_bytes(&samples)).unwrap();
        for mode in [Mode::Command, Mode::Note] {
            total += 1;
            let r = transcribe_wav(&wav, mode).unwrap();
            let invented = r.command.is_some() || !r.text.trim().is_empty();
            bad += usize::from(invented);
            println!(
                "{} nonspeech {:<10} {:?} heard={:?} -> {:?}",
                if invented { "FAIL" } else { "PASS" },
                name,
                mode,
                r.text,
                r.command
            );
        }
    }
    println!("--- non-speech: invented text/commands {bad}/{total}");
    (bad, total)
}

#[test]
fn synthesized_speech_roundtrip() {
    if !krovi_voice::available() {
        eprintln!("[skip] no on-device recognizer on this machine");
        return;
    }
    let keep = std::env::var_os("KROVI_TEST_WAV_DIR").map(PathBuf::from);
    let dir = keep.clone().unwrap_or_else(|| {
        std::env::temp_dir().join(format!("krovi-voice-test-{}", std::process::id()))
    });
    std::fs::create_dir_all(&dir).unwrap();

    let david = run_voice(&dir, "david", "Microsoft David Desktop", 16000, 0.0);
    let zira = run_voice(&dir, "zira", "Microsoft Zira Desktop", 16000, 0.0);
    // Different sample rate: SAPI must convert; and a little noise.
    let david22 = run_voice(&dir, "david22k", "Microsoft David Desktop", 22050, 0.0);
    let david_noisy = run_voice(&dir, "david-noisy", "Microsoft David Desktop", 16000, 0.02);
    let _zira_noisy = run_voice(&dir, "zira-noisy", "Microsoft Zira Desktop", 16000, 0.02);
    let (invented, _) = non_speech(&dir);
    if keep.is_none() {
        let _ = std::fs::remove_dir_all(&dir);
    }

    // Hard floors, from what the real engine delivers on clean synthetic speech. These guard
    // against regressions (grammar breaks, events dropped) without pretending TTS is a microphone.
    assert_eq!(
        invented, 0,
        "silence/noise must never become text or a command"
    );
    // The chatter set is adversarial on purpose ("the timer on the oven is broken"), so a stray
    // false accept is tolerated; systematic ones are not.
    if let Some(d) = david {
        assert!(
            d.cmd_ok * 100 >= d.cmd_total * 90,
            "David commands {}/{}",
            d.cmd_ok,
            d.cmd_total
        );
        assert!(
            d.false_accepts <= 1,
            "chatter turned into commands: {}",
            d.false_accepts
        );
        assert!(
            d.wer_sum / (d.wer_n as f64) < 0.35,
            "David dictation WER too high"
        );
    }
    if let Some(z) = zira {
        assert!(
            z.cmd_ok * 100 >= z.cmd_total * 70,
            "Zira commands {}/{}",
            z.cmd_ok,
            z.cmd_total
        );
        assert!(
            z.false_accepts <= 2,
            "Zira chatter false accepts {}",
            z.false_accepts
        );
    }
    if let Some(d) = david22 {
        assert!(
            d.cmd_ok * 100 >= d.cmd_total * 85,
            "22.05 kHz commands {}/{}",
            d.cmd_ok,
            d.cmd_total
        );
    }
    if let Some(d) = david_noisy {
        assert!(
            d.cmd_ok * 100 >= d.cmd_total * 70,
            "noisy commands {}/{}",
            d.cmd_ok,
            d.cmd_total
        );
        assert!(
            d.false_accepts <= 2,
            "noisy chatter false accepts {}",
            d.false_accepts
        );
    }
}
