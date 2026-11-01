//! Filenames: sanitising what a phone sends us, picking collision-safe names in the
//! receive folder, and encoding names in headers/HTML. Pure except `reserve_unique`.

use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

const MAX_NAME_CHARS: usize = 120;
const MAX_EXT_CHARS: usize = 16;

const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Turn an untrusted client-provided name into a single safe Windows file name:
/// no directories (so no `..` traversal), no reserved characters or device names, no
/// control/bidi characters (the "RLO" extension-spoofing trick), no trailing dots/spaces,
/// bounded length. Never returns an empty string.
pub fn sanitize_filename(raw: &str) -> String {
    // Only the last path component survives: "..\..\evil.exe" -> "evil.exe".
    let base = raw.rsplit(['/', '\\']).next().unwrap_or("");
    let cleaned: String = base
        .chars()
        .filter_map(|c| match c {
            c if c.is_control() => None,
            '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2066}'..='\u{2069}'
            | '\u{FEFF}' => None,
            '<' | '>' | ':' | '"' | '|' | '?' | '*' => Some('_'),
            c => Some(c),
        })
        .collect();
    // Windows silently strips trailing dots and spaces, which could re-create a dangerous name.
    let mut s = cleaned.trim().trim_end_matches(['.', ' ']).to_string();
    if s.is_empty() || s.chars().all(|c| c == '.') {
        s = "file".to_string();
    }
    // CON, NUL, COM1... are device names with or without an extension.
    let stem_upper = s
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end()
        .to_ascii_uppercase();
    if RESERVED.contains(&stem_upper.as_str()) {
        s.insert(0, '_');
    }
    clamp_len(&s)
}

fn clamp_len(s: &str) -> String {
    if s.chars().count() <= MAX_NAME_CHARS {
        return s.to_string();
    }
    let (stem, ext) = split_ext(s);
    let ext: String = ext.chars().take(MAX_EXT_CHARS).collect();
    let keep = MAX_NAME_CHARS.saturating_sub(ext.chars().count());
    let stem: String = stem.chars().take(keep).collect();
    let out = format!("{}{}", stem.trim_end_matches(['.', ' ']), ext);
    if out.is_empty() {
        "file".to_string()
    } else {
        out
    }
}

/// `("report", ".pdf")`, `("archive.tar", ".gz")`, `(".bashrc", "")`, `("README", "")`.
pub fn split_ext(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(i) if i > 0 && i + 1 < name.len() => (&name[..i], &name[i..]),
        _ => (name, ""),
    }
}

/// `report (2).pdf` for n = 2; n = 0 is the name itself.
pub fn numbered(name: &str, n: u32) -> String {
    if n == 0 {
        return name.to_string();
    }
    let (stem, ext) = split_ext(name);
    format!("{stem} ({n}){ext}")
}

/// Atomically claim a file name in `dir`: finds the first of `name`, `name (1)`, `name (2)`...
/// that does not exist and creates its `.part` sibling with `create_new` (so two concurrent
/// uploads can never pick the same name). Returns the open part file, its path, and the
/// final path/name to rename to once the upload completes.
pub struct Reservation {
    pub file: File,
    pub part_path: PathBuf,
    pub final_path: PathBuf,
    pub final_name: String,
}

pub fn reserve_unique(dir: &Path, name: &str) -> io::Result<Reservation> {
    for n in 0..10_000u32 {
        let candidate = numbered(name, n);
        let final_path = dir.join(&candidate);
        if final_path.exists() {
            continue;
        }
        let part_path = dir.join(format!("{candidate}.part"));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&part_path)
        {
            Ok(file) => {
                // Re-check: a file with the final name may have appeared in between.
                if final_path.exists() {
                    drop(file);
                    let _ = std::fs::remove_file(&part_path);
                    continue;
                }
                return Ok(Reservation {
                    file,
                    part_path,
                    final_path,
                    final_name: candidate,
                });
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "too many files with the same name",
    ))
}

// ---------------------------------------------------------------------------
// Percent-encoding and header/HTML escaping
// ---------------------------------------------------------------------------

/// Decode `%XX` (and `+` only when `plus_is_space`). `None` on malformed input or non-UTF-8.
pub fn percent_decode(s: &str, plus_is_space: bool) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' => {
                let hi = hex_val(*b.get(i + 1)?)?;
                let lo = hex_val(*b.get(i + 2)?)?;
                out.push(hi * 16 + lo);
                i += 3;
            }
            b'+' if plus_is_space => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

