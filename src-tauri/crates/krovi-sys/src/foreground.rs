//! Foreground-window sampling for screen-time (SPEC C11).
//!
//! The only thing read from the OS is the foreground process's **executable
//! name**. It deliberately reads **no window titles, no URLs, and no document
//! content** — those never enter Krovi. The normalized name plus an elapsed
//! duration are handed to `krovi_core::ScreenTime`, which aggregates per app per
//! day locally and reports nothing anywhere.
//!
//! The [`Sampler`] interval logic and the helpers are pure and tested (the
//! caller supplies `now_ms`, matching DECISIONS.md D-007). The `GetForegroundWindow`
//! call is isolated in the `native` submodule (CI-only).

/// Milliseconds in a day; the unit of the `day` index used by `ScreenTime`.
pub const MS_PER_DAY: u64 = 86_400_000;

/// Day index (days since the Unix epoch) for an epoch-millisecond timestamp.
/// Matches the `day` convention documented on `krovi_core::ScreenTime`.
pub fn day_index(epoch_ms: u64) -> u32 {
    (epoch_ms / MS_PER_DAY) as u32
}

/// Reduce an OS-reported path/handle to a bare executable name, e.g.
/// `C:\Program Files\App\app.exe` → `app.exe`. Carries no title or URL.
pub fn normalize_process(raw: &str) -> String {
    let name = raw.rsplit(['\\', '/']).next().unwrap_or(raw);
    name.trim().to_string()
}

/// Turns a stream of foreground-process samples into screen-time intervals.
///
/// Call [`Sampler::tick`] on a timer with the current foreground process (if
/// any) and the current time. Each tick credits the *previous* process with the
/// time elapsed since it became foreground, attributed to the day that interval
/// started on. Pure and deterministic — holds no clock of its own.
#[derive(Debug, Default)]
pub struct Sampler {
    last: Option<(String, u64)>,
}

impl Sampler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Close the previous interval and open a new one at `now_ms`.
    ///
    /// Returns `Some((day, app, ms))` to feed `ScreenTime::record`, or `None` on
    /// the first tick, when there was no previous foreground app, or when no
    /// time has elapsed.
    pub fn tick(&mut self, process: Option<String>, now_ms: u64) -> Option<(u32, String, u64)> {
        let emitted = match self.last.take() {
            Some((app, started)) => {
                let ms = now_ms.saturating_sub(started);
                if ms > 0 {
                    Some((day_index(started), app, ms))
                } else {
                    None
                }
            }
            None => None,
        };
        self.last = process.map(|p| (p, now_ms));
        emitted
    }
}

/// The current foreground process's executable name, if any.
///
/// Returns `Ok(None)` when there is no foreground window **and** in any build
/// without the `native` feature (so the demo build is a harmless no-op). The
/// real `GetForegroundWindow` read happens only under `native` on Windows.
#[cfg(all(windows, feature = "native"))]
pub fn foreground_process() -> crate::Result<Option<String>> {
    native::foreground_process()
}

/// See the native variant. Without `native`, no OS binding is compiled.
#[cfg(not(all(windows, feature = "native")))]
pub fn foreground_process() -> crate::Result<Option<String>> {
    Ok(None)
}

// ---------------------------------------------------------------------------
// Native implementation — CI-only.
//
// Authored against windows-rs 0.58. Not compiled/exercised on the local
// 32-bit-gcc host; no local test results claimed. Reads ONLY the executable
// name — never the window title or any document/URL content.
// ---------------------------------------------------------------------------
#[cfg(all(windows, feature = "native"))]
mod native {
    use crate::SysError;
    use windows::core::PWSTR;
    use windows::Win32::Foundation::{CloseHandle, MAX_PATH};
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

    fn err<E: std::fmt::Display>(e: E) -> SysError {
        SysError::Message(e.to_string())
    }

    pub(super) fn foreground_process() -> crate::Result<Option<String>> {
        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd.0.is_null() {
                return Ok(None);
            }
            let mut pid: u32 = 0;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid == 0 {
                return Ok(None);
            }
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).map_err(err)?;

            let mut buf = [0u16; MAX_PATH as usize];
            let mut len = buf.len() as u32;
            let query = QueryFullProcessImageNameW(
                handle,
                PROCESS_NAME_WIN32,
                PWSTR(buf.as_mut_ptr()),
                &mut len,
            );
            let _ = CloseHandle(handle);
            query.map_err(err)?;

            let full = String::from_utf16_lossy(&buf[..len as usize]);
            Ok(Some(super::normalize_process(&full)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn day_index_counts_days_since_epoch() {
        assert_eq!(day_index(0), 0);
        assert_eq!(day_index(MS_PER_DAY - 1), 0);
        assert_eq!(day_index(MS_PER_DAY), 1);
        assert_eq!(day_index(MS_PER_DAY * 20_000 + 123), 20_000);
    }

    #[test]
    fn normalize_strips_path_keeping_exe() {
        assert_eq!(normalize_process(r"C:\Program Files\App\app.exe"), "app.exe");
        assert_eq!(normalize_process("/usr/bin/code"), "code");
        assert_eq!(normalize_process("notepad.exe"), "notepad.exe");
        assert_eq!(normalize_process("  spaced.exe  "), "spaced.exe");
    }

    #[test]
    fn first_tick_emits_nothing() {
        let mut s = Sampler::new();
        assert_eq!(s.tick(Some("code.exe".into()), 1_000), None);
    }

    #[test]
    fn credits_previous_app_with_elapsed_time() {
        let mut s = Sampler::new();
        assert_eq!(s.tick(Some("code.exe".into()), 1_000), None);
        // 500ms later the foreground switched to browser: code gets the 500ms.
        assert_eq!(
            s.tick(Some("browser.exe".into()), 1_500),
            Some((day_index(1_000), "code.exe".to_string(), 500))
        );
        // another 250ms: browser gets 250ms.
        assert_eq!(
            s.tick(Some("code.exe".into()), 1_750),
            Some((day_index(1_500), "browser.exe".to_string(), 250))
        );
    }

    #[test]
    fn interval_is_attributed_to_the_day_it_started() {
        let mut s = Sampler::new();
        let near_midnight = MS_PER_DAY - 100;
        assert_eq!(s.tick(Some("app.exe".into()), near_midnight), None);
        // crosses into the next day; the whole span counts on the start day.
        let (day, app, ms) = s.tick(None, MS_PER_DAY + 400).unwrap();
        assert_eq!(day, 0);
        assert_eq!(app, "app.exe");
        assert_eq!(ms, 500);
    }

    #[test]
    fn no_elapsed_time_emits_nothing() {
        let mut s = Sampler::new();
        s.tick(Some("a.exe".into()), 1_000);
        assert_eq!(s.tick(Some("b.exe".into()), 1_000), None);
    }

    #[test]
    fn none_process_closes_the_interval() {
        let mut s = Sampler::new();
        s.tick(Some("a.exe".into()), 0);
        assert_eq!(s.tick(None, 100), Some((0, "a.exe".to_string(), 100)));
        // nothing foreground now → next tick has nothing to credit.
        assert_eq!(s.tick(None, 200), None);
    }

    #[cfg(not(all(windows, feature = "native")))]
    #[test]
    fn foreground_is_noop_without_native() {
        assert_eq!(foreground_process().unwrap(), None);
    }
}
