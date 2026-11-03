//! Per-monitor fullscreen detection for the pill ("hide in fullscreen").
//!
//! The pill lives on one monitor. A game or video that goes fullscreen on **another** monitor
//! must not hide it, so the decision is: *the foreground window is fullscreen **and** it is on
//! the same monitor as the pill*.
//!
//! * [`is_fullscreen`] / [`hides_pill`] are pure — rectangles, window-style bits and a class
//!   name in, a bool out — and unit-tested on any host.
//! * [`foreground_fullscreen_on_pill_monitor`] is the native glue (`GetForegroundWindow`,
//!   `MonitorFromWindow` for both windows, `GetMonitorInfoW`). It exists in every build and
//!   returns `false` unless the `native` feature is on (Windows only).
//!
//! What counts as fullscreen:
//!
//! * a window whose rectangle covers its whole **monitor** (±[`TOLERANCE_PX`], so a 1 px
//!   rounding or DPI fudge does not matter) — borderless-fullscreen games, exclusive
//!   fullscreen (a popup window the size of the monitor), video players, slideshows, F11 browsers;
//! * **not** a window that only covers the **work area**: a maximised window with the taskbar
//!   showing leaves the taskbar's strip uncovered, so the pill should stay;
//! * **not** an ordinary captioned window even when it reaches the monitor edge (a maximised
//!   window with an auto-hidden taskbar is exactly that), nor a maximised resizable frameless
//!   window (Electron-style custom title bars);
//! * **not** the desktop or the shell (`Progman`, `WorkerW`, the taskbars, Task View).
//!
//! Reads only geometry, style bits and the window class — never a title, process or content.

/// A rectangle in physical pixels, `right`/`bottom` exclusive (the `RECT` layout).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    pub const fn new(left: i32, top: i32, right: i32, bottom: i32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }

    pub const fn width(&self) -> i32 {
        self.right - self.left
    }

    pub const fn height(&self) -> i32 {
        self.bottom - self.top
    }

    /// Zero or negative area (a failed monitor query reports all zeros).
    pub const fn is_empty(&self) -> bool {
        self.width() <= 0 || self.height() <= 0
    }
}

/// Slack, in pixels, when comparing a window edge with a monitor edge. Fullscreen windows are
/// routinely off by one at fractional DPI scales; three pixels is clearly not "covering".
pub const TOLERANCE_PX: i32 = 2;

// Window style bits (`GetWindowLongW(hwnd, GWL_STYLE)`).
pub const WS_POPUP: u32 = 0x8000_0000;
pub const WS_MAXIMIZE: u32 = 0x0100_0000;
/// `WS_BORDER | WS_DLGFRAME`: a title bar. Both bits must be set — `WS_DLGFRAME` alone is a
/// dialog frame, `WS_BORDER` alone a thin border.
pub const WS_CAPTION: u32 = 0x00C0_0000;
pub const WS_THICKFRAME: u32 = 0x0004_0000;

/// Window classes that are the desktop or the shell, never an application going fullscreen.
const SHELL_CLASSES: &[&str] = &[
    "Progman",
    "WorkerW",
    "Shell_TrayWnd",
    "Shell_SecondaryTrayWnd",
    // Task View / Alt+Tab overlays cover the monitor but are the shell, not a fullscreen app.
    "MultitaskingViewFrame",
    "TaskSwitcherWnd",
    "XamlExplorerHostIslandWindow",
];

/// Is `class_name` the desktop or a shell window? (Window classes compare case-insensitively.)
pub fn is_shell_class(class_name: &str) -> bool {
    SHELL_CLASSES
        .iter()
        .any(|c| c.eq_ignore_ascii_case(class_name))
}

/// How much of a monitor a window covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Coverage {
    /// Neither the work area nor the monitor is covered.
    None,
    /// The work area (monitor minus taskbar) is covered but the taskbar's strip is not:
    /// a maximised window with the taskbar showing.
    WorkArea,
    /// The whole monitor is covered.
    Monitor,
}

/// `outer` reaches every edge of `inner`, give or take `tol` px. Overshoot is fine — a maximised
/// window's frame hangs past the work area by its invisible resize border.
fn covers(outer: Rect, inner: Rect, tol: i32) -> bool {
    outer.left <= inner.left + tol
        && outer.top <= inner.top + tol
        && outer.right >= inner.right - tol
        && outer.bottom >= inner.bottom - tol
}

