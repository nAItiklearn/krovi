//! End-to-end tests against a real listening server. The production code only binds to the
//! detected private LAN address; tests use the `lan_override` escape hatch to bind loopback
//! (which is not 0.0.0.0, and which the override validation still allows).
//! The client is a hand-rolled HTTP/1.1 client over `TcpStream`, so what is exercised is the
//! exact bytes a phone browser would exchange.

use krovi_transfer::{
    CloseReason, Config, Direction, Event, EventSink, ShareInfo, ShareOptions, TransferError,
    TransferManager,
};
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Default)]
struct Collect(Mutex<Vec<Event>>);
impl EventSink for Collect {
    fn emit(&self, e: Event) {
        self.0.lock().unwrap().push(e);
    }
}
impl Collect {
    fn all(&self) -> Vec<Event> {
        self.0.lock().unwrap().clone()
    }
}

struct Rig {
    mgr: TransferManager,
    sink: Arc<Collect>,
    dir: PathBuf,
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn rig(tweak: impl FnOnce(&mut Config)) -> Rig {
    let dir = std::env::temp_dir().join(format!(
        "krovi-http-{}",
        krovi_transfer::token::share_id().unwrap()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let mut cfg = Config {
        receive_dir: dir.join("recv"),
        lan_override: Some(Ipv4Addr::LOCALHOST),
        ..Config::default()
    };
    tweak(&mut cfg);
    let sink = Arc::new(Collect::default());
    Rig {
        mgr: TransferManager::new(cfg, sink.clone()),
        sink,
        dir,
    }
}

impl Rig {
    fn file(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let p = self.dir.join(name);
        std::fs::write(&p, bytes).unwrap();
        p
    }
}

fn addr(info: &ShareInfo) -> SocketAddr {
    format!("{}:{}", info.lan_ip, info.port).parse().unwrap()
}

/// Path part of the share URL: `/s/<token>`.
fn path_of(info: &ShareInfo) -> String {
    let rest = info.url.strip_prefix("http://").unwrap();
    rest[rest.find('/').unwrap()..].to_string()
}

struct Resp {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}
impl Resp {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k == &name.to_ascii_lowercase())
            .map(|(_, v)| v.as_str())
    }
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

fn parse_head(head: &str) -> (u16, Vec<(String, String)>) {
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .unwrap()
        .split(' ')
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    let headers = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
        .collect();
    (status, headers)
}

fn roundtrip(a: SocketAddr, head: &str, body: &[u8]) -> Resp {
    let mut s = TcpStream::connect_timeout(&a, Duration::from_secs(5)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
    s.write_all(head.as_bytes()).unwrap();
    s.write_all(body).unwrap();
    let mut buf = Vec::new();
    let _ = s.read_to_end(&mut buf);
    let split = buf
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("response head");
    let (status, headers) = parse_head(std::str::from_utf8(&buf[..split]).unwrap());
    Resp {
        status,
        headers,
        body: buf[split + 4..].to_vec(),
    }
}

fn get(a: SocketAddr, path: &str) -> Resp {
    get_with(a, path, "")
}

fn get_with(a: SocketAddr, path: &str, extra: &str) -> Resp {
    roundtrip(
        a,
        &format!("GET {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n{extra}\r\n"),
        b"",
    )
}

fn post(a: SocketAddr, path: &str, body: &[u8]) -> Resp {
    roundtrip(
        a,
        &format!("POST {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\nContent-Length: {}\r\nContent-Type: application/octet-stream\r\n\r\n", body.len()),
        body,
    )
}

fn port_closed(a: SocketAddr) -> bool {
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if TcpStream::connect_timeout(&a, Duration::from_millis(300)).is_err() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

fn pattern(n: usize) -> Vec<u8> {
    (0..n)
        .map(|i| (i.wrapping_mul(31) ^ (i >> 7)) as u8)
        .collect()
}

#[test]
fn serves_page_and_file_and_404s_everything_else() {
    let r = rig(|_| {});
    let data = pattern(300_000);
    let f = r.file("my report.bin", &data);
    let info = r.mgr.share_files(&[f], ShareOptions::default()).unwrap();
    let a = addr(&info);
    let base = path_of(&info);

    // the LAN page: names, a download link, security headers, self-contained
    let page = get(a, &base);
    assert_eq!(page.status, 200);
    assert!(page
        .header("content-type")
        .unwrap()
        .starts_with("text/html"));
    assert!(page.text().contains("my report.bin"));
    assert!(page.text().contains(&format!("{base}/0")));
    assert_eq!(page.header("x-content-type-options"), Some("nosniff"));
    assert!(page
        .header("content-security-policy")
        .unwrap()
        .contains("default-src 'none'"));
    assert_eq!(page.header("cache-control"), Some("no-store"));

    // the file: exact bytes, real Content-Length (not chunked), attachment
    let file = get(a, &format!("{base}/0"));
    assert_eq!(file.status, 200);
    assert_eq!(file.body, data);
    assert_eq!(file.header("content-length"), Some("300000"));
    assert!(file.header("transfer-encoding").is_none());
    assert_eq!(file.header("accept-ranges"), Some("bytes"));
    assert!(file
        .header("content-disposition")
        .unwrap()
        .starts_with("attachment; filename=\"my report.bin\""));
    assert_eq!(
        file.header("content-type"),
        Some("application/octet-stream")
    );

    // everything else: byte-identical 404 with no listing
    let tok = info.url.rsplit('/').next().unwrap().to_string();
    let wrong = "A".repeat(tok.len());
    let r404 = get(a, "/");
    assert_eq!(r404.status, 404);
    for p in [
        "/".to_string(),
        "/index.html".into(),
        "/s".into(),
        "/s/".into(),
        format!("/s/{wrong}"),
        format!("/s/{wrong}/0"),
        format!("/r/{wrong}"),
        format!("/r/{tok}"), // a send token on a receive route
        format!("{base}/5"), // no such file
        format!("{base}/abc"),
        format!("{base}/0/extra"),
        "/s/../../etc/passwd".into(),
        "/%2e%2e/%2e%2e/windows/win.ini".into(),
        "/favicon.ico".into(),
    ] {
        let x = get(a, &p);
        assert_eq!(x.status, 404, "{p}");
        assert_eq!(x.body, r404.body, "{p}");
        assert_eq!(x.header("content-type"), r404.header("content-type"), "{p}");
    }
    // POST only works on receive tokens: valid send token -> 405, unknown -> 404
    assert_eq!(post(a, &base, b"x").status, 405);
    assert_eq!(post(a, &format!("/r/{tok}/up?n=a"), b"x").status, 404);
    assert_eq!(post(a, "/", b"x").status, 404);
    assert_eq!(post(a, &format!("/s/{wrong}"), b"x").status, 404);
    r.mgr.shutdown();
}

#[test]
fn range_requests_resume_and_unsatisfiable() {
    let r = rig(|_| {});
    let data = pattern(10_000);
    let info = r
        .mgr
        .share_files(&[r.file("a.bin", &data)], ShareOptions::default())
        .unwrap();
    let (a, base) = (addr(&info), path_of(&info));

    let part = get_with(a, &format!("{base}/0"), "Range: bytes=100-199\r\n");
    assert_eq!(part.status, 206);
    assert_eq!(part.header("content-range"), Some("bytes 100-199/10000"));
    assert_eq!(part.header("content-length"), Some("100"));
    assert_eq!(part.body, &data[100..200]);

    let tail = get_with(a, &format!("{base}/0"), "Range: bytes=9990-\r\n");
    assert_eq!(tail.status, 206);
    assert_eq!(tail.body, &data[9990..]);

    let suffix = get_with(a, &format!("{base}/0"), "Range: bytes=-10\r\n");
    assert_eq!(suffix.body, &data[9990..]);

    let bad = get_with(a, &format!("{base}/0"), "Range: bytes=20000-\r\n");
    assert_eq!(bad.status, 416);
    assert_eq!(bad.header("content-range"), Some("bytes */10000"));

    // a resumed tail counts as a completed download; a middle slice does not
    let done = r
        .sink
        .all()
        .iter()
        .filter(|e| matches!(e, Event::Done { .. }))
        .count();
    assert_eq!(done, 2); // bytes=9990- and bytes=-10
    assert_eq!(r.mgr.status()[0].downloads, 2);
    r.mgr.shutdown();
}

#[test]
fn download_counts_events_and_progress_is_throttled() {
    let r = rig(|_| {});
    let data = pattern(24 * 1024 * 1024);
    let info = r
        .mgr
        .share_files(&[r.file("big.bin", &data)], ShareOptions::default())
        .unwrap();
    let (a, base) = (addr(&info), path_of(&info));
    assert!(!r.mgr.status()[0].reached);

    let t0 = Instant::now();
    let resp = get(a, &format!("{base}/0"));
    let secs = t0.elapsed().as_secs_f64();
    assert_eq!(resp.status, 200);
    assert!(resp.body == data, "byte-exact download");

    let ev = r.sink.all();
    assert_eq!(
        ev.iter()
            .filter(|e| matches!(e, Event::Reached { .. }))
            .count(),
        1
    );
    let progress: Vec<&Event> = ev
        .iter()
        .filter(|e| matches!(e, Event::Progress { .. }))
        .collect();
    assert!(!progress.is_empty());
    // <= ~10/s (+ the immediate first one + slack)
    let cap = (secs * 10.0).ceil() as usize + 3;
    assert!(
        progress.len() <= cap,
        "{} progress events in {secs:.2}s",
        progress.len()
    );
    if let Some(Event::Progress {
        total, direction, ..
    }) = progress.last()
    {
        assert_eq!(*total, data.len() as u64);
        assert_eq!(*direction, Direction::Send);
    }
    assert!(ev.iter().any(|e| matches!(e, Event::Done { size, direction: Direction::Send, .. } if *size == data.len() as u64)));
    let s = &r.mgr.status()[0];
    assert_eq!(s.downloads, 1);
    assert!(s.reached);
    r.mgr.shutdown();
}

#[test]
fn upload_sanitizes_names_and_never_overwrites() {
    let r = rig(|_| {});
    let info = r.mgr.receive(ShareOptions::default()).unwrap();
    let (a, base) = (addr(&info), path_of(&info));
    assert!(base.starts_with("/r/"));

    let page = get(a, &base);
    assert_eq!(page.status, 200);
    assert!(page.text().contains("Choose files"));

    let up = format!("{base}/up");
    let one = post(a, &format!("{up}?n=..%2F..%2FWindows%2Fevil.txt"), b"first");
    assert_eq!(one.status, 200, "{}", one.text());
    assert!(one.text().contains("\"name\":\"evil.txt\""));
    let two = post(a, &format!("{up}?n=..%5C..%5Cevil.txt"), b"second!");
    assert!(two.text().contains("\"name\":\"evil (1).txt\""));
    let con = post(a, &format!("{up}?n=CON.txt"), b"x");
    assert!(con.text().contains("\"name\":\"_CON.txt\""));
    let none = post(a, &up, b"anon");
    assert!(none.text().contains("\"name\":\"file\""));

    let recv = r.dir.join("recv");
    assert_eq!(std::fs::read(recv.join("evil.txt")).unwrap(), b"first");
    assert_eq!(
        std::fs::read(recv.join("evil (1).txt")).unwrap(),
        b"second!"
    );
    assert!(!r.dir.join("evil.txt").exists());
    assert!(!r.dir.parent().unwrap().join("evil.txt").exists());
    // no stray .part files after successful uploads
    assert!(std::fs::read_dir(&recv).unwrap().all(|e| !e
        .unwrap()
        .file_name()
        .to_string_lossy()
        .ends_with(".part")));

    #[cfg(windows)]
    {
        let ads = std::fs::read_to_string(format!(
            "{}:Zone.Identifier",
            recv.join("evil.txt").display()
        ))
        .unwrap();
        assert!(ads.contains("ZoneId=3"));
    }

    let s = &r.mgr.status()[0];
    assert_eq!(s.uploads, 4);
    assert_eq!(s.files.len(), 4);
    assert_eq!(s.received_bytes, 5 + 7 + 1 + 4);
    let saved: Vec<String> = r
        .sink
        .all()
        .iter()
        .filter_map(|e| match e {
            Event::Done {
                direction: Direction::Receive,
                saved_path: Some(p),
                ..
            } => Some(p.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(saved.len(), 4);
    assert!(saved
        .iter()
        .all(|p| p.starts_with(recv.to_string_lossy().as_ref())));
    // a receive token cannot download, and GET on the upload endpoint is 405
    assert_eq!(get(a, &up).status, 405);
    assert_eq!(get(a, &format!("{base}/0")).status, 404);
    r.mgr.shutdown();
}

#[test]
fn upload_size_cap_per_file_and_per_share() {
    let r = rig(|c| c.max_upload_bytes = 1000);
    let info = r.mgr.receive(ShareOptions::default()).unwrap();
    let (a, base) = (addr(&info), path_of(&info));
    let up = format!("{base}/up");

    // declared too large: rejected before anything is written
    let big = post(a, &format!("{up}?n=big.bin"), &vec![7u8; 1500]);
    assert_eq!(big.status, 413);
    assert!(!r.dir.join("recv").join("big.bin").exists());

    // per-share total: 600 fits, the next 600 does not, and nothing partial is left
    assert_eq!(
        post(a, &format!("{up}?n=a.bin"), &vec![1u8; 600]).status,
        200
    );
    let second = post(a, &format!("{up}?n=b.bin"), &vec![2u8; 600]);
    assert_eq!(second.status, 413);
    let names: Vec<String> = std::fs::read_dir(r.dir.join("recv"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec!["a.bin".to_string()]);
    assert_eq!(r.mgr.status()[0].received_bytes, 600);
    // a smaller one still fits in the remaining budget
    assert_eq!(
        post(a, &format!("{up}?n=c.bin"), &vec![3u8; 400]).status,
        200
    );
    assert!(r.sink.all().iter().any(|e| matches!(
        e,
        Event::Failed {
            direction: Direction::Receive,
            ..
        }
    )));
    r.mgr.shutdown();
}

#[test]
fn interrupted_upload_leaves_no_file() {
    let r = rig(|_| {});
    let info = r.mgr.receive(ShareOptions::default()).unwrap();
    let (a, base) = (addr(&info), path_of(&info));
    {
        // promise 100000 bytes, send 10 000, then hang up
        let mut s = TcpStream::connect(a).unwrap();
        s.write_all(
            format!(
                "POST {base}/up?n=cut.bin HTTP/1.1\r\nHost: x\r\nContent-Length: 100000\r\n\r\n"
            )
            .as_bytes(),
        )
        .unwrap();
        s.write_all(&vec![9u8; 10_000]).unwrap();
        s.flush().unwrap();
        std::thread::sleep(Duration::from_millis(200));
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let left: Vec<_> = std::fs::read_dir(r.dir.join("recv"))
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        if left.is_empty() {
            break;
        }
        assert!(Instant::now() < deadline, "leftover files: {left:?}");
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(r.mgr.status()[0].uploads, 0);
    assert_eq!(r.mgr.status()[0].received_bytes, 0);
    r.mgr.shutdown();
}

#[test]
fn server_lives_only_while_a_share_is_open() {
    let r = rig(|_| {});
    assert!(r.mgr.listening().is_none());
    let f = r.file("a.txt", b"hello");
    let one = r
        .mgr
        .share_files(std::slice::from_ref(&f), ShareOptions::default())
        .unwrap();
    let two = r.mgr.receive(ShareOptions::default()).unwrap();
    assert_eq!(one.port, two.port, "one server for all shares");
    let a = addr(&one);
    assert!(
        (49152..=65534).contains(&one.port),
        "random high port, got {}",
        one.port
    );

    r.mgr.stop(Some(&one.id));
    assert!(
        r.mgr.listening().is_some(),
        "still open for the second share"
    );
    assert_eq!(get(a, &path_of(&one)).status, 404, "stopped share is gone");
    assert_eq!(get(a, &path_of(&two)).status, 200);

    r.mgr.stop(Some(&two.id));
    assert!(r.mgr.listening().is_none());
    assert!(
        port_closed(a),
        "port must be closed after the last share stops"
    );
    assert!(
        r.sink
            .all()
            .iter()
            .filter(|e| matches!(
                e,
                Event::Closed {
                    reason: CloseReason::Stopped,
                    ..
                }
            ))
            .count()
            == 2
    );

    // and it can be started again on a (possibly different) port
    let again = r.mgr.share_files(&[f], ShareOptions::default()).unwrap();
    assert_eq!(get(addr(&again), &path_of(&again)).status, 200);
    r.mgr.shutdown();
    assert!(port_closed(addr(&again)));
}

#[test]
fn stop_all_closes_every_share_and_the_port() {
    let r = rig(|_| {});
    let f = r.file("a.txt", b"hello");
    let one = r.mgr.share_files(&[f], ShareOptions::default()).unwrap();
    let two = r.mgr.receive(ShareOptions::default()).unwrap();
    let a = addr(&one);
    assert_eq!(get(a, &path_of(&one)).status, 200);
    assert_eq!(r.mgr.stop(None), 2);
    assert!(r.mgr.status().is_empty());
    assert!(r.mgr.listening().is_none());
    assert!(port_closed(a), "port closed after stop(None)");
    assert_eq!(r.mgr.stop(None), 0, "stopping twice is harmless");
    assert_eq!(
        r.mgr.stop(Some(&two.id)),
        0,
        "unknown/closed id is harmless"
    );
    assert_eq!(
        r.sink
            .all()
            .iter()
            .filter(|e| matches!(
                e,
                Event::Closed {
                    reason: CloseReason::Stopped,
                    ..
                }
            ))
            .count(),
        2
    );
}

#[test]
fn shares_expire_and_close_the_port() {
    let r = rig(|_| {});
    let f = r.file("a.txt", b"hello");
    let info = r
        .mgr
        .share_files(
            &[f],
            ShareOptions {
                ttl_secs: Some(1),
                pin: None,
            },
        )
        .unwrap();
    let (a, base) = (addr(&info), path_of(&info));
    assert_eq!(get(a, &base).status, 200);
    std::thread::sleep(Duration::from_millis(1700));
    assert!(r.mgr.status().is_empty(), "expired share removed");
    assert!(r.sink.all().iter().any(|e| matches!(
        e,
        Event::Closed {
            reason: CloseReason::Expired,
            ..
        }
    )));
    assert!(r.mgr.listening().is_none());
    assert!(port_closed(a), "port closed after expiry");
}

#[test]
fn expired_share_returns_404_even_before_the_reaper_runs() {
    let r = rig(|_| {});
    let f = r.file("a.txt", b"hello");
    // keep a second long-lived share so the server stays up while the first one expires
    let keep = r.mgr.receive(ShareOptions::default()).unwrap();
    let info = r
        .mgr
        .share_files(
            &[f],
            ShareOptions {
                ttl_secs: Some(1),
                pin: None,
            },
        )
        .unwrap();
    let (a, base) = (addr(&info), path_of(&info));
    std::thread::sleep(Duration::from_millis(1100));
    assert_eq!(get(a, &base).status, 404);
    assert_eq!(get(a, &path_of(&keep)).status, 200);
    r.mgr.shutdown();
}

#[test]
fn stopping_aborts_a_running_download_but_expiry_lets_it_finish() {
    let size = 48 * 1024 * 1024;
    let data = pattern(size);

    // --- stop: the transfer is cut ---
    let r = rig(|_| {});
    let info = r
        .mgr
        .share_files(&[r.file("big.bin", &data)], ShareOptions::default())
        .unwrap();
    let (a, base) = (addr(&info), path_of(&info));
    let mut s = TcpStream::connect(a).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
    s.write_all(
        format!("GET {base}/0 HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").as_bytes(),
    )
    .unwrap();
    let mut chunk = vec![0u8; 64 * 1024];
    let mut got = s.read(&mut chunk).unwrap();
    r.mgr.stop(Some(&info.id));
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        match s.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => got += n,
        }
    }
    assert!(
        got < size,
        "download should be cut short after stop ({got} of {size})"
    );
    assert!(port_closed(a));

    // --- expiry: a download already in flight is allowed to finish ---
    let r = rig(|_| {});
    let info = r
        .mgr
        .share_files(
            &[r.file("big.bin", &data)],
            ShareOptions {
                ttl_secs: Some(1),
                pin: None,
            },
        )
        .unwrap();
    let (a, base) = (addr(&info), path_of(&info));
    let mut s = TcpStream::connect(a).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    s.write_all(
        format!("GET {base}/0 HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").as_bytes(),
    )
    .unwrap();
    let mut first = vec![0u8; 64 * 1024];
    let n0 = s.read(&mut first).unwrap();
    std::thread::sleep(Duration::from_millis(1700)); // expiry fires while the server is blocked writing
    assert!(r.mgr.status().is_empty(), "share closed on expiry");
    let mut total = n0;
    loop {
        match s.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => total += n,
            Err(e) => panic!("read failed after expiry: {e}"),
        }
    }
    // body + response head
    assert!(
        total >= size,
        "in-flight download finished after expiry ({total} of {size})"
    );
    // new connections are refused / 404 once expired
    assert!(port_closed(a));
}

#[test]
fn pin_gate_blocks_until_correct_and_locks_after_five_failures() {
    let r = rig(|_| {});
    let f = r.file("secret.txt", b"top secret");
    let info = r
        .mgr
        .share_files(
            &[f],
            ShareOptions {
                pin: Some(true),
                ttl_secs: None,
            },
        )
        .unwrap();
    let pin = info.pin.clone().expect("pin generated");
    assert_eq!(pin.len(), 4);
    let (a, base) = (addr(&info), path_of(&info));

    // no cookie: the page is the PIN form (file name not leaked), the file is refused
    let p = get(a, &base);
    assert_eq!(p.status, 200);
    assert!(p.text().contains("Enter the PIN"));
    assert!(!p.text().contains("secret.txt"));
    assert_eq!(get(a, &format!("{base}/0")).status, 403);

    // wrong then right
    let wrong_pin = if pin == "0000" { "1111" } else { "0000" };
    let w = get(a, &format!("{base}?pin={wrong_pin}"));
    assert_eq!(w.status, 403);
    assert!(w.text().contains("4 tries left"));
    let ok = get(a, &format!("{base}?pin={pin}"));
    assert_eq!(ok.status, 303);
    assert_eq!(ok.header("location"), Some(base.as_str()));
    let cookie = ok.header("set-cookie").unwrap().to_string();
    assert!(cookie.contains("HttpOnly") && cookie.contains("SameSite=Strict"));
    let kv = cookie.split(';').next().unwrap().to_string();

    let with = get_with(a, &base, &format!("Cookie: {kv}\r\n"));
    assert!(with.text().contains("secret.txt"));
    let file = get_with(a, &format!("{base}/0"), &format!("Cookie: {kv}\r\n"));
    assert_eq!(file.status, 200);
    assert_eq!(file.body, b"top secret");
    // a forged cookie is not accepted
    let name = kv.split('=').next().unwrap();
    assert_eq!(
        get_with(
            a,
            &format!("{base}/0"),
            &format!("Cookie: {name}=forged\r\n")
        )
        .status,
        403
    );

    // brute force: 5 wrong guesses close the share for good (even the right PIN fails after)
    let r2 = rig(|_| {});
    let f2 = r2.file("s.txt", b"x");
    let info2 = r2
        .mgr
        .share_files(
            &[f2],
            ShareOptions {
                pin: Some(true),
                ttl_secs: None,
            },
        )
        .unwrap();
    let (a2, base2) = (addr(&info2), path_of(&info2));
    let real = info2.pin.clone().unwrap();
    let bad = if real == "0000" { "1111" } else { "0000" };
    let mut last = 0;
    for _ in 0..5 {
        last = get(a2, &format!("{base2}?pin={bad}")).status;
    }
    assert_eq!(last, 429);
    assert!(r2.sink.all().iter().any(|e| matches!(
        e,
        Event::Closed {
            reason: CloseReason::Locked,
            ..
        }
    )));
    assert!(r2.mgr.status().is_empty());
    // it was the only share, so the whole server is gone: the right PIN cannot get in either
    assert!(port_closed(a2), "locked share closes the server");

    // with another share still open, the locked one answers 404 even for the right PIN
    let r3 = rig(|_| {});
    let f3 = r3.file("s.txt", b"x");
    let locked = r3
        .mgr
        .share_files(
            &[f3],
            ShareOptions {
                pin: Some(true),
                ttl_secs: None,
            },
        )
        .unwrap();
    let other = r3.mgr.receive(ShareOptions::default()).unwrap();
    let (a3, base3) = (addr(&locked), path_of(&locked));
    let real3 = locked.pin.clone().unwrap();
    let bad3 = if real3 == "0000" { "1111" } else { "0000" };
    for _ in 0..5 {
        get(a3, &format!("{base3}?pin={bad3}"));
    }
    assert_eq!(
        get(a3, &format!("{base3}?pin={real3}")).status,
        404,
        "locked share is closed"
    );
    assert_eq!(get(a3, &path_of(&other)).status, 200);
    r3.mgr.shutdown();
}

#[test]
fn refuses_unsafe_binds_and_bad_input() {
    let r = rig(|c| c.lan_override = Some(Ipv4Addr::UNSPECIFIED));
    let f = r.file("a.txt", b"x");
    assert!(matches!(
        r.mgr.share_files(&[f], ShareOptions::default()),
        Err(TransferError::BadBind(_))
    ));
    assert!(r.mgr.listening().is_none());

    let r = rig(|_| {});
    assert!(matches!(
        r.mgr.share_files(&[], ShareOptions::default()),
        Err(TransferError::NoFiles)
    ));
    assert!(matches!(
        r.mgr
            .share_files(&[r.dir.join("missing.txt")], ShareOptions::default()),
        Err(TransferError::BadFile(_))
    ));
    // folders are rejected with a clear message
    match r
        .mgr
        .share_files(std::slice::from_ref(&r.dir), ShareOptions::default())
    {
        Err(TransferError::BadFile(m)) => assert!(m.contains("folder")),
        other => panic!("{other:?}"),
    }
    assert!(
        r.mgr.listening().is_none(),
        "failed shares never start a server"
    );
}

#[test]
fn many_files_and_dedupe_and_concurrent_downloads() {
    let r = rig(|_| {});
    let a1 = r.file("one.txt", b"1111");
    let a2 = r.file("two.txt", b"22222");
    let info = r
        .mgr
        .share_files(&[a1.clone(), a2, a1], ShareOptions::default())
        .unwrap();
    assert_eq!(info.files.len(), 2, "same path twice is shared once");
    let (a, base) = (addr(&info), path_of(&info));
    let handles: Vec<_> = (0..12)
        .map(|i| {
            let b = base.clone();
            std::thread::spawn(move || get(a, &format!("{b}/{}", i % 2)))
        })
        .collect();
    for (i, h) in handles.into_iter().enumerate() {
        let resp = h.join().unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body.len(), if i % 2 == 0 { 4 } else { 5 });
    }
    assert_eq!(r.mgr.status()[0].downloads, 12);
    r.mgr.shutdown();
}
