//! "What's in my Downloads folder": a read-only summary of ONE directory (not recursive), using
//! only `std::fs`. Counts, total size of the files directly inside, and the newest entries.
//!
//! Hidden and system entries (desktop.ini, thumbs.db, ...) are skipped, matching what Explorer
//! shows by default. Scanning stops after [`SCAN_CAP`] entries so a pathological folder cannot
//! stall the app; the summary then says "at least".

use std::path::Path;
use std::time::SystemTime;

use crate::types::{ResultItem, ResultKind};

/// Newest entries returned.
pub const MAX_ENTRIES: usize = 50;
/// Directory entries examined before giving up on the count.
pub const SCAN_CAP: usize = 20_000;

#[derive(Debug, Clone, PartialEq)]
pub struct Listing {
    pub files: usize,
    pub folders: usize,
    /// Total bytes of the files directly inside.
    pub bytes: u64,
    /// True when the scan hit [`SCAN_CAP`]: the counts are "at least".
    pub truncated: bool,
    /// Newest first, at most [`MAX_ENTRIES`].
    pub newest: Vec<Entry>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub bytes: u64,
    pub modified: Option<SystemTime>,
}

#[cfg(windows)]
fn is_hidden(meta: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const HIDDEN: u32 = 0x2;
    const SYSTEM: u32 = 0x4;
    meta.file_attributes() & (HIDDEN | SYSTEM) != 0
}

#[cfg(not(windows))]
fn is_hidden(_meta: &std::fs::Metadata) -> bool {
    false
}

pub fn list_folder(path: &Path) -> std::io::Result<Listing> {
    let mut files = 0;
    let mut folders = 0;
    let mut bytes = 0u64;
    let mut truncated = false;
    let mut all: Vec<Entry> = Vec::new();
    for (seen, item) in std::fs::read_dir(path)?.enumerate() {
        if seen >= SCAN_CAP {
            truncated = true;
            break;
        }
        let Ok(item) = item else { continue };
        let Ok(meta) = item.metadata() else { continue };
        if is_hidden(&meta) {
            continue;
        }
        let is_dir = meta.is_dir();
        if is_dir {
            folders += 1;
        } else {
            files += 1;
            bytes += meta.len();
        }
        all.push(Entry {
            name: item.file_name().to_string_lossy().into_owned(),
            path: item.path().to_string_lossy().into_owned(),
            is_dir,
            bytes: if is_dir { 0 } else { meta.len() },
            modified: meta.modified().ok(),
        });
    }
    // newest first; entries without a timestamp last; ties by name for a stable order
    all.sort_by(|a, b| {
        b.modified
            .cmp(&a.modified)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    all.truncate(MAX_ENTRIES);
    Ok(Listing {
        files,
        folders,
        bytes,
        truncated,
        newest: all,
    })
}

/// "1.2 GB", "340 KB", "12 bytes".
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if bytes < 1024 {
        return format!("{bytes} byte{}", if bytes == 1 { "" } else { "s" });
    }
    let mut v = bytes as f64 / 1024.0;
    let mut unit = 0;
    while v >= 1024.0 && unit + 1 < UNITS.len() {
        v /= 1024.0;
        unit += 1;
    }
    if v >= 100.0 {
        format!("{:.0} {}", v, UNITS[unit])
    } else {
        format!("{:.1} {}", v, UNITS[unit])
    }
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// The one-line answer: "Downloads: 48 items (31 files, 17 folders), 1.2 GB of files. Newest: a, b, c."
pub fn summarize(name: &str, l: &Listing) -> String {
    let total = l.files + l.folders;
    if total == 0 {
        return format!("{name} is empty.");
    }
    let at_least = if l.truncated { "at least " } else { "" };
    let newest: Vec<&str> = l.newest.iter().take(3).map(|e| e.name.as_str()).collect();
    format!(
        "{name}: {at_least}{} ({}, {}), {} of files. Newest: {}.",
        plural(total, "item", "items"),
        plural(l.files, "file", "files"),
        plural(l.folders, "folder", "folders"),
        human_size(l.bytes),
        newest.join(", ")
    )
}

/// The rows shown in the panel for a listing, with a modified-time detail.
pub fn rows(l: &Listing) -> Vec<ResultItem> {
    l.newest
        .iter()
        .map(|e| {
            let when = e.modified.and_then(crate::civil::describe_system_time);
            let detail = match (e.is_dir, when) {
                (true, Some(w)) => format!("folder, {w}"),
                (true, None) => "folder".to_string(),
                (false, Some(w)) => format!("{}, {w}", human_size(e.bytes)),
                (false, None) => human_size(e.bytes),
            };
            ResultItem {
                kind: if e.is_dir {
                    ResultKind::Folder
                } else {
                    ResultKind::File
                },
                name: e.name.clone(),
                path: e.path.clone(),
                detail: Some(detail),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::Duration;

    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("krovi-listing-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn touch(path: &Path, bytes: usize, age_secs: u64) {
        fs::write(path, vec![b'x'; bytes]).unwrap();
        let t = SystemTime::now() - Duration::from_secs(age_secs);
        fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(t)
            .unwrap();
    }

    #[test]
    fn counts_sizes_and_newest_first() {
        let dir = scratch("basic");
        touch(&dir.join("old.txt"), 100, 3000);
        touch(&dir.join("mid.txt"), 2048, 2000);
        touch(&dir.join("new.bin"), 1_048_576, 10);
        fs::create_dir(dir.join("sub")).unwrap();
        fs::write(dir.join("sub").join("deep.txt"), "not counted").unwrap();
        let l = list_folder(&dir).unwrap();
        assert_eq!((l.files, l.folders), (3, 1));
        assert_eq!(
            l.bytes,
            100 + 2048 + 1_048_576,
            "files directly inside only"
        );
        assert!(!l.truncated);
        // "sub" was created just now, so it is the newest; the files follow by age
        let order: Vec<&str> = l.newest.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(order, ["sub", "new.bin", "mid.txt", "old.txt"]);
        let s = summarize("Downloads", &l);
        assert!(
            s.starts_with("Downloads: 4 items (3 files, 1 folder), 1.0 MB of files. Newest:"),
            "{s}"
        );
        let r = rows(&l);
        assert_eq!(r[0].kind, ResultKind::Folder);
        assert!(r[0].detail.as_deref().unwrap().starts_with("folder"));
        assert_eq!(r[1].kind, ResultKind::File);
        assert!(r[1].detail.as_deref().unwrap().starts_with("1.0 MB"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn caps_at_fifty_entries() {
        let dir = scratch("cap");
        for i in 0..60 {
            fs::write(dir.join(format!("f{i:02}.txt")), "x").unwrap();
        }
        let l = list_folder(&dir).unwrap();
        assert_eq!(l.files, 60);
        assert_eq!(l.newest.len(), MAX_ENTRIES);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn empty_missing_and_sizes() {
        let dir = scratch("empty");
        let l = list_folder(&dir).unwrap();
        assert_eq!(summarize("Desktop", &l), "Desktop is empty.");
        assert!(list_folder(&dir.join("nope")).is_err());
        assert_eq!(human_size(0), "0 bytes");
        assert_eq!(human_size(1), "1 byte");
        assert_eq!(human_size(1536), "1.5 KB");
        assert_eq!(human_size(5 * 1024 * 1024 * 1024), "5.0 GB");
        assert_eq!(human_size(300 * 1024), "300 KB");
        let _ = fs::remove_dir_all(&dir);
    }
}
