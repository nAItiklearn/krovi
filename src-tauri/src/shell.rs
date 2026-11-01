//! Island shell (owner: panel-shell): the pill/panel window model, tray, global
//! shortcut, single-instance, click-through and fullscreen auto-hide.
//! See docs/CONTRACTS.md §1.
//!
//! Design rules (these exist because v0.1 shipped a window nobody could close):
//!
//! * **Rust owns the truth.** The webview is only a renderer. Every Rust-initiated
//!   transition (shortcut, tray, focus loss, second launch) goes through
//!   [`request`], which asks the UI to animate (`shell-request`) and then *forces*
//!   the mode after [`WATCHDOG`] if the UI did not answer. A blank, crashed or
//!   still-loading frontend can therefore never leave the window stuck.
//! * **The way out never depends on the UI.** Tray -> "Quit Krovi", the global
//!   shortcut and click-outside are all handled here in Rust.
//! * **Lock discipline.** `Shell::st` is only ever held for short field reads/writes,
//!   never across a window call, so no thread can deadlock against the main thread.
//!   Every window mutation runs on the main thread ([`on_main`]).

use std::{
    fs,
    path::PathBuf,
    str::FromStr,
    sync::{
        atomic::{AtomicBool, AtomicIsize, Ordering},
        Mutex, MutexGuard, OnceLock,
    },
    thread::{self, ThreadId},
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::{
    image::Image,
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, Window, WindowEvent, Wry,
};
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

// ---------------------------------------------------------------------------
// Geometry constants. MUST match src/lib/shell.ts and docs/CONTRACTS.md §1.
// ---------------------------------------------------------------------------

/// Pill window, logical px.
pub const PILL_WIN: (f64, f64) = (440.0, 88.0);
/// Panel window, logical px.
pub const PANEL_WIN: (f64, f64) = (960.0, 690.0);
/// Distance of the shared top-centre anchor below the work-area top edge, logical px.
const ISLAND_TOP: f64 = 6.0;
/// Headroom between the window edge and the visible shape, logical px.
const ISLAND_PAD: f64 = 10.0;
/// Largest visible pill (default click-through rect until the UI reports its own).
const PILL_VIS: (f64, f64) = (400.0, 44.0);
/// Visible panel (default click-through rect until the UI reports its own).
const PANEL_VIS: (f64, f64) = (900.0, 620.0);

/// If the UI has not applied a requested mode within this long, Rust applies it itself.
const WATCHDOG: Duration = Duration::from_millis(1500);
/// Click-through / focus sampler period (~30 Hz).
const SAMPLE: Duration = Duration::from_millis(33);
/// After a panel opens, ignore blur / outside-click for this long (focus is settling).
const PANEL_GRACE: Duration = Duration::from_millis(350);
/// `shell_hold(true)` is a lease: a UI that crashes while holding can't disable
/// collapse-on-blur forever.
const HOLD_LEASE: Duration = Duration::from_secs(300);
/// Tray id (also used to remove the icon on quit so no ghost icon is left behind).
const TRAY_ID: &str = "krovi";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Pill,
    Panel,
    Hidden,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Action {
    Expand,
    Collapse,
    Toggle,
}

/// Visible-shape bounding rect, logical px, relative to the window's top-left.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct HitRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// Persisted in `app_config_dir()/shell.json`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ShellConfig {
    pub shortcut: String,
    pub show_pill: bool,
    pub collapse_on_blur: bool,
    pub launch_at_login: bool,
    pub hide_in_fullscreen: bool,
}

impl Default for ShellConfig {
    fn default() -> Self {
        Self {
            shortcut: "Ctrl+Alt+Space".into(),
            show_pill: true,
            collapse_on_blur: true,
            launch_at_login: false,
            hide_in_fullscreen: true,
        }
    }
}

struct St {
    /// The mode currently applied to the window (after normalisation).
    mode: Mode,
    cfg: ShellConfig,
    hold_since: Option<Instant>,
    /// UI-reported visible rect; `None` => contract default for the mode.
    hit: Option<HitRect>,
    /// Rust-initiated request awaiting the UI: (target, sequence).
    pending: Option<(Mode, u64)>,
    seq: u64,
    panel_since: Instant,
    /// When the last mode transition was applied (the sampler leaves geometry alone
    /// while one is in flight).
    last_apply: Instant,
    /// Pill hidden because a fullscreen app is in the foreground.
    fs_hidden: bool,
    /// Currently registered global shortcut.
    shortcut: Option<Shortcut>,
    shortcut_error: Option<String>,
    last_toggle: Instant,
    /// Last value handed to `set_ignore_cursor_events` (only call it on transitions).
    ignoring: bool,
    prev_btn: bool,
    /// Has our window held the foreground since the panel opened?
    had_focus: bool,
    /// Window that was foreground before the panel took focus; refocused on collapse.
    prev_fg: isize,
}

pub struct Shell {
    st: Mutex<St>,
    cfg_path: Mutex<Option<PathBuf>>,
    check_item: Mutex<Option<CheckMenuItem<Wry>>>,
    quitting: AtomicBool,
}

impl Shell {
    fn new() -> Self {
        Self {
            st: Mutex::new(St {
                mode: Mode::Pill,
                cfg: ShellConfig::default(),
                hold_since: None,
                hit: None,
                pending: None,
                seq: 0,
                panel_since: Instant::now(),
                last_apply: Instant::now(),
                fs_hidden: false,
                shortcut: None,
                shortcut_error: None,
                last_toggle: Instant::now() - Duration::from_secs(10),
                ignoring: false,
                prev_btn: false,
                had_focus: false,
                prev_fg: 0,
            }),
            cfg_path: Mutex::new(None),
            check_item: Mutex::new(None),
            quitting: AtomicBool::new(false),
        }
    }

