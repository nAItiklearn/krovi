//! OS clipboard watching (history capture) and write-back.
//!
//! Windows increments a *clipboard sequence number* on every change, so a watcher
//! can poll [`change_token`] (a few nanoseconds) and only read the clipboard when
//! it moves. The decision logic — which tokens to read, how long to retry a
//! clipboard another process still holds, how to classify and bound the text — is
//! pure ([`ClipGate`], [`classify_text`], [`prepare_text`], [`files_to_text`]) and
//! unit-tested. The OS calls (`GetClipboardSequenceNumber`, `arboard`, the
//! password-manager opt-out formats) sit behind `#[cfg(all(windows, feature =
//! "native"))]` in [`native`].
//!
//! ## Privacy
//! Content flagged by the app that produced it as *not for history* is never read
//! into Krovi: the registered formats `ExcludeClipboardContentFromMonitorProcessing`
//! (presence) and `CanIncludeInClipboardHistory` (DWORD `0`) are what password
//! managers set on a copied secret. [`read_capture`] returns
//! [`ReadOutcome::Sensitive`] *before touching the data*.

/// Text longer than this is not captured (a copied multi-megabyte log would bloat
/// the history database and the UI event payloads).
pub const MAX_TEXT_BYTES: usize = 512 * 1024;
/// Raw RGBA above this (≈ 40 megapixels) is thumbnailed but its full-size copy is
/// not retained for copy-back.
pub const MAX_FULL_IMAGE_BYTES: usize = 160 * 1024 * 1024;
/// A busy clipboard is retried on this many consecutive polls before giving up on
/// that change.
pub const MAX_BUSY_RETRIES: u8 = 4;

/// What was captured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureKind {
    Text,
    Link,
    Image,
}

/// One captured clipboard item, ready for the history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipCapture {
    pub kind: CaptureKind,
    /// Text / URL, or for images a `data:image/png;base64,…` thumbnail (≤ ~200 KB).
    pub content: String,
    /// Full-resolution PNG of an image capture, for lossless copy-back. `None` for
    /// text, or when the image was too large to retain.
    pub full_png: Option<Vec<u8>>,
}

/// Result of one attempt to read the clipboard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadOutcome {
    Capture(ClipCapture),
    /// The source flagged the content as sensitive (password manager). Nothing was read.
    Sensitive,
    /// Nothing Krovi stores (empty, whitespace-only, oversize, or an unsupported format).
    Empty,
    /// The clipboard was held by another process; worth retrying shortly.
    Busy,
}

// ---------------------------------------------------------------------------
// Pure logic
// ---------------------------------------------------------------------------

/// Decides when the clipboard should be (re)read, from the sequence-number stream.
///
/// * a token never seen before → read;
/// * the read succeeded or yielded nothing storable → remember the token;
/// * the clipboard was busy → retry on the next polls, up to [`MAX_BUSY_RETRIES`],
///   then give up on that token (so a clipboard owner that never releases it cannot
///   make the watcher spin);
/// * Krovi's own write → [`ClipGate::acknowledge`] the post-write token so the
///   copy-back is not re-ingested as a duplicate.
#[derive(Debug, Clone)]
pub struct ClipGate {
    seen: Option<u64>,
    busy_tries: u8,
}

impl ClipGate {
    /// `seen = Some(token)` treats the current clipboard as already handled (use at
    /// startup when the history is non-empty); `None` ingests whatever is there.
    pub fn new(seen: Option<u64>) -> Self {
        Self {
            seen,
            busy_tries: 0,
        }
    }

    /// Whether `token` calls for a read. Token `0` means "unavailable" and never does.
    pub fn should_read(&self, token: u64) -> bool {
        token != 0 && self.seen != Some(token)
    }

    /// Record the outcome of reading at `token`; returns `true` if the token is now
    /// considered handled (i.e. not a retry).
    pub fn record(&mut self, token: u64, outcome: &ReadOutcome) -> bool {
        match outcome {
            ReadOutcome::Busy => {
                self.busy_tries = self.busy_tries.saturating_add(1);
                if self.busy_tries >= MAX_BUSY_RETRIES {
                    self.seen = Some(token);
                    self.busy_tries = 0;
                    true
                } else {
                    false
                }
            }
            _ => {
                self.seen = Some(token);
                self.busy_tries = 0;
                true
            }
        }
    }

    /// Mark `token` as produced by Krovi itself.
    pub fn acknowledge(&mut self, token: u64) {
        self.seen = Some(token);
        self.busy_tries = 0;
    }

    /// The last handled token, if any.
    pub fn seen(&self) -> Option<u64> {
        self.seen
    }
}

