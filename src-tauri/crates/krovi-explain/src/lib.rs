//! Krovi Explain (owner: explain agent) - docs/CONTRACTS.md section 10.
//!
//! Paste any text; get an on-device extractive summary (zero network) and have it read aloud
//! with a Windows voice (zero network) or, optionally, an ElevenLabs voice with the user's own
//! key (the only thing in this crate that can open a socket).
//!
//! There is deliberately no AI "explain" engine in this version: no Ollama, no cloud model.

pub mod eleven;
pub mod secrets;
pub mod settings;
pub mod speak;
pub mod summary;
pub mod text;
pub mod win;

pub use settings::{Settings, VoiceEngine};
pub use speak::{Engine, SpeakEvent, SpeakInfo, SpeakRequest, SpeakSink, Speaker};
pub use summary::{summarize, Summary, SummaryError, SummaryLength, MAX_CHARS};
