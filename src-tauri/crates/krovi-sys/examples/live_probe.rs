//! Live probe for the native data layer. Exercises the SAME krovi-sys code the app's
//! live hub uses, against the real OS, and prints one JSON line per event.
//!
//! ```text
//! cargo run -p krovi-sys --features native --example live_probe -- clipboard 20 [--copyback]
//! cargo run -p krovi-sys --features native --example live_probe -- media 30
//! cargo run -p krovi-sys --features native --example live_probe -- stats 5
//! cargo run -p krovi-sys --features native --example live_probe -- set-sensitive "s3cret" exclude|cannot-include|include
//! ```
//!
//! * `clipboard <secs>` polls like the live hub (250 ms): change token -> gate -> read.
//!   With `--copyback` every captured item is written straight back to the OS
//!   clipboard and the post-write token acknowledged, to prove it is not re-ingested.
//! * `media <secs>` runs the MTA `MediaWorker` and prints every `media-changed`.
//! * `stats <n>` prints `n` samples, 2 s apart.
//! * `set-sensitive` writes text to the clipboard with a password-manager style
//!   opt-out format, to prove sensitive content is skipped.

use std::time::{Duration, Instant};

use krovi_sys::{
    change_token, now_ms, read_capture, write_text, CaptureKind, ClipGate, MediaWorker,
    ReadOutcome, StatsSampler, TransportCommand,
};