/// `true` for a single token that looks like a link: `http(s)://`, `ftp(s)://`,
/// `mailto:` or a bare `www.` host. Whitespace anywhere disqualifies it (a sentence
/// that *contains* a URL is text).
pub fn is_link(s: &str) -> bool {
    let t = s.trim();
    if t.is_empty() || t.chars().any(char::is_whitespace) {
        return false;
    }
    let lower = t.to_ascii_lowercase();
    for scheme in ["http://", "https://", "ftp://", "ftps://", "mailto:"] {
        if let Some(rest) = lower.strip_prefix(scheme) {
            return !rest.is_empty();
        }
    }
    lower
        .strip_prefix("www.")
        .is_some_and(|rest| rest.contains('.') && !rest.starts_with('.'))
}

/// Link or plain text.
pub fn classify_text(s: &str) -> CaptureKind {
    if is_link(s) {
        CaptureKind::Link
    } else {
        CaptureKind::Text
    }
}

/// Bound and normalize copied text: `None` if it is empty / whitespace-only or
/// larger than [`MAX_TEXT_BYTES`]. The text itself is kept verbatim (a copied code
/// block keeps its indentation) except that a pure URL loses surrounding whitespace.
pub fn prepare_text(s: &str) -> Option<String> {
    if s.trim().is_empty() || s.len() > MAX_TEXT_BYTES {
        return None;
    }
    if is_link(s) {
        Some(s.trim().to_string())
    } else {
        Some(s.to_string())
    }
}

/// A copied file list as history text: one path per line.
pub fn files_to_text<S: AsRef<str>>(paths: &[S]) -> Option<String> {
    let lines: Vec<&str> = paths
        .iter()
        .map(|p| p.as_ref().trim())
        .filter(|p| !p.is_empty())
        .collect();
    if lines.is_empty() {
        None
    } else {
        prepare_text(&lines.join("\n"))
    }
}

// ---------------------------------------------------------------------------
// OS surface (Windows + `native`)
// ---------------------------------------------------------------------------

/// The Windows clipboard sequence number (changes on every clipboard change), or
/// `0` when unavailable. Cheap enough to poll every few hundred ms.
#[cfg(all(windows, feature = "native"))]
pub fn change_token() -> u64 {
    native::change_token()
}

/// See the native variant. Without `native` there is no clipboard.
#[cfg(not(all(windows, feature = "native")))]
pub fn change_token() -> u64 {
    0
}

/// Whether the current clipboard content is flagged sensitive by its source.
#[cfg(all(windows, feature = "native"))]
pub fn is_sensitive() -> bool {
    native::is_sensitive()
}

/// See the native variant.
#[cfg(not(all(windows, feature = "native")))]
pub fn is_sensitive() -> bool {
    false
}

/// Read the current clipboard into a storable capture (text → link/text, file list
/// → text, image → thumbnail + full PNG). Sensitive content is skipped unread.
#[cfg(all(windows, feature = "native"))]
pub fn read_capture() -> ReadOutcome {
    native::read_capture()
}

/// See the native variant.
#[cfg(not(all(windows, feature = "native")))]
pub fn read_capture() -> ReadOutcome {
    ReadOutcome::Empty
}

/// Replace the OS clipboard with `text`.
#[cfg(all(windows, feature = "native"))]
pub fn write_text(text: &str) -> crate::Result<()> {
    native::write_text(text)
}

/// See the native variant.
#[cfg(not(all(windows, feature = "native")))]
pub fn write_text(text: &str) -> crate::Result<()> {
    let _ = text;
    Err(crate::SysError::Unsupported)
}

/// Replace the OS clipboard with the image in `png`.
#[cfg(all(windows, feature = "native"))]
pub fn write_image_png(png: &[u8]) -> crate::Result<()> {
    native::write_image_png(png)
}

/// See the native variant.
#[cfg(not(all(windows, feature = "native")))]
pub fn write_image_png(png: &[u8]) -> crate::Result<()> {
    let _ = png;
    Err(crate::SysError::Unsupported)
}

