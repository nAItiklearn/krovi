//! Moving confirmed items to the Recycle Bin.
//!
//! Safety rules, all enforced here and covered by tests:
//! * Only ids from the stored scan are honoured (and only for the scan id the UI quotes).
//!   Paths never come from the webview.
//! * Every item is re-checked right before it is moved: it still exists, is not a link, resolves
//!   to exactly the path that was scanned (so a link swapped in anywhere along the way, or a `..`,
//!   is caught), lies in the folder the scan used, and has not changed (no new or recently
//!   modified files, no new links).
//! * The shell is handed the resolved path, never the original text.
//! * Items that cannot be recycled safely (path too long, too big for the Recycle Bin) are left
//!   alone and reported, because Windows deletes those outright instead of recycling them.
//! * The platform call is behind [`Recycler`], so all of this is tested with a fake.
//! * A file in use is a reported failure. At most one extra attempt is made, only to find out
//!   which file of a failed batch was the problem; there is no retry loop.

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::SystemTime;

use crate::categories::Shape;
use crate::fsx::{
    canonical, has_wildcard, inside_strict, is_reparse, normal_depth, path_too_long, same_path,
};
use crate::model::{DeleteReport, DeleteRequest, Failure, Guard, GuardRoot, Scan, ScanItem};
use crate::scan::{old_enough, Dir, Progress, ScanOptions, Walker, DEFAULT_MAX_DEPTH};

/// Paths handed to the shell in one call.
pub const BATCH_SIZE: usize = 32;
/// Windows silently deletes (instead of recycling) anything bigger than the Recycle Bin allows.
/// Nothing above this is moved; the user can delete such a file in File Explorer.
pub const MAX_RECYCLE_ITEM_BYTES: u64 = 2 * 1024 * 1024 * 1024;

pub const STALE_SCAN: &str = "This scan is out of date. Scan again before cleaning.";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecycleError {
    pub code: i32,
    pub reason: String,
}

/// Moves paths to the Recycle Bin. `Ok(())` means the whole call succeeded; on `Err` any
/// subset may already have been moved, so callers re-check what still exists.
pub trait Recycler {
    fn recycle(&self, paths: &[PathBuf]) -> Result<(), RecycleError>;

    /// Will moving `path` (about `bytes` big) really go to the Recycle Bin? If Windows would
    /// delete it for good instead (Recycle Bin turned off for the drive, item bigger than the
    /// bin allows, ...) the answer must be an `Err` with a sentence for the user, and the item
    /// is left alone. The default says yes.
    fn will_recycle(&self, _path: &Path, _bytes: u64) -> Result<(), String> {
        Ok(())
    }
}

/// What Windows says about the Recycle Bin of the drive an item lives on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BinFacts {
    /// A fixed local drive (removable and network drives have no Recycle Bin).
    pub fixed_drive: bool,
    /// The drive's volume could be identified, so its Recycle Bin settings could be read.
    pub volume_known: bool,
    /// "Don't move files to the Recycle Bin. Remove files immediately when deleted."
    pub turned_off: bool,
    /// Group policy that deletes instead of recycling.
    pub policy_deletes: bool,
    /// The drive's Recycle Bin size limit in MiB, if one is set.
    pub max_capacity_mib: Option<u64>,
}

/// The decision from [`BinFacts`]: `Ok` only when the item will really be recycled.
pub fn recycle_verdict(f: &BinFacts, bytes: u64) -> Result<(), String> {
    if !f.fixed_drive {
        return Err("It is not on a fixed drive, and Windows keeps no Recycle Bin there (it would be deleted for good), so it was left alone.".into());
    }
    if f.policy_deletes {
        return Err("A Windows policy deletes files instead of using the Recycle Bin, so it was left alone.".into());
    }
    if !f.volume_known {
        return Err(
            "Krovi could not confirm the Recycle Bin is on for this drive, so it was left alone."
                .into(),
        );
    }
    if f.turned_off {
        return Err("The Recycle Bin is turned off for this drive (files would be deleted for good), so it was left alone.".into());
    }
    if let Some(mib) = f.max_capacity_mib {
        if bytes > mib.saturating_mul(1024 * 1024) {
            return Err("It is bigger than this drive's Recycle Bin allows (Windows would delete it for good), so it was left alone.".into());
        }
    }
    Ok(())
}

#[derive(Clone, Debug)]
pub struct DeleteOptions {
    pub now: SystemTime,
    pub max_depth: usize,
    pub batch_size: usize,
    pub max_item_bytes: u64,
}

impl Default for DeleteOptions {
    fn default() -> Self {
        DeleteOptions {
            now: SystemTime::now(),
            max_depth: DEFAULT_MAX_DEPTH,
            batch_size: BATCH_SIZE,
            max_item_bytes: MAX_RECYCLE_ITEM_BYTES,
        }
    }
}

/// Plain-English text for a `SHFileOperation` / Win32 error code.
pub fn shell_error_text(code: i32) -> String {
    match code {
        0x20 | 0x21 => "It is in use by another program.".into(),
        0x05 | 0x78 => "Windows would not allow it (no permission, or it is in use).".into(),
        0x02 | 0x03 | 0x7C => "Windows could not find it.".into(),
        0x79 | 0x81 => "Its path is too long for Windows to move.".into(),
        0x85 => "It is too large for the Recycle Bin.".into(),
        0x74 => "A drive's top folder cannot be moved.".into(),
        0x75 | 0x4C7 => "Windows cancelled the operation.".into(),
        c => format!("Windows could not move it (error code 0x{c:X})."),
    }
}