fn j(s: &str) -> String {
    // minimal JSON string escaping for the probe's own output
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn clipboard(secs: u64, copyback: bool) {
    // `None`: ingest whatever is on the clipboard at start, like a fresh install.
    let mut gate = ClipGate::new(None);
    let end = Instant::now() + Duration::from_secs(secs);
    println!(
        "{{\"t\":{},\"ev\":\"watching\",\"token\":{}}}",
        now_ms(),
        change_token()
    );
    while Instant::now() < end {
        let token = change_token();
        if gate.should_read(token) {
            let outcome = read_capture();
            let handled = gate.record(token, &outcome);
            match &outcome {
                ReadOutcome::Capture(c) => {
                    let kind = match c.kind {
                        CaptureKind::Text => "Text",
                        CaptureKind::Link => "Link",
                        CaptureKind::Image => "Image",
                    };
                    let shown: String = c.content.chars().take(120).collect();
                    println!(
                        "{{\"t\":{},\"ev\":\"clip\",\"token\":{token},\"kind\":\"{kind}\",\"content\":{},\"len\":{},\"fullPng\":{}}}",
                        now_ms(),
                        j(&shown),
                        c.content.len(),
                        c.full_png.as_ref().map(|p| p.len()).unwrap_or(0)
                    );
                    if copyback && c.kind != CaptureKind::Image {
                        write_text(&c.content).expect("write back");
                        let after = change_token();
                        gate.acknowledge(after);
                        println!(
                            "{{\"t\":{},\"ev\":\"copyback\",\"before\":{token},\"after\":{after}}}",
                            now_ms()
                        );
                    }
                }
                ReadOutcome::Sensitive => println!(
                    "{{\"t\":{},\"ev\":\"skipped-sensitive\",\"token\":{token}}}",
                    now_ms()
                ),
                ReadOutcome::Empty => println!(
                    "{{\"t\":{},\"ev\":\"skipped-empty\",\"token\":{token}}}",
                    now_ms()
                ),
                ReadOutcome::Busy => println!(
                    "{{\"t\":{},\"ev\":\"busy\",\"token\":{token},\"handled\":{handled}}}",
                    now_ms()
                ),
            }
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    println!("{{\"t\":{},\"ev\":\"done\"}}", now_ms());
}

/// `transport`: optional `(command, at_second)` — run a transport command through the
/// worker (the same path the app's `media_transport` command uses) `at_second` into the run.
fn media(secs: u64, transport: Option<(TransportCommand, u64)>) {
    let worker = MediaWorker::spawn(|state| match state {
        Some(s) => println!(
            "{{\"t\":{},\"ev\":\"media\",\"title\":{},\"artist\":{},\"album\":{},\"playing\":{},\"positionSec\":{},\"durationSec\":{},\"sampledAt\":{},\"sourceApp\":{},\"artwork\":{}}}",
            now_ms(),
            j(&s.title),
            j(&s.artist),
            j(&s.album),
            s.playing,
            s.position_sec,
            s.duration_sec,
            s.sampled_at,
            j(&s.source_app),
            s.artwork
                .as_ref()
                .map(|a| {
                    let dims = krovi_sys::imaging::parse_data_url(a)
                        .and_then(|(_, b)| krovi_sys::imaging::decode_png(&b))
                        .map(|(w, h, _)| format!("{w}x{h}"))
                        .unwrap_or_else(|| "non-png".into());
                    format!("\"{}... ({} chars, {dims})\"", &a[..a.len().min(30)], a.len())
                })
                .unwrap_or_else(|| "null".into())
        ),
        None => println!("{{\"t\":{},\"ev\":\"media\",\"state\":null}}", now_ms()),
    })
    .expect("spawn media worker");
    println!("{{\"t\":{},\"ev\":\"media-watching\"}}", now_ms());
    let mut slept = 0;
    if let Some((cmd, at)) = transport {
        std::thread::sleep(Duration::from_secs(at));
        slept = at;
        let r = worker.transport(cmd);
        println!(
            "{{\"t\":{},\"ev\":\"transport\",\"cmd\":\"{}\",\"result\":{}}}",
            now_ms(),
            cmd.as_str(),
            j(&format!("{r:?}"))
        );
    }
    std::thread::sleep(Duration::from_secs(secs.saturating_sub(slept)));
    drop(worker);
    println!("{{\"t\":{},\"ev\":\"done\"}}", now_ms());
}

fn stats(n: u32) {
    let mut s = StatsSampler::new();
    for i in 0..n {
        let st = s.sample().expect("sample");
        println!(
            "{{\"t\":{},\"ev\":\"stats\",\"memUsed\":{},\"memTotal\":{},\"diskUsed\":{},\"diskTotal\":{},\"diskLabel\":{},\"cpu\":{}}}",
            now_ms(),
            st.mem_used,
            st.mem_total,
            st.disk_used,
            st.disk_total,
            j(&st.disk_label),
            st.cpu
        );
        if i + 1 < n {
            std::thread::sleep(Duration::from_millis(2000));
        }
    }
}

/// Put `text` on the clipboard together with a password-manager style marker.
/// `mode`: `exclude` (ExcludeClipboardContentFromMonitorProcessing present),
/// `cannot-include` (CanIncludeInClipboardHistory = 0) or `include`
/// (CanIncludeInClipboardHistory = 1 — must NOT be skipped).
fn set_sensitive(text: &str, mode: &str) {
    use windows::core::w;
    use windows::Win32::Foundation::{HANDLE, HWND};
    use windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
    };
    use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};

    unsafe {
        let mut opened = false;
        for _ in 0..50 {
            if OpenClipboard(HWND::default()).is_ok() {
                opened = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(opened, "could not open the clipboard");
        EmptyClipboard().expect("empty");

        let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        let h = GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2).expect("alloc text");
        let p = GlobalLock(h) as *mut u16;
        std::ptr::copy_nonoverlapping(wide.as_ptr(), p, wide.len());
        let _ = GlobalUnlock(h);
        SetClipboardData(13 /* CF_UNICODETEXT */, HANDLE(h.0)).expect("set text");

        let put_dword = |fmt: u32, value: u32| {
            let h = GlobalAlloc(GMEM_MOVEABLE, 4).expect("alloc dword");
            let p = GlobalLock(h) as *mut u32;
            p.write_unaligned(value);
            let _ = GlobalUnlock(h);
            SetClipboardData(fmt, HANDLE(h.0)).expect("set dword");
        };
        match mode {
            "exclude" => put_dword(
                RegisterClipboardFormatW(w!("ExcludeClipboardContentFromMonitorProcessing")),
                1,
            ),
            "cannot-include" => put_dword(
                RegisterClipboardFormatW(w!("CanIncludeInClipboardHistory")),
                0,
            ),
            "include" => put_dword(
                RegisterClipboardFormatW(w!("CanIncludeInClipboardHistory")),
                1,
            ),
            other => panic!("unknown mode {other}"),
        }
        CloseClipboard().expect("close");
    }
    println!(
        "{{\"t\":{},\"ev\":\"set-sensitive\",\"mode\":{}}}",
        now_ms(),
        j(mode)
    );
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("help");
    let num = |i: usize, default: u64| args.get(i).and_then(|v| v.parse().ok()).unwrap_or(default);
    match cmd {
        "clipboard" => clipboard(num(1, 20), args.iter().any(|a| a == "--copyback")),
        "media" => {
            // optional: --transport <cmd>@<seconds>, e.g. --transport playpause@8
            let t = args
                .iter()
                .position(|a| a == "--transport")
                .and_then(|i| args.get(i + 1))
                .and_then(|spec| spec.split_once('@'))
                .and_then(|(c, s)| Some((TransportCommand::from_str(c)?, s.parse().ok()?)));
            media(num(1, 30), t)
        }
        "stats" => stats(num(1, 3) as u32),
        "location" => {
            let started = Instant::now();
            match krovi_sys::current_location() {
                Ok(Some((lat, lon))) => println!(
                    "{{\"t\":{},\"ev\":\"location\",\"lat\":{lat},\"lon\":{lon},\"ms\":{}}}",
                    now_ms(),
                    started.elapsed().as_millis()
                ),
                Ok(None) => println!("{{\"t\":{},\"ev\":\"location\",\"result\":null}}", now_ms()),
                Err(e) => println!(
                    "{{\"t\":{},\"ev\":\"location\",\"error\":{},\"ms\":{}}}",
                    now_ms(),
                    j(&e.to_string()),
                    started.elapsed().as_millis()
                ),
            }
        }
        "set-sensitive" => set_sensitive(
            args.get(1).map(String::as_str).unwrap_or("s3cret"),
            args.get(2).map(String::as_str).unwrap_or("exclude"),
        ),
        _ => eprintln!("usage: live_probe clipboard <secs> [--copyback] | media <secs> | stats <n> | set-sensitive <text> exclude|cannot-include|include"),
    }
}
