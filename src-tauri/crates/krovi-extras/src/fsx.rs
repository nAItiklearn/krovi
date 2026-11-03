//! File-system helpers shared by the organizer and the file-name search.
//!
//! The link and path rules are Clean My OS's, re-exported so there is ONE definition of "do not
//! follow": a reparse point (symlink, junction, OneDrive placeholder, ...) is never followed,
//! moved or searched, and every path is compared component by component.

use std::fs::Metadata;
use std::io;

pub use krovi_clean::fsx::{
    canonical, has_wildcard, inside_or_same, inside_strict, is_reparse, modified_ms, normal_depth,
    path_too_long, same_path,
};

/// `FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM`.
const HIDDEN_OR_SYSTEM: u32 = 0x2 | 0x4;

/// Hidden or system entries (desktop.ini, thumbs.db, `$RECYCLE.BIN`, ...). On other platforms a
/// leading dot counts as hidden, so the tests mean the same thing everywhere.
pub fn is_hidden_or_system(meta: &Metadata, name: &str) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        let _ = name;
        meta.file_attributes() & HIDDEN_OR_SYSTEM != 0
    }
    #[cfg(not(windows))]
    {
        let _ = (meta, HIDDEN_OR_SYSTEM);
        name.starts_with('.')
    }
}

/// Download managers and browsers write `name.crdownload`, `name.part`, ... while a file is still
/// arriving. Moving one would break the download, so they are never touched.
pub fn is_unfinished_download(name: &str) -> bool {
    let n = name.to_lowercase();
    [
        "crdownload",
        "part",
        "partial",
        "download",
        "tmp",
        "opdownload",
    ]
    .iter()
    .any(|e| n.ends_with(&format!(".{e}")))
}

/// Types that run code when opened. Search results of these types can be shown in their folder
/// but are never opened by Krovi.
pub fn is_risky_to_open(name: &str) -> bool {
    let n = name.to_lowercase();
    const RISKY: &[&str] = &[
        "exe",
        "msi",
        "msp",
        "bat",
        "cmd",
        "com",
        "scr",
        "pif",
        "ps1",
        "psm1",
        "vbs",
        "vbe",
        "js",
        "jse",
        "wsf",
        "wsh",
        "hta",
        "jar",
        "reg",
        "lnk",
        "url",
        "cpl",
        "msc",
        "dll",
        "sys",
        "appx",
        "msix",
        "application",
        "gadget",
        "inf",
        "scf",
    ];
    RISKY.iter().any(|e| n.ends_with(&format!(".{e}")))
}

/// A sentence for the user from an I/O error, never the raw OS text.
pub fn io_reason(e: &io::Error) -> String {
    match e.raw_os_error() {
        // ERROR_SHARING_VIOLATION, ERROR_LOCK_VIOLATION
        Some(32) | Some(33) if cfg!(windows) => "It is in use by another program.".into(),
        // ERROR_NOT_SAME_DEVICE
        Some(17) if cfg!(windows) => {
            "It is on a different drive than the destination, and Krovi only moves within one drive.".into()
        }
        // ERROR_PATH_NOT_FOUND / ERROR_FILE_NOT_FOUND
        Some(2) | Some(3) if cfg!(windows) => "Windows could not find it.".into(),
        _ => match e.kind() {
            io::ErrorKind::NotFound => "Windows could not find it.".into(),
            io::ErrorKind::PermissionDenied => {
                "Windows would not allow it (no permission, or it is in use).".into()
            }
            io::ErrorKind::AlreadyExists => "A file with that name already exists.".into(),
            _ => "Windows could not move it.".into(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unfinished_downloads_are_recognised_case_insensitively() {
        assert!(is_unfinished_download("movie.mkv.crdownload"));
        assert!(is_unfinished_download("ISO.PART"));
        assert!(is_unfinished_download("x.tmp"));
        assert!(!is_unfinished_download("report.pdf"));
        assert!(!is_unfinished_download("partial-notes.txt"));
    }

    #[test]
    fn programs_and_scripts_are_risky_to_open_documents_are_not() {
        for n in [
            "setup.exe",
            "run.BAT",
            "x.ps1",
            "link.lnk",
            "a.js",
            "b.msi",
            "c.hta",
        ] {
            assert!(is_risky_to_open(n), "{n}");
        }
        for n in [
            "notes.txt",
            "photo.jpg",
            "report.pdf",
            "data.csv",
            "exe",
            "noext",
        ] {
            assert!(!is_risky_to_open(n), "{n}");
        }
    }

    #[test]
    fn io_errors_read_as_sentences() {
        let e = io::Error::new(io::ErrorKind::PermissionDenied, "x");
        assert!(io_reason(&e).contains("permission"));
        let e = io::Error::new(io::ErrorKind::NotFound, "x");
        assert!(io_reason(&e).contains("could not find"));
        let e = io::Error::new(io::ErrorKind::AlreadyExists, "x");
        assert!(io_reason(&e).contains("already exists"));
    }
}