/// Why an item was not moved. Every variant has a sentence the user can read.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Invalid {
    Gone,
    Link,
    Changed,
    Outside,
    TooLong,
    TooBig,
    BadName,
    Unreadable(io::ErrorKind),
}

impl Invalid {
    pub(crate) fn reason(&self) -> String {
        match self {
            Invalid::Gone => "It was already gone.".into(),
            Invalid::Link => "It is now a link (shortcut). Krovi never follows links, so it was left alone.".into(),
            Invalid::Changed => "It changed after the scan (new or recently changed files, or it could not be fully checked), so it was left alone.".into(),
            Invalid::Outside => "It is no longer where the scan found it, so it was left alone.".into(),
            Invalid::TooLong => "Its path is too long for the Recycle Bin, so it was left alone.".into(),
            Invalid::TooBig => "It is too large to be sure it fits in the Recycle Bin (Windows would delete it for good), so it was left alone. You can delete it yourself in File Explorer.".into(),
            Invalid::BadName => "Its name contains a character the Windows shell treats specially, so it was left alone.".into(),
            Invalid::Unreadable(io::ErrorKind::PermissionDenied) => "Krovi has no permission to open it, so it was left alone.".into(),
            Invalid::Unreadable(_) => "Krovi could not read it, so it was left alone.".into(),
        }
    }
}

pub(crate) struct Valid {
    /// The resolved path to hand to the shell.
    pub path: PathBuf,
    pub bytes: u64,
    pub files: u64,
}

fn matches_root(canon: &Path, is_dir: bool, root: &GuardRoot) -> bool {
    match &root.shape {
        Shape::Tree => inside_strict(canon, &root.canon),
        Shape::Whole => is_dir && same_path(canon, &root.canon),
        Shape::Files(filter) => {
            !is_dir
                && canon.parent().is_some_and(|p| same_path(p, &root.canon))
                && canon.file_name().is_some_and(|n| filter.matches(n))
        }
    }
}

/// Re-checks one item against the disk right now. See the module docs for the rules.
pub(crate) fn validate(
    item: &ScanItem,
    guard: &Guard,
    opts: &DeleteOptions,
    cancel: &AtomicBool,
) -> Result<Valid, Invalid> {
    if has_wildcard(&item.path) {
        return Err(Invalid::BadName);
    }
    let unreadable = |e: io::Error| match e.kind() {
        io::ErrorKind::NotFound => Invalid::Gone,
        k => Invalid::Unreadable(k),
    };
    let meta = fs::symlink_metadata(&item.path).map_err(unreadable)?;
    if is_reparse(&meta) {
        return Err(Invalid::Link);
    }
    if meta.is_dir() != item.is_dir {
        return Err(Invalid::Changed);
    }
    let canon = canonical(&item.path).map_err(unreadable)?;
    // The path must still resolve to itself: a link swapped in anywhere along it, or a `..`,
    // makes the real location differ from the scanned one.
    if !same_path(&canon, &item.path) {
        return Err(Invalid::Outside);
    }
    if normal_depth(&canon) < 2
        || !guard
            .roots
            .iter()
            .any(|r| matches_root(&canon, item.is_dir, r))
    {
        return Err(Invalid::Outside);
    }
    if path_too_long(&canon) {
        return Err(Invalid::TooLong);
    }
    let (bytes, files) = if item.is_dir {
        // Walk it again with the same rules as the scan: nothing new, nothing recent, no links.
        let sopts = ScanOptions {
            now: opts.now,
            max_depth: opts.max_depth,
            max_items: 1,
            progress_every: u64::MAX,
        };
        let mut quiet = |_: &Progress| {};
        let mut walker = Walker::new(item.category, guard.min_age, &sopts, cancel, &mut quiet);
        match walker.walk_dir(&canon, 0, false) {
            Dir::Clean(t) if t.files > 0 => (t.bytes, t.files),
            Dir::Clean(_) => return Err(Invalid::Gone),
            Dir::Dirty | Dir::Cancelled => return Err(Invalid::Changed),
        }
    } else {
        if !old_enough(opts.now, guard.min_age, meta.modified().ok()) {
            return Err(Invalid::Changed);
        }
        (meta.len(), 1)
    };
    if bytes > opts.max_item_bytes {
        return Err(Invalid::TooBig);
    }
    Ok(Valid {
        path: canon,
        bytes,
        files,
    })
}

fn is_gone(p: &Path) -> bool {
    matches!(fs::symlink_metadata(p), Err(e) if e.kind() == io::ErrorKind::NotFound)
}

const NOT_MOVED: &str = "Windows reported success but the item is still there.";

