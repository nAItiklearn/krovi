//! Report whether an ElevenLabs key is saved in Windows Credential Manager and, only if one is
//! there, check it against the real service. The key is never printed.
//!
//!   cargo run -p krovi-explain --example eleven_probe             # presence + (if saved) voice list
//!   cargo run -p krovi-explain --example eleven_probe -- --speak  # also speak the Test sentence
//!
//! With no key saved (the normal state) this makes NO network request at all.

use std::sync::mpsc;
use std::sync::Arc;
use std::time::Instant;

use krovi_explain::eleven;
use krovi_explain::secrets;
use krovi_explain::speak::{SpeakEvent, SpeakSink, Speaker};
use krovi_explain::win::Audio;

struct Log(Instant, mpsc::Sender<SpeakEvent>);
impl SpeakSink for Log {
    fn event(&self, ev: SpeakEvent) {
        println!("[{:>6} ms] {:?}", self.0.elapsed().as_millis(), ev);
        let _ = self.1.send(ev);
    }
}

fn main() {
    let speak = std::env::args().any(|a| a == "--speak");
    let key = match secrets::read(secrets::ELEVEN_TARGET) {
        Ok(Some(k)) => k,
        Ok(None) => {
            println!(
                "no ElevenLabs key is saved in Windows Credential Manager (target \"{}\").",
                secrets::ELEVEN_TARGET
            );
            println!("that is the default: Krovi has made no request to ElevenLabs.");
            return;
        }
        Err(e) => {
            eprintln!("Credential Manager error: {e}");
            std::process::exit(1);
        }
    };
    println!(
        "a key is saved (ends in ...{})",
        secrets::key_hint(&key).unwrap_or_else(|| "????".into())
    );

    let t = Instant::now();
    let voices = match eleven::list_voices(&key) {
        Ok(v) => v,
        Err(e) => {
            println!("voice list failed: {:?}: {}", e.kind, e.message);
            std::process::exit(2);
        }
    };
    println!(
        "voice list OK: {} voices in {} ms (first: {:?})",
        voices.len(),
        t.elapsed().as_millis(),
        voices.first().map(|v| &v.name)
    );
    if !speak {
        return;
    }
    let Some(v) = voices.first() else {
        println!("no voices to test with");
        return;
    };
    let t = Instant::now();
    match eleven::synthesize(
        &key,
        &v.id,
        eleven::DEFAULT_MODEL,
        eleven::TEST_SENTENCE,
        1.0,
    ) {
        Ok(bytes) => {
            println!("synthesised {} bytes of MP3 in {} ms with voice {:?} (memory only, nothing written to disk)", bytes.len(), t.elapsed().as_millis(), v.name);
            let (tx, rx) = mpsc::channel();
            let speaker = Speaker::new();
            speaker
                .play_clip(
                    Audio {
                        bytes,
                        mime: "audio/mpeg",
                    },
                    v.name.clone(),
                    Arc::new(Log(Instant::now(), tx)),
                )
                .expect("play");
            while let Ok(ev) = rx.recv() {
                if matches!(
                    ev,
                    SpeakEvent::Done { .. } | SpeakEvent::Failed { .. } | SpeakEvent::Stopped
                ) {
                    break;
                }
            }
        }
        Err(e) => println!("speech failed: {:?}: {}", e.kind, e.message),
    }
}
