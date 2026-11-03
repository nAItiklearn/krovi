//! Engine-neutral speech-to-text types, so the dictation pipeline compiles and is tested
//! without whisper.cpp. `stt::Engine` (feature `whisper`) is the real implementation.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SttOptions {
    /// ISO 639-1 code ("en", "hi"), or `None` to auto-detect.
    pub language: Option<String>,
    /// Biasing prompt: the user's vocabulary (names, product terms).
    pub prompt: String,
    /// Decoder threads; `None` = all cores but one, capped at 8.
    pub threads: Option<i32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Transcript {
    pub text: String,
    /// Language the engine used (detected or forced).
    pub language: Option<String>,
    /// Highest no-speech probability over the segments (0..1).
    pub no_speech: f32,
    pub segments: usize,
}

/// Anything that turns 16 kHz mono f32 audio into text, locally.
pub trait Transcriber: Send + Sync {
    fn transcribe(
        &self,
        audio: &[f32],
        opts: &SttOptions,
        cancel: Arc<AtomicBool>,
    ) -> Result<Transcript, String>;
}
