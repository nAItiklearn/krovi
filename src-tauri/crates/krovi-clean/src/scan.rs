//! The scanner: walks each category's folders and lists what could be moved to the Recycle Bin.
//! It only reads. It never follows links, never lists anything it could not fully check, and
//! stops promptly when asked.
//!
//! What counts as an item (the unit the user ticks and Krovi later moves):
//! * a **file**, or
//! * a **folder** whose whole contents were checked and are fine: no links, nothing unreadable,
//!   nothing too new, nothing nested deeper than the depth cap.
//!
//! A folder that fails any of those is not an item itself; its fine children are listed one by
//! one instead and the rest is counted in the `skipped_*` / `unreadable` numbers.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

use crate::categories::{CategorySpec, Kind, NameFilter, Shape};
use crate::fsx::{canonical, is_reparse, system_ms};
use crate::model::{CategoryResult, Guard, GuardRoot, RecycleBinSize, Scan, ScanItem};

pub const DEFAULT_MAX_DEPTH: usize = 24;
pub const DEFAULT_MAX_ITEMS: usize = 2_000;
pub const DEFAULT_PROGRESS_EVERY: u64 = 256;

#[derive(Clone, Debug)]
pub struct ScanOptions {
    /// "Now" for the age rules (injected so tests are deterministic).
    pub now: SystemTime,
    /// Folders nested deeper than this below a root are not entered.
    pub max_depth: usize,
    /// Most items listed per category; the rest are only counted.
    pub max_items: usize,
    /// Progress callback cadence, in files.
    pub progress_every: u64,
}

impl Default for ScanOptions {
    fn default() -> Self {
        ScanOptions {
            now: SystemTime::now(),
            max_depth: DEFAULT_MAX_DEPTH,
            max_items: DEFAULT_MAX_ITEMS,
            progress_every: DEFAULT_PROGRESS_EVERY,
        }
    }
}

/// What has been looked at so far in a category (not the final totals).
pub struct Progress<'a> {
    pub category: &'a str,
    pub files: u64,
    pub bytes: u64,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Totals {
    pub bytes: u64,
    pub files: u64,
    pub newest: Option<SystemTime>,
}

impl Totals {
    pub(crate) fn file(len: u64, mtime: Option<SystemTime>) -> Totals {
        Totals {
            bytes: len,
            files: 1,
            newest: mtime,
        }
    }

    pub(crate) fn add(&mut self, o: &Totals) {
        self.bytes = self.bytes.saturating_add(o.bytes);
        self.files = self.files.saturating_add(o.files);
        self.newest = match (self.newest, o.newest) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        };
    }
}

pub(crate) enum Dir {
    /// Everything inside was checked and is fine.
    Clean(Totals),
    /// Something inside is not fine (the reason is counted); not usable as one item.
    Dirty,
    /// Stopped part-way: the totals would be incomplete.
    Cancelled,
}

struct Unit {
    path: PathBuf,
    label: Option<String>,
    is_dir: bool,
    totals: Totals,
}

fn by_size_then_path(a: &Unit, b: &Unit) -> std::cmp::Ordering {
    b.totals
        .bytes
        .cmp(&a.totals.bytes)
        .then_with(|| a.path.cmp(&b.path))
}

/// True when something last changed `min_age` or longer ago. Unknown or future times count as
/// "too new": when in doubt, leave it alone.
pub(crate) fn old_enough(
    now: SystemTime,
    min_age: Option<Duration>,
    mtime: Option<SystemTime>,
) -> bool {
    match min_age {
        None => true,
        Some(min) => match mtime {
            Some(m) => now.duration_since(m).map(|d| d >= min).unwrap_or(false),
            None => false,
        },
    }
}

pub(crate) struct Walker<'a> {
    now: SystemTime,
    max_depth: usize,
    min_age: Option<Duration>,
    cancel: &'a AtomicBool,
    category: &'static str,
    on_progress: &'a mut dyn FnMut(&Progress),
    progress_every: u64,
    cap: usize,
    units: Vec<Unit>,
    /// Whether the fine children of a folder that is not itself fine become items.
    bubble: bool,
    pub skipped_recent: u64,
    pub skipped_links: u64,
    pub skipped_deep: u64,
    pub unreadable: u64,
    seen_files: u64,
    seen_bytes: u64,
    since_report: u64,
    unlisted_items: u64,
    unlisted_bytes: u64,
    unlisted_files: u64,
}

