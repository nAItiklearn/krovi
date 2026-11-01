//! Point-and-ask context: what "this" / "it" / "that" refers to. Read ONLY when the user starts a
//! voice command (never continuously), and only the minimum:
//!
//! 1. the foreground app's name (executable name, no window title),
//! 2. the selected text, via UI Automation `TextPattern.GetSelection` on the focused element,
//! 3. the selected items of the foreground Explorer window, via the Shell COM `IShellWindows`
//!    object (paths only),
//! 4. only if the user enabled it in Voice settings and UI Automation found nothing: a brief
//!    Ctrl+C with the previous clipboard restored byte-for-byte (see [`copy_via_clipboard`]).
//!
//! Nothing here stores anything. Password fields are never read, and the clipboard fallback
//! refuses terminals (where Ctrl+C means "interrupt") and any clipboard it cannot restore exactly.

use std::ffi::c_void;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use windows::core::{Interface, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HGLOBAL, HWND};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, IServiceProvider, CLSCTX_ALL,
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData,
    GetClipboardSequenceNumber, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{
    GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE,
};
use windows::Win32::System::Threading::{
    GetCurrentProcessId, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationTextPattern, UIA_TextPatternId,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};
use windows::Win32::UI::Shell::{
    IFolderView2, IShellBrowser, IShellItemArray, IShellWindows, ShellWindows, SIGDN_FILESYSPATH,
    SVGIO_SELECTION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetAncestor, GetClassNameW, GetForegroundWindow, GetWindowThreadProcessId, IsWindowVisible,
    GA_ROOT,
};

use crate::types::{ContextCapture, VoiceContext};

/// Longest selection we will carry (characters). Longer selections are cut, never refused.
pub const MAX_TEXT_CHARS: usize = 20_000;
/// Most Explorer items we will report.
pub const MAX_FILES: usize = 100;

/// The window that had focus when voice started.
#[derive(Debug, Clone)]
pub struct Foreground {
    pub hwnd: isize,
    pub pid: u32,
    /// Executable name without extension, lower-case ("notepad", "explorer", "chrome").
    pub app: String,
    pub class: String,
}

/// Why there is no foreground app to read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForegroundIssue {
    NoWindow,
    /// Krovi itself has focus (the panel is open), so "this" cannot refer to another app.
    IsKrovi,
}

/// What the selection read found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selection {
    Text(String),
    Files(Vec<String>),
    /// A password field was focused: deliberately not read.
    Protected,
    Nothing,
}

/// The outcome of [`capture`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Captured {
    pub context: Option<VoiceContext>,
    /// Plain-language reason there is no usable selection (shown if a command needs one).
    pub hint: Option<String>,
}

// ---------------------------------------------------------------------------------------------
// foreground window
// ---------------------------------------------------------------------------------------------

fn class_name(hwnd: HWND) -> String {
    let mut buf = [0u16; 128];
    // SAFETY: buffer is valid for its length.
    let n = unsafe { GetClassNameW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

fn process_stem(pid: u32) -> Option<String> {
    // SAFETY: standard query of a process image name; the handle is closed below.
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 520];
        let mut len = buf.len() as u32;
        let ok =
            QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len)
                .is_ok();
        let _ = CloseHandle(h);
        if !ok {
            return None;
        }
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        let file = path.rsplit(['\\', '/']).next().unwrap_or(&path);
        Some(
            file.rsplit_once('.')
                .map_or(file, |(stem, _)| stem)
                .to_lowercase(),
        )
    }
}

/// The foreground window's app, unless that is Krovi itself.
pub fn foreground() -> Result<Foreground, ForegroundIssue> {
    // SAFETY: plain window queries.
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return Err(ForegroundIssue::NoWindow);
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return Err(ForegroundIssue::NoWindow);
        }
        if pid == GetCurrentProcessId() {
            return Err(ForegroundIssue::IsKrovi);
        }
        let app = process_stem(pid).unwrap_or_else(|| "unknown".to_string());
        Ok(Foreground {
            hwnd: hwnd.0 as isize,
            pid,
            app,
            class: class_name(hwnd),
        })
    }
}