/// Classify `window` against `monitor` and its `work_area`.
pub fn coverage(window: Rect, monitor: Rect, work_area: Rect) -> Coverage {
    if monitor.is_empty() {
        Coverage::None
    } else if covers(window, monitor, TOLERANCE_PX) {
        Coverage::Monitor
    } else if !work_area.is_empty() && covers(window, work_area, TOLERANCE_PX) {
        Coverage::WorkArea
    } else {
        Coverage::None
    }
}

/// Everything the pure decision needs about the foreground window and the monitor it is judged on.
#[derive(Debug, Clone, Copy)]
pub struct WindowFacts<'a> {
    /// `GetWindowRect`, physical px.
    pub window: Rect,
    /// `rcMonitor` of the monitor the window is on.
    pub monitor: Rect,
    /// `rcWork` of that monitor (the monitor minus the taskbar).
    pub work_area: Rect,
    /// `GWL_STYLE` bits.
    pub style: u32,
    /// `GetClassNameW`.
    pub class_name: &'a str,
}

/// Is this window fullscreen on its monitor?
pub fn is_fullscreen(f: &WindowFacts<'_>) -> bool {
    if is_shell_class(f.class_name) {
        return false;
    }
    if coverage(f.window, f.monitor, f.work_area) != Coverage::Monitor {
        return false;
    }
    // A title bar means an ordinary window that happens to reach the edges (maximised with an
    // auto-hidden taskbar). Fullscreen games and players are popups without one.
    if f.style & WS_CAPTION == WS_CAPTION {
        return false;
    }
    // Maximised *and* resizable without a title bar: a custom-chrome app (Electron, WinUI...),
    // not a fullscreen game, which has no sizing frame.
    if f.style & WS_MAXIMIZE != 0 && f.style & WS_THICKFRAME != 0 {
        return false;
    }
    true
}

/// Should the pill hide? Only when the foreground window is fullscreen **on the pill's monitor**.
/// `window_monitor` / `pill_monitor` are `HMONITOR` values (0 = unknown, which never matches).
pub fn hides_pill(f: &WindowFacts<'_>, window_monitor: isize, pill_monitor: isize) -> bool {
    window_monitor != 0 && window_monitor == pill_monitor && is_fullscreen(f)
}

/// Is the foreground window fullscreen on the same monitor as the pill window `pill` (an `HWND`)?
///
/// `false` for the pill/Krovi's own windows, hidden windows, the desktop and shell, and in any
/// build without the `native` feature.
#[cfg(all(windows, feature = "native"))]
pub fn foreground_fullscreen_on_pill_monitor(pill: isize) -> bool {
    native::foreground_fullscreen_on_pill_monitor(pill)
}

/// See the native variant. Without `native` there is nothing to ask the OS, so nothing hides.
#[cfg(not(all(windows, feature = "native")))]
pub fn foreground_fullscreen_on_pill_monitor(_pill: isize) -> bool {
    false
}

// ---------------------------------------------------------------------------
// Native implementation.
//
// Raw user32 FFI, declared the same way as the app crate's `shell::sys` (the `windows` crate
// would need the extra `Win32_Graphics_Gdi` feature for the monitor calls). All coordinates
// are physical px: the process is per-monitor-DPI aware, so a window rect and a monitor rect
// are in the same space. Reads geometry, style bits and the class name only.
// ---------------------------------------------------------------------------
#[cfg(all(windows, feature = "native"))]
mod native {
    use super::{hides_pill, Rect, WindowFacts};

    type Hwnd = isize;
    type Hmonitor = isize;

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct RawRect {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }

    impl From<RawRect> for Rect {
        fn from(r: RawRect) -> Self {
            Rect::new(r.left, r.top, r.right, r.bottom)
        }
    }

    /// `MONITORINFO` (`size` and `flags` are only ever written, for the call's sake).
    #[repr(C)]
    #[allow(dead_code)]
    struct MonitorInfo {
        size: u32,
        monitor: RawRect,
        work: RawRect,
        flags: u32,
    }

    #[link(name = "user32")]
    extern "system" {
        fn GetForegroundWindow() -> Hwnd;
        fn GetWindowRect(h: Hwnd, r: *mut RawRect) -> i32;
        fn GetWindowLongW(h: Hwnd, index: i32) -> i32;
        fn GetClassNameW(h: Hwnd, buf: *mut u16, n: i32) -> i32;
        fn IsWindowVisible(h: Hwnd) -> i32;
        fn GetWindowThreadProcessId(h: Hwnd, pid: *mut u32) -> u32;
        fn MonitorFromWindow(h: Hwnd, flags: u32) -> Hmonitor;
        fn GetMonitorInfoW(m: Hmonitor, mi: *mut MonitorInfo) -> i32;
    }

