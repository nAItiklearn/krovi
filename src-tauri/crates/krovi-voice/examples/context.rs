//! Real-world check of context capture (no speech involved). Arrange a window first (select text
//! in Notepad, select files in Explorer, ...) then run, from a shell that does not take focus:
//!
//!   context capture [off|selection|fallback] [delay_ms]   -> JSON of what Krovi would capture
//!   context fallback [delay_ms]                           -> run ONLY the Ctrl+C clipboard fallback
//!
//! `delay_ms` waits first, so you can click into the target window after launching.

use krovi_voice::context::{capture, copy_via_clipboard, foreground};
use krovi_voice::ContextCapture;
use std::time::Duration;

fn main() {
    let mut args = std::env::args().skip(1);
    let cmd = args.next().unwrap_or_else(|| "capture".into());
    match cmd.as_str() {
        "capture" => {
            let level = match args.next().as_deref() {
                Some("off") => ContextCapture::Off,
                Some("fallback") => ContextCapture::SelectionClipboardFallback,
                _ => ContextCapture::Selection,
            };
            let delay: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(0);
            std::thread::sleep(Duration::from_millis(delay));
            let started = std::time::Instant::now();
            let captured = capture(level, |fg| match copy_via_clipboard(fg) {
                Ok(text) => {
                    eprintln!("[fallback] copied {:?}", text.as_ref().map(|t| t.len()));
                    text
                }
                Err(e) => {
                    eprintln!("[fallback] refused: {e}");
                    None
                }
            });
            println!(
                "{}",
                serde_json::json!({ "level": format!("{level:?}"), "context": captured.context, "hint": captured.hint, "ms": started.elapsed().as_millis() as u64 })
            );
        }
        "fallback" => {
            let delay: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(0);
            std::thread::sleep(Duration::from_millis(delay));
            match foreground() {
                Ok(fg) => {
                    println!("foreground: {fg:?}");
                    match copy_via_clipboard(&fg) {
                        Ok(text) => println!("{}", serde_json::json!({ "copied": text })),
                        Err(e) => println!("{}", serde_json::json!({ "refused": e })),
                    }
                }
                Err(e) => println!("no foreground app: {e:?}"),
            }
        }
        other => eprintln!("unknown command {other:?}"),
    }
}