/// Hands one batch of already re-checked items to the recycler and records, per item, whether it
/// really left its place. One extra attempt at most, alone, only to tell which item of a failed
/// batch was the problem.
fn move_batch(recycler: &dyn Recycler, ready: &[(&ScanItem, Valid)], report: &mut DeleteReport) {
    if ready.is_empty() {
        return;
    }
    let paths: Vec<PathBuf> = ready.iter().map(|(_, v)| v.path.clone()).collect();
    let first = recycler.recycle(&paths);
    for (item, v) in ready {
        let outcome: Result<(), String> = if is_gone(&v.path) {
            Ok(())
        } else {
            match &first {
                Ok(()) => Err(NOT_MOVED.to_string()),
                Err(e) if ready.len() == 1 => Err(e.reason.clone()),
                Err(_) => {
                    // The batch failed somewhere and this one is still here: it either caused
                    // the failure or was never reached. Ask about it alone, once.
                    let single = recycler.recycle(std::slice::from_ref(&v.path));
                    if is_gone(&v.path) {
                        Ok(())
                    } else {
                        Err(match single {
                            Ok(()) => NOT_MOVED.to_string(),
                            Err(e) => e.reason,
                        })
                    }
                }
            }
        };
        match outcome {
            Ok(()) => {
                report.deleted += 1;
                report.deleted_files = report.deleted_files.saturating_add(v.files);
                report.freed_bytes = report.freed_bytes.saturating_add(v.bytes);
            }
            Err(reason) => report.failed.push(Failure::new(&item.path, reason)),
        }
    }
}