    fn lk(&self) -> MutexGuard<'_, St> {
        self.st.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl St {
    fn hold_active(&self) -> bool {
        self.hold_since.is_some_and(|t| t.elapsed() < HOLD_LEASE)
    }
}

static MAIN_THREAD: OnceLock<ThreadId> = OnceLock::new();
static HWND: AtomicIsize = AtomicIsize::new(0);

fn is_main() -> bool {
    MAIN_THREAD
        .get()
        .is_some_and(|t| *t == thread::current().id())
}

/// Run `f` on the main thread: inline if we're already there, queued otherwise.
/// All window mutations go through here so they are serialised and can never
/// deadlock against the event loop.
fn on_main(app: &AppHandle, f: impl FnOnce() + Send + 'static) {
    if is_main() {
        f();
    } else {
        let _ = app.run_on_main_thread(f);
    }
}

/// Opt-in diagnostics: set `KROVI_SHELL_LOG=1` to append to `%TEMP%\krovi-shell.log`.
fn log(msg: impl AsRef<str>) {
    static LOG: OnceLock<Option<PathBuf>> = OnceLock::new();
    let path = LOG.get_or_init(|| {
        std::env::var_os("KROVI_SHELL_LOG").map(|_| std::env::temp_dir().join("krovi-shell.log"))
    });
    if let Some(p) = path {
        use std::io::Write;
        if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(p) {
            let ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() % 1_000_000)
                .unwrap_or(0);
            let _ = writeln!(f, "[{ms:>6}] {}", msg.as_ref());
        }
    }
}

// ---------------------------------------------------------------------------
// Win32 (raw FFI; no extra crates). All coordinates are physical px: tao makes the
// process per-monitor-v2 DPI aware.
// ---------------------------------------------------------------------------

#[cfg(windows)]
mod sys {
    use std::ffi::c_void;

    pub type Hwnd = isize;

    #[repr(C)]
    #[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
    pub struct Point {
        pub x: i32,
        pub y: i32,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
    pub struct Rect {
        pub left: i32,
        pub top: i32,
        pub right: i32,
        pub bottom: i32,
    }

    impl Rect {
        pub fn w(&self) -> i32 {
            self.right - self.left
        }
        pub fn h(&self) -> i32 {
            self.bottom - self.top
        }
    }

    #[repr(C)]
    struct MonitorInfo {
        size: u32,
        monitor: Rect,
        work: Rect,
        flags: u32,
    }

    #[link(name = "user32")]
    extern "system" {
        fn GetCursorPos(p: *mut Point) -> i32;
        fn GetWindowRect(h: Hwnd, r: *mut Rect) -> i32;
        fn GetForegroundWindow() -> Hwnd;
        fn GetWindowThreadProcessId(h: Hwnd, pid: *mut u32) -> u32;
        fn SetWindowPos(h: Hwnd, after: Hwnd, x: i32, y: i32, cx: i32, cy: i32, flags: u32) -> i32;
        fn SystemParametersInfoW(action: u32, ui_param: u32, pv: *mut c_void, win_ini: u32) -> i32;
        fn MonitorFromWindow(h: Hwnd, flags: u32) -> isize;
        fn MonitorFromPoint(pt: Point, flags: u32) -> isize;
        fn GetMonitorInfoW(m: isize, mi: *mut MonitorInfo) -> i32;
        fn GetAsyncKeyState(vk: i32) -> i16;
        fn IsZoomed(h: Hwnd) -> i32;
        fn IsWindow(h: Hwnd) -> i32;
        fn IsWindowVisible(h: Hwnd) -> i32;
        fn GetClassNameW(h: Hwnd, buf: *mut u16, n: i32) -> i32;
        fn GetWindowLongW(h: Hwnd, index: i32) -> i32;
        fn SetForegroundWindow(h: Hwnd) -> i32;
    }

    #[link(name = "shcore")]
    extern "system" {
        fn GetDpiForMonitor(m: isize, kind: i32, x: *mut u32, y: *mut u32) -> i32;
    }

    const SWP_NOACTIVATE: u32 = 0x0010;
    const HWND_TOPMOST: Hwnd = -1;
    const GWL_EXSTYLE: i32 = -20;
    const WS_EX_TOPMOST: i32 = 0x0000_0008;
    const SPI_GETWORKAREA: u32 = 0x0030;
    const MONITOR_DEFAULTTONEAREST: u32 = 2;
    const MONITOR_DEFAULTTOPRIMARY: u32 = 1;

    pub fn cursor_pos() -> Option<Point> {
        let mut p = Point::default();
        (unsafe { GetCursorPos(&mut p) } != 0).then_some(p)
    }

    pub fn window_rect(h: Hwnd) -> Option<Rect> {
        let mut r = Rect::default();
        (unsafe { GetWindowRect(h, &mut r) } != 0).then_some(r)
    }

    pub fn is_visible(h: Hwnd) -> bool {
        unsafe { IsWindowVisible(h) != 0 }
    }

    pub fn is_topmost(h: Hwnd) -> bool {
        unsafe { GetWindowLongW(h, GWL_EXSTYLE) & WS_EX_TOPMOST != 0 }
    }

    pub fn foreground() -> Hwnd {
        unsafe { GetForegroundWindow() }
    }

    pub fn pid_of(h: Hwnd) -> u32 {
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(h, &mut pid) };
        pid
    }

    /// Move/resize atomically and (re)assert topmost, without activating.
    pub fn place(h: Hwnd, x: i32, y: i32, w: i32, hh: i32) {
        unsafe { SetWindowPos(h, HWND_TOPMOST, x, y, w, hh, SWP_NOACTIVATE) };
    }

    /// Re-assert topmost without moving or activating.
    pub fn reassert_topmost(h: Hwnd) {
        // SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE
        unsafe {
            SetWindowPos(
                h,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                0x0002 | 0x0001 | SWP_NOACTIVATE,
            )
        };
    }

