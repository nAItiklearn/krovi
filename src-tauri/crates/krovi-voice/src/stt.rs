//! Local speech-to-text with whisper.cpp (via `whisper-rs`). Behind the `whisper` feature.
//!
//! * **Offline and on-device.** The model file is a local ggml file the user chose to
//!   download (see `krovi-net::whisper_models`); audio never leaves the process.
//! * **Lazy.** Nothing is loaded at startup. [`Engine::load`] reads the model only when the
//!   user first dictates; the caller drops the engine after an idle period to give the
//!   memory back.
//! * **Cancellable.** [`Engine::transcribe`] checks `cancel` between decoder steps.
//! * Multilingual models auto-detect the language unless one is set.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

pub use crate::speech::{SttOptions, Transcriber, Transcript};

pub struct Engine {
    ctx: WhisperContext,
}

fn default_threads() -> i32 {
    let n = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    n.saturating_sub(1).clamp(1, 8) as i32
}

impl Engine {
    /// Load a ggml model from disk (tens to hundreds of MB; takes a moment).
    pub fn load(model: &Path) -> Result<Self, String> {
        if !model.is_file() {
            return Err("The speech model is not installed yet.".into());
        }
        let ctx = WhisperContext::new_with_params(model, WhisperContextParameters::default())
            .map_err(|e| format!("Could not load the speech model: {e}"))?;
        Ok(Self { ctx })
    }
}

impl Transcriber for Engine {
    /// Transcribe 16 kHz mono f32 audio.
    fn transcribe(
        &self,
        audio: &[f32],
        opts: &SttOptions,
        cancel: Arc<AtomicBool>,
    ) -> Result<Transcript, String> {
        if audio.is_empty() {
            return Ok(Transcript {
                text: String::new(),
                language: opts.language.clone(),
                no_speech: 1.0,
                segments: 0,
            });
        }
        let mut state = self
            .ctx
            .create_state()
            .map_err(|e| format!("Speech engine error: {e}"))?;
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_n_threads(opts.threads.unwrap_or_else(default_threads));
        params.set_translate(false);
        params.set_no_context(true);
        params.set_suppress_blank(true);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_special(false);
        params.set_print_timestamps(false);
        params.set_no_speech_thold(0.6);
        match opts.language.as_deref() {
            Some(lang) if !lang.is_empty() => params.set_language(Some(lang)),
            _ => params.set_language(Some("auto")),
        }
        let prompt = opts.prompt.replace('\0', " ");
        if !prompt.trim().is_empty() {
            params.set_initial_prompt(&prompt);
        }
        let flag = cancel.clone();
        params.set_abort_callback_safe(move || flag.load(Ordering::Relaxed));

        state.full(params, audio).map_err(|e| {
            if cancel.load(Ordering::Relaxed) {
                "Cancelled".to_string()
            } else {
                format!("Transcription failed: {e}")
            }
        })?;
        if cancel.load(Ordering::Relaxed) {
            return Err("Cancelled".into());
        }

        let mut text = String::new();
        let mut no_speech: f32 = 0.0;
        let mut segments = 0usize;
        for seg in state.as_iter() {
            segments += 1;
            no_speech = no_speech.max(seg.no_speech_probability());
            if let Ok(t) = seg.to_str_lossy() {
                text.push_str(&t);
            }
        }
        let language = whisper_rs::get_lang_str(state.full_lang_id_from_state()).map(String::from);
        Ok(Transcript {
            text: text.trim().to_string(),
            language,
            no_speech,
            segments,
        })
    }
}
