//! Voice v3 on the real machine, without the GUI: enumerate the folder / app catalog, search the
//! Windows Search index, list a folder, and run spoken commands (typed text or WAV files through
//! the real SAPI recognizer) all the way to the OS action.
//!
//!   cargo run -p krovi-voice --features native --example osactions -- catalog
//!   cargo run ... --example osactions -- search invoice
//!   cargo run ... --example osactions -- list downloads
//!   cargo run ... --example osactions -- say "open downloads" [--exec]
//!   cargo run ... --example osactions -- wav some.wav [--exec]
//!   cargo run ... --example osactions -- flow a.wav b.wav c.wav     (always executes)
//!
//! It prints one JSON object per result (what `voice-result` would carry) and makes no network
//! call of any kind.
#![cfg(all(windows, feature = "native"))]

use std::sync::Arc;
use std::time::Instant;

use krovi_voice::actions::{apply, Host};
use krovi_voice::os::{scan_catalog, WinHost};
use krovi_voice::{
    parse_intent, transcribe_wav_with, Catalog, Env, Mode, Parsed, StartOptions, VoiceResult,
};

fn show(label: &str, mut result: VoiceResult, exec: bool, host: &dyn Host) {
    if exec {
        apply(&mut result, host);
    }
    let mut shown = result.clone();
    shown.results.truncate(8);
    println!(
        "[{label}] {}",
        serde_json::to_string(&shown).unwrap_or_default()
    );
    if result.results.len() > 8 {
        println!("[{label}] ... and {} more rows", result.results.len() - 8);
    }
}

fn catalog_timed() -> Arc<Catalog> {
    let t = Instant::now();
    let c = scan_catalog();
    eprintln!(
        "catalog: {} folders, {} apps in {} ms",
        c.folders.len(),
        c.apps.len(),
        t.elapsed().as_millis()
    );
    Arc::new(c)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let exec = args.iter().any(|a| a == "--exec");
    let rest: Vec<&str> = args
        .iter()
        .map(String::as_str)
        .filter(|a| *a != "--exec")
        .collect();
    let host = WinHost;
    match rest.as_slice() {
        ["catalog"] => {
            let c = catalog_timed();
            println!("FOLDERS ({}):", c.folders.len());
            for f in &c.folders {
                println!("  {:<28} {}", f.name, f.path);
            }
            println!("APPS ({}):", c.apps.len());
            for a in &c.apps {
                println!("  {:<40} {}", a.name, a.path);
            }
            println!(
                "GRAMMAR PHRASES: {} folder forms, {} app forms",
                c.folder_phrases().len(),
                c.app_phrases().len()
            );
        }
        ["search", term @ ..] => {
            let t = Instant::now();
            let r = host.search_files(&term.join(" "), 20);
            eprintln!("search took {} ms", t.elapsed().as_millis());
            match r {
                Ok(rows) => {
                    println!("{} rows", rows.len());
                    for row in rows {
                        println!(
                            "  {:?} {} | {} | {}",
                            row.kind,
                            row.name,
                            row.path,
                            row.detail.unwrap_or_default()
                        );
                    }
                }
                Err(e) => println!("search error: {e}"),
            }
        }
        ["list", name @ ..] => {
            let c = catalog_timed();
            let words: Vec<&str> = name.to_vec();
            let text = format!("what's in {}", words.join(" "));
            let env = Env::now(None).with_catalog(Some(c));
            match parse_intent(&text, &env) {
                Parsed::Command(cmd) => {
                    let mut r = VoiceResult::new(Mode::Command, text, Some(cmd));
                    apply(&mut r, &host);
                    show("list", r, false, &host);
                }
                other => println!("not understood: {other:?}"),
            }
        }
        ["say", text] => {
            let c = catalog_timed();
            let env = Env::now(None).with_catalog(Some(c));
            let mut r = VoiceResult::new(Mode::Command, *text, None);
            match parse_intent(text, &env) {
                Parsed::Command(cmd) => r.command = Some(cmd),
                Parsed::Unresolved { feedback, results } => {
                    r.feedback = Some(feedback);
                    r.results = results;
                }
                other => println!("(parsed as {other:?})"),
            }
            show("say", r, exec, &host);
        }
        ["wav", path] => {
            let c = catalog_timed();
            run_wav(path, c, exec, &host);
        }
        ["flow", paths @ ..] => {
            let c = catalog_timed();
            for p in paths {
                run_wav(p, c.clone(), true, &host);
            }
        }
        // like `flow` but never executes (recognition and parsing only)
        ["batch", paths @ ..] => {
            let c = catalog_timed();
            for p in paths {
                run_wav(p, c.clone(), false, &host);
            }
        }
        _ => eprintln!(
            "usage: osactions catalog | search <term> | list <folder> | say \"<text>\" [--exec] | wav <file> [--exec] | flow <files...> (executes) | batch <files...> (does not execute)"
        ),
    }
}

fn run_wav(path: &str, catalog: Arc<Catalog>, exec: bool, host: &dyn Host) {
    let opts = StartOptions {
        catalog: Some(catalog),
        ..Default::default()
    };
    let t = Instant::now();
    match transcribe_wav_with(path, Mode::Command, opts) {
        Ok(r) => {
            eprintln!("recognised in {} ms", t.elapsed().as_millis());
            show(path, r, exec, host);
        }
        Err(e) => println!("[{path}] error: {e}"),
    }
}