    /// Work area (screen minus taskbar) of the primary monitor, physical px.
    pub fn primary_work_area() -> Rect {
        let mut r = Rect::default();
        let ok = unsafe {
            SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut r as *mut Rect as *mut c_void, 0)
        };
        if ok != 0 && r.w() > 0 && r.h() > 0 {
            r
        } else {
            Rect {
                left: 0,
                top: 0,
                right: 1280,
                bottom: 720,
            }
        }
    }

    /// Effective DPI scale of the primary monitor (1.0 = 96 dpi).
    pub fn primary_scale() -> f64 {
        unsafe {
            let m = MonitorFromPoint(Point { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
            let (mut x, mut y) = (96u32, 96u32);
            if m != 0 && GetDpiForMonitor(m, 0, &mut x, &mut y) == 0 && x >= 48 {
                x as f64 / 96.0
            } else {
                1.0
            }
        }
    }

    /// Any physical mouse button currently down?
    pub fn button_down() -> bool {
        unsafe {
            [0x01, 0x02, 0x04]
                .iter()
                .any(|&vk| (GetAsyncKeyState(vk) as u16) & 0x8000 != 0)
        }
    }

    pub fn activate(h: Hwnd) {
        unsafe {
            if h != 0 && IsWindow(h) != 0 && IsWindowVisible(h) != 0 {
                SetForegroundWindow(h);
            }
        }
    }

    fn class_of(h: Hwnd) -> String {
        let mut buf = [0u16; 64];
        let n = unsafe { GetClassNameW(h, buf.as_mut_ptr(), 64) };
        String::from_utf16_lossy(&buf[..n.max(0) as usize])
    }

    /// Does the foreground window cover its whole monitor (game, video, slideshow)?
    /// A merely *maximised* window does not count: it is exactly the case where the
    /// pill should stay visible.
    pub fn foreground_is_fullscreen(own: Hwnd) -> bool {
        unsafe {
            let fg = GetForegroundWindow();
            if fg == 0 || fg == own || pid_of(fg) == std::process::id() || IsWindowVisible(fg) == 0
            {
                return false;
            }
            if matches!(
                class_of(fg).as_str(),
                "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd"
            ) {
                return false;
            }
            if IsZoomed(fg) != 0 {
                return false;
            }
            let Some(r) = window_rect(fg) else {
                return false;
            };
            let m = MonitorFromWindow(fg, MONITOR_DEFAULTTONEAREST);
            let mut mi = MonitorInfo {
                size: std::mem::size_of::<MonitorInfo>() as u32,
                monitor: Rect::default(),
                work: Rect::default(),
                flags: 0,
            };
            if m == 0 || GetMonitorInfoW(m, &mut mi) == 0 {
                return false;
            }
            r.left <= mi.monitor.left
                && r.top <= mi.monitor.top
                && r.right >= mi.monitor.right
                && r.bottom >= mi.monitor.bottom
        }
    }
}

#[cfg(not(windows))]
mod sys {
    pub type Hwnd = isize;
    #[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
    pub struct Point {
        pub x: i32,
        pub y: i32,
    }
    #[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
    pub struct Rect {
        pub left: i32,
        pub top: i32,
        pub right: i32,
        pub bottom: i32,
    }
    impl Rect {
        pub fn w(&self) -> i32 {
            self.right - self.left
        }
        pub fn h(&self) -> i32 {
            self.bottom - self.top
        }
    }
    pub fn cursor_pos() -> Option<Point> {
        None
    }
    pub fn window_rect(_: Hwnd) -> Option<Rect> {
        None
    }
    pub fn is_visible(_: Hwnd) -> bool {
        false
    }
    pub fn is_topmost(_: Hwnd) -> bool {
        true
    }
    pub fn foreground() -> Hwnd {
        0
    }
    pub fn pid_of(_: Hwnd) -> u32 {
        0
    }
    pub fn place(_: Hwnd, _: i32, _: i32, _: i32, _: i32) {}
    pub fn reassert_topmost(_: Hwnd) {}
    pub fn primary_work_area() -> Rect {
        Rect {
            left: 0,
            top: 0,
            right: 1280,
            bottom: 720,
        }
    }
    pub fn primary_scale() -> f64 {
        1.0
    }
    pub fn button_down() -> bool {
        false
    }
    pub fn activate(_: Hwnd) {}
    pub fn foreground_is_fullscreen(_: Hwnd) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// Pure geometry helpers (unit-tested)
// ---------------------------------------------------------------------------

fn win_logical(mode: Mode) -> Option<(f64, f64)> {
    match mode {
        Mode::Pill => Some(PILL_WIN),
        Mode::Panel => Some(PANEL_WIN),
        Mode::Hidden => None,
    }
}

/// Contract default for the visible shape: centred horizontally, `ISLAND_PAD` from the top.
fn default_hit(mode: Mode) -> HitRect {
    let (win, vis) = match mode {
        Mode::Pill => (PILL_WIN, PILL_VIS),
        Mode::Panel => (PANEL_WIN, PANEL_VIS),
        Mode::Hidden => {
            return HitRect {
                x: 0.0,
                y: 0.0,
                w: 0.0,
                h: 0.0,
            }
        }
    };
    HitRect {
        x: (win.0 - vis.0) / 2.0,
        y: ISLAND_PAD,
        w: vis.0,
        h: vis.1,
    }
}

/// Physical rect of the window for `mode`: top-centre anchor on `work`, `scale` = DPI scale.
fn anchor_rect(work: sys::Rect, scale: f64, mode: Mode) -> Option<sys::Rect> {
    let (lw, lh) = win_logical(mode)?;
    let w = (lw * scale).round() as i32;
    let h = (lh * scale).round() as i32;
    let x = work.left + (work.w() - w) / 2;
    let y = work.top + (ISLAND_TOP * scale).round() as i32;
    Some(sys::Rect {
        left: x,
        top: y,
        right: x + w,
        bottom: y + h,
    })
}

/// UI-reported rect (or the default) -> physical rect on screen, given the window's
/// actual screen rect. The scale is derived from the window itself so rounding in
/// the DPI conversion can never skew the hit area.
fn hit_to_screen(mode: Mode, hit: Option<HitRect>, win: sys::Rect) -> sys::Rect {
    let Some((lw, _)) = win_logical(mode) else {
        return sys::Rect::default();
    };
    let s = win.w() as f64 / lw;
    let r = hit.unwrap_or_else(|| default_hit(mode));
    sys::Rect {
        left: win.left + (r.x * s).floor() as i32,
        top: win.top + (r.y * s).floor() as i32,
        right: win.left + ((r.x + r.w) * s).ceil() as i32,
        bottom: win.top + ((r.y + r.h) * s).ceil() as i32,
    }
}

fn point_in(r: sys::Rect, p: sys::Point, slop: i32) -> bool {
    p.x >= r.left - slop && p.x < r.right + slop && p.y >= r.top - slop && p.y < r.bottom + slop
}

/// Clamp a UI-reported rect into the window. Rejects NaN / non-positive sizes.
fn sanitize_hit(r: HitRect, mode: Mode) -> Option<HitRect> {
    if ![r.x, r.y, r.w, r.h].iter().all(|v| v.is_finite()) || r.w < 1.0 || r.h < 1.0 {
        return None;
    }
    let (lw, lh) = win_logical(mode).unwrap_or(PANEL_WIN);
    let x = r.x.clamp(0.0, lw);
    let y = r.y.clamp(0.0, lh);
    let w = r.w.min(lw - x);
    let h = r.h.min(lh - y);
    (w >= 1.0 && h >= 1.0).then_some(HitRect { x, y, w, h })
}

fn parse_shortcut(s: &str) -> Result<Shortcut, String> {
    let s = s.trim();
    if s.is_empty() {
        return Err("The shortcut is empty.".into());
    }
    let sc = Shortcut::from_str(s).map_err(|e| format!("'{s}' is not a valid shortcut ({e})."))?;
    let key = format!("{:?}", sc.key);
    let is_fkey =
        key.len() >= 2 && key.starts_with('F') && key[1..].chars().all(|c| c.is_ascii_digit());
    if sc.mods.is_empty() && !is_fkey {
        return Err(format!(
            "'{s}' needs a modifier (Ctrl, Alt, Shift or Win) so it doesn't hijack typing."
        ));
    }
    Ok(sc)
}

// ---------------------------------------------------------------------------
// Config persistence
// ---------------------------------------------------------------------------

fn load_cfg(path: &Option<PathBuf>) -> ShellConfig {
    path.as_ref()
        .and_then(|p| fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str::<ShellConfig>(&s).ok())
        .unwrap_or_default()
}

fn save_cfg(app: &AppHandle) {
    let sh = app.state::<Shell>();
    let cfg = sh.lk().cfg.clone();
    let path = sh
        .cfg_path
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    if let Some(p) = path {
        if let Some(dir) = p.parent() {
            let _ = fs::create_dir_all(dir);
        }
        if let Ok(s) = serde_json::to_string_pretty(&cfg) {
            let tmp = p.with_extension("json.tmp");
            if fs::write(&tmp, s).is_ok() {
                let _ = fs::rename(&tmp, &p);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Placement
// ---------------------------------------------------------------------------

fn own_hwnd() -> isize {
    HWND.load(Ordering::Relaxed)
}

/// Position + size the window for `mode` on the primary monitor's work area.
fn place(mode: Mode) {
    let hwnd = own_hwnd();
    if hwnd == 0 {
        return;
    }
    if let Some(r) = anchor_rect(sys::primary_work_area(), sys::primary_scale(), mode) {
        sys::place(hwnd, r.left, r.top, r.w(), r.h());
    }
}

fn emit_mode(app: &AppHandle, mode: Mode) {
    let _ = app.emit("shell-mode", json!({ "mode": mode }));
}

/// Give focus back to whatever the user was in before the panel took it.
fn restore_foreground(app: &AppHandle) {
    let own = own_hwnd();
    if sys::foreground() == own {
        let prev = app.state::<Shell>().lk().prev_fg;
        sys::activate(prev);
    }
}

/// THE state transition. Always runs on the main thread. Idempotent.
fn apply_mode_main(app: &AppHandle, requested: Mode, why: &str) {
    let sh = app.state::<Shell>();
    let own = own_hwnd();
    let (mode, fs_hidden, prev_mode) = {
        let mut st = sh.lk();
        let mode = if requested == Mode::Pill && !st.cfg.show_pill {
            Mode::Hidden
        } else {
            requested
        };
        let prev = st.mode;
        st.mode = mode;
        st.pending = None;
        st.last_apply = Instant::now();
        if prev != mode {
            st.hit = None; // stale rect from the other shape
        }
        if mode == Mode::Panel {
            st.panel_since = Instant::now();
            st.had_focus = false;
        } else {
            st.hold_since = None;
        }
        (mode, st.fs_hidden, prev)
    };
    log(format!("apply {prev_mode:?} -> {mode:?} ({why})"));

    let Some(win) = app.get_webview_window("main") else {
        return;
    };
    match mode {
        Mode::Pill => {
            if prev_mode == Mode::Panel {
                restore_foreground(app);
            }
            if fs_hidden {
                let _ = win.hide();
            } else {
                place(Mode::Pill);
                let _ = win.set_focusable(false); // pill never takes focus
                let _ = win.show();
            }
        }
        Mode::Panel => {
            let fg = sys::foreground();
            if fg != 0 && fg != own && sys::pid_of(fg) != std::process::id() {
                sh.lk().prev_fg = fg;
            }
            place(Mode::Panel);
            let _ = win.set_focusable(true);
            let _ = win.show();
            let _ = win.set_focus();
            if sys::foreground() == own {
                sh.lk().had_focus = true;
            }
        }
        Mode::Hidden => {
            if prev_mode == Mode::Panel {
                restore_foreground(app);
            }
            let _ = win.hide();
        }
    }
    emit_mode(app, mode);
}

fn apply_mode(app: &AppHandle, mode: Mode, why: &'static str) {
    let a = app.clone();
    on_main(app, move || apply_mode_main(&a, mode, why));
}

/// Show/hide the window to match the logical mode + fullscreen state (main thread).
fn sync_visibility(app: &AppHandle) {
    let sh = app.state::<Shell>();
    let (mode, fs_hidden) = {
        let st = sh.lk();
        (st.mode, st.fs_hidden)
    };
    let want = match mode {
        Mode::Hidden => false,
        Mode::Pill => !fs_hidden,
        Mode::Panel => true,
    };
    let Some(win) = app.get_webview_window("main") else {
        return;
    };
    let visible = sys::is_visible(own_hwnd());
    if want && !visible {
        place(mode);
        if mode == Mode::Pill {
            let _ = win.set_focusable(false);
        }
        let _ = win.show();
        log(format!("show ({mode:?})"));
    } else if !want && visible {
        restore_foreground(app);
        let _ = win.hide();
        log(format!("hide ({mode:?}, fs={fs_hidden})"));
    }
}

/// Re-anchor the current mode (display / DPI / taskbar changes) on the main thread.
fn fix_geometry(app: &AppHandle) {
    let mode = app.state::<Shell>().lk().mode;
    if mode != Mode::Hidden && sys::is_visible(own_hwnd()) {
        place(mode);
    }
}

// ---------------------------------------------------------------------------
// Rust-initiated requests + watchdog
// ---------------------------------------------------------------------------

/// Ask for a mode change with the UI in the loop, guaranteeing it happens anyway.
fn request(app: &AppHandle, action: Action, why: &'static str) {
    let sh = app.state::<Shell>();
    let (target, direct, seq) = {
        let mut st = sh.lk();
        let resolved = match action {
            Action::Toggle => {
                if st.mode == Mode::Panel {
                    Action::Collapse
                } else {
                    Action::Expand
                }
            }
            a => a,
        };
        match resolved {
            Action::Expand => {
                // No visible pill to morph from (hidden / fullscreen) or already open
                // (just refocus): apply directly.
                let pill_visible = st.mode == Mode::Pill && !st.fs_hidden;
                if !pill_visible {
                    (Mode::Panel, true, 0)
                } else {
                    if matches!(st.pending, Some((Mode::Panel, _))) {
                        return;
                    }
                    st.seq += 1;
                    st.pending = Some((Mode::Panel, st.seq));
                    (Mode::Panel, false, st.seq)
                }
            }
            _ => {
                if st.mode != Mode::Panel {
                    return;
                }
                let target = if st.cfg.show_pill {
                    Mode::Pill
                } else {
                    Mode::Hidden
                };
                if matches!(st.pending, Some((t, _)) if t == target) {
                    return;
                }
                st.seq += 1;
                st.pending = Some((target, st.seq));
                (target, false, st.seq)
            }
        }
    };
    log(format!(
        "request {action:?} -> {target:?} direct={direct} ({why})"
    ));
    if direct {
        apply_mode(app, target, why);
        return;
    }
    let verb = if target == Mode::Panel {
        "expand"
    } else {
        "collapse"
    };
    let _ = app.emit("shell-request", json!({ "action": verb }));

    // Watchdog: the UI normally answers with shell_set_mode within a few ms.
    let app2 = app.clone();
    thread::spawn(move || {
        thread::sleep(WATCHDOG);
        let still_pending = app2.state::<Shell>().lk().pending == Some((target, seq));
        if still_pending {
            log(format!("WATCHDOG forcing {target:?}"));
            apply_mode(&app2, target, "watchdog");
        }
    });
}

/// Collapse-on-focus-loss decision. Called after a short settle delay.
fn blur_check(app: &AppHandle) {
    let (ok, own) = {
        let sh = app.state::<Shell>();
        let st = sh.lk();
        (
            st.mode == Mode::Panel
                && st.cfg.collapse_on_blur
                && !st.hold_active()
                && st.panel_since.elapsed() > PANEL_GRACE,
            own_hwnd(),
        )
    };
    if !ok {
        return;
    }
    let fg = sys::foreground();
    // Still ours (webview popup, tray menu, native dialog): not a real focus loss.
    if fg == 0 || fg == own || sys::pid_of(fg) == std::process::id() {
        return;
    }
    request(app, Action::Collapse, "focus-lost");
}

// ---------------------------------------------------------------------------
// Click-through + fullscreen sampler (~30 Hz)
// ---------------------------------------------------------------------------

fn start_sampler(app: AppHandle) {
    let _ = thread::Builder::new()
        .name("krovi-shell-sampler".into())
        .spawn(move || {
            let mut n: u64 = 0;
            let mut blur_ticks: u32 = 0;
            let mut glance = Glance::default();
            loop {
                thread::sleep(SAMPLE);
                if app.state::<Shell>().quitting.load(Ordering::Relaxed) {
                    break;
                }
                n = n.wrapping_add(1);
                tick(&app, n, &mut blur_ticks, &mut glance);
            }
        });
}

/// Cursor-glance emitter state for the idle companion's eyes (`shell-cursor`).
/// Lives in the sampler thread; nothing here is shared.
#[derive(Default)]
struct Glance {
    /// Last in-range vector we emitted; `None` = out of range (final recentre already sent).
    last: Option<(f64, f64)>,
    at: Option<Instant>,
}

/// Cursor vector in logical px from the visible pill's centre, and its length.
fn glance_vector(win: sys::Rect, hit: Option<HitRect>, pt: sys::Point) -> (f64, f64, f64) {
    let s = (win.w() as f64 / PILL_WIN.0).max(0.01);
    let h = hit.unwrap_or_else(|| default_hit(Mode::Pill));
    let dx = (pt.x - win.left) as f64 / s - (h.x + h.w / 2.0);
    let dy = (pt.y - win.top) as f64 / s - (h.y + h.h / 2.0);
    (dx, dy, dx.hypot(dy))
}

/// Emit `shell-cursor` at <=10 Hz, only while in range (<=400 px), skipping moves of
/// <2 px; one final `{0,0,9999}` when the cursor leaves range / the pill goes away.
/// Costs nothing while out of range (no emit, no allocation).
fn glance_update(app: &AppHandle, g: &mut Glance, sample: Option<(f64, f64, f64)>) {
    match sample {
        Some((dx, dy, dist)) => {
            if g.at
                .is_some_and(|t| t.elapsed() < Duration::from_millis(100))
            {
                return;
            }
            if g.last
                .is_some_and(|(lx, ly)| (dx - lx).abs() < 2.0 && (dy - ly).abs() < 2.0)
            {
                return;
            }
            g.last = Some((dx, dy));
            g.at = Some(Instant::now());
            log(format!(
                "glance {} {} {}",
                dx.round(),
                dy.round(),
                dist.round()
            ));
            let _ = app.emit(
                "shell-cursor",
                json!({ "dx": dx.round(), "dy": dy.round(), "dist": dist.round() }),
            );
        }
        None => {
            if g.last.take().is_some() {
                g.at = None;
                let _ = app.emit("shell-cursor", json!({ "dx": 0, "dy": 0, "dist": 9999 }));
            }
        }
    }
}

fn tick(app: &AppHandle, n: u64, blur_ticks: &mut u32, glance: &mut Glance) {
    let own = own_hwnd();
    if own == 0 {
        return;
    }
    let sh = app.state::<Shell>();
    let visible = sys::is_visible(own);
    let mut sample: Option<(f64, f64, f64)> = None;

    if visible {
        let (mode, hit, ignoring, panel_age, collapse, hold, prev_btn, had_focus) = {
            let st = sh.lk();
            (
                st.mode,
                st.hit,
                st.ignoring,
                st.panel_since.elapsed(),
                st.cfg.collapse_on_blur,
                st.hold_active(),
                st.prev_btn,
                st.had_focus,
            )
        };
        if let (Some(wr), Some(pt)) = (sys::window_rect(own), sys::cursor_pos()) {
            let btn = sys::button_down();
            let inside = point_in(hit_to_screen(mode, hit, wr), pt, 1);

            if mode == Mode::Pill {
                let v = glance_vector(wr, hit, pt);
                if v.2 <= 400.0 {
                    sample = Some(v);
                }
            }

            // Click-through. Keep the window interactive while a drag that started
            // inside is still in progress.
            let want_ignore = !inside && !(btn && !ignoring);
            if want_ignore != ignoring {
                sh.lk().ignoring = want_ignore;
                let a = app.clone();
                on_main(app, move || {
                    if let Some(w) = a.get_webview_window("main") {
                        let _ = w.set_ignore_cursor_events(want_ignore);
                    }
                });
            }

            // Click outside the panel: it landed on the app underneath (click-through);
            // collapse regardless of whether a focus event ever fires.
            if mode == Mode::Panel
                && btn
                && !prev_btn
                && !inside
                && collapse
                && !hold
                && panel_age > PANEL_GRACE
            {
                request(app, Action::Collapse, "outside-click");
            }
            if btn != prev_btn {
                sh.lk().prev_btn = btn;
            }
        }

        // Safety net for focus loss that produced no WindowEvent::Focused(false)
        // (e.g. focus stolen by another process while we held it).
        if mode == Mode::Panel {
            let fg = sys::foreground();
            if fg == own {
                *blur_ticks = 0;
                if !had_focus {
                    sh.lk().had_focus = true;
                }
            } else if collapse
                && !hold
                && had_focus
                && fg != 0
                && sys::pid_of(fg) != std::process::id()
                && panel_age > PANEL_GRACE
            {
                *blur_ticks += 1;
                if *blur_ticks >= 4 {
                    *blur_ticks = 0;
                    request(app, Action::Collapse, "foreground-lost");
                }
            } else {
                *blur_ticks = 0;
            }
        } else {
            *blur_ticks = 0;
        }
    }

    // Eyes: in range only while the pill itself is visible; otherwise recentre once.
    glance_update(app, glance, sample);

    // ~2 Hz housekeeping.
    if n.is_multiple_of(15) {
        let (mode, hide_fs, fs_now, settled) = {
            let st = sh.lk();
            (
                st.mode,
                st.cfg.hide_in_fullscreen,
                st.fs_hidden,
                st.last_apply.elapsed() > Duration::from_millis(400),
            )
        };
        let fs = hide_fs && sys::foreground_is_fullscreen(own);
        if fs != fs_now {
            sh.lk().fs_hidden = fs;
            log(format!("fullscreen={fs}"));
            if mode == Mode::Pill {
                let a = app.clone();
                on_main(app, move || sync_visibility(&a));
            }
        }
        if visible && settled && mode != Mode::Hidden {
            let off = match (
                anchor_rect(sys::primary_work_area(), sys::primary_scale(), mode),
                sys::window_rect(own),
            ) {
                (Some(want), Some(have)) => {
                    (want.left - have.left).abs() > 1
                        || (want.top - have.top).abs() > 1
                        || (want.w() - have.w()).abs() > 1
                        || (want.h() - have.h()).abs() > 1
                }
                _ => false,
            };
            if off {
                log("geometry drifted: re-anchoring");
                let a = app.clone();
                on_main(app, move || fix_geometry(&a));
            } else if !sys::is_topmost(own) {
                sys::reassert_topmost(own);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Global shortcut
// ---------------------------------------------------------------------------

fn register_shortcut(app: &AppHandle, sc: Shortcut) -> Result<(), String> {
    app.global_shortcut()
        .on_shortcut(sc, move |app, _s, ev| {
            if ev.state() != ShortcutState::Pressed {
                return;
            }
            let sh = app.state::<Shell>();
            {
                let mut st = sh.lk();
                if st.last_toggle.elapsed() < Duration::from_millis(200) {
                    return;
                }
                st.last_toggle = Instant::now();
            }
            request(app, Action::Toggle, "shortcut");
        })
        .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Tray
// ---------------------------------------------------------------------------

fn tray_tooltip(cfg: &ShellConfig) -> String {
    format!("Krovi ({})", cfg.shortcut)
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let sh = app.state::<Shell>();
    let cfg = sh.lk().cfg.clone();

    let open = MenuItem::with_id(app, "open", "Open Krovi", true, None::<&str>)?;
    let show_pill =
        CheckMenuItem::with_id(app, "pill", "Show pill", true, cfg.show_pill, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Settings\u{2026}", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit Krovi", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &show_pill, &settings, &sep, &quit_item])?;

    *sh.check_item.lock().unwrap_or_else(|e| e.into_inner()) = Some(show_pill.clone());

    let icon = Image::from_bytes(include_bytes!("../icons/32x32.png"))?;
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .tooltip(tray_tooltip(&cfg))
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, ev| match ev.id().as_ref() {
            "open" => request(app, Action::Expand, "tray-open"),
            "pill" => {
                let show = !app.state::<Shell>().lk().cfg.show_pill;
                set_show_pill(app, show);
            }
            "settings" => {
                request(app, Action::Expand, "tray-settings");
                let app = app.clone();
                thread::spawn(move || {
                    // Wait for the panel to be up, then give the UI a beat to mount.
                    for _ in 0..40 {
                        if app.state::<Shell>().lk().mode == Mode::Panel {
                            break;
                        }
                        thread::sleep(Duration::from_millis(50));
                    }
                    thread::sleep(Duration::from_millis(200));
                    let _ = app.emit("shell-open-tool", json!({ "tool": "settings" }));
                });
            }
            "quit" => quit(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, ev| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = ev
            {
                request(tray.app_handle(), Action::Toggle, "tray-click");
            }
        })
        .build(app)?;
    Ok(())
}

/// Flip `showPill`, persist, and reflect it in the tray check + window.
fn set_show_pill(app: &AppHandle, show: bool) {
    let sh = app.state::<Shell>();
    sh.lk().cfg.show_pill = show;
    save_cfg(app);
    if let Some(item) = sh
        .check_item
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
    {
        let _ = item.set_checked(show);
    }
    let mode = sh.lk().mode;
    if mode != Mode::Panel {
        apply_mode(app, Mode::Pill, "show-pill-toggle");
    }
}

/// Quit for real. Never depends on the UI; leaves no orphan process or tray ghost.
fn quit(app: &AppHandle) {
    let sh = app.state::<Shell>();
    if sh.quitting.swap(true, Ordering::SeqCst) {
        return;
    }
    log("quit");
    let _ = app.remove_tray_by_id(TRAY_ID);
    // Fail-safe: if teardown ever hangs, the process still ends.
    thread::spawn(|| {
        thread::sleep(Duration::from_secs(3));
        std::process::exit(0);
    });
    app.exit(0);
}

// ---------------------------------------------------------------------------
// Plugin / setup / window events (called from lib.rs)
// ---------------------------------------------------------------------------

pub fn plugins(builder: tauri::Builder<Wry>) -> tauri::Builder<Wry> {
    builder
        // Single-instance must be registered first.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            // `Krovi.exe --quit` is a last-resort, scriptable way to close a running
            // instance entirely (works with no UI, no tray, no shortcut).
            if args.iter().any(|a| a == "--quit") {
                quit(app);
                return;
            }
            request(app, Action::Expand, "second-instance");
        }))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .arg("--autostart")
                .build(),
        )
        .manage(Shell::new())
}

pub fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let _ = MAIN_THREAD.set(thread::current().id());
    let handle = app.handle().clone();
    let sh = handle.state::<Shell>();

    // Config (reflect the real autostart state; never enable it implicitly).
    let path = handle
        .path()
        .app_config_dir()
        .ok()
        .map(|d| d.join("shell.json"));
    let mut cfg = load_cfg(&path);
    if let Ok(on) = handle.autolaunch().is_enabled() {
        cfg.launch_at_login = on;
    }
    *sh.cfg_path.lock().unwrap_or_else(|e| e.into_inner()) = path;
    sh.lk().cfg = cfg.clone();

    match handle.get_webview_window("main").map(|w| w.hwnd()) {
        Some(Ok(h)) => HWND.store(h.0 as isize, Ordering::Relaxed),
        other => log(format!(
            "no main window hwnd: {:?}",
            other.map(|r| r.is_ok())
        )),
    }

    if let Err(e) = build_tray(&handle) {
        log(format!("tray failed: {e}"));
    }

    // Global shortcut. A taken accelerator must never prevent startup: the tray works.
    match parse_shortcut(&cfg.shortcut).and_then(|sc| register_shortcut(&handle, sc).map(|_| sc)) {
        Ok(sc) => sh.lk().shortcut = Some(sc),
        Err(e) => {
            log(format!("shortcut '{}' unavailable: {e}", cfg.shortcut));
            sh.lk().shortcut_error = Some(e);
        }
    }

    apply_mode_main(&handle, Mode::Pill, "startup");
    start_sampler(handle);
    Ok(())
}

pub fn on_window_event(window: &Window<Wry>, event: &WindowEvent) {
    if window.label() != "main" {
        return;
    }
    let app = window.app_handle();
    match event {
        WindowEvent::Focused(false) => {
            let app = app.clone();
            thread::spawn(move || {
                thread::sleep(Duration::from_millis(120));
                blur_check(&app);
            });
        }
        WindowEvent::Focused(true) => {
            app.state::<Shell>().lk().had_focus = true;
        }
        WindowEvent::CloseRequested { api, .. } => {
            // Alt+F4 etc. must never destroy the island; "closing" the panel collapses it.
            if !app.state::<Shell>().quitting.load(Ordering::Relaxed) {
                api.prevent_close();
                request(app, Action::Collapse, "close-requested");
            }
        }
        WindowEvent::ScaleFactorChanged { .. } => {
            let a = app.clone();
            on_main(app, move || fix_geometry(&a));
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// UI -> Rust: apply a mode now. Returns the mode actually applied (`pill` becomes
/// `hidden` when the user turned the pill off).
#[tauri::command]
pub fn shell_set_mode(app: AppHandle, mode: Mode) -> Mode {
    // Sync commands run on the main thread, so this is applied inline: the window is
    // already resized/shown when the UI's promise resolves (the contract's
    // "window grows BEFORE the shape does").
    apply_mode(&app, mode, "ui");
    app.state::<Shell>().lk().mode
}

/// Current logical mode (the UI syncs this once at start-up).
#[tauri::command]
pub fn shell_get_mode(app: AppHandle) -> Mode {
    app.state::<Shell>().lk().mode
}

/// Suppress collapse-on-blur while a native dialog is open.
#[tauri::command]
pub fn shell_hold(app: AppHandle, hold: bool) {
    app.state::<Shell>().lk().hold_since = hold.then(Instant::now);
}

/// Quit the whole app. Always works; never returns to a live process.
#[tauri::command]
pub fn shell_quit(app: AppHandle) {
    quit(&app);
}

/// Report the visible shape's bounding rect (logical px, window-relative).
#[tauri::command]
pub fn shell_set_hit_rect(app: AppHandle, rect: HitRect) {
    let sh = app.state::<Shell>();
    let mut st = sh.lk();
    let mode = st.mode;
    if let Some(r) = sanitize_hit(rect, mode) {
        st.hit = Some(r);
    }
}

#[tauri::command]
pub fn shell_get_config(app: AppHandle) -> ShellConfig {
    app.state::<Shell>().lk().cfg.clone()
}

/// Validate + apply a new config. On a bad / taken shortcut the OLD one keeps working
/// and a human-readable error is returned.
#[tauri::command]
pub fn shell_set_config(app: AppHandle, config: ShellConfig) -> Result<ShellConfig, String> {
    let sh = app.state::<Shell>();
    let mut new = config;
    new.shortcut = new.shortcut.trim().to_string();
    let sc = parse_shortcut(&new.shortcut)?;

    let (old_sc, old_cfg) = {
        let st = sh.lk();
        (st.shortcut, st.cfg.clone())
    };

    let mut registered_new = false;
    if old_sc != Some(sc) {
        register_shortcut(&app, sc).map_err(|e| {
            let keep = if old_sc.is_some() {
                format!(" Your current shortcut ({}) still works.", old_cfg.shortcut)
            } else {
                String::new()
            };
            format!(
                "Couldn't use '{}': {e}. It may already be taken by another app.{keep}",
                new.shortcut
            )
        })?;
        registered_new = true;
    }

    if new.launch_at_login != old_cfg.launch_at_login {
        let al = app.autolaunch();
        let r = if new.launch_at_login {
            al.enable()
        } else {
            al.disable()
        };
        if let Err(e) = r {
            if registered_new {
                let _ = app.global_shortcut().unregister(sc);
            }
            return Err(format!("Couldn't change launch at login: {e}"));
        }
    }

    if registered_new {
        if let Some(old) = old_sc {
            let _ = app.global_shortcut().unregister(old);
        }
    }

    let show_changed = new.show_pill != old_cfg.show_pill;
    let fs_changed = new.hide_in_fullscreen != old_cfg.hide_in_fullscreen;
    {
        let mut st = sh.lk();
        st.shortcut = Some(sc);
        st.shortcut_error = None;
        st.cfg = new.clone();
        if fs_changed && !new.hide_in_fullscreen {
            st.fs_hidden = false;
        }
    }
    save_cfg(&app);

    if let Some(item) = sh
        .check_item
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
    {
        let _ = item.set_checked(new.show_pill);
    }
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_tooltip(Some(tray_tooltip(&new)));
    }
    if show_changed || fs_changed {
        let mode = sh.lk().mode;
        if mode != Mode::Panel {
            apply_mode(&app, Mode::Pill, "config");
        }
    }
    Ok(new)
}

// ---------------------------------------------------------------------------
// Tests (pure logic only)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn work(w: i32, h: i32) -> sys::Rect {
        sys::Rect {
            left: 0,
            top: 0,
            right: w,
            bottom: h,
        }
    }

    #[test]
    fn pill_and_panel_share_the_top_centre_anchor() {
        for scale in [1.0, 1.25, 1.5, 2.0] {
            let wa = work(1920, 1032);
            let pill = anchor_rect(wa, scale, Mode::Pill).unwrap();
            let panel = anchor_rect(wa, scale, Mode::Panel).unwrap();
            assert_eq!(pill.top, panel.top, "same top at scale {scale}");
            // centres agree to within a pixel (odd-pixel rounding)
            let (cp, cn) = ((pill.left + pill.right) / 2, (panel.left + panel.right) / 2);
            assert!(
                (cp - cn).abs() <= 1,
                "same horizontal centre at scale {scale}"
            );
            assert_eq!(pill.top, (ISLAND_TOP * scale).round() as i32);
        }
    }

    #[test]
    fn pill_window_size_matches_contract_at_125_percent() {
        let r = anchor_rect(work(1920, 1032), 1.25, Mode::Pill).unwrap();
        assert_eq!((r.w(), r.h()), (550, 110));
        assert_eq!(r.left, (1920 - 550) / 2);
        assert_eq!(r.top, 8); // round(6 * 1.25)
    }

    #[test]
    fn anchor_respects_work_area_offset() {
        let wa = sys::Rect {
            left: 0,
            top: 40,
            right: 1920,
            bottom: 1080,
        }; // top taskbar
        assert_eq!(anchor_rect(wa, 1.0, Mode::Pill).unwrap().top, 46);
    }

    #[test]
    fn default_hit_is_centred_pill() {
        let h = default_hit(Mode::Pill);
        assert_eq!((h.x, h.y, h.w, h.h), (20.0, 10.0, 400.0, 44.0));
        let p = default_hit(Mode::Panel);
        assert_eq!((p.x, p.y, p.w, p.h), (30.0, 10.0, 900.0, 620.0));
    }

    #[test]
    fn hit_maps_to_screen_using_window_scale() {
        let win = sys::Rect {
            left: 685,
            top: 8,
            right: 1235,
            bottom: 118,
        }; // 125%
        let r = hit_to_screen(Mode::Pill, None, win);
        assert_eq!(r.left, 685 + 25); // 20 * 1.25
        assert_eq!(r.top, 8 + 12); // floor(10 * 1.25)
        assert_eq!(r.w(), 500); // 400 * 1.25
        assert!(point_in(r, sys::Point { x: 960, y: 40 }, 0));
        assert!(
            !point_in(r, sys::Point { x: 690, y: 40 }, 0),
            "transparent margin is outside"
        );
    }

    #[test]
    fn glance_vector_is_logical_px_from_pill_centre() {
        // 125%: window 685,8 550x110. Default pill centre = (220, 32) logical.
        let win = sys::Rect {
            left: 685,
            top: 8,
            right: 1235,
            bottom: 118,
        };
        // cursor exactly on the pill centre
        let c = sys::Point {
            x: 685 + 275,
            y: 8 + 40,
        };
        let (dx, dy, d) = glance_vector(win, None, c);
        assert!(dx.abs() < 1.0 && dy.abs() < 1.0 && d < 1.5, "{dx} {dy} {d}");
        // 125 physical px right of centre = 100 logical
        let r = sys::Point {
            x: c.x + 125,
            y: c.y,
        };
        let (dx, dy, d) = glance_vector(win, None, r);
        assert!((dx - 100.0).abs() < 1.0 && dy.abs() < 1.0 && (d - 100.0).abs() < 1.5);
        // UI-reported rect moves the centre
        let hit = HitRect {
            x: 100.0,
            y: 10.0,
            w: 100.0,
            h: 44.0,
        }; // centre (150, 32)
        let (dx, _, _) = glance_vector(win, Some(hit), c);
        assert!((dx - 70.0).abs() < 1.0, "{dx}");
    }

    #[test]
    fn sanitize_rejects_garbage_and_clamps() {
        let bad = HitRect {
            x: f64::NAN,
            y: 0.0,
            w: 10.0,
            h: 10.0,
        };
        assert!(sanitize_hit(bad, Mode::Pill).is_none());
        let zero = HitRect {
            x: 0.0,
            y: 0.0,
            w: 0.0,
            h: 10.0,
        };
        assert!(sanitize_hit(zero, Mode::Pill).is_none());
        let big = HitRect {
            x: -5.0,
            y: 0.0,
            w: 9999.0,
            h: 9999.0,
        };
        let c = sanitize_hit(big, Mode::Pill).unwrap();
        assert_eq!((c.x, c.w, c.h), (0.0, 440.0, 88.0));
    }

    #[test]
    fn shortcut_validation() {
        assert!(parse_shortcut("Ctrl+Alt+Space").is_ok());
        assert!(parse_shortcut("  ctrl+shift+k ").is_ok());
        assert!(parse_shortcut("F9").is_ok());
        assert!(parse_shortcut("").is_err());
        assert!(
            parse_shortcut("A").is_err(),
            "bare letter would hijack typing"
        );
        assert!(parse_shortcut("Ctrl+Banana").is_err());
    }

    #[test]
    fn config_roundtrips_and_defaults_missing_fields() {
        let c: ShellConfig = serde_json::from_str(r#"{"shortcut":"Ctrl+Shift+K"}"#).unwrap();
        assert_eq!(c.shortcut, "Ctrl+Shift+K");
        assert!(c.show_pill && c.collapse_on_blur && c.hide_in_fullscreen && !c.launch_at_login);
        let s = serde_json::to_string(&ShellConfig::default()).unwrap();
        assert!(s.contains("hideInFullscreen") && s.contains("launchAtLogin"));
    }
}
