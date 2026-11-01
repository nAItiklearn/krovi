//! QA hook: run WAV files through the real recognizer + command parser and print the result.
//!
//!   cargo run -p krovi-voice --features native --example transcribe -- <note|command> file1.wav [file2.wav ...]
//!   cargo run -p krovi-voice --features native --example transcribe -- --dir D:\wavs   (mode inferred from the
//!                                                                    "cmd-"/"note-" file name prefix)
//!
//! Prints one JSON line per file: { "file", "mode", "text", "command" }.

use std::path::PathBuf;

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: transcribe <note|command> <files...>  |  transcribe --dir <dir>");
        std::process::exit(2);
    }

    let a = krovi_voice::availability();
    eprintln!(
        "engine: {} (dictation={}, commands={})",
        a.engine, a.dictation, a.commands
    );

    let mut jobs: Vec<(PathBuf, krovi_voice::Mode)> = Vec::new();
    if args[0] == "--dir" {
        let dir = PathBuf::from(args.get(1).expect("--dir needs a directory"));
        let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
            .expect("read dir")
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "wav"))
            .collect();
        files.sort();
        for f in files {
            let name = f.file_name().unwrap().to_string_lossy().to_string();
            let mode = if name.contains("note-") {
                krovi_voice::Mode::Note
            } else {
                krovi_voice::Mode::Command
            };
            jobs.push((f, mode));
        }
    } else {
        let mode = krovi_voice::Mode::parse(&args.remove(0)).expect("mode must be note or command");
        for f in args {
            jobs.push((PathBuf::from(f), mode));
        }
    }

    for (file, mode) in jobs {
        let started = std::time::Instant::now();
        match krovi_voice::transcribe_wav(&file, mode) {
            Ok(result) => {
                let line = serde_json::json!({
                    "file": file.file_name().unwrap().to_string_lossy(),
                    "mode": mode.as_str(),
                    "text": result.text,
                    "command": result.command,
                    "ms": started.elapsed().as_millis() as u64,
                });
                println!("{line}");
            }
            Err(e) => println!(
                "{}",
                serde_json::json!({ "file": file.file_name().unwrap().to_string_lossy(), "mode": mode.as_str(), "error": e.to_string() })
            ),
        }
    }
}