impl<'a> Walker<'a> {
    pub(crate) fn new(
        category: &'static str,
        min_age: Option<Duration>,
        opts: &ScanOptions,
        cancel: &'a AtomicBool,
        on_progress: &'a mut dyn FnMut(&Progress),
    ) -> Walker<'a> {
        Walker {
            now: opts.now,
            max_depth: opts.max_depth,
            min_age,
            cancel,
            category,
            on_progress,
            progress_every: opts.progress_every.max(1),
            cap: opts.max_items.max(1),
            units: Vec::new(),
            bubble: true,
            skipped_recent: 0,
            skipped_links: 0,
            skipped_deep: 0,
            unreadable: 0,
            seen_files: 0,
            seen_bytes: 0,
            since_report: 0,
            unlisted_items: 0,
            unlisted_bytes: 0,
            unlisted_files: 0,
        }
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    fn count_file(&mut self, len: u64) {
        self.seen_files += 1;
        self.seen_bytes = self.seen_bytes.saturating_add(len);
        self.since_report += 1;
        if self.since_report >= self.progress_every {
            self.since_report = 0;
            self.report();
        }
    }

    fn report(&mut self) {
        (self.on_progress)(&Progress {
            category: self.category,
            files: self.seen_files,
            bytes: self.seen_bytes,
        });
    }

    fn push_unit(&mut self, u: Unit) {
        if u.totals.files == 0 {
            return; // nothing to free
        }
        self.units.push(u);
        if self.units.len() >= self.cap.saturating_mul(2).max(16) {
            self.trim();
        }
    }

    /// Keep the largest `cap` units; the rest are only counted.
    fn trim(&mut self) {
        if self.units.len() <= self.cap {
            return;
        }
        self.units.sort_by(by_size_then_path);
        for u in self.units.drain(self.cap..) {
            self.unlisted_items += 1;
            self.unlisted_bytes = self.unlisted_bytes.saturating_add(u.totals.bytes);
            self.unlisted_files = self.unlisted_files.saturating_add(u.totals.files);
        }
    }

    /// Lists the fine children of a folder that cannot be one item itself. Off for `Whole`
    /// roots, which are all or nothing.
    fn flush(&mut self, pending: Vec<Unit>) {
        if !self.bubble {
            return;
        }
        for u in pending {
            self.push_unit(u);
        }
    }

    /// Looks at everything in `dir`. With `keep_children` (the root of a `Tree`) the fine
    /// children are always listed, because the root itself is never an item.
    pub(crate) fn walk_dir(&mut self, dir: &Path, depth: usize, keep_children: bool) -> Dir {
        let rd = match fs::read_dir(dir) {
            Ok(r) => r,
            Err(_) => {
                self.unreadable += 1;
                return Dir::Dirty;
            }
        };
        let mut totals = Totals::default();
        let mut pending: Vec<Unit> = Vec::new();
        let mut dirty = false;
        for entry in rd {
            if self.cancelled() {
                // The children already in `pending` are complete and fine on their own.
                self.flush(pending);
                return Dir::Cancelled;
            }
            let Ok(entry) = entry else {
                self.unreadable += 1;
                dirty = true;
                continue;
            };
            let path = entry.path();
            let Ok(meta) = entry.metadata() else {
                self.unreadable += 1;
                dirty = true;
                continue;
            };
            if is_reparse(&meta) {
                self.skipped_links += 1;
                dirty = true;
                continue;
            }
            if meta.is_dir() {
                if depth + 1 > self.max_depth {
                    self.skipped_deep += 1;
                    dirty = true;
                    continue;
                }
                match self.walk_dir(&path, depth + 1, false) {
                    Dir::Cancelled => {
                        self.flush(pending);
                        return Dir::Cancelled;
                    }
                    Dir::Dirty => dirty = true,
                    Dir::Clean(t) => {
                        totals.add(&t);
                        pending.push(Unit {
                            path,
                            label: None,
                            is_dir: true,
                            totals: t,
                        });
                    }
                }
            } else if meta.is_file() {
                let mtime = meta.modified().ok();
                if !old_enough(self.now, self.min_age, mtime) {
                    self.skipped_recent += 1;
                    dirty = true;
                    continue;
                }
                let t = Totals::file(meta.len(), mtime);
                totals.add(&t);
                self.count_file(meta.len());
                pending.push(Unit {
                    path,
                    label: None,
                    is_dir: false,
                    totals: t,
                });
            } else {
                // sockets, devices, anything that is neither a file nor a folder
                self.unreadable += 1;
                dirty = true;
            }
        }
        if keep_children || dirty {
            self.flush(pending);
        }
        if dirty {
            Dir::Dirty
        } else {
            Dir::Clean(totals)
        }
    }