/// Selected text as the rest of Krovi should see it: `\r\n` and lone `\r` become `\n`, trailing
/// line breaks and spaces (an artefact of how some editors select "to end of line") are dropped,
/// and absurdly long selections are cut at [`MAX_TEXT_CHARS`]. Nothing else is altered.
pub fn normalize_selection(s: &str) -> String {
    let unified = s.replace("\r\n", "\n").replace('\r', "\n");
    let trimmed = unified.trim_end_matches(['\n', ' ', '\t']);
    if trimmed.chars().count() > MAX_TEXT_CHARS {
        trimmed.chars().take(MAX_TEXT_CHARS).collect()
    } else {
        trimmed.to_string()
    }
}

fn cut(s: String) -> String {
    normalize_selection(&s)
}

/// Whether sending Ctrl+C to this app could do harm (terminals interpret it as "interrupt").
pub fn ctrl_c_is_unsafe(app: &str, class: &str) -> bool {
    CTRL_C_UNSAFE_APPS.contains(&app) || CTRL_C_UNSAFE_CLASSES.contains(&class)
}

// ---------------------------------------------------------------------------------------------
// selected text: UI Automation
// ---------------------------------------------------------------------------------------------

/// SAFETY: COM must be initialised on the calling thread.
unsafe fn uia_selected_text(fg: &Foreground) -> Result<Selection, windows::core::Error> {
    let uia: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)?;
    let mut element = uia.GetFocusedElement()?;
    // The focused element must belong to the foreground app (focus may be mid-switch).
    if element.CurrentProcessId()? as u32 != fg.pid {
        return Ok(Selection::Nothing);
    }
    if element.CurrentIsPassword()?.as_bool() {
        return Ok(Selection::Protected);
    }
    // The focused element is usually the text control; web content exposes TextPattern on an
    // ancestor (the document), so look a few levels up.
    let walker = uia.ControlViewWalker()?;
    for _ in 0..5 {
        if let Ok(pattern) =
            element.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId)
        {
            let ranges = pattern.GetSelection()?;
            let mut parts: Vec<String> = Vec::new();
            for i in 0..ranges.Length()? {
                let text = ranges
                    .GetElement(i)?
                    .GetText((MAX_TEXT_CHARS + 1) as i32)?
                    .to_string();
                if !text.is_empty() {
                    parts.push(text);
                }
            }
            let joined = parts.join("\n");
            return Ok(if joined.trim().is_empty() {
                Selection::Nothing
            } else {
                Selection::Text(cut(joined))
            });
        }
        match walker.GetParentElement(&element) {
            Ok(parent) => element = parent,
            Err(_) => break,
        }
    }
    Ok(Selection::Nothing)
}

// ---------------------------------------------------------------------------------------------
// selected Explorer items: Shell COM
// ---------------------------------------------------------------------------------------------

/// SAFETY: COM must be initialised on the calling thread.
unsafe fn explorer_selected_paths(fg: &Foreground) -> Result<Vec<String>, windows::core::Error> {
    let windows: IShellWindows = CoCreateInstance(&ShellWindows, None, CLSCTX_ALL)?;
    let count = windows.Count()?;
    for i in 0..count {
        let Ok(dispatch) = windows.Item(&windows::core::VARIANT::from(i)) else {
            continue;
        };
        let Ok(provider) = dispatch.cast::<IServiceProvider>() else {
            continue;
        };
        // SID_STopLevelBrowser
        let sid = windows::core::GUID::from_u128(0x4C96BE40_915C_11CF_99D3_00AA004AE837);
        let Ok(browser) = provider.QueryService::<IShellBrowser>(&sid) else {
            continue;
        };
        let Ok(tab) = browser.GetWindow() else {
            continue;
        };
        // The tab belonging to the foreground window, and the active one (Windows 11 tabs).
        if GetAncestor(tab, GA_ROOT).0 as isize != fg.hwnd || !IsWindowVisible(tab).as_bool() {
            continue;
        }
        let view = browser.QueryActiveShellView()?;
        let folder: IFolderView2 = view.cast()?;
        let items: IShellItemArray = folder.Items(SVGIO_SELECTION)?;
        let mut paths = Vec::new();
        for k in 0..items.GetCount()?.min(MAX_FILES as u32) {
            let item = items.GetItemAt(k)?;
            if let Ok(name) = item.GetDisplayName(SIGDN_FILESYSPATH) {
                paths.push(name.to_string().unwrap_or_default());
                CoTaskMemFree(Some(name.0 as *const c_void));
            }
        }
        paths.retain(|p| !p.is_empty());
        return Ok(paths);
    }
    Ok(Vec::new())
}