    const GWL_STYLE: i32 = -16;
    const MONITOR_DEFAULTTONEAREST: u32 = 2;

    fn window_rect(h: Hwnd) -> Option<Rect> {
        let mut r = RawRect::default();
        // SAFETY: `r` is a valid, writable RECT for the call.
        (unsafe { GetWindowRect(h, &mut r) } != 0).then(|| r.into())
    }

    fn class_of(h: Hwnd) -> String {
        let mut buf = [0u16; 256];
        // SAFETY: `buf` is writable for the `buf.len()` code units passed alongside it.
        let n = unsafe { GetClassNameW(h, buf.as_mut_ptr(), buf.len() as i32) };
        String::from_utf16_lossy(&buf[..n.max(0) as usize])
    }

    /// `(rcMonitor, rcWork)` of `m`.
    fn monitor_rects(m: Hmonitor) -> Option<(Rect, Rect)> {
        let mut mi = MonitorInfo {
            size: std::mem::size_of::<MonitorInfo>() as u32,
            monitor: RawRect::default(),
            work: RawRect::default(),
            flags: 0,
        };
        // SAFETY: `mi` is a valid MONITORINFO with `cbSize` set, as the call requires.
        (m != 0 && unsafe { GetMonitorInfoW(m, &mut mi) } != 0)
            .then(|| (mi.monitor.into(), mi.work.into()))
    }

