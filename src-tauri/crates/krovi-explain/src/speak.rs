//! Read-aloud orchestration.
//!
//! The text is split into sentences (the same splitter the summariser uses), each sentence is
//! turned into audio and played, and the UI is told which character range is being spoken so it
//! can highlight it. Audio for the next piece is prepared while the current one plays.
//!
//! Engines
//! * Windows (default): on-device `SpeechSynthesizer` voices, one sentence at a time. No network.
//! * ElevenLabs (opt-in, the user's own key): a few sentences per request. If a request fails
//!   (out of credits, bad key, offline, ...) the rest is read with the Windows voice and the UI
//!   gets a one-line notice, so reading never just stops.
//!
//! Audio only ever exists in memory; nothing is written to disk.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde::Serialize;

use crate::eleven::{self, ErrorKind, VoiceError};
use crate::text;
use crate::win;

/// One piece the voice will speak, with where it sits in the text the user sees.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sentence {
    /// UTF-16 offsets into the text (what the web UI slices strings by).
    pub start: usize,
    pub end: usize,
    /// What the voice is given (markers, markdown and URLs removed).
    pub spoken: String,
}

/// Longest single piece handed to a voice; longer sentences are split at spaces.
const MAX_PIECE_CHARS: usize = 1_200;
/// ElevenLabs requests carry about this many characters (several sentences read as one
/// phrase sound better than sentence-by-sentence).
const ELEVEN_CHUNK_CHARS: usize = 700;

pub fn plan_sentences(input: &str) -> Vec<Sentence> {
    let mut out = Vec::new();
    for u in text::split_units(input) {
        let spoken = text::speakable(&u.text);
        if !spoken.chars().any(|c| c.is_alphanumeric()) {
            continue;
        }
        let (start, end) = (
            text::utf16_offset(input, u.start),
            text::utf16_offset(input, u.end),
        );
        for piece in split_long(&spoken, MAX_PIECE_CHARS) {
            out.push(Sentence {
                start,
                end,
                spoken: piece,
            });
        }
    }
    out
}