    /// Files directly in `dir` whose names match. Returns true if cancelled.
    fn scan_files(&mut self, dir: &Path, filter: &NameFilter) -> bool {
        let rd = match fs::read_dir(dir) {
            Ok(r) => r,
            Err(_) => {
                self.unreadable += 1;
                return false;
            }
        };
        for entry in rd {
            if self.cancelled() {
                return true;
            }
            let Ok(entry) = entry else {
                self.unreadable += 1;
                continue;
            };
            if !filter.matches(&entry.file_name()) {
                continue;
            }
            let Ok(meta) = entry.metadata() else {
                self.unreadable += 1;
                continue;
            };
            if is_reparse(&meta) {
                self.skipped_links += 1;
                continue;
            }
            if !meta.is_file() {
                continue;
            }
            let mtime = meta.modified().ok();
            if !old_enough(self.now, self.min_age, mtime) {
                self.skipped_recent += 1;
                continue;
            }
            self.count_file(meta.len());
            self.push_unit(Unit {
                path: entry.path(),
                label: None,
                is_dir: false,
                totals: Totals::file(meta.len(), mtime),
            });
        }
        false
    }

    fn finish(mut self, roots_found: u32) -> CategoryResult {
        self.report();
        self.trim();
        let mut units = std::mem::take(&mut self.units);
        units.sort_by(by_size_then_path);
        let items: Vec<ScanItem> = units
            .into_iter()
            .enumerate()
            .map(|(n, u)| ScanItem {
                id: format!("{}#{}", self.category, n + 1),
                category: self.category,
                path: u.path,
                label: u.label,
                is_dir: u.is_dir,
                bytes: u.totals.bytes,
                files: u.totals.files,
                modified_ms: u.totals.newest.map(system_ms),
            })
            .collect();
        let listed_bytes: u64 = items.iter().map(|i| i.bytes).sum();
        let listed_files: u64 = items.iter().map(|i| i.files).sum();
        CategoryResult {
            id: self.category,
            bytes: listed_bytes.saturating_add(self.unlisted_bytes),
            files: listed_files.saturating_add(self.unlisted_files),
            items,
            unlisted_items: self.unlisted_items,
            unlisted_bytes: self.unlisted_bytes,
            unlisted_files: self.unlisted_files,
            skipped_recent: self.skipped_recent,
            skipped_links: self.skipped_links,
            skipped_deep: self.skipped_deep,
            unreadable: self.unreadable,
            roots_found,
        }
    }
}

/// Scans one file category. Returns the result, the guard that later delete checks use, and
/// whether the scan was cancelled part-way.
pub fn scan_category(
    spec: &CategorySpec,
    opts: &ScanOptions,
    cancel: &AtomicBool,
    on_progress: &mut dyn FnMut(&Progress),
) -> (CategoryResult, Guard, bool) {
    let mut w = Walker::new(spec.info.id, spec.min_age, opts, cancel, on_progress);
    let mut guard_roots: Vec<GuardRoot> = Vec::new();
    let mut cancelled = false;

    for root in &spec.roots {
        if cancel.load(Ordering::Relaxed) {
            cancelled = true;
            break;
        }
        let meta = match fs::symlink_metadata(&root.path) {
            Ok(m) => m,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(_) => {
                w.unreadable += 1;
                continue;
            }
        };
        if is_reparse(&meta) {
            // A folder that is itself a link is never followed, even if the user set it up.
            w.skipped_links += 1;
            continue;
        }
        if !meta.is_dir() {
            continue;
        }
        let Ok(canon) = canonical(&root.path) else {
            w.unreadable += 1;
            continue;
        };
        guard_roots.push(GuardRoot {
            canon: canon.clone(),
            shape: root.shape.clone(),
        });
        let stopped = match &root.shape {
            Shape::Tree => matches!(w.walk_dir(&canon, 0, true), Dir::Cancelled),
            Shape::Whole => {
                // all or nothing: the folder is the item, or nothing from it is offered
                w.bubble = false;
                let outcome = w.walk_dir(&canon, 0, false);
                w.bubble = true;
                match outcome {
                    Dir::Cancelled => true,
                    Dir::Clean(t) => {
                        w.push_unit(Unit {
                            path: canon,
                            label: root.label.clone(),
                            is_dir: true,
                            totals: t,
                        });
                        false
                    }
                    Dir::Dirty => false,
                }
            }
            Shape::Files(filter) => w.scan_files(&canon, filter),
        };
        if stopped {
            cancelled = true;
            break;
        }
    }

    let roots_found = guard_roots.len() as u32;
    let result = w.finish(roots_found);
    let guard = Guard {
        category: spec.info.id,
        roots: guard_roots,
        min_age: spec.min_age,
    };
    (result, guard, cancelled)
}