/// Read the selection in the foreground app (UI Automation / Explorer). Runs on a short-lived
/// STA thread with a timeout, so a hung target app can never hang Krovi.
pub fn read_selection(fg: &Foreground) -> Selection {
    let fg = fg.clone();
    let (tx, rx) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("krovi-voice-context".into())
        .spawn(move || {
            // SAFETY: COM is initialised for this thread and released before it ends.
            let result = unsafe {
                let inited = CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok();
                let selection = if fg.app == "explorer" {
                    match explorer_selected_paths(&fg) {
                        Ok(paths) if !paths.is_empty() => Selection::Files(paths),
                        _ => Selection::Nothing,
                    }
                } else {
                    uia_selected_text(&fg).unwrap_or(Selection::Nothing)
                };
                if inited {
                    CoUninitialize();
                }
                selection
            };
            let _ = tx.send(result);
        });
    if spawned.is_err() {
        return Selection::Nothing;
    }
    rx.recv_timeout(Duration::from_millis(2500))
        .unwrap_or(Selection::Nothing)
}

// ---------------------------------------------------------------------------------------------
// the opt-in clipboard fallback
// ---------------------------------------------------------------------------------------------

/// Apps where Ctrl+C means "interrupt", not "copy": never send it there.
const CTRL_C_UNSAFE_APPS: &[&str] = &[
    "cmd",
    "powershell",
    "pwsh",
    "windowsterminal",
    "wt",
    "conhost",
    "openconsole",
    "mintty",
    "putty",
    "bash",
    "wsl",
    "alacritty",
    "wezterm-gui",
    "kitty",
    "hyper",
    "tabby",
    "ubuntu",
    "git-bash",
    "sh",
];
const CTRL_C_UNSAFE_CLASSES: &[&str] = &[
    "ConsoleWindowClass",
    "CASCADIA_HOSTING_WINDOW_CLASS",
    "mintty",
    "PuTTY",
    "VirtualConsoleClass",
];

const CF_UNICODETEXT: u32 = 13;
const MAX_SNAPSHOT_BYTES: usize = 64 * 1024 * 1024;

struct Snapshot {
    formats: Vec<(u32, Vec<u8>)>,
}

/// Copy every clipboard format as raw bytes, or refuse (`None`) if any cannot be reproduced
/// exactly: a restore must be complete or not happen at all.
///
/// SAFETY: the clipboard must be open on this thread.
unsafe fn snapshot_open_clipboard() -> Option<Snapshot> {
    let mut seen: Vec<u32> = Vec::new();
    let mut fmt = 0u32;
    loop {
        fmt = EnumClipboardFormats(fmt);
        if fmt == 0 {
            break;
        }
        seen.push(fmt);
    }
    let has_dib = seen.iter().any(|f| *f == 8 || *f == 17); // CF_DIB / CF_DIBV5
    let mut formats = Vec::new();
    let mut total = 0usize;
    for f in seen {
        match f {
            // GDI objects the OS synthesises from a DIB; restoring the DIB restores them too.
            2 | 9 | 14 | 3 if has_dib => continue,
            // CF_BITMAP, CF_PALETTE, CF_METAFILEPICT, CF_ENHMETAFILE, owner-display / DSP formats and
            // the GDI-object private ranges are handles, not bytes: cannot be copied safely.
            2 | 3 | 9 | 14 | 0x80..=0x8F | 0x300..=0x3FF => return None,
            _ => {}
        }
        let handle = GetClipboardData(f).ok()?;
        if handle.is_invalid() {
            return None;
        }
        let hglobal = HGLOBAL(handle.0);
        let size = GlobalSize(hglobal);
        if size == 0 {
            return None; // not a plain memory block (e.g. a delayed OLE stream)
        }
        total += size;
        if total > MAX_SNAPSHOT_BYTES {
            return None;
        }
        let ptr = GlobalLock(hglobal) as *const u8;
        if ptr.is_null() {
            return None;
        }
        let bytes = std::slice::from_raw_parts(ptr, size).to_vec();
        let _ = GlobalUnlock(hglobal);
        formats.push((f, bytes));
    }
    Some(Snapshot { formats })
}

