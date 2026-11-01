//! Starts a real Quick Transfer share from the command line (no Tauri). Used for QA with an
//! Android emulator / curl, and handy for trying the phone pages.
//!
//!   cargo run -p krovi-transfer --example serve -- send C:\path\a.zip C:\path\b.jpg
//!   cargo run -p krovi-transfer --example serve -- receive [--dir D:\inbox] [--max 5000000]
//!   options: --pin  --ttl <secs>  --lan <ip>   (--lan overrides interface detection: QA only)
//!   cargo run -p krovi-transfer --example serve -- qr "text to encode"   (prints the SVG)
//!
//! While running, type `stop` + Enter to close all shares, `status` to print them.
//! The process exits when the last share closes (the server goes away with it).

use krovi_transfer::{net, qr, Config, Event, EventSink, ShareOptions, TransferManager};
use std::io::BufRead;
use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

struct Print;
impl EventSink for Print {
    fn emit(&self, e: Event) {
        match &e {
            Event::Progress { .. } => {} // too chatty for a terminal
            _ => println!("EVENT {} {}", e.name(), e.payload()),
        }
    }
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: serve send <files...> | receive | qr <text>   [--pin] [--ttl s] [--dir path] [--max bytes] [--lan ip]");
        std::process::exit(2);
    }
    let cmd = args.remove(0);
    if cmd == "qr" {
        let text = args.first().cloned().unwrap_or_default();
        println!("{}", qr::svg_for(&text).expect("encodable"));
        return;
    }

    let mut cfg = Config::default();
    let mut opts = ShareOptions::default();
    let mut paths: Vec<PathBuf> = vec![];
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--pin" => opts.pin = Some(true),
            "--ttl" => opts.ttl_secs = it.next().and_then(|v| v.parse().ok()),
            "--dir" => cfg.receive_dir = PathBuf::from(it.next().expect("--dir needs a path")),
            "--max" => {
                cfg.max_upload_bytes = it
                    .next()
                    .and_then(|v| v.parse().ok())
                    .expect("--max needs bytes")
            }
            "--lan" => cfg.lan_override = it.next().and_then(|v| v.parse::<Ipv4Addr>().ok()),
            other => paths.push(PathBuf::from(other)),
        }
    }

    let detected = net::detect();
    println!(
        "LAN {}",
        detected
            .as_ref()
            .map(|l| format!("{} ({})", l.ip, l.adapter))
            .unwrap_or_else(|| "none".into())
    );

    let mgr = Arc::new(TransferManager::new(cfg, Arc::new(Print)));
    let share = match cmd.as_str() {
        "send" => mgr.share_files(&paths, opts),
        "receive" => mgr.receive(opts),
        _ => {
            eprintln!("unknown command {cmd}");
            std::process::exit(2);
        }
    };
    let share = match share {
        Ok(s) => s,
        Err(e) => {
            eprintln!("ERROR {e}");
            std::process::exit(1);
        }
    };
    println!("URL {}", share.url);
    println!("PORT {}", share.port);
    println!("ID {}", share.id);
    if let Some(p) = &share.pin {
        println!("PIN {p}");
    }
    println!("EXPIRES_AT {}", share.expires_at);
    for f in &share.files {
        println!("FILE {} {}", f.name, f.size);
    }

    {
        let mgr = mgr.clone();
        std::thread::spawn(move || {
            for line in std::io::stdin().lock().lines().map_while(Result::ok) {
                // Windows PowerShell pipes a UTF-8 BOM in front of the first line.
                match line.trim().trim_start_matches('\u{feff}') {
                    "stop" => {
                        mgr.stop(None);
                    }
                    "status" => println!(
                        "STATUS {}",
                        serde_json::to_string(
                            &mgr.status()
                                .iter()
                                .map(|s| (&s.id, s.downloads, s.uploads, s.reached))
                                .collect::<Vec<_>>()
                        )
                        .unwrap()
                    ),
                    _ => {}
                }
            }
        });
    }
    while !mgr.status().is_empty() {
        std::thread::sleep(Duration::from_millis(200));
    }
    println!("CLOSED (port should now refuse connections)");
    std::thread::sleep(Duration::from_millis(300));
}