/// Scans the given categories in order. `recycle_bin` supplies the Recycle Bin size (a Windows
/// call, injected so the scanner stays testable).
pub fn scan(
    specs: &[CategorySpec],
    scan_id: u64,
    opts: &ScanOptions,
    cancel: &AtomicBool,
    recycle_bin: &dyn Fn() -> Option<RecycleBinSize>,
    on_progress: &mut dyn FnMut(&Progress),
) -> Scan {
    let mut categories = Vec::with_capacity(specs.len());
    let mut guards = Vec::new();
    let mut cancelled = false;
    for spec in specs {
        if cancel.load(Ordering::Relaxed) {
            cancelled = true;
            break;
        }
        match spec.kind {
            Kind::RecycleBin => {
                let size = recycle_bin();
                categories.push(CategoryResult {
                    id: spec.info.id,
                    bytes: size.map(|s| s.bytes).unwrap_or(0),
                    files: size.map(|s| s.items).unwrap_or(0),
                    roots_found: u32::from(size.is_some()),
                    ..CategoryResult::default()
                });
            }
            Kind::Files => {
                let (result, guard, stopped) = scan_category(spec, opts, cancel, on_progress);
                categories.push(result);
                guards.push(guard);
                if stopped {
                    cancelled = true;
                    break;
                }
            }
        }
    }
    Scan {
        scan_id,
        categories,
        cancelled,
        scanned_at_ms: system_ms(SystemTime::now()),
        guards,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::categories::{Root, INFOS, RECYCLE_BIN, USER_TEMP};
    use crate::testutil::{make_dir_link, remove_dir_link, TempDir, DAY, HOUR};
    use std::cell::RefCell;

    fn spec(roots: Vec<Root>, min_age: Option<Duration>) -> CategorySpec {
        CategorySpec {
            info: INFOS[0].clone(),
            kind: Kind::Files,
            roots,
            min_age,
        }
    }

    fn tree(p: &Path) -> Root {
        Root {
            path: p.to_path_buf(),
            shape: Shape::Tree,
            label: None,
        }
    }

    fn run(spec: &CategorySpec, opts: &ScanOptions) -> CategoryResult {
        let cancel = AtomicBool::new(false);
        scan_category(spec, opts, &cancel, &mut |_| {}).0
    }

    fn names(r: &CategoryResult, root: &Path) -> Vec<String> {
        r.items
            .iter()
            .map(|i| {
                i.path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect()
    }

    #[test]
    fn whole_folders_when_fine_and_single_files_when_not() {
        let t = TempDir::new("scan-basic");
        let root = t.mkdir("root");
        t.write_aged("root/old_dir/a.bin", 100, 2 * DAY);
        t.write_aged("root/old_dir/b.bin", 50, 2 * DAY);
        t.write("root/recent_dir/c.bin", 10); // just written: may be in use
        t.write_aged("root/loose.tmp", 30, 3 * DAY);
        t.write_aged("root/mixed/old.bin", 70, 2 * DAY);
        t.write("root/mixed/new.bin", 5);
        t.mkdir("root/empty_dir");

        let r = run(
            &spec(vec![tree(&root)], Some(TEMP_MIN)),
            &ScanOptions::default(),
        );
        // old_dir is one item; mixed is not an item, only its old file is; recent_dir is nothing.
        assert_eq!(
            names(&r, &t.path().join("root")),
            ["old_dir", "mixed/old.bin", "loose.tmp"]
        );
        assert!(r.items[0].is_dir && r.items[0].files == 2 && r.items[0].bytes == 150);
        assert!(!r.items[1].is_dir && r.items[1].files == 1 && r.items[1].bytes == 70);
        assert_eq!(r.bytes, 250);
        assert_eq!(r.files, 4);
        assert_eq!(r.skipped_recent, 2, "c.bin and new.bin");
        assert_eq!(r.roots_found, 1);
        assert_eq!(
            (
                r.unlisted_items,
                r.unreadable,
                r.skipped_links,
                r.skipped_deep
            ),
            (0, 0, 0, 0)
        );
        // the folder item's date is its newest file, about two days old
        let age_ms = system_ms(SystemTime::now()) - r.items[0].modified_ms.unwrap();
        assert!(age_ms > (2 * DAY).as_millis() as u64 - 60_000, "{age_ms}");
        assert!(age_ms < (2 * DAY + HOUR).as_millis() as u64, "{age_ms}");
    }

    const TEMP_MIN: Duration = Duration::from_secs(24 * 3600);

    #[test]
    fn without_an_age_rule_fresh_files_are_listed() {
        let t = TempDir::new("scan-noage");
        let root = t.mkdir("root");
        t.write("root/a/one.bin", 4);
        t.write("root/two.bin", 9);
        let r = run(&spec(vec![tree(&root)], None), &ScanOptions::default());
        assert_eq!(names(&r, &root), ["two.bin", "a"]);
        assert_eq!(r.skipped_recent, 0);
    }

    #[test]
    fn the_root_itself_is_never_an_item() {
        let t = TempDir::new("scan-rootnot");
        let root = t.mkdir("root");
        t.write("root/a.bin", 4);
        let r = run(&spec(vec![tree(&root)], None), &ScanOptions::default());
        assert!(r.items.iter().all(|i| i.path != root));
    }

    #[test]
    fn links_are_not_followed_counted_or_listed() {
        let t = TempDir::new("scan-links");
        let elsewhere = TempDir::new("scan-links-elsewhere");
        elsewhere.write("precious/secret.doc", 5_000);
        let root = t.mkdir("root");
        t.write("root/plain.bin", 10);
        t.write("root/clean_dir/ok.bin", 20);
        t.write("root/dir_with_link/ok.bin", 30);
        let top_link = root.join("shortcut");
        let inner_link = root.join("dir_with_link").join("inner");
        if !make_dir_link(&top_link, &elsewhere.path().join("precious"))
            || !make_dir_link(&inner_link, &elsewhere.path().join("precious"))
        {
            eprintln!("skipped: this machine does not allow creating directory links");
            return;
        }

        let r = run(&spec(vec![tree(&root)], None), &ScanOptions::default());
        assert_eq!(r.skipped_links, 2);
        assert_eq!(r.bytes, 60, "only the three real files");
        let listed = names(&r, &root);
        assert!(listed.contains(&"plain.bin".to_string()));
        assert!(listed.contains(&"clean_dir".to_string()));
        // the folder that holds a link is not offered whole; its real file is listed alone
        assert!(!listed.contains(&"dir_with_link".to_string()), "{listed:?}");
        assert!(
            listed.contains(&"dir_with_link/ok.bin".to_string()),
            "{listed:?}"
        );
        assert!(r
            .items
            .iter()
            .all(|i| !i.path.to_string_lossy().contains("secret")));
        assert!(r
            .items
            .iter()
            .all(|i| !i.path.to_string_lossy().contains("precious")));
        remove_dir_link(&top_link);
        remove_dir_link(&inner_link);
        assert!(elsewhere
            .path()
            .join("precious")
            .join("secret.doc")
            .exists());
    }

    #[test]
    fn a_root_that_is_itself_a_link_is_not_scanned() {
        let t = TempDir::new("scan-linkroot");
        let elsewhere = TempDir::new("scan-linkroot-target");
        elsewhere.write("f.bin", 100);
        let link = t.path().join("temp");
        if !make_dir_link(&link, elsewhere.path()) {
            eprintln!("skipped: this machine does not allow creating directory links");
            return;
        }
        let r = run(&spec(vec![tree(&link)], None), &ScanOptions::default());
        assert_eq!((r.roots_found, r.skipped_links, r.bytes), (0, 1, 0));
        assert!(r.items.is_empty());
        remove_dir_link(&link);
    }

    #[test]
    fn depth_cap_stops_descending_and_counts_it() {
        let t = TempDir::new("scan-depth");
        let root = t.mkdir("root");
        t.write("root/a/b/ok.bin", 7);
        t.write("root/a/b/c/deep.bin", 99);
        let opts = ScanOptions {
            max_depth: 2,
            ..ScanOptions::default()
        };
        let r = run(&spec(vec![tree(&root)], None), &opts);
        assert_eq!(r.skipped_deep, 1);
        assert_eq!(
            names(&r, &root),
            ["a/b/ok.bin"],
            "the too-deep folder is never summed or listed"
        );
        assert_eq!(r.bytes, 7);
    }

    #[test]
    fn item_cap_keeps_the_largest_and_still_counts_everything() {
        let t = TempDir::new("scan-cap");
        let root = t.mkdir("root");
        let mut total = 0u64;
        for i in 0..40u64 {
            // name order is unrelated to size order
            let size = 10 + ((i * 7) % 40) * 3;
            total += size;
            t.write(&format!("root/f{i:02}.bin"), size as usize);
        }
        let opts = ScanOptions {
            max_items: 5,
            ..ScanOptions::default()
        };
        let r = run(&spec(vec![tree(&root)], None), &opts);
        assert_eq!(r.items.len(), 5);
        assert_eq!(r.unlisted_items, 35);
        assert_eq!(r.bytes, total);
        assert_eq!(r.files, 40);
        let mut sizes: Vec<u64> = r.items.iter().map(|i| i.bytes).collect();
        assert!(
            sizes.windows(2).all(|w| w[0] >= w[1]),
            "largest first: {sizes:?}"
        );
        sizes.sort_unstable();
        assert_eq!(
            sizes[0],
            10 + 39 * 3 - 12,
            "the 5th largest of 10 + 3k for k in 0..40"
        );
        assert_eq!(
            r.bytes,
            r.items.iter().map(|i| i.bytes).sum::<u64>() + r.unlisted_bytes
        );
        assert_eq!(r.unlisted_files, 35);
    }

    #[test]
    fn ids_are_unique_and_ties_are_ordered_by_path() {
        let t = TempDir::new("scan-ids");
        let root = t.mkdir("root");
        for n in ["b.bin", "a.bin", "c.bin"] {
            t.write(&format!("root/{n}"), 10);
        }
        let r = run(&spec(vec![tree(&root)], None), &ScanOptions::default());
        assert_eq!(names(&r, &root), ["a.bin", "b.bin", "c.bin"]);
        let ids: Vec<&str> = r.items.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                format!("{USER_TEMP}#1"),
                format!("{USER_TEMP}#2"),
                format!("{USER_TEMP}#3")
            ]
        );
    }

    #[test]
    fn cancel_before_the_scan_does_nothing() {
        let t = TempDir::new("scan-cancel0");
        let root = t.mkdir("root");
        t.write("root/a.bin", 10);
        let cancel = AtomicBool::new(true);
        let s = scan(
            &[spec(vec![tree(&root)], None)],
            1,
            &ScanOptions::default(),
            &cancel,
            &|| None,
            &mut |_| {},
        );
        assert!(s.cancelled);
        assert!(s.categories.is_empty());
    }

    #[test]
    fn cancel_inside_a_folder_never_lists_that_folder_as_a_whole() {
        let t = TempDir::new("scan-cancel1");
        let root = t.mkdir("root");
        for i in 0..30 {
            t.write(&format!("root/big/f{i:02}.bin"), 10);
        }
        t.write("root/other.bin", 10);
        let cancel = AtomicBool::new(false);
        let opts = ScanOptions {
            progress_every: 1,
            ..ScanOptions::default()
        };
        // stop as soon as the first file has been looked at
        let s = scan(
            &[spec(vec![tree(&root)], None)],
            1,
            &opts,
            &cancel,
            &|| None,
            &mut |_| cancel.store(true, Ordering::Relaxed),
        );
        assert!(s.cancelled);
        let r = &s.categories[0];
        assert!(
            r.items.iter().all(|i| i.path != root.join("big")),
            "an interrupted folder has unknown contents and must not be offered whole"
        );
        for i in &r.items {
            assert!(
                i.path.is_file(),
                "only complete, real files may be listed: {:?}",
                i.path
            );
        }
        assert!(r.items.len() < 31, "the scan stopped early");
    }

    #[test]
    fn progress_reports_growing_counts_for_the_category() {
        let t = TempDir::new("scan-progress");
        let root = t.mkdir("root");
        for i in 0..7 {
            t.write(&format!("root/d/f{i}.bin"), 10);
        }
        let seen = RefCell::new(Vec::<(String, u64, u64)>::new());
        let cancel = AtomicBool::new(false);
        let opts = ScanOptions {
            progress_every: 2,
            ..ScanOptions::default()
        };
        let _ = scan_category(&spec(vec![tree(&root)], None), &opts, &cancel, &mut |p| {
            seen.borrow_mut()
                .push((p.category.to_string(), p.files, p.bytes));
        });
        let seen = seen.into_inner();
        assert!(seen.len() >= 3, "{seen:?}");
        assert!(seen.iter().all(|(c, _, _)| c == USER_TEMP));
        assert!(seen
            .windows(2)
            .all(|w| w[0].1 <= w[1].1 && w[0].2 <= w[1].2));
        assert_eq!(*seen.last().unwrap(), (USER_TEMP.to_string(), 7, 70));
    }

    #[test]
    fn a_whole_root_is_one_labelled_item() {
        let t = TempDir::new("scan-whole");
        let cache = t.mkdir("User Data/Default/Cache/Cache_Data");
        t.write("User Data/Default/Cache/Cache_Data/f_000001", 400);
        t.write(
            "User Data/Default/Cache/Cache_Data/index/the-real-index",
            100,
        );
        let root = Root {
            path: cache.clone(),
            shape: Shape::Whole,
            label: Some("Chrome / Default / Cache".into()),
        };
        let r = run(&spec(vec![root], None), &ScanOptions::default());
        assert_eq!(r.items.len(), 1);
        let it = &r.items[0];
        assert!(it.is_dir && it.files == 2 && it.bytes == 500);
        assert_eq!(it.label.as_deref(), Some("Chrome / Default / Cache"));
        assert!(crate::fsx::same_path(&it.path, &cache));
    }

    #[test]
    fn a_whole_root_with_a_link_inside_is_not_offered() {
        let t = TempDir::new("scan-whole-link");
        let elsewhere = TempDir::new("scan-whole-link-target");
        elsewhere.write("keep.doc", 10);
        let cache = t.mkdir("cache");
        t.write("cache/f_1", 50);
        let link = cache.join("sneaky");
        if !make_dir_link(&link, elsewhere.path()) {
            eprintln!("skipped: this machine does not allow creating directory links");
            return;
        }
        let root = Root {
            path: cache,
            shape: Shape::Whole,
            label: None,
        };
        let r = run(&spec(vec![root], None), &ScanOptions::default());
        assert!(
            r.items.is_empty(),
            "all or nothing: a folder with a link inside is left alone"
        );
        assert_eq!(r.skipped_links, 1);
        remove_dir_link(&link);
    }

    #[test]
    fn a_whole_root_that_is_not_fine_never_leaks_its_children() {
        let t = TempDir::new("scan-whole-leak");
        let cache = t.mkdir("cache");
        t.write_aged("cache/old.bin", 50, 2 * DAY);
        t.write("cache/fresh.bin", 5);
        let root = Root {
            path: cache,
            shape: Shape::Whole,
            label: None,
        };
        let r = run(&spec(vec![root], Some(TEMP_MIN)), &ScanOptions::default());
        assert!(r.items.is_empty(), "{:?}", r.items);
        assert_eq!((r.skipped_recent, r.bytes), (1, 0));
    }

    #[test]
    fn an_empty_or_missing_whole_root_lists_nothing() {
        let t = TempDir::new("scan-whole-empty");
        let empty = t.mkdir("empty");
        let roots = vec![
            Root {
                path: empty,
                shape: Shape::Whole,
                label: None,
            },
            Root {
                path: t.path().join("missing"),
                shape: Shape::Whole,
                label: None,
            },
        ];
        let r = run(&spec(roots, None), &ScanOptions::default());
        assert!(r.items.is_empty());
        assert_eq!(
            r.roots_found, 1,
            "the empty one exists, the missing one does not"
        );
    }

    #[test]
    fn name_filtered_roots_only_see_matching_files_directly_inside() {
        let t = TempDir::new("scan-filter");
        let dir = t.mkdir("Explorer");
        t.write("Explorer/thumbcache_32.db", 300);
        t.write("Explorer/thumbcache_idx.db", 40);
        t.write("Explorer/iconcache_32.db", 999);
        t.write("Explorer/notes.txt", 999);
        t.write("Explorer/sub/thumbcache_hidden.db", 999);
        t.mkdir("Explorer/thumbcache_dir.db"); // a folder with a matching name
        let root = Root {
            path: dir.clone(),
            shape: Shape::Files(NameFilter {
                prefix: Some("thumbcache_"),
                extensions: &["db"],
            }),
            label: None,
        };
        let r = run(&spec(vec![root], None), &ScanOptions::default());
        assert_eq!(names(&r, &dir), ["thumbcache_32.db", "thumbcache_idx.db"]);
        assert_eq!(r.bytes, 340);
    }

    #[test]
    fn installers_must_be_old_enough() {
        let t = TempDir::new("scan-installers");
        let dir = t.mkdir("Downloads");
        t.write_aged("Downloads/old-setup.exe", 500, 40 * DAY);
        t.write_aged("Downloads/old.MSI", 200, 31 * DAY);
        t.write_aged("Downloads/recent-setup.exe", 900, 29 * DAY);
        t.write_aged("Downloads/holiday.jpg", 900, 90 * DAY);
        t.write_aged("Downloads/unzipped/setup.exe", 900, 90 * DAY);
        let root = Root {
            path: dir.clone(),
            shape: Shape::Files(NameFilter {
                prefix: None,
                extensions: &["exe", "msi"],
            }),
            label: None,
        };
        let r = run(
            &spec(vec![root], Some(crate::categories::INSTALLER_MIN_AGE)),
            &ScanOptions::default(),
        );
        assert_eq!(names(&r, &dir), ["old-setup.exe", "old.MSI"]);
        assert_eq!(r.skipped_recent, 1);
    }

    #[test]
    fn missing_or_non_folder_roots_report_nothing_found() {
        let t = TempDir::new("scan-missing");
        let file = t.write("a-file", 5);
        let roots = vec![tree(&t.path().join("nope")), tree(&file)];
        let r = run(&spec(roots, None), &ScanOptions::default());
        assert_eq!(
            (r.roots_found, r.bytes, r.items.len(), r.unreadable),
            (0, 0, 0, 0)
        );
    }

    #[test]
    fn guards_record_the_canonical_roots_and_rules() {
        let t = TempDir::new("scan-guard");
        let root = t.mkdir("root");
        let cancel = AtomicBool::new(false);
        let (_, guard, _) = scan_category(
            &spec(vec![tree(&root)], Some(TEMP_MIN)),
            &ScanOptions::default(),
            &cancel,
            &mut |_| {},
        );
        assert_eq!(guard.category, USER_TEMP);
        assert_eq!(guard.min_age, Some(TEMP_MIN));
        assert_eq!(guard.roots.len(), 1);
        assert!(crate::fsx::same_path(&guard.roots[0].canon, &root));
    }

    #[test]
    fn the_recycle_bin_is_reported_never_listed() {
        let bin = CategorySpec {
            info: INFOS.iter().find(|i| i.id == RECYCLE_BIN).unwrap().clone(),
            kind: Kind::RecycleBin,
            roots: Vec::new(),
            min_age: None,
        };
        let cancel = AtomicBool::new(false);
        let opts = ScanOptions::default();
        let s = scan(
            std::slice::from_ref(&bin),
            3,
            &opts,
            &cancel,
            &|| {
                Some(RecycleBinSize {
                    bytes: 12_345,
                    items: 7,
                })
            },
            &mut |_| {},
        );
        let c = s.category(RECYCLE_BIN).unwrap();
        assert_eq!((c.bytes, c.files, c.roots_found), (12_345, 7, 1));
        assert!(c.items.is_empty());
        assert!(
            s.guard(RECYCLE_BIN).is_none(),
            "nothing in it can ever be deleted"
        );

        let s2 = scan(
            std::slice::from_ref(&bin),
            4,
            &opts,
            &cancel,
            &|| None,
            &mut |_| {},
        );
        assert_eq!(s2.category(RECYCLE_BIN).unwrap().roots_found, 0);
    }

    #[test]
    fn age_rule_treats_unknown_and_future_times_as_too_new() {
        let now = SystemTime::now();
        let day = Some(DAY);
        assert!(old_enough(now, None, None));
        assert!(old_enough(now, day, Some(now - 2 * DAY)));
        assert!(old_enough(now, day, Some(now - DAY)));
        assert!(!old_enough(now, day, Some(now - HOUR)));
        assert!(
            !old_enough(now, day, Some(now + HOUR)),
            "future-dated: skip"
        );
        assert!(!old_enough(now, day, None));
    }
}
