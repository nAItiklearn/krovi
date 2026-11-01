//! Krovi local speech (owner: voice agent). See docs/CONTRACTS.md §3 and docs/DECISIONS.md.
//!
//! Everything runs on-device through Windows' own SAPI 5 recognizer; there is no network code in
//! this crate and no cloud speech API anywhere. Layout:
//! - [`parser`] / [`intent`]  pure text -> [`VoiceCommand`], including reminders, "this" actions,
//!   media and timer control (always built, fully unit-tested)
//! - [`when`] / [`civil`]  spoken dates and times against a given local "now"
//! - [`clean`]  rule-based transcript cleanup for dictated notes
//! - [`catalog`] / [`actions`] / [`listing`]  Voice v3: the closed set of folders and apps, the
//!   logic that opens / launches / searches / lists them behind a `Host` trait, and the folder
//!   summary (pure, unit-tested); [`os`] is the Windows implementation (Shell namespace,
//!   `ShellExecuteW`, the Windows Search index) - local COM calls only, no network
//! - `grammar`   the closed command grammar as data (cross-checked against the parser)
//! - `session`   endpointing / silence / stop rules as a pure state machine
//! - `result`    recognised phrases -> the `VoiceResult` the UI receives
//! - [`context`] point-and-ask: foreground app, selected text, selected Explorer items
//!   (Windows, `native` feature)
//! - `sapi`      the Windows speech engine; compiled only with the `native` feature on Windows
//!
//! Public surface: [`availability`], [`Voice`] (`start` / `stop` for the microphone) and
//! [`transcribe_wav`] (the same pipeline fed from a WAV file).

// grammar/session are only consumed by the native engine; keep them compiled (and tested) always.
#![cfg_attr(not(all(windows, feature = "native")), allow(dead_code))]

pub mod actions;
pub mod catalog;
pub mod civil;
pub mod clean;
pub mod intent;
pub mod listing;
pub mod parser;
pub mod types;
pub mod when;

mod grammar;
mod osintent;
mod result;
mod session;

#[cfg(all(windows, feature = "native"))]
pub mod context;
#[cfg(all(windows, feature = "native"))]
pub mod os;
#[cfg(all(windows, feature = "native"))]
mod sapi;

use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};
#[cfg(all(windows, feature = "native"))]
use std::time::Duration;

pub use catalog::{Catalog, Entry as CatalogEntry, Lookup};
pub use civil::{Date, LocalNow};
pub use clean::{clean_fragment, clean_phrases, clean_text};
pub use intent::{parse as parse_intent, ContextAction, Env, Parsed};
pub use parser::{parse_command, tidy_text};
pub use types::*;

/// What the on-device engine can do. Probed once (creating the recognizer takes a moment) and
/// cached; never opens the microphone.
pub fn availability() -> Availability {
    static CACHE: OnceLock<Availability> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            #[cfg(all(windows, feature = "native"))]
            {
                sapi::probe()
            }
            #[cfg(not(all(windows, feature = "native")))]
            {
                Availability::none("built without the `native` feature (Windows SAPI 5)")
            }
        })
        .clone()
}

/// Whether any on-device recognition is possible.
pub fn available() -> bool {
    let a = availability();
    a.dictation || a.commands
}

/// Recognise a WAV file with the same recognizer, grammars and command parser the microphone
/// path uses. This is the QA hook behind `voice_transcribe_file`. Blocks until the file is done.
pub fn transcribe_wav(path: impl AsRef<Path>, mode: Mode) -> Result<VoiceResult, VoiceError> {
    transcribe_wav_with(path, mode, StartOptions::default())
}

/// Like [`transcribe_wav`], with the same per-session inputs a live session takes: the captured
/// context ("this") and whether to clean the transcript.
pub fn transcribe_wav_with(
    path: impl AsRef<Path>,
    mode: Mode,
    opts: StartOptions,
) -> Result<VoiceResult, VoiceError> {
    #[cfg(all(windows, feature = "native"))]
    {
        sapi::transcribe_file(path.as_ref(), mode, opts)
    }
    #[cfg(not(all(windows, feature = "native")))]
    {
        let _ = (path, mode, opts);
        Err(VoiceError::Unavailable(
            "built without the `native` feature (Windows SAPI 5)".into(),
        ))
    }
}

/// Controls at most one live microphone session at a time.
#[derive(Default)]
pub struct Voice {
    #[cfg(all(windows, feature = "native"))]
    active: Mutex<Option<sapi::MicSession>>,
    #[cfg(not(all(windows, feature = "native")))]
    active: Mutex<Option<()>>,
}

impl Voice {
    pub fn new() -> Self {
        Self::default()
    }

    /// Start listening. Returns once the microphone is open and the recognizer is running, or
    /// with the reason it could not be (also reported through `sink` as an `error` state).
    /// Starting the mode that is already running is a no-op; a different mode replaces it.
    pub fn start(&self, mode: Mode, sink: Arc<dyn Sink>) -> Result<(), VoiceError> {
        self.start_with(mode, sink, StartOptions::default())
    }

    /// [`Voice::start`] with the per-session inputs: the context captured at the moment the user
    /// triggered voice, and whether to clean dictated text.
    pub fn start_with(
        &self,
        mode: Mode,
        sink: Arc<dyn Sink>,
        opts: StartOptions,
    ) -> Result<(), VoiceError> {
        #[cfg(all(windows, feature = "native"))]
        {
            let mut guard = self.active.lock().unwrap_or_else(|p| p.into_inner());
            if let Some(session) = guard.as_mut() {
                if !session.is_done() {
                    if session.mode == mode {
                        return Ok(());
                    }
                    session.request_stop();
                }
                if !session.wait_done(Duration::from_secs(4)) {
                    return Err(VoiceError::Engine(
                        "the previous voice session is still finishing".into(),
                    ));
                }
                *guard = None;
            }
            *guard = Some(sapi::start_mic(mode, sink, opts)?);
            Ok(())
        }
        #[cfg(not(all(windows, feature = "native")))]
        {
            let _ = (&self.active, opts);
            let e = VoiceError::Unavailable(
                "built without the `native` feature (Windows SAPI 5)".into(),
            );
            sink.state(StateChange::error(mode, e.to_string()));
            Err(e)
        }
    }

    /// Stop listening. The in-flight phrase (if any) is finalised and delivered, then the
    /// session goes idle. Safe to call when nothing is running.
    pub fn stop(&self) {
        #[cfg(all(windows, feature = "native"))]
        if let Some(session) = self
            .active
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
        {
            session.request_stop();
        }
    }

    /// True while a microphone session is running (including its short finishing phase).
    pub fn is_active(&self) -> bool {
        #[cfg(all(windows, feature = "native"))]
        {
            self.active
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .as_ref()
                .is_some_and(|s| !s.is_done())
        }
        #[cfg(not(all(windows, feature = "native")))]
        {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transcribe_missing_file_is_a_clear_error() {
        let err = transcribe_wav("Z:\\definitely\\not\\here.wav", Mode::Note).unwrap_err();
        assert!(
            matches!(
                err,
                VoiceError::InvalidInput(_) | VoiceError::Unavailable(_)
            ),
            "{err:?}"
        );
    }

    #[test]
    fn stop_without_a_session_is_harmless() {
        let v = Voice::new();
        v.stop();
        assert!(!v.is_active());
    }
}