fn split_long(s: &str, max: usize) -> Vec<String> {
    if s.chars().count() <= max {
        return vec![s.to_string()];
    }
    let mut out = Vec::new();
    let mut cur = String::new();
    for word in s.split(' ') {
        if !cur.is_empty() && cur.chars().count() + 1 + word.chars().count() > max {
            out.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(word);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// End (exclusive) of the ElevenLabs request that starts at sentence `from`.
pub fn chunk_end(sents: &[Sentence], from: usize, max_chars: usize) -> usize {
    let mut total = 0usize;
    let mut j = from;
    while j < sents.len() {
        let len = sents[j].spoken.chars().count() + 1;
        if j > from && total + len > max_chars {
            break;
        }
        total += len;
        j += 1;
    }
    j.max(from + 1).min(sents.len())
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Estimate {
    pub sentences: usize,
    /// Characters that would be sent to ElevenLabs.
    pub chars: usize,
}

pub fn estimate(input: &str) -> Estimate {
    let s = plan_sentences(input);
    Estimate {
        sentences: s.len(),
        chars: s
            .iter()
            .map(|x| x.spoken.chars().count() + 1)
            .sum::<usize>()
            .saturating_sub(1),
    }
}

// ---------------------------------------------------------------------------------------
// Requests, events, the speaker
// ---------------------------------------------------------------------------------------

pub enum Engine {
    Windows {
        voice_id: Option<String>,
    },
    Eleven {
        key: String,
        voice_id: String,
        model: String,
    },
}

pub struct SpeakRequest {
    pub text: String,
    pub engine: Engine,
    pub rate: f32,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SpeakInfo {
    /// "windows" or "elevenlabs"
    pub engine: &'static str,
    pub sentences: usize,
    pub chars: usize,
    /// ElevenLabs credits this will use at most (None for the Windows voice).
    pub credits: Option<u64>,
}

/// What the UI is told while reading. `Speaking` carries the range to highlight.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(
    tag = "state",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum SpeakEvent {
    Speaking {
        index: usize,
        count: usize,
        start: usize,
        end: usize,
        engine: String,
        voice: String,
    },
    Notice {
        message: String,
    },
    Done {
        ms: u64,
    },
    Failed {
        message: String,
    },
    Stopped,
}

pub trait SpeakSink: Send + Sync + 'static {
    fn event(&self, ev: SpeakEvent);
}

struct Session {
    cancel: AtomicBool,
}

type Current = Arc<Mutex<Option<Arc<Session>>>>;

#[derive(Default)]
pub struct Speaker {
    current: Current,
}

/// A session may report only while it is still the current one. When a new read replaces it,
/// the old one goes silent, so its late "stopped" cannot flip the UI to idle under the new audio.
struct SessionSink {
    inner: Arc<dyn SpeakSink>,
    current: Current,
    session: Arc<Session>,
}

impl SessionSink {
    fn emit(&self, ev: SpeakEvent) {
        let live = self
            .current
            .lock()
            .map(|g| g.as_ref().is_some_and(|c| Arc::ptr_eq(c, &self.session)))
            .unwrap_or(false);
        if live {
            self.inner.event(ev);
        }
    }
}

fn fallback_notice(e: &VoiceError) -> String {
    let why = match e.kind {
        ErrorKind::OutOfCredits => "ElevenLabs is out of credits",
        ErrorKind::InvalidKey | ErrorKind::MissingPermission => "ElevenLabs didn't accept your key",
        ErrorKind::Offline => "ElevenLabs couldn't be reached",
        ErrorKind::RateLimited => "ElevenLabs is limiting requests",
        ErrorKind::VoiceNotFound => "That ElevenLabs voice isn't available",
        _ => "ElevenLabs didn't respond properly",
    };
    format!("{why}, so the rest is read with your Windows voice.")
}

enum Msg {
    Segment {
        from: usize,
        to: usize,
        audio: win::Audio,
        engine: &'static str,
        voice: String,
    },
    Notice(String),
    Failed(String),
}

impl Speaker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Start reading `req.text`. Returns at once; progress arrives through `sink`. Any reading
    /// already in progress is stopped first.
    pub fn speak(&self, req: SpeakRequest, sink: Arc<dyn SpeakSink>) -> Result<SpeakInfo, String> {
        let sents = plan_sentences(&req.text);
        if sents.is_empty() {
            return Err("There is nothing to read aloud.".into());
        }
        let chars: usize = sents
            .iter()
            .map(|s| s.spoken.chars().count() + 1)
            .sum::<usize>()
            .saturating_sub(1);
        let info = match &req.engine {
            Engine::Windows { .. } => SpeakInfo {
                engine: "windows",
                sentences: sents.len(),
                chars,
                credits: None,
            },
            Engine::Eleven { model, .. } => SpeakInfo {
                engine: "elevenlabs",
                sentences: sents.len(),
                chars,
                credits: Some(eleven::credits(chars, model)),
            },
        };
        let session = Arc::new(Session {
            cancel: AtomicBool::new(false),
        });
        {
            let mut cur = self
                .current
                .lock()
                .map_err(|_| "internal lock error".to_string())?;
            if let Some(old) = cur.replace(session.clone()) {
                old.cancel.store(true, Ordering::SeqCst);
            }
        }
        let sink = SessionSink {
            inner: sink,
            current: self.current.clone(),
            session: session.clone(),
        };
        std::thread::Builder::new()
            .name("krovi-explain-speak".into())
            .spawn(move || run(req, sents, session, sink))
            .map_err(|e| e.to_string())?;
        Ok(info)
    }

    /// Play already-prepared audio (used by "Test voice"). Same stop behaviour as `speak`.
    pub fn play_clip(
        &self,
        audio: win::Audio,
        label: String,
        sink: Arc<dyn SpeakSink>,
    ) -> Result<(), String> {
        let session = Arc::new(Session {
            cancel: AtomicBool::new(false),
        });
        {
            let mut cur = self
                .current
                .lock()
                .map_err(|_| "internal lock error".to_string())?;
            if let Some(old) = cur.replace(session.clone()) {
                old.cancel.store(true, Ordering::SeqCst);
            }
        }
        let sink = SessionSink {
            inner: sink,
            current: self.current.clone(),
            session: session.clone(),
        };
        std::thread::Builder::new()
            .name("krovi-explain-clip".into())
            .spawn(move || {
                win::init_mta();
                let t = Instant::now();
                sink.emit(SpeakEvent::Speaking {
                    index: 0,
                    count: 1,
                    start: 0,
                    end: 0,
                    engine: "elevenlabs".into(),
                    voice: label,
                });
                match win::play(&audio, &session.cancel) {
                    Ok(()) if session.cancel.load(Ordering::SeqCst) => {
                        sink.emit(SpeakEvent::Stopped)
                    }
                    Ok(()) => sink.emit(SpeakEvent::Done {
                        ms: t.elapsed().as_millis() as u64,
                    }),
                    Err(m) => sink.emit(SpeakEvent::Failed { message: m }),
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Stop reading now. Safe to call when nothing is playing.
    pub fn stop(&self) {
        if let Ok(cur) = self.current.lock() {
            if let Some(s) = cur.as_ref() {
                s.cancel.store(true, Ordering::SeqCst);
            }
        }
    }
}

fn run(req: SpeakRequest, sents: Vec<Sentence>, session: Arc<Session>, sink: SessionSink) {
    win::init_mta();
    let started = Instant::now();
    let n = sents.len();
    let sents = Arc::new(sents);
    let (tx, rx) = mpsc::sync_channel::<Msg>(1);

    // Producer: turns sentences into audio one step ahead of the player.
    let producer = {
        let sents = sents.clone();
        let session = session.clone();
        let rate = req.rate;
        let windows_voice = req.engine_voice();
        let mut eleven_cfg = match req.engine {
            Engine::Eleven {
                key,
                voice_id,
                model,
            } => Some((key, voice_id, model)),
            Engine::Windows { .. } => None,
        };
        std::thread::Builder::new()
            .name("krovi-explain-synth".into())
            .spawn(move || {
                win::init_mta();
                let mut synth: Option<win::Synth> = None;
                let mut i = 0usize;
                while i < n && !session.cancel.load(Ordering::SeqCst) {
                    if let Some((key, voice_id, model)) = eleven_cfg.as_ref() {
                        let j = chunk_end(&sents, i, ELEVEN_CHUNK_CHARS);
                        let chunk = sents[i..j]
                            .iter()
                            .map(|s| s.spoken.as_str())
                            .collect::<Vec<_>>()
                            .join(" ");
                        match eleven::synthesize(key, voice_id, model, &chunk, rate) {
                            Ok(bytes) => {
                                let seg = Msg::Segment {
                                    from: i,
                                    to: j,
                                    audio: win::Audio {
                                        bytes,
                                        mime: "audio/mpeg",
                                    },
                                    engine: "elevenlabs",
                                    voice: "ElevenLabs".into(),
                                };
                                if tx.send(seg).is_err() {
                                    return;
                                }
                                i = j;
                            }
                            Err(e) => {
                                if tx.send(Msg::Notice(fallback_notice(&e))).is_err() {
                                    return;
                                }
                                eleven_cfg = None; // fall through to the Windows voice for the rest
                            }
                        }
                    } else {
                        if synth.is_none() {
                            match win::Synth::new(windows_voice.as_deref(), rate) {
                                Ok(s) => synth = Some(s),
                                Err(m) => {
                                    let _ = tx.send(Msg::Failed(m));
                                    return;
                                }
                            }
                        }
                        let s = synth.as_ref().expect("synth was just created");
                        match s.synth(&sents[i].spoken) {
                            Ok(audio) => {
                                if tx
                                    .send(Msg::Segment {
                                        from: i,
                                        to: i + 1,
                                        audio,
                                        engine: "windows",
                                        voice: s.voice.clone(),
                                    })
                                    .is_err()
                                {
                                    return;
                                }
                                i += 1;
                            }
                            Err(m) => {
                                let _ = tx.send(Msg::Failed(m));
                                return;
                            }
                        }
                    }
                }
            })
    };
    if let Err(e) = producer {
        sink.emit(SpeakEvent::Failed {
            message: format!("Could not start reading aloud ({e})."),
        });
        return;
    }

    let mut failed = false;
    for msg in rx.iter() {
        if session.cancel.load(Ordering::SeqCst) {
            break;
        }
        match msg {
            Msg::Notice(m) => sink.emit(SpeakEvent::Notice { message: m }),
            Msg::Failed(m) => {
                sink.emit(SpeakEvent::Failed { message: m });
                failed = true;
                break;
            }
            Msg::Segment {
                from,
                to,
                audio,
                engine,
                voice,
            } => {
                sink.emit(SpeakEvent::Speaking {
                    index: from,
                    count: n,
                    start: sents[from].start,
                    end: sents[to - 1].end,
                    engine: engine.into(),
                    voice,
                });
                if let Err(m) = win::play(&audio, &session.cancel) {
                    sink.emit(SpeakEvent::Failed { message: m });
                    failed = true;
                    break;
                }
            }
        }
    }
    drop(rx);
    if failed {
        return;
    }
    if session.cancel.load(Ordering::SeqCst) {
        sink.emit(SpeakEvent::Stopped);
    } else {
        sink.emit(SpeakEvent::Done {
            ms: started.elapsed().as_millis() as u64,
        });
    }
}

impl SpeakRequest {
    fn engine_voice(&self) -> Option<String> {
        match &self.engine {
            Engine::Windows { voice_id } => voice_id.clone(),
            // when ElevenLabs falls back, the user's chosen Windows voice is not known here:
            // the system default voice is used
            Engine::Eleven { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plans_sentences_with_utf16_ranges_into_the_original_text() {
        let text = "\u{2022} First point here.\n\u{2022} Second point, with a link https://example.com/x.\nA closing line.";
        let s = plan_sentences(text);
        assert_eq!(s.len(), 3);
        assert_eq!(s[0].spoken, "First point here.");
        assert_eq!(s[1].spoken, "Second point, with a link link.");
        let units: Vec<u16> = text.encode_utf16().collect();
        let first = String::from_utf16(&units[s[0].start..s[0].end]).unwrap();
        assert_eq!(first, "First point here.");
        let last = String::from_utf16(&units[s[2].start..s[2].end]).unwrap();
        assert_eq!(last, "A closing line.");
    }

    #[test]
    fn symbols_only_lines_are_skipped() {
        assert!(plan_sentences("-----\n*** \n\u{2022}").is_empty());
        assert!(plan_sentences("   ").is_empty());
    }

    #[test]
    fn very_long_sentences_are_split_at_spaces() {
        let long = format!("{}.", "word ".repeat(600).trim());
        let s = plan_sentences(&long);
        assert!(s.len() >= 3, "{}", s.len());
        assert!(s
            .iter()
            .all(|p| p.spoken.chars().count() <= MAX_PIECE_CHARS));
        assert_eq!(
            s.iter().map(|p| p.spoken.split(' ').count()).sum::<usize>(),
            600
        );
    }

    #[test]
    fn elevenlabs_chunks_group_whole_sentences() {
        let text = "A short one. Another short one. ".repeat(60);
        let s = plan_sentences(&text);
        let mut i = 0;
        let mut chunks = 0;
        while i < s.len() {
            let j = chunk_end(&s, i, ELEVEN_CHUNK_CHARS);
            assert!(j > i);
            let len: usize = s[i..j].iter().map(|x| x.spoken.len() + 1).sum();
            assert!(len <= ELEVEN_CHUNK_CHARS + 40, "{len}");
            i = j;
            chunks += 1;
        }
        assert!(chunks >= 2 && chunks < s.len());
        // a single sentence bigger than the chunk size still goes alone
        let big = plan_sentences(&format!("{}.", "x".repeat(900)));
        assert_eq!(chunk_end(&big, 0, 700), 1);
    }

    #[test]
    fn estimate_counts_what_would_be_sent() {
        let e = estimate("Hello world. Second sentence here.");
        assert_eq!(e.sentences, 2);
        assert_eq!(e.chars, "Hello world. Second sentence here.".len());
        assert_eq!(estimate("").chars, 0);
    }

    #[test]
    fn fallback_notices_are_one_line_and_say_what_happens() {
        for k in [
            ErrorKind::OutOfCredits,
            ErrorKind::InvalidKey,
            ErrorKind::Offline,
            ErrorKind::RateLimited,
            ErrorKind::VoiceNotFound,
            ErrorKind::Other,
        ] {
            let m = fallback_notice(&VoiceError::new(k));
            assert!(m.contains("Windows voice") && !m.contains('\n'), "{m}");
        }
    }

    #[test]
    fn events_serialise_the_way_the_ui_reads_them() {
        let v = serde_json::to_value(SpeakEvent::Speaking {
            index: 1,
            count: 4,
            start: 10,
            end: 30,
            engine: "windows".into(),
            voice: "Microsoft David".into(),
        })
        .unwrap();
        assert_eq!(v["state"], "speaking");
        assert_eq!(v["start"], 10);
        assert_eq!(v["voice"], "Microsoft David");
        assert_eq!(
            serde_json::to_value(SpeakEvent::Stopped).unwrap()["state"],
            "stopped"
        );
        assert_eq!(
            serde_json::to_value(SpeakEvent::Done { ms: 5 }).unwrap()["ms"],
            5
        );
    }

    #[test]
    fn a_replaced_session_goes_silent_so_its_late_stopped_cannot_end_the_new_one() {
        struct Rec(Mutex<Vec<SpeakEvent>>);
        impl SpeakSink for Rec {
            fn event(&self, ev: SpeakEvent) {
                self.0.lock().unwrap().push(ev);
            }
        }
        let rec = Arc::new(Rec(Mutex::new(Vec::new())));
        let current: Current = Arc::new(Mutex::new(None));
        let old = Arc::new(Session {
            cancel: AtomicBool::new(false),
        });
        *current.lock().unwrap() = Some(old.clone());
        let old_sink = SessionSink {
            inner: rec.clone(),
            current: current.clone(),
            session: old,
        };
        old_sink.emit(SpeakEvent::Notice {
            message: "still current".into(),
        });

        let new = Arc::new(Session {
            cancel: AtomicBool::new(false),
        });
        *current.lock().unwrap() = Some(new.clone());
        old_sink.emit(SpeakEvent::Stopped); // the old thread winding down: must be dropped
        SessionSink {
            inner: rec.clone(),
            current,
            session: new,
        }
        .emit(SpeakEvent::Done { ms: 1 });

        let got = rec.0.lock().unwrap().clone();
        assert_eq!(
            got,
            vec![
                SpeakEvent::Notice {
                    message: "still current".into()
                },
                SpeakEvent::Done { ms: 1 }
            ]
        );
    }

    #[test]
    fn nothing_to_read_is_an_error_not_a_silent_success() {
        struct Null;
        impl SpeakSink for Null {
            fn event(&self, _: SpeakEvent) {}
        }
        let sp = Speaker::new();
        let r = sp.speak(
            SpeakRequest {
                text: "  ".into(),
                engine: Engine::Windows { voice_id: None },
                rate: 1.0,
            },
            Arc::new(Null),
        );
        assert!(r.is_err());
    }
}