/// SAFETY: the clipboard must be open on this thread.
unsafe fn restore_open_clipboard(snapshot: &Snapshot) -> bool {
    if EmptyClipboard().is_err() {
        return false;
    }
    let mut ok = true;
    for (fmt, bytes) in &snapshot.formats {
        let Ok(mem) = GlobalAlloc(GMEM_MOVEABLE, bytes.len().max(1)) else {
            ok = false;
            continue;
        };
        let dst = GlobalLock(mem) as *mut u8;
        if dst.is_null() {
            ok = false;
            continue;
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), dst, bytes.len());
        let _ = GlobalUnlock(mem);
        if SetClipboardData(*fmt, HANDLE(mem.0)).is_err() {
            ok = false;
        }
    }
    ok
}

fn open_clipboard_retry() -> bool {
    for _ in 0..10 {
        // SAFETY: opens the clipboard for the current task; paired with CloseClipboard.
        if unsafe { OpenClipboard(HWND::default()) }.is_ok() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

fn modifiers_down() -> bool {
    [VK_CONTROL, VK_MENU, VK_SHIFT, VK_LWIN, VK_RWIN]
        .iter()
        .any(|k| unsafe { GetAsyncKeyState(i32::from(k.0)) } < 0)
}

fn key(vk: VIRTUAL_KEY, up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: if up {
                    KEYEVENTF_KEYUP
                } else {
                    KEYBD_EVENT_FLAGS(0)
                },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

/// Briefly copy the selection with Ctrl+C and put the previous clipboard back exactly.
///
/// This is the *fallback*, used only when the user enabled it AND UI Automation found nothing.
/// It refuses (returns `Err`/`Ok(None)` without touching anything) when: the app is a terminal, a
/// modifier key is still held, the focus moved, or the existing clipboard contains something that
/// cannot be restored byte-for-byte (images as bare GDI handles, delayed/virtual files, > 64 MB).
/// Callers must hold Krovi's clipboard I/O lock and acknowledge the new change token afterwards
/// so the clipboard watcher does not record the transient copy.
///
/// Caveat that no code can remove: if Windows clipboard history is on, Windows itself may record
/// the transient copy. This is why the fallback is off by default.
pub fn copy_via_clipboard(fg: &Foreground) -> Result<Option<String>, String> {
    if ctrl_c_is_unsafe(&fg.app, &fg.class) {
        return Err(format!(
            "{} is a terminal; Ctrl+C would interrupt it, so it was not used",
            fg.app
        ));
    }
    // Let go of the global shortcut keys first, or Ctrl+Alt+C would be sent.
    let deadline = Instant::now() + Duration::from_millis(700);
    while modifiers_down() {
        if Instant::now() > deadline {
            return Err("a modifier key is still held down".into());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    // SAFETY: window/clipboard/input calls on this thread; every opened handle is closed.
    unsafe {
        if GetForegroundWindow().0 as isize != fg.hwnd {
            return Err("focus moved to another window".into());
        }
        if !open_clipboard_retry() {
            return Err("the clipboard is busy".into());
        }
        let snapshot = snapshot_open_clipboard();
        let _ = CloseClipboard();
        let Some(snapshot) = snapshot else {
            return Err("the clipboard holds something that cannot be restored exactly, so it was left alone".into());
        };

        let before = GetClipboardSequenceNumber();
        let inputs = [
            key(VK_CONTROL, false),
            key(VIRTUAL_KEY(0x43), false),
            key(VIRTUAL_KEY(0x43), true),
            key(VK_CONTROL, true),
        ];
        if SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) != inputs.len() as u32 {
            return Err("could not send Ctrl+C".into());
        }
        // Wait for the app to put the selection on the clipboard.
        let wait_until = Instant::now() + Duration::from_millis(600);
        while GetClipboardSequenceNumber() == before && Instant::now() < wait_until {
            std::thread::sleep(Duration::from_millis(15));
        }
        if GetClipboardSequenceNumber() == before {
            return Ok(None); // nothing was selected, nothing changed
        }

        // Read the copied text, then restore, even if reading failed.
        let mut text = None;
        if open_clipboard_retry() {
            if let Ok(h) = GetClipboardData(CF_UNICODETEXT) {
                let hglobal = HGLOBAL(h.0);
                let p = GlobalLock(hglobal) as *const u16;
                if !p.is_null() {
                    let units = GlobalSize(hglobal) / 2;
                    let slice = std::slice::from_raw_parts(p, units);
                    let end = slice.iter().position(|c| *c == 0).unwrap_or(units);
                    text = Some(String::from_utf16_lossy(&slice[..end]));
                    let _ = GlobalUnlock(hglobal);
                }
            }
            let restored = restore_open_clipboard(&snapshot);
            let _ = CloseClipboard();
            if !restored {
                return Err("the previous clipboard could not be fully restored".into());
            }
        } else {
            return Err(
                "the clipboard stayed busy, so the previous contents could not be restored".into(),
            );
        }
        Ok(text.map(cut).filter(|t| !t.trim().is_empty()))
    }
}

// ---------------------------------------------------------------------------------------------
// the whole capture
// ---------------------------------------------------------------------------------------------

/// Capture the context for a voice command that is starting NOW.
///
/// `level` comes from the user's Voice settings. `fallback` is called with the foreground window
/// only when the level allows the clipboard fallback and UI Automation found no text; the caller
/// supplies it so it can take Krovi's clipboard lock around [`copy_via_clipboard`].
pub fn capture(
    level: ContextCapture,
    fallback: impl FnOnce(&Foreground) -> Option<String>,
) -> Captured {
    if level == ContextCapture::Off {
        return Captured {
            context: None,
            hint: Some("Reading the selection is turned off in Voice settings.".into()),
        };
    }
    let fg = match foreground() {
        Ok(fg) => fg,
        Err(ForegroundIssue::IsKrovi) => {
            return Captured {
                context: None,
                hint: Some("Krovi was in front, so it could not see your selection. Start voice from the pill or the shortcut while the other app is focused.".into()),
            }
        }
        Err(ForegroundIssue::NoWindow) => return Captured { context: None, hint: Some("No app had focus, so there was no selection to read.".into()) },
    };
    let app = Some(fg.app.clone());
    let mut selection = read_selection(&fg);
    if matches!(selection, Selection::Nothing)
        && level == ContextCapture::SelectionClipboardFallback
        && fg.app != "explorer"
    {
        if let Some(text) = fallback(&fg) {
            selection = Selection::Text(text);
        }
    }
    match selection {
        Selection::Text(value) => Captured {
            context: Some(VoiceContext::Text { value, app }),
            hint: None,
        },
        Selection::Files(value) => Captured {
            context: Some(VoiceContext::Files { value, app }),
            hint: None,
        },
        Selection::Protected => Captured {
            context: Some(VoiceContext::App { value: fg.app }),
            hint: Some("That is a password field, so Krovi did not read it.".into()),
        },
        Selection::Nothing => Captured {
            context: Some(VoiceContext::App { value: fg.app }),
            hint: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_text_is_normalised_but_not_rewritten() {
        assert_eq!(
            normalize_selection("Remember to renew the passport\r"),
            "Remember to renew the passport"
        );
        assert_eq!(
            normalize_selection("line one\r\nline two\r\n"),
            "line one\nline two"
        );
        assert_eq!(
            normalize_selection("  keep leading  space \n\n"),
            "  keep leading  space"
        );
        assert_eq!(
            normalize_selection("um the the original"),
            "um the the original",
            "words are never cleaned"
        );
        assert_eq!(normalize_selection(""), "");
        let long = "x".repeat(MAX_TEXT_CHARS + 500);
        assert_eq!(normalize_selection(&long).chars().count(), MAX_TEXT_CHARS);
    }

    #[test]
    fn terminals_are_refused_for_the_clipboard_fallback() {
        for app in [
            "cmd",
            "powershell",
            "pwsh",
            "windowsterminal",
            "wt",
            "conhost",
            "mintty",
            "putty",
        ] {
            assert!(ctrl_c_is_unsafe(app, "SomeClass"), "{app}");
        }
        for class in ["ConsoleWindowClass", "CASCADIA_HOSTING_WINDOW_CLASS"] {
            assert!(ctrl_c_is_unsafe("anything", class), "{class}");
        }
        for app in ["notepad", "chrome", "winword", "code", "msedge"] {
            assert!(!ctrl_c_is_unsafe(app, "Chrome_WidgetWin_1"), "{app}");
        }
    }

    #[test]
    fn capture_off_reads_nothing_and_says_why() {
        let c = capture(ContextCapture::Off, |_| {
            panic!("fallback must not run when capture is off")
        });
        assert_eq!(c.context, None);
        assert!(c.hint.unwrap().contains("turned off"));
    }

    /// Reads one clipboard format as raw bytes.
    unsafe fn read_format(fmt: u32) -> Option<Vec<u8>> {
        let h = GetClipboardData(fmt).ok()?;
        let g = HGLOBAL(h.0);
        let size = GlobalSize(g);
        let p = GlobalLock(g) as *const u8;
        if p.is_null() {
            return None;
        }
        let v = std::slice::from_raw_parts(p, size).to_vec();
        let _ = GlobalUnlock(g);
        Some(v)
    }

    /// Puts `bytes` on the (open) clipboard as `fmt`.
    unsafe fn put_format(fmt: u32, bytes: &[u8]) {
        let mem = GlobalAlloc(GMEM_MOVEABLE, bytes.len()).unwrap();
        let dst = GlobalLock(mem) as *mut u8;
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), dst, bytes.len());
        let _ = GlobalUnlock(mem);
        SetClipboardData(fmt, HANDLE(mem.0)).unwrap();
    }

    /// Overwrites the REAL clipboard, so it only runs on request:
    /// `cargo test -p krovi-voice --features native -- --ignored clipboard_snapshot`
    #[test]
    #[ignore = "overwrites the real clipboard"]
    fn clipboard_snapshot_restore_is_byte_exact() {
        // a 4x4 24-bit DIB (BITMAPINFOHEADER + bottom-up rows padded to 4 bytes)
        let mut dib = Vec::new();
        dib.extend_from_slice(&40u32.to_le_bytes()); // biSize
        dib.extend_from_slice(&4i32.to_le_bytes()); // width
        dib.extend_from_slice(&4i32.to_le_bytes()); // height (bottom-up)
        dib.extend_from_slice(&1u16.to_le_bytes()); // planes
        dib.extend_from_slice(&24u16.to_le_bytes()); // bpp
        dib.extend_from_slice(&[0u8; 24]); // compression .. clrImportant
        for row in 0..4u8 {
            for col in 0..4u8 {
                dib.extend_from_slice(&[col * 40, row * 40, 200]);
            }
        }
        let text: Vec<u8> = "snapshot me"
            .encode_utf16()
            .chain([0])
            .flat_map(|c| c.to_le_bytes())
            .collect();
        let other: Vec<u8> = "something else"
            .encode_utf16()
            .chain([0])
            .flat_map(|c| c.to_le_bytes())
            .collect();

        unsafe {
            assert!(open_clipboard_retry());
            EmptyClipboard().unwrap();
            put_format(8, &dib); // CF_DIB
            put_format(CF_UNICODETEXT, &text);
            let _ = CloseClipboard();

            assert!(open_clipboard_retry());
            let snap = snapshot_open_clipboard().expect("snapshot");
            let _ = CloseClipboard();
            let fmts: Vec<u32> = snap.formats.iter().map(|(f, _)| *f).collect();
            println!("snapshotted formats: {fmts:?}");

            // something else lands on the clipboard (the Ctrl+C)
            assert!(open_clipboard_retry());
            EmptyClipboard().unwrap();
            put_format(CF_UNICODETEXT, &other);
            let _ = CloseClipboard();

            assert!(open_clipboard_retry());
            assert!(restore_open_clipboard(&snap));
            let _ = CloseClipboard();

            assert!(open_clipboard_retry());
            for (fmt, want) in [(8u32, &dib), (CF_UNICODETEXT, &text)] {
                let got = read_format(fmt).unwrap_or_default();
                // GlobalSize may round up; the payload must match exactly up to the original length
                assert!(
                    got.len() >= want.len(),
                    "format {fmt}: restored {} bytes, want {}",
                    got.len(),
                    want.len()
                );
                assert_eq!(
                    &got[..want.len()],
                    &want[..],
                    "format {fmt} differs after restore"
                );
            }
            let _ = EmptyClipboard();
            let _ = CloseClipboard();
        }
    }
    #[test]
    fn the_foreground_app_has_a_name_or_a_reason() {
        match foreground() {
            Ok(fg) => assert!(!fg.app.is_empty() && fg.pid != 0),
            Err(e) => assert!(matches!(
                e,
                ForegroundIssue::NoWindow | ForegroundIssue::IsKrovi
            )),
        }
    }
}
