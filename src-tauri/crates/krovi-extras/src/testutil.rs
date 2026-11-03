//! Test-only helpers. Every test works inside its own uniquely named directory under
//! `std::env::temp_dir()`; nothing outside it is ever created, changed or removed, and no real user
//! folder is ever touched.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

static SEQ: AtomicU64 = AtomicU64::new(0);

pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    pub fn new(tag: &str) -> TempDir {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let name = format!(
            "krovi-extras-test-{tag}-{}-{nanos}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        );
        let path = std::env::temp_dir().join(name);
        fs::create_dir_all(&path).expect("create test dir");
        // Work on the real path so tests do not depend on 8.3 names or link spellings.
        let path = crate::fsx::canonical(&path).unwrap_or(path);
        TempDir { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn mkdir(&self, rel: &str) -> PathBuf {
        let p = self.path.join(rel);
        fs::create_dir_all(&p).expect("mkdir");
        p
    }

    /// Creates `rel` (and its parents) holding `bytes` bytes. The file's age is "just now".
    pub fn write(&self, rel: &str, bytes: usize) -> PathBuf {
        let p = self.path.join(rel);
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).expect("mkdir parent");
        }
        let mut f = fs::File::create(&p).expect("create file");
        f.write_all(&vec![b'k'; bytes]).expect("write");
        p
    }

    /// Like [`TempDir::write`] but with the modification time `age` in the past.
    pub fn write_aged(&self, rel: &str, bytes: usize, age: Duration) -> PathBuf {
        let p = self.write(rel, bytes);
        set_age(&p, age);
        p
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        // Only ever our own scratch directory. `remove_dir_all` does not follow links.
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// Moves a file's modification time to `age` ago.
pub fn set_age(p: &Path, age: Duration) {
    let f = fs::OpenOptions::new()
        .write(true)
        .open(p)
        .expect("open for set_modified");
    f.set_modified(SystemTime::now() - age)
        .expect("set_modified");
}

pub const DAY: Duration = Duration::from_secs(24 * 3600);

/// Creates a directory link at `link` pointing to `target`: a symlink if allowed, otherwise an NTFS
/// junction (`mklink /J`, no privilege needed). Returns false if neither could be made.
pub fn make_dir_link(link: &Path, target: &Path) -> bool {
    #[cfg(windows)]
    {
        if std::os::windows::fs::symlink_dir(target, link).is_ok() {
            return true;
        }
        std::process::Command::new("cmd")
            .arg("/C")
            .arg("mklink")
            .arg("/J")
            .arg(link)
            .arg(target)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link).is_ok()
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = (link, target);
        false
    }
}

/// Removes a link made by [`make_dir_link`] without touching what it points to.
pub fn remove_dir_link(link: &Path) {
    let _ = fs::remove_dir(link).or_else(|_| fs::remove_file(link));
}
