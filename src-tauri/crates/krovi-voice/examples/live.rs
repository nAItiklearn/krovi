//! Live-microphone smoke test: starts a session, prints every state/partial/result event, and
//! stops it. Nobody needs to speak for this to be a useful check: with silence it must report
//! `listening`, then end by itself with an empty result and `idle` (never hang).
//!
//!   cargo run -p krovi-voice --features native --example live -- <note|command> [seconds-before-manual-stop]
//!
//! With a seconds argument the session is stopped manually after that long (tests `stop()`);
//! without it, the session ends by its own no-speech / silence rules.

use krovi_voice::{Mode, Sink, StateChange, Voice, VoiceResult};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

struct Print {
    t0: Instant,
    finished: Arc<AtomicBool>,
}

impl Print {
    fn stamp(&self) -> String {
        format!("[{:>5} ms]", self.t0.elapsed().as_millis())
    }
}

impl Sink for Print {
    fn state(&self, change: StateChange) {
        println!(
            "{} state   {}",
            self.stamp(),
            serde_json::to_string(&change).unwrap()
        );
        if matches!(
            change.state,
            krovi_voice::State::Idle | krovi_voice::State::Error
        ) {
            self.finished.store(true, Ordering::SeqCst);
        }
    }
    fn partial(&self, text: &str) {
        println!("{} partial {text:?}", self.stamp());
    }
    fn result(&self, result: VoiceResult) {
        println!(
            "{} result  {}",
            self.stamp(),
            serde_json::to_string(&result).unwrap()
        );
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mode = args
        .next()
        .and_then(|m| Mode::parse(&m))
        .unwrap_or(Mode::Note);
    let manual_stop: Option<u64> = args.next().and_then(|s| s.parse().ok());

    let a = krovi_voice::availability();
    println!(
        "engine: {} (dictation={}, commands={})",
        a.engine, a.dictation, a.commands
    );

    let finished = Arc::new(AtomicBool::new(false));
    let sink = Arc::new(Print {
        t0: Instant::now(),
        finished: finished.clone(),
    });
    let voice = Voice::new();

    match voice.start(mode, sink) {
        Ok(()) => println!("start() returned Ok"),
        Err(e) => {
            println!("start() returned Err: {e}");
            return;
        }
    }

    let started = Instant::now();
    let mut stopped = false;
    while !finished.load(Ordering::SeqCst) && started.elapsed() < Duration::from_secs(150) {
        if let Some(s) = manual_stop {
            if !stopped && started.elapsed() >= Duration::from_secs(s) {
                println!("-- calling stop() after {s}s");
                voice.stop();
                stopped = true;
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    // Give the session thread a moment to wind down, then confirm it is really gone.
    std::thread::sleep(Duration::from_millis(300));
    println!(
        "finished={} is_active={} after {:?}",
        finished.load(Ordering::SeqCst),
        voice.is_active(),
        started.elapsed()
    );
}