fn hex_val(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// RFC 5987 `attr-char` percent-encoding for `filename*=UTF-8''...`.
pub fn percent_encode_attr(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for &b in s.as_bytes() {
        if b.is_ascii_alphanumeric() || b"!#$&+-.^_`|~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// `Content-Disposition: attachment` with an ASCII fallback and the exact UTF-8 name.
pub fn content_disposition(name: &str) -> String {
    let ascii: String = name
        .chars()
        .map(|c| {
            if c.is_ascii() && !c.is_ascii_control() && c != '"' && c != '\\' && c != '%' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!(
        "attachment; filename=\"{ascii}\"; filename*=UTF-8''{}",
        percent_encode_attr(name)
    )
}

pub fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_directories_and_traversal() {
        assert_eq!(sanitize_filename("../../etc/passwd"), "passwd");
        assert_eq!(
            sanitize_filename("..\\..\\Windows\\System32\\evil.dll"),
            "evil.dll"
        );
        assert_eq!(sanitize_filename("C:\\Users\\x\\a.txt"), "a.txt");
        assert_eq!(sanitize_filename("/abs/path/file.png"), "file.png");
        assert_eq!(sanitize_filename(".."), "file");
        assert_eq!(sanitize_filename("..."), "file");
        assert_eq!(sanitize_filename("a/"), "file");
    }

    #[test]
    fn replaces_reserved_characters_and_streams() {
        assert_eq!(
            sanitize_filename("a<b>c:d\"e|f?g*h.txt"),
            "a_b_c_d_e_f_g_h.txt"
        );
        // NTFS alternate data stream syntax must not survive.
        assert_eq!(sanitize_filename("file.txt:secret"), "file.txt_secret");
    }

    #[test]
    fn handles_windows_device_names() {
        assert_eq!(sanitize_filename("CON"), "_CON");
        assert_eq!(sanitize_filename("nul.txt"), "_nul.txt");
        assert_eq!(sanitize_filename("Com1.tar.gz"), "_Com1.tar.gz");
        assert_eq!(sanitize_filename("LPT9"), "_LPT9");
        assert_eq!(sanitize_filename("console.log"), "console.log");
        assert_eq!(sanitize_filename("CON .txt"), "_CON .txt");
    }

    #[test]
    fn trims_trailing_dots_and_spaces() {
        assert_eq!(sanitize_filename("report.pdf. . "), "report.pdf");
        assert_eq!(sanitize_filename("  spaced name.txt  "), "spaced name.txt");
    }

    #[test]
    fn removes_control_and_bidi_characters() {
        // U+202E (right-to-left override) makes "exe.txt" display as "txt.exe".
        assert_eq!(sanitize_filename("photo\u{202E}gpj.exe"), "photogpj.exe");
        assert_eq!(sanitize_filename("a\u{0000}b\nc.txt"), "abc.txt");
        assert_eq!(sanitize_filename("zero\u{200B}width.txt"), "zerowidth.txt");
    }

    #[test]
    fn keeps_unicode_and_dotfiles() {
        assert_eq!(sanitize_filename("résumé 日本語.pdf"), "résumé 日本語.pdf");
        assert_eq!(sanitize_filename(".gitignore"), ".gitignore");
    }

    #[test]
    fn empty_becomes_file() {
        assert_eq!(sanitize_filename(""), "file");
        assert_eq!(sanitize_filename("   "), "file");
        assert_eq!(sanitize_filename("<>"), "__");
    }

    #[test]
    fn clamps_long_names_but_keeps_extension() {
        let long = format!("{}.jpeg", "x".repeat(400));
        let out = sanitize_filename(&long);
        assert!(out.chars().count() <= MAX_NAME_CHARS);
        assert!(out.ends_with(".jpeg"));
        let no_ext = "y".repeat(400);
        assert_eq!(sanitize_filename(&no_ext).chars().count(), MAX_NAME_CHARS);
    }

    #[test]
    fn numbering_inserts_before_the_extension() {
        assert_eq!(numbered("a.pdf", 0), "a.pdf");
        assert_eq!(numbered("a.pdf", 2), "a (2).pdf");
        assert_eq!(numbered("archive.tar.gz", 1), "archive.tar (1).gz");
        assert_eq!(numbered("README", 3), "README (3)");
        assert_eq!(numbered(".bashrc", 1), ".bashrc (1)");
    }

    #[test]
    fn reserve_unique_never_collides() {
        let dir =
            std::env::temp_dir().join(format!("krovi-names-{}", crate::token::share_id().unwrap()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), b"existing").unwrap();
        let r1 = reserve_unique(&dir, "a.txt").unwrap();
        assert_eq!(r1.final_name, "a (1).txt");
        // a second concurrent reservation must take the next number even before r1 finishes
        let r2 = reserve_unique(&dir, "a.txt").unwrap();
        assert_eq!(r2.final_name, "a (2).txt");
        assert!(r1.part_path.exists() && r2.part_path.exists());
        assert_ne!(r1.final_path, r2.final_path);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn percent_decode_roundtrip_and_errors() {
        assert_eq!(percent_decode("a%20b%2Fc", false).unwrap(), "a b/c");
        assert_eq!(percent_decode("a+b", true).unwrap(), "a b");
        assert_eq!(percent_decode("a+b", false).unwrap(), "a+b");
        assert_eq!(percent_decode("%E6%97%A5", false).unwrap(), "日");
        assert!(percent_decode("%zz", false).is_none());
        assert!(percent_decode("%4", false).is_none());
        assert!(percent_decode("%FF%FE", false).is_none()); // not UTF-8
    }

    #[test]
    fn content_disposition_is_ascii_safe() {
        let h = content_disposition("日本 \"x\".pdf");
        assert!(h.is_ascii());
        assert!(h.starts_with("attachment; filename=\""));
        assert!(h.contains("filename*=UTF-8''%E6%97%A5%E6%9C%AC%20%22x%22.pdf"));
        assert!(!h.contains("\"x\""));
    }

    #[test]
    fn html_escape_covers_the_dangerous_five() {
        assert_eq!(
            html_escape("<a href=\"x\" onclick='y'>&"),
            "&lt;a href=&quot;x&quot; onclick=&#39;y&#39;&gt;&amp;"
        );
    }
}
