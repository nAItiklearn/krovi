//! Summarise a text file on the command line and time it (no network, no OS calls).
//!
//!   cargo run -p krovi-explain --example summarize -- <file> [short|medium|points]

use std::time::Instant;

use krovi_explain::{summarize, SummaryLength};

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: summarize <file> [short|medium|points]");
        std::process::exit(2);
    };
    let length = match args.next().as_deref() {
        Some("short") => SummaryLength::Short,
        Some("points") => SummaryLength::Points,
        _ => SummaryLength::Medium,
    };
    let input = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("cannot read {path}: {e}");
            std::process::exit(1);
        }
    };
    let t = Instant::now();
    let result = summarize(&input, length);
    let took = t.elapsed();
    match result {
        Ok(s) => {
            println!(
                "input   : {} chars, {} words, {} sentences",
                s.stats.chars, s.stats.words, s.stats.sentences
            );
            println!("mode    : {length:?}");
            println!("took    : {:.1} ms", took.as_secs_f64() * 1000.0);
            println!(
                "output  : {} words in {} sentences ({:.0}% of the original)",
                s.stats.summary_words,
                s.stats.summary_sentences,
                s.stats.ratio * 100.0
            );
            println!(
                "reading : {:.0} s -> {:.0} s, saves {:.0} s",
                s.stats.read_secs, s.stats.summary_read_secs, s.stats.saved_secs
            );
            if let Some(n) = &s.note {
                println!("note    : {n}");
            }
            println!("----");
            println!("{}", s.text);
        }
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}
