//! The local dictation pipeline: microphone audio → text typed into the active app.
//!
//! `recorded audio → trim silence (vad) → nothing said? stop → transcribe locally
//! (Transcriber, Whisper in the app) → clean up (dictation::process) → type it (typing)`.
//!
//! Every step is local. Dictated text is never parsed as a command: commands have their
//! own shortcut and path (Agent Mode), so a dictated private message can't trigger one.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use crate::dictation::{self, DictationOptions};
use crate::speech::{SttOptions, Transcriber};
use crate::typing::{type_text, TypeError};
use crate::vad;

/// Shortest voiced audio worth transcribing.
pub const MIN_SPEECH_MS: u64 = 250;
/// Above this no-speech probability Whisper's text is treated as a hallucination.
pub const NO_SPEECH_REJECT: f32 = 0.85;

#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// Text was typed into the focused app.
    Typed {
        text: String,
        chars: usize,
        language: Option<String>,
    },
    /// Nothing was said (or only noise / a hallucination); nothing typed.
    NothingHeard,
    /// The text was recognised but could not be typed; it is returned so the UI can
    /// offer to copy it (on the user's click) instead of losing it.
    NotTyped { text: String, reason: String },
}

/// What a finished recording becomes. `type_it = false` only transcribes (for tests and
/// for Krovi's own fields that receive the text directly).
pub fn finish(
    audio: &[f32],
    engine: &dyn Transcriber,
    stt: &SttOptions,
    opts: &DictationOptions,
    cancel: Arc<AtomicBool>,
    type_it: bool,
) -> Result<Outcome, String> {
    let speech = vad::trim(audio);
    if !vad::has_speech(speech, MIN_SPEECH_MS) {
        return Ok(Outcome::NothingHeard);
    }
    let t = engine.transcribe(speech, stt, cancel)?;
    let words = t.text.split_whitespace().count();
    if t.text.trim().is_empty() || (t.no_speech > NO_SPEECH_REJECT && words <= 3) {
        return Ok(Outcome::NothingHeard);
    }
    let text = dictation::process(&t.text, opts);
    if text.trim().is_empty() {
        return Ok(Outcome::NothingHeard);
    }
    if !type_it {
        return Ok(Outcome::Typed {
            chars: text.chars().count(),
            text,
            language: t.language,
        });
    }
    match type_text(&text) {
        Ok(_) => Ok(Outcome::Typed {
            chars: text.chars().count(),
            text,
            language: t.language,
        }),
        Err(e) => {
            let reason = match &e {
                TypeError::Blocked { typed } | TypeError::FocusChanged { typed } if *typed > 0 => {
                    format!("{e} ({typed} characters were typed before it stopped.)")
                }
                _ => e.to_string(),
            };
            Ok(Outcome::NotTyped { text, reason })
        }
    }
}

/// The Whisper biasing prompt for the user's vocabulary and replacements.
pub fn prompt_for(vocabulary: &[String], opts: &DictationOptions) -> String {
    dictation::initial_prompt(vocabulary, &opts.replacements)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::speech::Transcript;

    struct Fixed(&'static str, f32);
    impl Transcriber for Fixed {
        fn transcribe(
            &self,
            _audio: &[f32],
            _opts: &SttOptions,
            _cancel: Arc<AtomicBool>,
        ) -> Result<Transcript, String> {
            Ok(Transcript {
                text: self.0.into(),
                language: Some("en".into()),
                no_speech: self.1,
                segments: 1,
            })
        }
    }

    fn voiced() -> Vec<f32> {
        (0..16_000)
            .map(|i| 0.2 * (i as f32 * 0.0864).sin())
            .collect()
    }

    fn run(audio: &[f32], engine: &Fixed) -> Outcome {
        finish(
            audio,
            engine,
            &SttOptions::default(),
            &DictationOptions::default(),
            Arc::new(AtomicBool::new(false)),
            false,
        )
        .unwrap()
    }

    #[test]
    fn silence_is_never_transcribed() {
        assert_eq!(
            run(&vec![0.0; 32_000], &Fixed("Thank you.", 0.1)),
            Outcome::NothingHeard
        );
    }

    #[test]
    fn likely_hallucination_is_dropped() {
        assert_eq!(run(&voiced(), &Fixed("you", 0.95)), Outcome::NothingHeard);
    }

    #[test]
    fn speech_becomes_cleaned_text() {
        match run(&voiced(), &Fixed("um so the build is green", 0.05)) {
            Outcome::Typed { text, .. } => {
                assert!(!text.to_lowercase().starts_with("um"), "{text}");
                assert!(text.contains("build is green"), "{text}");
            }
            other => panic!("{other:?}"),
        }
    }
}
