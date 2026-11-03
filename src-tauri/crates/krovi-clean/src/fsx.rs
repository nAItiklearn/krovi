//! Small, pure-ish file-system helpers shared by the scanner and the delete step.
//!
//! The rule that matters most lives here: [`is_reparse`]. A reparse point (symlink, junction,
//! OneDrive placeholder, ...) is never followed, never counted and never deleted.

use std::fs::Metadata;
use std::io;
use std::path::{Component, Path, PathBuf, Prefix};
use std::time::{SystemTime, UNIX_EPOCH};

/// `FILE_ATTRIBUTE_REPARSE_POINT`.
pub const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;

/// True when `meta` (from `symlink_metadata` or `DirEntry::metadata`, which never follow links)
/// describes a symlink, a junction or any other reparse point.
pub fn is_reparse(meta: &Metadata) -> bool {
    if meta.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return true;
        }
    }
    false
}

/// Modification time in epoch milliseconds, if the file system reports one.
pub fn modified_ms(meta: &Metadata) -> Option<u64> {
    let t = meta.modified().ok()?;
    Some(t.duration_since(UNIX_EPOCH).ok()?.as_millis() as u64)
}

/// `SystemTime` -> epoch milliseconds (0 before 1970).
pub fn system_ms(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Turns a `\\?\C:\x` style path (what `canonicalize` returns on Windows) back into `C:\x`, and
/// `\\?\UNC\srv\share\x` into `\\srv\share\x`. The Windows shell file operations do not accept
/// the verbatim form. Anything else is returned unchanged.
pub fn simplify_verbatim(p: &Path) -> PathBuf {
    let mut comps = p.components();
    let head = match comps.next() {
        Some(Component::Prefix(pre)) => pre,
        _ => return p.to_path_buf(),
    };
    let mut out = match head.kind() {
        Prefix::VerbatimDisk(letter) => PathBuf::from(format!("{}:", letter as char)),
        Prefix::VerbatimUNC(server, share) => PathBuf::from(format!(
            r"\\{}\{}",
            server.to_string_lossy(),
            share.to_string_lossy()
        )),
        _ => return p.to_path_buf(),
    };
    for c in comps {
        match c {
            Component::RootDir => out.push(r"\"),
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// `fs::canonicalize` without the Windows verbatim prefix.
pub fn canonical(p: &Path) -> io::Result<PathBuf> {
    Ok(simplify_verbatim(&std::fs::canonicalize(p)?))
}

/// Components of a path as comparable strings (case-insensitive on Windows, where the file
/// system is).
fn keys(p: &Path) -> Vec<String> {
    p.components()
        .map(|c| {
            let s = c.as_os_str().to_string_lossy();
            if cfg!(windows) {
                s.to_lowercase()
            } else {
                s.into_owned()
            }
        })
        .collect()
}

/// Same path, comparing whole components (so `C:\Temp2` is not `C:\Temp`).
pub fn same_path(a: &Path, b: &Path) -> bool {
    keys(a) == keys(b)
}

/// `child` is strictly below `root` (equal paths are NOT inside), compared component-wise.
pub fn inside_strict(child: &Path, root: &Path) -> bool {
    let c = keys(child);
    let r = keys(root);
    c.len() > r.len() && c[..r.len()] == r[..]
}

/// `a` is `b` or lies below `b`.
pub fn inside_or_same(a: &Path, b: &Path) -> bool {
    let (x, y) = (keys(a), keys(b));
    x.len() >= y.len() && x[..y.len()] == y[..]
}

/// Number of real folder/file names in the path (`C:\` has 0, `C:\Temp` has 1).
pub fn normal_depth(p: &Path) -> usize {
    p.components()
        .filter(|c| matches!(c, Component::Normal(_)))
        .count()
}

/// Windows shell operations stop working (and may delete outright instead of recycling) near
/// MAX_PATH. Anything this long is left alone.
pub const MAX_RECYCLE_PATH_UTF16: usize = 259;

pub fn path_too_long(p: &Path) -> bool {
    p.to_string_lossy().encode_utf16().count() > MAX_RECYCLE_PATH_UTF16
}

/// `*` and `?` are wildcards to the shell file operations; a real NTFS name never has them, so
/// seeing one means something is wrong.
pub fn has_wildcard(p: &Path) -> bool {
    let s = p.to_string_lossy();
    s.contains('*') || s.contains('?') || s.contains('\0')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{make_dir_link, TempDir};

    #[test]
    fn inside_is_component_wise_and_strict() {
        let root = Path::new(if cfg!(windows) { r"C:\Temp" } else { "/tmp/t" });
        let kid = root.join("a").join("b");
        let sibling = Path::new(if cfg!(windows) {
            r"C:\Temp2\a"
        } else {
            "/tmp/t2/a"
        });
        assert!(inside_strict(&kid, root));
        assert!(
            !inside_strict(root, root),
            "the root itself is not inside itself"
        );
        assert!(
            !inside_strict(sibling, root),
            "a name that merely starts with the root's name"
        );
        assert!(inside_or_same(root, root));
        assert!(inside_or_same(&kid, root));
        assert!(!inside_or_same(root, &kid));
    }

    #[cfg(windows)]
    #[test]
    fn comparison_ignores_case_on_windows() {
        assert!(inside_strict(
            Path::new(r"C:\USERS\Me\x"),
            Path::new(r"c:\users\me")
        ));
        assert!(same_path(Path::new(r"C:\Temp"), Path::new(r"c:\TEMP")));
    }

    #[cfg(windows)]
    #[test]
    fn verbatim_prefix_is_removed() {
        assert_eq!(
            simplify_verbatim(Path::new(r"\\?\C:\Users\me\a.txt")),
            PathBuf::from(r"C:\Users\me\a.txt")
        );
        assert_eq!(
            simplify_verbatim(Path::new(r"\\?\UNC\srv\share\dir\f")),
            PathBuf::from(r"\\srv\share\dir\f")
        );
        assert_eq!(
            simplify_verbatim(Path::new(r"C:\plain")),
            PathBuf::from(r"C:\plain")
        );
        assert_eq!(
            simplify_verbatim(Path::new(r"relative\path")),
            PathBuf::from(r"relative\path")
        );
    }

    #[test]
    fn canonical_has_no_verbatim_prefix_and_resolves_dotdot() {
        let t = TempDir::new("canon");
        t.mkdir("a/b");
        let p = t.path().join("a").join("b").join("..").join("b");
        let c = canonical(&p).unwrap();
        assert!(!c.to_string_lossy().starts_with(r"\\?\"), "{c:?}");
        assert!(same_path(
            &c,
            &canonical(&t.path().join("a").join("b")).unwrap()
        ));
    }

    #[test]
    fn depth_counts_names_only() {
        assert_eq!(
            normal_depth(Path::new(if cfg!(windows) { r"C:\" } else { "/" })),
            0
        );
        assert_eq!(
            normal_depth(Path::new(if cfg!(windows) { r"C:\a\b" } else { "/a/b" })),
            2
        );
    }

    #[test]
    fn long_paths_and_wildcards_are_flagged() {
        let long = format!("C:\\{}", "x".repeat(300));
        assert!(path_too_long(Path::new(&long)));
        assert!(!path_too_long(Path::new(r"C:\short\file.tmp")));
        assert!(has_wildcard(Path::new(r"C:\dir\*.tmp")));
        assert!(has_wildcard(Path::new(r"C:\dir\a?.tmp")));
        assert!(!has_wildcard(Path::new(r"C:\dir\a.tmp")));
    }

    #[test]
    fn regular_entries_are_not_reparse_points() {
        let t = TempDir::new("reparse-no");
        let f = t.write("f.txt", 3);
        let d = t.mkdir("d");
        assert!(!is_reparse(&std::fs::symlink_metadata(&f).unwrap()));
        assert!(!is_reparse(&std::fs::symlink_metadata(&d).unwrap()));
    }

    #[test]
    fn a_directory_link_is_a_reparse_point() {
        let t = TempDir::new("reparse-yes");
        let target = TempDir::new("reparse-target");
        let link = t.path().join("link");
        if !make_dir_link(&link, target.path()) {
            eprintln!("skipped: this machine does not allow creating directory links");
            return;
        }
        let meta = std::fs::symlink_metadata(&link).unwrap();
        assert!(is_reparse(&meta), "junctions and symlinks must be detected");
        // the link must never be mistaken for a plain directory by a following stat
        assert!(std::fs::metadata(&link).unwrap().is_dir());
    }
}
