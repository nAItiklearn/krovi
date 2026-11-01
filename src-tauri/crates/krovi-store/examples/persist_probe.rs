//! Process-restart probe for the SQLite store. Run it twice, as two separate processes:
//!
//! ```text
//! cargo run -p krovi-store --features sqlite --example persist_probe -- write <db-path>
//! cargo run -p krovi-store --features sqlite --example persist_probe -- read  <db-path>
//! ```
//!
//! `write` fills the engines the way the app does (a text clip, a pinned link, an
//! image thumbnail, notes, tasks, a calendar event, screen time, a setting) and saves
//! each collection to the database. `read` starts from nothing, restores everything
//! with `krovi_store::restore_all` (the exact startup path of the app) and prints it.

use krovi_core::{Calendar, ClipKind, ClipboardHistory, Notes, ScreenTime, TodoList};
use krovi_store::{restore_all, screen_time_records, SqliteStore, Store};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (cmd, path) = match (args.first(), args.get(1)) {
        (Some(c), Some(p)) => (c.as_str(), p.as_str()),
        _ => {
            eprintln!("usage: persist_probe write|read <db-path>");
            std::process::exit(2);
        }
    };
    let mut store = SqliteStore::open(path).expect("open database");
    match cmd {
        "write" => {
            let mut h = ClipboardHistory::default();
            h.add(ClipKind::Text, "restart-probe text clip", 1_000);
            let link = h.add(ClipKind::Link, "https://krovi.tech", 2_000);
            h.pin(link);
            h.add(ClipKind::Image, "data:image/png;base64,iVBORw0KGgo=", 3_000);
            let mut notes = Notes::new();
            notes.create("restart-probe note\nwith two lines", 4_000);
            let mut tasks = TodoList::new();
            let t = tasks.add("restart-probe task", 5_000);
            tasks.add("an open task", 5_500);
            tasks.toggle(t, 6_000);
            let mut cal = Calendar::new();
            cal.add("2026-11-02", "restart-probe event", Some("14:30"))
                .unwrap();
            let mut screen = ScreenTime::new();
            screen.record(20_000, "probe.exe", 123_000);

            let clips: Vec<_> = h.visible().into_iter().cloned().collect();
            store.save_clipboard(&clips).unwrap();
            store.save_notes(notes.list()).unwrap();
            store.save_tasks(tasks.tasks()).unwrap();
            store.save_calendar(cal.list()).unwrap();
            store
                .save_screen_time(&screen_time_records(&screen))
                .unwrap();
            store
                .set_setting("weather.place", r#"{"city":"Pune"}"#)
                .unwrap();
            println!(
                "wrote 3 clips, 1 note, 2 tasks, 1 event, 1 screen-time row, 1 setting to {path}"
            );
        }
        "read" => {
            let r = restore_all(&store).expect("restore");
            println!("clips ({}):", r.clipboard.len());
            for c in r.clipboard.visible() {
                println!("  #{} {:?} pinned={} {}", c.id, c.kind, c.pinned, c.content);
            }
            for n in r.notes.list() {
                println!("note #{}: {:?}", n.id, n.body);
            }
            for t in r.tasks.tasks() {
                println!(
                    "task #{}: {:?} done={} completed_at={:?}",
                    t.id, t.title, t.done, t.completed_at
                );
            }
            for e in r.calendar.list() {
                println!("event #{}: {} {:?} {}", e.id, e.date, e.time, e.title);
            }
            println!(
                "screen time: probe.exe on day 20000 = {} ms",
                r.screen.app_total(20_000, "probe.exe")
            );
            println!(
                "setting weather.place = {:?}",
                store.get_setting("weather.place").unwrap()
            );
        }
        other => {
            eprintln!("unknown command {other}");
            std::process::exit(2);
        }
    }
}