    pub(super) fn foreground_fullscreen_on_pill_monitor(pill: isize) -> bool {
        // SAFETY: plain user32 queries on handles that may have gone stale; each call tolerates
        // that and the results are re-checked (a vanished window just yields `false`).
        unsafe {
            let fg = GetForegroundWindow();
            if fg == 0 || fg == pill || IsWindowVisible(fg) == 0 {
                return false;
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(fg, &mut pid);
            if pid == std::process::id() {
                return false;
            }
            let Some(window) = window_rect(fg) else {
                return false;
            };
            let fg_monitor = MonitorFromWindow(fg, MONITOR_DEFAULTTONEAREST);
            let pill_monitor = MonitorFromWindow(pill, MONITOR_DEFAULTTONEAREST);
            let Some((monitor, work_area)) = monitor_rects(fg_monitor) else {
                return false;
            };
            let class_name = class_of(fg);
            let facts = WindowFacts {
                window,
                monitor,
                work_area,
                style: GetWindowLongW(fg, GWL_STYLE) as u32,
                class_name: &class_name,
            };
            hides_pill(&facts, fg_monitor, pill_monitor)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A two-monitor desktop: M1 primary 1920x1080 with the taskbar along the bottom (40 px),
    // M2 to its right, and M0 to the left at negative coordinates.
    const M1: Rect = Rect::new(0, 0, 1920, 1080);
    const W1: Rect = Rect::new(0, 0, 1920, 1040);
    const M2: Rect = Rect::new(1920, 0, 3840, 1080);
    const W2: Rect = Rect::new(1920, 0, 3840, 1040);
    const M0: Rect = Rect::new(-1920, 0, 0, 1080);
    const W0: Rect = Rect::new(-1920, 0, 0, 1040);

    // Typical style words.
    const POPUP: u32 = WS_POPUP | 0x1000_0000; // WS_POPUP | WS_VISIBLE
    const OVERLAPPED: u32 = 0x00CF_0000 | 0x1000_0000; // WS_OVERLAPPEDWINDOW | WS_VISIBLE
    const MAXIMIZED: u32 = OVERLAPPED | WS_MAXIMIZE;
    const FRAMELESS_MAX: u32 = WS_THICKFRAME | 0x1000_0000 | WS_MAXIMIZE; // no caption

    fn facts<'a>(
        window: Rect,
        monitor: Rect,
        work_area: Rect,
        style: u32,
        class: &'a str,
    ) -> WindowFacts<'a> {
        WindowFacts {
            window,
            monitor,
            work_area,
            style,
            class_name: class,
        }
    }

    fn grow(r: Rect, d: i32) -> Rect {
        Rect::new(r.left - d, r.top - d, r.right + d, r.bottom + d)
    }

    #[test]
    fn rect_helpers() {
        assert_eq!(M1.width(), 1920);
        assert_eq!(M1.height(), 1080);
        assert!(!M1.is_empty());
        assert!(Rect::default().is_empty());
        assert!(Rect::new(10, 10, 10, 50).is_empty());
        assert!(Rect::new(10, 10, 5, 50).is_empty());
    }

    #[test]
    fn fullscreen_decision_table() {
        struct Case {
            name: &'static str,
            f: WindowFacts<'static>,
            want: bool,
        }
        let cases = [
            Case {
                name: "borderless-fullscreen game (popup the size of the monitor)",
                f: facts(M1, M1, W1, POPUP, "UnityWndClass"),
                want: true,
            },
            Case {
                name: "exclusive fullscreen (D3D popup, monitor sized, topmost)",
                f: facts(M1, M1, W1, POPUP, "SDL_app"),
                want: true,
            },
            Case {
                name: "browser / player F11 (no caption, no sizing frame, maximize bit still set)",
                f: facts(M1, M1, W1, POPUP | WS_MAXIMIZE, "Chrome_WidgetWin_1"),
                want: true,
            },
            Case {
                name: "fullscreen on the second monitor, judged on that monitor",
                f: facts(M2, M2, W2, POPUP, "mpv"),
                want: true,
            },
            Case {
                name: "fullscreen on a monitor at negative coordinates",
                f: facts(M0, M0, W0, POPUP, "screenClass"),
                want: true,
            },
            Case {
                name: "window overshoots the monitor (frame hangs past it)",
                f: facts(grow(M1, 8), M1, W1, POPUP, "Game"),
                want: true,
            },
            Case {
                name: "maximised captioned window, taskbar visible",
                f: facts(Rect::new(-8, -8, 1928, 1048), M1, W1, MAXIMIZED, "Notepad"),
                want: false,
            },
            Case {
                name: "maximised captioned window, taskbar auto-hidden (reaches every edge)",
                f: facts(Rect::new(-8, -8, 1928, 1088), M1, M1, MAXIMIZED, "Notepad"),
                want: false,
            },
            Case {
                name: "maximised frameless resizable app, taskbar auto-hidden",
                f: facts(grow(M1, 8), M1, M1, FRAMELESS_MAX, "Chrome_WidgetWin_1"),
                want: false,
            },
            Case {
                name: "borderless window sized to the work area (taskbar showing)",
                f: facts(W1, M1, W1, POPUP, "Game"),
                want: false,
            },
            Case {
                name: "ordinary restored window",
                f: facts(
                    Rect::new(200, 120, 1100, 800),
                    M1,
                    W1,
                    OVERLAPPED,
                    "Notepad",
                ),
                want: false,
            },
            Case {
                name: "small popup",
                f: facts(Rect::new(200, 120, 1000, 720), M1, W1, POPUP, "Dialog"),
                want: false,
            },
            Case {
                name: "window judged against the wrong monitor does not cover it",
                f: facts(M2, M1, W1, POPUP, "mpv"),
                want: false,
            },
            Case {
                name: "degenerate monitor (failed query)",
                f: facts(M1, Rect::default(), Rect::default(), POPUP, "Game"),
                want: false,
            },
            Case {
                name: "desktop (Progman) covers the monitor",
                f: facts(M1, M1, W1, POPUP, "Progman"),
                want: false,
            },
            Case {
                name: "desktop worker (WorkerW) covers the monitor",
                f: facts(M1, M1, W1, POPUP, "WorkerW"),
                want: false,
            },
            Case {
                name: "taskbar",
                f: facts(
                    Rect::new(0, 1040, 1920, 1080),
                    M1,
                    W1,
                    POPUP,
                    "Shell_TrayWnd",
                ),
                want: false,
            },
            Case {
                name: "secondary taskbar, even if it were monitor sized",
                f: facts(M2, M2, W2, POPUP, "Shell_SecondaryTrayWnd"),
                want: false,
            },
            Case {
                name: "Task View overlay, class compared case-insensitively",
                f: facts(M1, M1, W1, POPUP, "multitaskingviewframe"),
                want: false,
            },
        ];
        for c in cases {
            assert_eq!(is_fullscreen(&c.f), c.want, "{}", c.name);
        }
    }

    #[test]
    fn tolerance_is_two_pixels() {
        let f = |w: Rect| facts(w, M1, W1, POPUP, "Game");
        // Off by one or two in any direction still covers.
        assert!(is_fullscreen(&f(Rect::new(1, 1, 1919, 1079))));
        assert!(is_fullscreen(&f(Rect::new(2, 2, 1918, 1078))));
        assert!(is_fullscreen(&f(Rect::new(-1, -2, 1921, 1082))));
        // Three short on one side does not.
        assert!(!is_fullscreen(&f(Rect::new(3, 0, 1920, 1080))));
        assert!(!is_fullscreen(&f(Rect::new(0, 3, 1920, 1080))));
        assert!(!is_fullscreen(&f(Rect::new(0, 0, 1917, 1080))));
        assert!(!is_fullscreen(&f(Rect::new(0, 0, 1920, 1077))));
    }

    #[test]
    fn coverage_separates_work_area_from_monitor() {
        // Maximised with the taskbar showing: work area yes, monitor no.
        assert_eq!(
            coverage(Rect::new(-8, -8, 1928, 1048), M1, W1),
            Coverage::WorkArea
        );
        // Exactly the work area.
        assert_eq!(coverage(W1, M1, W1), Coverage::WorkArea);
        // The whole monitor.
        assert_eq!(coverage(M1, M1, W1), Coverage::Monitor);
        // With no taskbar the work area *is* the monitor.
        assert_eq!(coverage(M1, M1, M1), Coverage::Monitor);
        // A small window, and a window on another monitor.
        assert_eq!(
            coverage(Rect::new(10, 10, 300, 300), M1, W1),
            Coverage::None
        );
        assert_eq!(coverage(M2, M1, W1), Coverage::None);
        // Nothing to cover.
        assert_eq!(
            coverage(M1, Rect::default(), Rect::default()),
            Coverage::None
        );
    }

    #[test]
    fn caption_needs_both_bits() {
        let border_only = 0x0080_0000 | WS_POPUP; // WS_BORDER without WS_DLGFRAME
        assert!(is_fullscreen(&facts(M1, M1, W1, border_only, "Game")));
        assert!(!is_fullscreen(&facts(
            M1,
            M1,
            W1,
            WS_CAPTION | WS_POPUP,
            "Game"
        )));
    }

    #[test]
    fn only_the_pills_monitor_hides_the_pill() {
        let game_on_2 = facts(M2, M2, W2, POPUP, "Game");
        let game_on_1 = facts(M1, M1, W1, POPUP, "Game");
        let (mon1, mon2) = (0x1001, 0x1002);

        // Pill on monitor 1.
        assert!(
            !hides_pill(&game_on_2, mon2, mon1),
            "fullscreen on the other monitor"
        );
        assert!(
            hides_pill(&game_on_1, mon1, mon1),
            "fullscreen on the pill's monitor"
        );
        // Pill moved to monitor 2.
        assert!(hides_pill(&game_on_2, mon2, mon2));
        assert!(!hides_pill(&game_on_1, mon1, mon2));
    }

    #[test]
    fn same_monitor_but_not_fullscreen_keeps_the_pill() {
        let maximised = facts(Rect::new(-8, -8, 1928, 1048), M1, W1, MAXIMIZED, "Code");
        assert!(!hides_pill(&maximised, 7, 7));
    }

    #[test]
    fn unknown_monitor_never_hides() {
        let game = facts(M1, M1, W1, POPUP, "Game");
        assert!(!hides_pill(&game, 0, 0));
        assert!(!hides_pill(&game, 0, 5));
        assert!(!hides_pill(&game, 5, 0));
    }

    #[test]
    fn shell_class_list() {
        for c in [
            "Progman",
            "WorkerW",
            "Shell_TrayWnd",
            "Shell_SecondaryTrayWnd",
            "PROGMAN",
        ] {
            assert!(is_shell_class(c), "{c}");
        }
        for c in [
            "",
            "Chrome_WidgetWin_1",
            "UnityWndClass",
            "Notepad",
            "Progman2",
        ] {
            assert!(!is_shell_class(c), "{c}");
        }
    }

    #[cfg(not(all(windows, feature = "native")))]
    #[test]
    fn without_native_nothing_hides() {
        assert!(!foreground_fullscreen_on_pill_monitor(0));
        assert!(!foreground_fullscreen_on_pill_monitor(0x1234));
    }

    // The foreground window is whatever the desktop session has, so only the invariants that hold
    // for any window are asserted: the call must not panic, and Krovi's own pill handle (0 here:
    // no such window) must not make it hide.
    #[cfg(all(windows, feature = "native"))]
    #[test]
    fn native_call_is_safe_to_make() {
        let _ = foreground_fullscreen_on_pill_monitor(0);
    }
}