#[cfg(all(windows, feature = "native"))]
mod native {
    use super::{
        classify_text, files_to_text, prepare_text, CaptureKind, ClipCapture, ReadOutcome,
        MAX_FULL_IMAGE_BYTES,
    };
    use crate::imaging;
    use crate::SysError;
    use windows::core::w;
    use windows::Win32::Foundation::{HGLOBAL, HWND};
    use windows::Win32::System::DataExchange::{
        CloseClipboard, GetClipboardData, GetClipboardSequenceNumber, IsClipboardFormatAvailable,
        OpenClipboard, RegisterClipboardFormatW,
    };
    use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};

    fn err<E: std::fmt::Display>(e: E) -> SysError {
        SysError::Message(e.to_string())
    }

    pub(super) fn change_token() -> u64 {
        u64::from(unsafe { GetClipboardSequenceNumber() })
    }

    /// Read a `DWORD`-valued registered format (e.g. `CanIncludeInClipboardHistory`).
    /// The clipboard may be held by another process, so open it with a short retry.
    unsafe fn read_dword(format: u32) -> Option<u32> {
        let mut opened = false;
        for _ in 0..5 {
            if OpenClipboard(HWND::default()).is_ok() {
                opened = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        if !opened {
            return None;
        }
        let mut value = None;
        if let Ok(h) = GetClipboardData(format) {
            let hg = HGLOBAL(h.0);
            if GlobalSize(hg) >= 4 {
                let p = GlobalLock(hg) as *const u32;
                if !p.is_null() {
                    value = Some(p.read_unaligned());
                    let _ = GlobalUnlock(hg);
                }
            }
        }
        let _ = CloseClipboard();
        value
    }

    pub(super) fn is_sensitive() -> bool {
        unsafe {
            // Presence alone means "exclude from monitoring" (KeePass, Bitwarden, 1Password...).
            let exclude =
                RegisterClipboardFormatW(w!("ExcludeClipboardContentFromMonitorProcessing"));
            if exclude != 0 && IsClipboardFormatAvailable(exclude).is_ok() {
                return true;
            }
            // DWORD 0 = "do not include in Win+V history".
            let can_include = RegisterClipboardFormatW(w!("CanIncludeInClipboardHistory"));
            if can_include != 0 && IsClipboardFormatAvailable(can_include).is_ok() {
                return read_dword(can_include) == Some(0);
            }
            false
        }
    }

    pub(super) fn read_capture() -> ReadOutcome {
        if is_sensitive() {
            return ReadOutcome::Sensitive;
        }
        let Ok(mut cb) = arboard::Clipboard::new() else {
            return ReadOutcome::Busy;
        };

        match cb.get_text() {
            Ok(text) => {
                if let Some(content) = prepare_text(&text) {
                    return ReadOutcome::Capture(ClipCapture {
                        kind: classify_text(&content),
                        content,
                        full_png: None,
                    });
                }
            }
            Err(arboard::Error::ClipboardOccupied) => return ReadOutcome::Busy,
            Err(_) => {}
        }

        match cb.get().file_list() {
            Ok(files) if !files.is_empty() => {
                let paths: Vec<String> = files
                    .iter()
                    .map(|p| p.to_string_lossy().into_owned())
                    .collect();
                return match files_to_text(&paths) {
                    Some(content) => ReadOutcome::Capture(ClipCapture {
                        kind: CaptureKind::Text,
                        content,
                        full_png: None,
                    }),
                    None => ReadOutcome::Empty,
                };
            }
            Err(arboard::Error::ClipboardOccupied) => return ReadOutcome::Busy,
            _ => {}
        }

        match cb.get_image() {
            Ok(img) => {
                let (w, h) = (img.width as u32, img.height as u32);
                let Some(thumb) = imaging::make_thumbnail(
                    w,
                    h,
                    &img.bytes,
                    imaging::THUMB_MAX_EDGE,
                    imaging::THUMB_CAP_BYTES,
                ) else {
                    return ReadOutcome::Empty;
                };
                let full_png = if img.bytes.len() <= MAX_FULL_IMAGE_BYTES {
                    imaging::encode_png(w, h, &img.bytes)
                } else {
                    None
                };
                ReadOutcome::Capture(ClipCapture {
                    kind: CaptureKind::Image,
                    content: imaging::data_url("image/png", &thumb.png),
                    full_png,
                })
            }
            Err(arboard::Error::ClipboardOccupied) => ReadOutcome::Busy,
            Err(_) => ReadOutcome::Empty,
        }
    }

    pub(super) fn write_text(text: &str) -> crate::Result<()> {
        let mut cb = arboard::Clipboard::new().map_err(err)?;
        cb.set_text(text.to_owned()).map_err(err)
    }

    pub(super) fn write_image_png(png: &[u8]) -> crate::Result<()> {
        let (w, h, rgba) =
            imaging::decode_png(png).ok_or_else(|| SysError::Message("bad image".into()))?;
        let mut cb = arboard::Clipboard::new().map_err(err)?;
        cb.set_image(arboard::ImageData {
            width: w as usize,
            height: h as usize,
            bytes: rgba.into(),
        })
        .map_err(err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate_reads_each_new_token_once() {
        let mut g = ClipGate::new(Some(10));
        assert!(!g.should_read(10)); // already handled at startup
        assert!(g.should_read(11));
        assert!(g.record(11, &ReadOutcome::Empty));
        assert!(!g.should_read(11));
        assert!(g.should_read(12));
    }

    #[test]
    fn gate_without_a_seen_token_ingests_the_current_clipboard() {
        let g = ClipGate::new(None);
        assert!(g.should_read(5));
        assert!(!g.should_read(0)); // 0 = unavailable
    }

    #[test]
    fn gate_retries_a_busy_clipboard_then_gives_up() {
        let mut g = ClipGate::new(Some(1));
        for _ in 0..(MAX_BUSY_RETRIES - 1) {
            assert!(!g.record(2, &ReadOutcome::Busy));
            assert!(g.should_read(2)); // still worth another go
        }
        assert!(g.record(2, &ReadOutcome::Busy)); // budget exhausted
        assert!(!g.should_read(2));
    }

    #[test]
    fn busy_counter_resets_after_a_good_read() {
        let mut g = ClipGate::new(Some(1));
        g.record(2, &ReadOutcome::Busy);
        g.record(2, &ReadOutcome::Sensitive);
        assert!(!g.should_read(2));
        // a later busy streak gets a fresh budget
        for _ in 0..(MAX_BUSY_RETRIES - 1) {
            assert!(!g.record(3, &ReadOutcome::Busy));
        }
    }

    #[test]
    fn sensitive_tokens_are_handled_not_retried() {
        let mut g = ClipGate::new(None);
        assert!(g.should_read(7));
        assert!(g.record(7, &ReadOutcome::Sensitive));
        assert!(!g.should_read(7));
    }

    #[test]
    fn own_write_is_acknowledged_so_it_is_not_reingested() {
        let mut g = ClipGate::new(Some(40));
        // Krovi writes the clipboard; the OS bumps the token to 41.
        g.acknowledge(41);
        assert!(!g.should_read(41));
        // The next external copy (42) is still seen.
        assert!(g.should_read(42));
        assert_eq!(g.seen(), Some(41));
    }

    #[test]
    fn link_detection() {
        for yes in [
            "https://krovi.tech",
            "  http://x.y/z?a=1  ",
            "HTTPS://EXAMPLE.COM",
            "ftp://files.example.org/a",
            "mailto:a@b.co",
            "www.example.com",
        ] {
            assert!(is_link(yes), "{yes:?} should be a link");
            assert_eq!(classify_text(yes), CaptureKind::Link);
        }
        for no in [
            "",
            "   ",
            "https://",
            "http://has space.com",
            "see https://krovi.tech for details",
            "www.",
            "www.nodot",
            "plain text",
            "line1\nhttps://x.y",
            "C:\\Users\\me\\file.txt",
        ] {
            assert!(!is_link(no), "{no:?} should not be a link");
            assert_eq!(classify_text(no), CaptureKind::Text);
        }
    }

    #[test]
    fn prepare_text_bounds_and_preserves() {
        assert_eq!(prepare_text(""), None);
        assert_eq!(prepare_text("  \n\t "), None);
        // verbatim for text (indentation survives)
        assert_eq!(
            prepare_text("  fn x() {}\n"),
            Some("  fn x() {}\n".to_string())
        );
        // a bare URL is trimmed
        assert_eq!(
            prepare_text("  https://a.b \n"),
            Some("https://a.b".to_string())
        );
        let huge = "x".repeat(MAX_TEXT_BYTES + 1);
        assert_eq!(prepare_text(&huge), None);
        assert!(prepare_text(&"x".repeat(MAX_TEXT_BYTES)).is_some());
    }

    #[test]
    fn file_lists_become_one_path_per_line() {
        let paths = vec![
            r"C:\a\b.txt".to_string(),
            "  ".to_string(),
            r"C:\a\c d.png".to_string(),
        ];
        assert_eq!(
            files_to_text(&paths),
            Some("C:\\a\\b.txt\nC:\\a\\c d.png".to_string())
        );
        assert_eq!(files_to_text::<String>(&[]), None);
        assert_eq!(files_to_text(&["  "]), None);
    }

    #[cfg(not(all(windows, feature = "native")))]
    #[test]
    fn os_calls_degrade_without_native() {
        assert_eq!(change_token(), 0);
        assert!(!is_sensitive());
        assert_eq!(read_capture(), ReadOutcome::Empty);
        assert!(matches!(write_text("x"), Err(crate::SysError::Unsupported)));
        assert!(matches!(
            write_image_png(&[]),
            Err(crate::SysError::Unsupported)
        ));
    }
}