/// Deletes what the request names from the stored scan. Items are re-checked and moved one batch
/// at a time, so the gap between "checked" and "moved" is a single batch. `on_progress(done,
/// total)` is called at the start and after every batch.
pub fn run_delete(
    scan: &Scan,
    req: &DeleteRequest,
    recycler: &dyn Recycler,
    opts: &DeleteOptions,
    cancel: &AtomicBool,
    on_progress: &mut dyn FnMut(usize, usize),
) -> Result<DeleteReport, String> {
    if req.scan_id != scan.scan_id {
        return Err(STALE_SCAN.into());
    }

    let mut report = DeleteReport::default();
    let wanted_items: HashSet<&str> = req.item_ids.iter().map(String::as_str).collect();
    let wanted_cats: HashSet<&str> = req.category_ids.iter().map(String::as_str).collect();

    // Ids that are not in this scan are refused outright; nothing is derived from them.
    for id in &req.item_ids {
        if scan.item(id).is_none() {
            report.failed.push(Failure {
                path: id.clone(),
                reason: "Not part of the last scan, so it was not touched.".into(),
            });
        }
    }
    for id in &req.category_ids {
        if scan.category(id).is_none() {
            report.failed.push(Failure {
                path: id.clone(),
                reason: "Not part of the last scan, so it was not touched.".into(),
            });
        }
    }

    let chosen: Vec<&ScanItem> = scan
        .categories
        .iter()
        .flat_map(|c| c.items.iter())
        .filter(|i| wanted_items.contains(i.id.as_str()) || wanted_cats.contains(i.category))
        .collect();
    let total = chosen.len();

    on_progress(0, total);
    let batch = opts.batch_size.max(1);
    let mut done = 0usize;
    for group in chosen.chunks(batch) {
        if cancel.load(Ordering::Relaxed) {
            report.cancelled = true;
            report.not_attempted = (total - done) as u64;
            break;
        }
        // Re-check this batch against the disk right now, then move what passed.
        let mut ready: Vec<(&ScanItem, Valid)> = Vec::with_capacity(group.len());
        for &item in group {
            let Some(guard) = scan.guard(item.category) else {
                report
                    .failed
                    .push(Failure::new(&item.path, Invalid::Outside.reason()));
                continue;
            };
            match validate(item, guard, opts, cancel) {
                Ok(v) => match recycler.will_recycle(&v.path, v.bytes) {
                    Ok(()) => ready.push((item, v)),
                    Err(reason) => report.failed.push(Failure::new(&item.path, reason)),
                },
                Err(Invalid::Gone) => report.already_gone += 1,
                Err(e) => report.failed.push(Failure::new(&item.path, e.reason())),
            }
        }
        move_batch(recycler, &ready, &mut report);
        done += group.len();
        on_progress(done, total);
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::categories::{
        CategorySpec, Kind, NameFilter, Root, INFOS, RECYCLE_BIN, TEMP_MIN_AGE, USER_TEMP,
    };
    use crate::model::CategoryResult;
    use crate::scan::scan;
    use crate::testutil::{make_dir_link, remove_dir_link, set_age, TempDir, DAY};
    use std::cell::{Cell, RefCell};
    use std::time::Duration;

    /// A stand-in for the Recycle Bin: "recycling" renames into a scratch folder. A locked path
    /// makes the call fail at that path, after moving the ones before it (like the shell).
    struct FakeBin {
        bin: TempDir,
        locked: Vec<PathBuf>,
        /// Paths whose drive has the Recycle Bin turned off.
        no_bin: Vec<PathBuf>,
        lie: bool,
        calls: RefCell<Vec<usize>>,
        attempts: RefCell<Vec<PathBuf>>,
        next: Cell<u32>,
    }

    impl FakeBin {
        fn new() -> FakeBin {
            FakeBin {
                bin: TempDir::new("fake-bin"),
                locked: Vec::new(),
                no_bin: Vec::new(),
                lie: false,
                calls: RefCell::new(Vec::new()),
                attempts: RefCell::new(Vec::new()),
                next: Cell::new(0),
            }
        }

        fn binned(&self) -> usize {
            fs::read_dir(self.bin.path()).unwrap().count()
        }
    }

    impl Recycler for FakeBin {
        fn will_recycle(&self, path: &Path, bytes: u64) -> Result<(), String> {
            let facts = BinFacts {
                fixed_drive: true,
                volume_known: true,
                turned_off: self.no_bin.iter().any(|p| same_path(p, path)),
                ..BinFacts::default()
            };
            recycle_verdict(&facts, bytes)
        }

        fn recycle(&self, paths: &[PathBuf]) -> Result<(), RecycleError> {
            self.calls.borrow_mut().push(paths.len());
            for p in paths {
                self.attempts.borrow_mut().push(p.clone());
                if self.locked.iter().any(|l| same_path(l, p)) {
                    return Err(RecycleError {
                        code: 32,
                        reason: shell_error_text(32),
                    });
                }
                if self.lie {
                    continue;
                }
                let n = self.next.get();
                self.next.set(n + 1);
                fs::rename(p, self.bin.path().join(format!("moved-{n}"))).expect("fake recycle");
            }
            Ok(())
        }
    }

    fn user_temp_spec(root: &Path, min_age: Option<Duration>) -> CategorySpec {
        CategorySpec {
            info: INFOS[0].clone(),
            kind: Kind::Files,
            roots: vec![Root {
                path: root.to_path_buf(),
                shape: Shape::Tree,
                label: None,
            }],
            min_age,
        }
    }

    fn scan_root(root: &Path, min_age: Option<Duration>) -> Scan {
        let cancel = AtomicBool::new(false);
        scan(
            &[user_temp_spec(root, min_age)],
            7,
            &ScanOptions::default(),
            &cancel,
            &|| None,
            &mut |_| {},
        )
    }

    fn id_of(s: &Scan, root: &Path, rel: &str) -> String {
        let want = root.join(rel);
        s.categories
            .iter()
            .flat_map(|c| c.items.iter())
            .find(|i| same_path(&i.path, &want))
            .unwrap_or_else(|| panic!("{rel} is not an item"))
            .id
            .clone()
    }

    fn req(ids: &[&str]) -> DeleteRequest {
        DeleteRequest {
            scan_id: 7,
            item_ids: ids.iter().map(|s| s.to_string()).collect(),
            category_ids: Vec::new(),
        }
    }

    fn go(s: &Scan, r: &DeleteRequest, rec: &dyn Recycler) -> DeleteReport {
        run_delete(
            s,
            r,
            rec,
            &DeleteOptions::default(),
            &AtomicBool::new(false),
            &mut |_, _| {},
        )
        .expect("delete runs")
    }

    fn fixture() -> (TempDir, PathBuf) {
        let t = TempDir::new("del");
        let root = t.mkdir("root");
        t.write_aged("root/dir/a.bin", 100, 2 * DAY);
        t.write_aged("root/dir/b.bin", 50, 2 * DAY);
        t.write_aged("root/one.tmp", 30, 3 * DAY);
        t.write_aged("root/two.tmp", 20, 3 * DAY);
        (t, root)
    }

    #[test]
    fn moves_exactly_the_selected_items_and_reports_what_it_did() {
        let (_t, root) = fixture();
        let s = scan_root(&root, Some(TEMP_MIN_AGE));
        let bin = FakeBin::new();
        let r = go(
            &s,
            &req(&[&id_of(&s, &root, "dir"), &id_of(&s, &root, "one.tmp")]),
            &bin,
        );
        assert_eq!(r.deleted, 2);
        assert_eq!(r.deleted_files, 3);
        assert_eq!(r.freed_bytes, 180);
        assert!(r.failed.is_empty() && !r.cancelled && r.already_gone == 0);
        assert_eq!(bin.binned(), 2);
        assert!(!root.join("dir").exists() && !root.join("one.tmp").exists());
        assert!(root.join("two.tmp").exists(), "not selected, not touched");
        assert!(root.exists(), "the root itself is never moved");
    }

    #[test]
    fn a_category_selection_expands_to_all_its_items() {
        let (_t, root) = fixture();
        let s = scan_root(&root, Some(TEMP_MIN_AGE));
        let bin = FakeBin::new();
        let r = go(
            &s,
            &DeleteRequest {
                scan_id: 7,
                item_ids: vec![],
                category_ids: vec![USER_TEMP.into()],
            },
            &bin,
        );
        assert_eq!((r.deleted, r.freed_bytes), (3, 200));
        assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
    }

    #[test]
    fn ids_must_come_from_the_scan_and_the_scan_must_be_current() {
        let (_t, root) = fixture();
        let s = scan_root(&root, Some(TEMP_MIN_AGE));
        let bin = FakeBin::new();

        // a made-up id, a path pretending to be an id, and an unknown category
        let victim = root.join("two.tmp");
        let r = go(
            &s,
            &DeleteRequest {
                scan_id: 7,
                item_ids: vec!["user-temp#99".into(), victim.to_string_lossy().into_owned()],
                category_ids: vec!["system32".into()],
            },
            &bin,
        );
        assert_eq!(r.deleted, 0);
        assert_eq!(r.failed.len(), 3);
        assert!(r
            .failed
            .iter()
            .all(|f| f.reason.contains("Not part of the last scan")));
        assert!(victim.exists());
        assert_eq!(bin.calls.borrow().len(), 0, "the recycler was never called");

        // a scan id from some other scan
        let stale = DeleteRequest {
            scan_id: 6,
            item_ids: vec![id_of(&s, &root, "one.tmp")],
            category_ids: vec![],
        };
        let e = run_delete(
            &s,
            &stale,
            &bin,
            &DeleteOptions::default(),
            &AtomicBool::new(false),
            &mut |_, _| {},
        )
        .unwrap_err();
        assert_eq!(e, STALE_SCAN);
        assert!(root.join("one.tmp").exists());
    }

    #[test]
    fn the_recycle_bin_category_has_nothing_to_delete() {
        let (_t, root) = fixture();
        let mut s = scan_root(&root, Some(TEMP_MIN_AGE));
        s.categories.push(CategoryResult {
            id: RECYCLE_BIN,
            bytes: 999,
            files: 3,
            ..CategoryResult::default()
        });
        let bin = FakeBin::new();
        let r = go(
            &s,
            &DeleteRequest {
                scan_id: 7,
                item_ids: vec![],
                category_ids: vec![RECYCLE_BIN.into()],
            },
            &bin,
        );
        assert_eq!((r.deleted, r.failed.len()), (0, 0));
        assert_eq!(bin.calls.borrow().len(), 0);
    }

    #[test]
    fn a_folder_swapped_for_a_link_after_the_scan_is_refused() {
        let (t, root) = fixture();
        let s = scan_root(&root, Some(TEMP_MIN_AGE));
        let elsewhere = TempDir::new("del-elsewhere");
        elsewhere.write("documents/thesis.doc", 4_000);
        // the scanned folder is replaced by a link to somewhere precious
        fs::remove_dir_all(root.join("dir")).unwrap();
        if !make_dir_link(&root.join("dir"), &elsewhere.path().join("documents")) {
            eprintln!("skipped: this machine does not allow creating directory links");
            return;
        }
        let bin = FakeBin::new();
        let r = go(&s, &req(&[&id_of(&s, &root, "dir")]), &bin);
        assert_eq!(r.deleted, 0);
        assert_eq!(r.failed.len(), 1);
        assert!(r.failed[0].reason.contains("link"), "{:?}", r.failed);
        assert!(bin.attempts.borrow().is_empty());
        assert!(elsewhere
            .path()
            .join("documents")
            .join("thesis.doc")
            .exists());
        remove_dir_link(&root.join("dir"));
        drop(t);
    }

    #[test]
    fn a_link_swapped_into_the_middle_of_the_path_is_refused() {
        let t = TempDir::new("del-mid");
        let root = t.mkdir("root");
        t.write_aged("root/sub/old.tmp", 40, 2 * DAY);
        t.write("root/sub/new.tmp", 5); // keeps `sub` from being one item: old.tmp is listed alone
        let s = scan_root(&root, Some(TEMP_MIN_AGE));
        let id = id_of(&s, &root, "sub/old.tmp");

        let elsewhere = TempDir::new("del-mid-elsewhere");
        elsewhere.write_aged("old.tmp", 40, 2 * DAY);
        fs::remove_dir_all(root.join("sub")).unwrap();
        if !make_dir_link(&root.join("sub"), elsewhere.path()) {
            eprintln!("skipped: this machine does not allow creating directory links");
            return;
        }
        // `root\sub\old.tmp` is now a perfectly ordinary file as far as a stat can tell
        assert!(!is_reparse(
            &fs::symlink_metadata(root.join("sub").join("old.tmp")).unwrap()
        ));

        let bin = FakeBin::new();
        let r = go(&s, &req(&[&id]), &bin);
        assert_eq!(r.deleted, 0);
        assert!(
            r.failed[0].reason.contains("no longer where"),
            "{:?}",
            r.failed
        );
        assert!(bin.attempts.borrow().is_empty());
        assert!(elsewhere.path().join("old.tmp").exists());
        remove_dir_link(&root.join("sub"));
    }

    fn manual_scan(root: &Path, shape: Shape, items: Vec<ScanItem>) -> Scan {
        Scan {
            scan_id: 7,
            categories: vec![CategoryResult {
                id: USER_TEMP,
                items,
                ..CategoryResult::default()
            }],
            cancelled: false,
            scanned_at_ms: 0,
            guards: vec![Guard {
                category: USER_TEMP,
                roots: vec![GuardRoot {
                    canon: canonical(root).unwrap(),
                    shape,
                }],
                min_age: None,
            }],
        }
    }

    fn item(n: usize, path: PathBuf, is_dir: bool) -> ScanItem {
        ScanItem {
            id: format!("{USER_TEMP}#{n}"),
            category: USER_TEMP,
            path,
            label: None,
            is_dir,
            bytes: 1,
            files: 1,
            modified_ms: None,
        }
    }

    #[test]
    fn dotdot_and_paths_outside_the_scanned_folder_are_refused() {
        let t = TempDir::new("del-dotdot");
        let root = t.mkdir("root");
        t.write("root/inside.tmp", 3);
        t.write("outside/precious.doc", 3);
        let sneaky = root
            .join("sub")
            .join("..")
            .join("..")
            .join("outside")
            .join("precious.doc");
        fs::create_dir_all(root.join("sub")).unwrap();
        let items = vec![
            item(1, sneaky, false),
            item(2, root.clone(), true), // the root itself
            item(3, root.parent().unwrap().to_path_buf(), true), // above the root
            item(4, t.path().join("outside").join("precious.doc"), false), // a sibling folder
            item(5, root.join("inside.tmp"), false), // the one legitimate item
        ];
        let s = manual_scan(&root, Shape::Tree, items);
        let bin = FakeBin::new();
        let ids: Vec<String> = (1..=5).map(|n| format!("{USER_TEMP}#{n}")).collect();
        let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
        let r = go(&s, &req(&ids), &bin);
        assert_eq!(r.deleted, 1, "{:?}", r.failed);
        assert_eq!(r.failed.len(), 4);
        assert!(t.path().join("outside").join("precious.doc").exists());
        assert!(root.exists() && t.path().exists());
        assert!(!root.join("inside.tmp").exists());
    }

    #[test]
    fn the_shape_of_the_root_decides_what_may_be_moved() {
        let t = TempDir::new("del-shape");
        let cache = t.mkdir("cache");
        t.write("cache/f_1", 4);
        let bin = FakeBin::new();

        // Whole: only the folder itself, never something inside it
        let s = manual_scan(
            &cache,
            Shape::Whole,
            vec![
                item(1, cache.join("f_1"), false),
                item(2, cache.clone(), true),
            ],
        );
        let r = go(&s, &req(&[&format!("{USER_TEMP}#1")]), &bin);
        assert_eq!((r.deleted, r.failed.len()), (0, 1));
        assert!(cache.join("f_1").exists());
        let r = go(&s, &req(&[&format!("{USER_TEMP}#2")]), &bin);
        assert_eq!(r.deleted, 1, "{:?}", r.failed);
        assert!(!cache.exists());

        // Files: only matching names directly inside
        let ex = t.mkdir("Explorer");
        t.write("Explorer/thumbcache_32.db", 4);
        t.write("Explorer/notes.txt", 4);
        t.write("Explorer/sub/thumbcache_9.db", 4);
        let filter = NameFilter {
            prefix: Some("thumbcache_"),
            extensions: &["db"],
        };
        let s = manual_scan(
            &ex,
            Shape::Files(filter),
            vec![
                item(1, ex.join("thumbcache_32.db"), false),
                item(2, ex.join("notes.txt"), false),
                item(3, ex.join("sub").join("thumbcache_9.db"), false),
            ],
        );
        let bin = FakeBin::new();
        let r = go(
            &s,
            &req(&["user-temp#1", "user-temp#2", "user-temp#3"]),
            &bin,
        );
        assert_eq!(r.deleted, 1, "{:?}", r.failed);
        assert_eq!(r.failed.len(), 2);
        assert!(ex.join("notes.txt").exists() && ex.join("sub").join("thumbcache_9.db").exists());
    }

    #[test]
    fn items_that_changed_after_the_scan_are_left_alone() {
        let (_t, root) = fixture();
        let s = scan_root(&root, Some(TEMP_MIN_AGE));
        let bin = FakeBin::new();
        // `one.tmp` was just modified by an app; `dir` gained a brand-new file
        set_age(&root.join("one.tmp"), Duration::ZERO);
        fs::write(root.join("dir").join("fresh.bin"), b"new").unwrap();
        let r = go(
            &s,
            &req(&[
                &id_of(&s, &root, "one.tmp"),
                &id_of(&s, &root, "dir"),
                &id_of(&s, &root, "two.tmp"),
            ]),
            &bin,
        );
        assert_eq!(r.deleted, 1, "only two.tmp is still safe: {:?}", r.failed);
        assert_eq!(r.failed.len(), 2);
        assert!(r
            .failed
            .iter()
            .all(|f| f.reason.contains("changed after the scan")));
        assert!(root.join("one.tmp").exists() && root.join("dir").join("a.bin").exists());
    }

    #[test]
    fn something_that_vanished_is_counted_not_failed() {
        let (_t, root) = fixture();
        let s = scan_root(&root, Some(TEMP_MIN_AGE));
        fs::remove_file(root.join("two.tmp")).unwrap();
        let bin = FakeBin::new();
        let r = go(
            &s,
            &req(&[&id_of(&s, &root, "two.tmp"), &id_of(&s, &root, "one.tmp")]),
            &bin,
        );
        assert_eq!((r.deleted, r.already_gone, r.failed.len()), (1, 1, 0));
        assert_eq!(r.freed_bytes, 30);
    }

    #[test]
    fn a_locked_file_is_reported_and_costs_at_most_one_extra_attempt() {
        let t = TempDir::new("del-locked");
        let root = t.mkdir("root");
        for (n, size) in [(1, 500), (2, 400), (3, 300), (4, 200), (5, 100)] {
            t.write_aged(&format!("root/f{n}.tmp"), size, 2 * DAY);
        }
        let s = scan_root(&root, Some(TEMP_MIN_AGE));
        let mut bin = FakeBin::new();
        bin.locked.push(root.join("f3.tmp"));
        let ids: Vec<String> = (1..=5)
            .map(|n| id_of(&s, &root, &format!("f{n}.tmp")))
            .collect();
        let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
        let r = go(&s, &req(&ids), &bin);

        assert_eq!(r.deleted, 4);
        assert_eq!(r.freed_bytes, 500 + 400 + 200 + 100);
        assert_eq!(r.failed.len(), 1);
        assert!(r.failed[0].path.ends_with("f3.tmp"));
        assert!(r.failed[0].reason.contains("in use"), "{:?}", r.failed);
        assert!(root.join("f3.tmp").exists());
        // one batch of five, then the three that were left (f3 and the two after it) one by one
        assert_eq!(*bin.calls.borrow(), vec![5, 1, 1, 1]);
        let locked_tries = bin
            .attempts
            .borrow()
            .iter()
            .filter(|p| same_path(p, &root.join("f3.tmp")))
            .count();
        assert_eq!(
            locked_tries, 2,
            "once in the batch, once alone: never a loop"
        );
    }

    #[test]
    fn a_failure_in_a_batch_of_one_is_reported_directly() {
        let (_t, root) = fixture();
        let s = scan_root(&root, Some(TEMP_MIN_AGE));
        let mut bin = FakeBin::new();
        bin.locked.push(root.join("one.tmp"));
        let r = go(&s, &req(&[&id_of(&s, &root, "one.tmp")]), &bin);
        assert_eq!((r.deleted, r.failed.len()), (0, 1));
        assert_eq!(*bin.calls.borrow(), vec![1]);
    }

    #[test]
    fn success_without_the_item_actually_moving_is_a_failure() {
        let (_t, root) = fixture();
        let s = scan_root(&root, Some(TEMP_MIN_AGE));
        let mut bin = FakeBin::new();
        bin.lie = true;
        let r = go(&s, &req(&[&id_of(&s, &root, "one.tmp")]), &bin);
        assert_eq!((r.deleted, r.freed_bytes), (0, 0));
        assert_eq!(r.failed[0].reason, NOT_MOVED);
        assert!(root.join("one.tmp").exists());
    }

    #[test]
    fn an_item_that_windows_would_delete_for_good_is_never_handed_over() {
        let (_t, root) = fixture();
        let s = scan_root(&root, Some(TEMP_MIN_AGE));
        let mut bin = FakeBin::new();
        bin.no_bin.push(root.join("one.tmp"));
        let r = go(
            &s,
            &req(&[&id_of(&s, &root, "one.tmp"), &id_of(&s, &root, "two.tmp")]),
            &bin,
        );
        assert_eq!(r.deleted, 1);
        assert_eq!(r.failed.len(), 1);
        assert!(r.failed[0].path.ends_with("one.tmp"));
        assert!(r.failed[0].reason.contains("turned off"), "{:?}", r.failed);
        assert!(root.join("one.tmp").exists());
        assert!(
            bin.attempts
                .borrow()
                .iter()
                .all(|p| !p.ends_with("one.tmp")),
            "the recycler must not even be asked about it"
        );
    }

    #[test]
    fn the_recycle_verdict_covers_every_way_windows_deletes_for_good() {
        let good = BinFacts {
            fixed_drive: true,
            volume_known: true,
            ..BinFacts::default()
        };
        assert!(
            recycle_verdict(&good, 5_000_000_000).is_ok(),
            "no limit set: any size"
        );
        let mib = |n| BinFacts {
            max_capacity_mib: Some(n),
            ..good
        };
        assert!(recycle_verdict(&mib(100), 100 * 1024 * 1024).is_ok());
        assert!(recycle_verdict(&mib(100), 100 * 1024 * 1024 + 1)
            .unwrap_err()
            .contains("bigger"));
        assert!(recycle_verdict(
            &BinFacts {
                turned_off: true,
                ..good
            },
            1
        )
        .unwrap_err()
        .contains("turned off"));
        assert!(recycle_verdict(
            &BinFacts {
                policy_deletes: true,
                ..good
            },
            1
        )
        .unwrap_err()
        .contains("policy"));
        assert!(recycle_verdict(
            &BinFacts {
                volume_known: false,
                ..good
            },
            1
        )
        .unwrap_err()
        .contains("could not confirm"));
        assert!(recycle_verdict(
            &BinFacts {
                fixed_drive: false,
                ..good
            },
            1
        )
        .unwrap_err()
        .contains("fixed drive"));
        assert!(
            recycle_verdict(&BinFacts::default(), 1).is_err(),
            "unknown facts never pass"
        );
    }

    #[test]
    fn items_too_big_for_the_recycle_bin_are_left_alone() {
        let (_t, root) = fixture();
        let s = scan_root(&root, Some(TEMP_MIN_AGE));
        let bin = FakeBin::new();
        let opts = DeleteOptions {
            max_item_bytes: 60,
            ..DeleteOptions::default()
        };
        let r = run_delete(
            &s,
            &req(&[&id_of(&s, &root, "dir"), &id_of(&s, &root, "one.tmp")]),
            &bin,
            &opts,
            &AtomicBool::new(false),
            &mut |_, _| {},
        )
        .unwrap();
        assert_eq!(r.deleted, 1, "only the 30 byte file fits");
        assert!(r.failed[0].reason.contains("too large"));
        assert!(root.join("dir").exists());
    }

    #[test]
    fn very_long_paths_are_left_alone() {
        let t = TempDir::new("del-long");
        let mut rel = String::from("root");
        for _ in 0..12 {
            rel.push_str("/segment-with-a-fairly-long-name");
        }
        let file = t.path().join(&rel).join("old.tmp");
        let fresh = t.path().join(&rel).join("new.tmp");
        if fs::create_dir_all(file.parent().unwrap()).is_err()
            || fs::write(&file, b"x").is_err()
            || fs::write(&fresh, b"y").is_err()
        {
            eprintln!("skipped: this machine cannot create paths this long");
            return;
        }
        // the fresh file keeps every folder above from being one item, so old.tmp is listed alone
        set_age(&file, 2 * DAY);
        let root = t.path().join("root");
        let s = scan_root(&root, Some(TEMP_MIN_AGE));
        let Some(it) = s.categories[0].items.first() else {
            eprintln!("skipped: the scan could not reach the long path here");
            return;
        };
        assert!(path_too_long(&it.path));
        let bin = FakeBin::new();
        let r = go(&s, &req(&[&it.id.clone()]), &bin);
        assert_eq!(r.deleted, 0);
        assert!(r.failed[0].reason.contains("too long"), "{:?}", r.failed);
        assert!(bin.attempts.borrow().is_empty());
    }

    #[test]
    fn cancelling_between_batches_stops_cleanly() {
        let t = TempDir::new("del-cancel");
        let root = t.mkdir("root");
        for n in 0..6 {
            t.write_aged(&format!("root/f{n}.tmp"), 10 + n, 2 * DAY);
        }
        let s = scan_root(&root, Some(TEMP_MIN_AGE));
        let bin = FakeBin::new();
        let cancel = AtomicBool::new(false);
        let opts = DeleteOptions {
            batch_size: 2,
            ..DeleteOptions::default()
        };
        let ids: Vec<String> = s.categories[0].items.iter().map(|i| i.id.clone()).collect();
        let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
        let mut seen = Vec::new();
        let r = run_delete(&s, &req(&ids), &bin, &opts, &cancel, &mut |done, total| {
            seen.push((done, total));
            if done >= 2 {
                cancel.store(true, Ordering::Relaxed);
            }
        })
        .unwrap();
        assert!(r.cancelled);
        assert_eq!((r.deleted, r.not_attempted), (2, 4));
        assert_eq!(seen, vec![(0, 6), (2, 6)]);
        assert_eq!(fs::read_dir(&root).unwrap().count(), 4);
    }

    #[test]
    fn each_batch_is_checked_right_before_it_is_moved() {
        let t = TempDir::new("del-lazy");
        let root = t.mkdir("root");
        t.write_aged("root/a.tmp", 200, 2 * DAY);
        t.write_aged("root/b.tmp", 100, 2 * DAY);
        let s = scan_root(&root, Some(TEMP_MIN_AGE));
        let ids: Vec<String> = s.categories[0].items.iter().map(|i| i.id.clone()).collect();
        let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
        let bin = FakeBin::new();
        let opts = DeleteOptions {
            batch_size: 1,
            ..DeleteOptions::default()
        };
        // while the first batch is being reported, an app starts using the second file
        let r = run_delete(
            &s,
            &req(&ids),
            &bin,
            &opts,
            &AtomicBool::new(false),
            &mut |done, _| {
                if done == 1 {
                    set_age(&root.join("b.tmp"), Duration::ZERO);
                }
            },
        )
        .unwrap();
        assert_eq!(r.deleted, 1);
        assert_eq!(r.failed.len(), 1);
        assert!(r.failed[0].path.ends_with("b.tmp"));
        assert!(
            r.failed[0].reason.contains("changed after the scan"),
            "{:?}",
            r.failed
        );
        assert!(root.join("b.tmp").exists() && !root.join("a.tmp").exists());
    }

    #[test]
    fn progress_ends_at_the_total() {
        let (_t, root) = fixture();
        let s = scan_root(&root, Some(TEMP_MIN_AGE));
        let bin = FakeBin::new();
        let opts = DeleteOptions {
            batch_size: 1,
            ..DeleteOptions::default()
        };
        let mut seen = Vec::new();
        let ids: Vec<String> = s.categories[0].items.iter().map(|i| i.id.clone()).collect();
        let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
        run_delete(
            &s,
            &req(&ids),
            &bin,
            &opts,
            &AtomicBool::new(false),
            &mut |d, t| seen.push((d, t)),
        )
        .unwrap();
        assert_eq!(seen, vec![(0, 3), (1, 3), (2, 3), (3, 3)]);
    }

    #[test]
    fn shell_errors_read_as_plain_english() {
        assert!(shell_error_text(0x20).contains("in use"));
        assert!(shell_error_text(0x21).contains("in use"));
        assert!(shell_error_text(0x78).contains("permission"));
        assert!(shell_error_text(0x7C).contains("could not find"));
        assert!(shell_error_text(0x81).contains("too long"));
        assert!(shell_error_text(0x85).contains("too large"));
        assert!(shell_error_text(0x75).contains("cancelled"));
        assert!(shell_error_text(0x402).contains("0x402"));
    }
}
