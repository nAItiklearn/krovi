//! Foreground-window sampling for screen-time (SPEC C11).
//!
//! The only things read from the OS are the foreground process's **executable
//! name** and whether a person is at the machine (time since the last input event,
//! and whether the session is locked). It deliberately reads **no window titles,
//! no URLs, no document content and no keystrokes** — the "last input" call returns
//! only a tick count, never what was typed or clicked. The normalized name plus an
//! elapsed duration are handed to `krovi_core::ScreenTime`, which aggregates per app
//! per day locally and reports nothing anywhere.
//!
//! What counts as time spent in an app:
//! * **Local calendar days.** Time is bucketed by the user's local day (the caller
//!   passes the zone offset in effect at the sample), and an interval that spans local
//!   midnight is split between the two days.
//! * **Only continuous sampling is credited.** A gap between two ticks longer than
//!   [`MAX_CREDIT_GAP_MS`] means the machine slept or the process stalled; that time
//!   is dropped, never handed to whichever app happened to be in front before.
//! * **Only while present.** No time is credited while the session is locked or the
//!   user has been idle for [`IDLE_THRESHOLD_MS`] or more.
//!
//! The [`Sampler`] interval logic and every decision above are pure and tested (the
//! caller supplies `now_ms`, the zone offset and the presence reading, matching
//! DECISIONS.md D-007). The OS reads are isolated in the `native` submodule (CI-only).

/// Milliseconds in a day; the unit of the `day` index used by `ScreenTime`.
pub const MS_PER_DAY: u64 = 86_400_000;

/// How often the app samples the foreground (the live hub's tick).
pub const SAMPLE_INTERVAL_MS: u64 = 5_000;

/// The longest span between two ticks that is still credited: two sample intervals,
/// which leaves room for timer jitter and a briefly busy machine but is far below any
/// sleep/resume or hibernate gap.
pub const MAX_CREDIT_GAP_MS: u64 = 2 * SAMPLE_INTERVAL_MS;

/// No input for this long means the user has stepped away (5 minutes).
pub const IDLE_THRESHOLD_MS: u64 = 5 * 60 * 1000;

/// Day index (days since 1970-01-01) of the **local** calendar day containing
/// `epoch_ms`, where `offset_ms` is the zone offset (local minus UTC, in
/// milliseconds, DST included) in effect at that instant. Instants before 1970 local
/// time clamp to day 0.
pub fn local_day_index(epoch_ms: u64, offset_ms: i64) -> u32 {
    let local = (epoch_ms as i64).saturating_add(offset_ms);
    local.div_euclid(MS_PER_DAY as i64).max(0) as u32
}

/// Reduce an OS-reported path/handle to a bare executable name, e.g.
/// `C:\Program Files\App\app.exe` → `app.exe`. Carries no title or URL.
pub fn normalize_process(raw: &str) -> String {
    let name = raw.rsplit(['\\', '/']).next().unwrap_or(raw);
    name.trim().to_string()
}

// ---------------------------------------------------------------------------
// Presence: is a person actually at the machine?
// ---------------------------------------------------------------------------

/// What the OS says about the user right now. Carries no content — only how long ago
/// the last input event was and whether the interactive desktop is locked/secure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Presence {
    /// Milliseconds since the last keyboard/mouse/touch input.
    pub idle_ms: u64,
    /// The input desktop is not the user's desktop (lock screen, UAC prompt,
    /// Ctrl+Alt+Del, screen saver, or a disconnected session).
    pub locked: bool,
}

/// Whether `name` is the user's normal interactive desktop (Windows calls it
/// `Default`). Anything else — `Winlogon` (lock screen / UAC / Ctrl+Alt+Del),
/// `Screen-saver` — is not the user working.
pub fn is_default_desktop(name: &str) -> bool {
    name.eq_ignore_ascii_case("default")
}

/// Whether a foreground process is the lock screen or sign-in UI itself. Belt and
/// braces next to [`Presence::locked`]: Windows can report the lock screen app as the
/// foreground window for a moment around a lock.
pub fn is_lock_screen_process(process: &str) -> bool {
    process.eq_ignore_ascii_case("lockapp.exe") || process.eq_ignore_ascii_case("logonui.exe")
}

/// Whether time should be credited right now: the session is unlocked, the user has
/// given input within `idle_threshold_ms`, and the foreground is not the lock screen.
pub fn is_present(presence: Presence, foreground: Option<&str>, idle_threshold_ms: u64) -> bool {
    !presence.locked
        && presence.idle_ms < idle_threshold_ms
        && !foreground.is_some_and(is_lock_screen_process)
}

// ---------------------------------------------------------------------------
// The interval sampler
// ---------------------------------------------------------------------------

/// A span of foreground time to add to `ScreenTime`: `ms` for `app` on local `day`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credit {
    pub day: u32,
    pub app: String,
    pub ms: u64,
}

/// The foreground interval currently open.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Open {
    app: String,
    started_ms: u64,
    /// Zone offset in effect when the interval started; used to place it on a day.
    offset_ms: i64,
}

/// Turns a stream of foreground-process samples into screen-time credits.
///
/// Call [`Sampler::tick`] on a timer with the current foreground process (if any),
/// the current time, the zone offset at that time and the presence reading. Each tick
/// credits the *previous* process with the time since it became foreground — but only
/// if that span is a continuous one (see [`MAX_CREDIT_GAP_MS`]) and the user is
/// present now. Pure and deterministic — holds no clock of its own.
#[derive(Debug)]
pub struct Sampler {
    last: Option<Open>,
    max_gap_ms: u64,
    idle_threshold_ms: u64,
}

impl Default for Sampler {
    fn default() -> Self {
        Self::with_limits(MAX_CREDIT_GAP_MS, IDLE_THRESHOLD_MS)
    }
}

impl Sampler {
    pub fn new() -> Self {
        Self::default()
    }

    /// A sampler with explicit limits (the app uses [`Sampler::new`]).
    pub fn with_limits(max_gap_ms: u64, idle_threshold_ms: u64) -> Self {
        Self {
            last: None,
            max_gap_ms,
            idle_threshold_ms,
        }
    }

    /// Close the previous interval and open a new one at `now_ms`.
    ///
    /// `offset_ms` is the local zone offset (east of UTC, DST included) at `now_ms`; it
    /// is remembered with the interval it opens, so that interval is placed on a local
    /// day using the offset valid when it began. (A DST change inside one ≤10 s
    /// interval can misplace at most that interval around midnight; not worth more.)
    ///
    /// Returns the credits for the closed interval for `ScreenTime::record`: usually
    /// one, two when the interval straddled local midnight, none on the first tick,
    /// when there was no previous foreground app, when no time elapsed (or the clock
    /// went backwards), when the gap exceeded the cap (sleep/resume), or when the user
    /// is idle/away now. While away nothing stays open, so the first present tick
    /// afterwards starts a fresh interval and the away time is never back-filled.
    pub fn tick(
        &mut self,
        process: Option<String>,
        now_ms: u64,
        offset_ms: i64,
        presence: Presence,
    ) -> Vec<Credit> {
        let previous = self.last.take();
        let mut credits = Vec::new();
        if !is_present(presence, process.as_deref(), self.idle_threshold_ms) {
            return credits;
        }
        if let Some(open) = previous {
            let ms = now_ms.saturating_sub(open.started_ms);
            if ms > 0 && ms <= self.max_gap_ms {
                split_across_local_days(&open, ms, &mut credits);
            }
        }
        self.last = process.map(|app| Open {
            app,
            started_ms: now_ms,
            offset_ms,
        });
        credits
    }
}

/// Spread `ms` starting at `open.started_ms` over the local days it covers.
fn split_across_local_days(open: &Open, ms: u64, out: &mut Vec<Credit>) {
    let day_len = MS_PER_DAY as i64;
    let mut local = (open.started_ms as i64).saturating_add(open.offset_ms);
    let mut remaining = ms;
    while remaining > 0 {
        let left_in_day = (day_len - local.rem_euclid(day_len)) as u64; // always >= 1
        let take = remaining.min(left_in_day);
        out.push(Credit {
            day: local.div_euclid(day_len).max(0) as u32,
            app: open.app.clone(),
            ms: take,
        });
        remaining -= take;
        local = local.saturating_add(take as i64);
    }
}

// ---------------------------------------------------------------------------
// OS entry points (inert without the `native` feature)
// ---------------------------------------------------------------------------

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

/// Whether a person is at the machine: time since the last input event (`GetLastInputInfo`,
/// a tick count — never the input itself) and whether the input desktop is locked
/// (`OpenInputDesktop` + its name). Cheap (two user32 calls), safe to call every tick.
#[cfg(all(windows, feature = "native"))]
pub fn presence() -> Presence {
    native::presence()
}

/// See the native variant. Without `native` there is nothing to read, so the user is
/// reported as present (and `foreground_process` reports no app, so nothing is credited).
#[cfg(not(all(windows, feature = "native")))]
pub fn presence() -> Presence {
    Presence::default()
}

// ---------------------------------------------------------------------------
// Native implementation — CI-only.
//
// Authored against windows-rs 0.58 (signatures checked against the 0.58.0 crate
// source). Not linked on the local GNU host; no local runtime results claimed. Reads
// ONLY the executable name, the last-input tick count and the input desktop's name —
// never the window title, any document/URL content, or any input itself.
// ---------------------------------------------------------------------------
#[cfg(all(windows, feature = "native"))]
mod native {
    use super::Presence;
    use crate::SysError;
    use windows::core::PWSTR;
    use windows::Win32::Foundation::{CloseHandle, BOOL, HANDLE, MAX_PATH};
    use windows::Win32::System::StationsAndDesktops::{
        CloseDesktop, GetUserObjectInformationW, OpenInputDesktop, DESKTOP_CONTROL_FLAGS,
        DESKTOP_READOBJECTS, UOI_NAME,
    };
    use windows::Win32::System::SystemInformation::GetTickCount;
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
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

    pub(super) fn presence() -> Presence {
        Presence {
            idle_ms: idle_ms().unwrap_or(0),
            locked: session_locked(),
        }
    }

    /// Milliseconds since the last input event, or `None` if Windows will not say.
    /// `GetLastInputInfo` and `GetTickCount` are both 32-bit tick counts that wrap
    /// together every ~49.7 days; the wrapping subtraction is correct across the wrap.
    fn idle_ms() -> Option<u64> {
        let mut info = LASTINPUTINFO {
            cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
            dwTime: 0,
        };
        // SAFETY: `info` is a valid, correctly sized LASTINPUTINFO for the call.
        unsafe {
            if !GetLastInputInfo(&mut info).as_bool() {
                return None;
            }
            Some(u64::from(GetTickCount().wrapping_sub(info.dwTime)))
        }
    }

    /// Whether the input desktop is anything other than the user's `Default` desktop.
    /// A normal process cannot open the secure `Winlogon` desktop that shows the lock
    /// screen / UAC prompt / Ctrl+Alt+Del, so a failed open means "not the user's
    /// desktop". If the open succeeds but the name cannot be read, the user is assumed
    /// present (never silently zero out tracking on an odd failure).
    fn session_locked() -> bool {
        // SAFETY: the desktop handle is used only inside this block and always closed;
        // the buffer outlives the call and its byte length is passed alongside it.
        unsafe {
            let desktop =
                match OpenInputDesktop(DESKTOP_CONTROL_FLAGS(0), BOOL(0), DESKTOP_READOBJECTS) {
                    Ok(d) => d,
                    Err(_) => return true,
                };
            let mut name = [0u16; 128];
            let mut needed = 0u32;
            let read = GetUserObjectInformationW(
                HANDLE(desktop.0),
                UOI_NAME,
                Some(name.as_mut_ptr().cast()),
                (name.len() * std::mem::size_of::<u16>()) as u32,
                Some(&mut needed),
            );
            let _ = CloseDesktop(desktop);
            if read.is_err() {
                return false;
            }
            let end = name.iter().position(|&c| c == 0).unwrap_or(name.len());
            !super::is_default_desktop(&String::from_utf16_lossy(&name[..end]))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const IST: i64 = 5 * 3_600_000 + 30 * 60_000; // UTC+05:30
    const PST: i64 = -8 * 3_600_000; // UTC-08:00
    const HERE: Presence = Presence {
        idle_ms: 0,
        locked: false,
    };

    fn tick(s: &mut Sampler, p: Option<&str>, now: u64) -> Vec<Credit> {
        s.tick(p.map(String::from), now, 0, HERE)
    }

    fn credit(day: u32, app: &str, ms: u64) -> Credit {
        Credit {
            day,
            app: app.to_string(),
            ms,
        }
    }

    // ---- local day buckets ----------------------------------------------------

    #[test]
    fn local_day_index_counts_days_since_epoch_in_utc_when_offset_is_zero() {
        assert_eq!(local_day_index(0, 0), 0);
        assert_eq!(local_day_index(MS_PER_DAY - 1, 0), 0);
        assert_eq!(local_day_index(MS_PER_DAY, 0), 1);
        assert_eq!(local_day_index(MS_PER_DAY * 20_000 + 123, 0), 20_000);
    }

    #[test]
    fn the_local_day_turns_over_at_local_midnight_not_at_utc_midnight() {
        // 20_000 days after the epoch, UTC midnight
        let d = 20_000 * MS_PER_DAY;
        // India: local midnight is 18:30 UTC the evening before
        let ist_midnight = d + MS_PER_DAY - IST as u64; // 18:30 UTC on UTC-day 20_000
                                                        // 23:59:59.999 IST is still day 20_000; 00:00:00.000 IST is day 20_001
        assert_eq!(local_day_index(ist_midnight - 1, IST), 20_000);
        assert_eq!(local_day_index(ist_midnight, IST), 20_001);
        // a UTC bucket would still call that same instant day 20_000
        assert_eq!(local_day_index(ist_midnight, 0), 20_000);
        // 18:30 UTC..24:00 UTC is already "tomorrow" in India
        assert_eq!(local_day_index(d + MS_PER_DAY - 1, IST), 20_001);
    }

    #[test]
    fn a_zone_west_of_utc_lags_behind_utc() {
        // 06:00 UTC on day 20_001 is 22:00 the previous evening in UTC-8
        let t = 20_001 * MS_PER_DAY + 6 * 3_600_000;
        assert_eq!(local_day_index(t, 0), 20_001);
        assert_eq!(local_day_index(t, PST), 20_000);
        // local midnight in UTC-8 is 08:00 UTC
        assert_eq!(
            local_day_index(20_001 * MS_PER_DAY + 8 * 3_600_000, PST),
            20_001
        );
    }

    #[test]
    fn local_day_index_clamps_before_the_epoch() {
        assert_eq!(local_day_index(0, PST), 0);
        assert_eq!(local_day_index(1_000, -1_000_000_000), 0);
    }

    // ---- process names -----------------------------------------------------------

    #[test]
    fn normalize_strips_path_keeping_exe() {
        assert_eq!(
            normalize_process(r"C:\Program Files\App\app.exe"),
            "app.exe"
        );
        assert_eq!(normalize_process("/usr/bin/code"), "code");
        assert_eq!(normalize_process("notepad.exe"), "notepad.exe");
        assert_eq!(normalize_process("  spaced.exe  "), "spaced.exe");
    }

    // ---- basic interval credit (unchanged behaviour) --------------------------------

    #[test]
    fn first_tick_emits_nothing() {
        let mut s = Sampler::new();
        assert!(tick(&mut s, Some("code.exe"), 1_000).is_empty());
    }

    #[test]
    fn credits_previous_app_with_elapsed_time() {
        let mut s = Sampler::new();
        assert!(tick(&mut s, Some("code.exe"), 1_000).is_empty());
        // 500ms later the foreground switched to browser: code gets the 500ms.
        assert_eq!(
            tick(&mut s, Some("browser.exe"), 1_500),
            vec![credit(0, "code.exe", 500)]
        );
        // another 250ms: browser gets 250ms.
        assert_eq!(
            tick(&mut s, Some("code.exe"), 1_750),
            vec![credit(0, "browser.exe", 250)]
        );
    }

    #[test]
    fn no_elapsed_time_emits_nothing() {
        let mut s = Sampler::new();
        tick(&mut s, Some("a.exe"), 1_000);
        assert!(tick(&mut s, Some("b.exe"), 1_000).is_empty());
    }

    #[test]
    fn none_process_closes_the_interval() {
        let mut s = Sampler::new();
        tick(&mut s, Some("a.exe"), 0);
        assert_eq!(tick(&mut s, None, 100), vec![credit(0, "a.exe", 100)]);
        // nothing foreground now → next tick has nothing to credit.
        assert!(tick(&mut s, None, 200).is_empty());
    }

    #[test]
    fn a_clock_that_went_backwards_credits_nothing_and_recovers() {
        let mut s = Sampler::new();
        tick(&mut s, Some("a.exe"), 10_000);
        assert!(tick(&mut s, Some("a.exe"), 4_000).is_empty());
        assert_eq!(
            tick(&mut s, Some("a.exe"), 7_000),
            vec![credit(0, "a.exe", 3_000)]
        );
    }

    // ---- local days: the midnight split ---------------------------------------------

    #[test]
    fn an_interval_across_local_midnight_is_split_between_the_two_local_days() {
        // India. Local midnight of day 20_001 = 18:30:00 UTC on UTC-day 20_000.
        let midnight = 20_000 * MS_PER_DAY + MS_PER_DAY - IST as u64;
        let mut s = Sampler::new();
        // 23:59:57 local ...
        assert!(s
            .tick(Some("app.exe".into()), midnight - 3_000, IST, HERE)
            .is_empty());
        // ... to 00:00:02 local: 3 s belong to day 20_000, 2 s to day 20_001
        let credits = s.tick(Some("app.exe".into()), midnight + 2_000, IST, HERE);
        assert_eq!(
            credits,
            vec![
                credit(20_000, "app.exe", 3_000),
                credit(20_001, "app.exe", 2_000)
            ]
        );
        assert_eq!(
            credits.iter().map(|c| c.ms).sum::<u64>(),
            5_000,
            "nothing lost or doubled"
        );
    }

    #[test]
    fn utc_midnight_is_not_a_boundary_for_a_user_in_india() {
        // 00:00:00 UTC = 05:30 in India: an interval around it stays on one local day
        let utc_midnight = 20_001 * MS_PER_DAY;
        let mut s = Sampler::new();
        s.tick(Some("app.exe".into()), utc_midnight - 2_000, IST, HERE);
        assert_eq!(
            s.tick(Some("app.exe".into()), utc_midnight + 3_000, IST, HERE),
            vec![credit(20_001, "app.exe", 5_000)],
            "a UTC bucket would have credited day 20_000 or split it here"
        );
    }

    #[test]
    fn an_interval_that_ends_exactly_at_local_midnight_stays_on_its_day() {
        let midnight = 20_000 * MS_PER_DAY + MS_PER_DAY - IST as u64;
        let mut s = Sampler::new();
        s.tick(Some("a.exe".into()), midnight - 5_000, IST, HERE);
        assert_eq!(
            s.tick(Some("a.exe".into()), midnight, IST, HERE),
            vec![credit(20_000, "a.exe", 5_000)]
        );
    }

    #[test]
    fn a_west_of_utc_user_gets_the_split_at_their_own_midnight() {
        // UTC-8: local midnight of day 20_001 is 08:00 UTC on UTC-day 20_001
        let midnight = 20_001 * MS_PER_DAY + 8 * 3_600_000;
        let mut s = Sampler::new();
        s.tick(Some("a.exe".into()), midnight - 1_000, PST, HERE);
        assert_eq!(
            s.tick(Some("b.exe".into()), midnight + 4_000, PST, HERE),
            vec![
                credit(20_000, "a.exe", 1_000),
                credit(20_001, "a.exe", 4_000)
            ]
        );
    }

    #[test]
    fn the_interval_is_placed_with_the_offset_it_started_under() {
        // start 23:59:58 under UTC+0, then the zone jumps (DST) before the next tick:
        // the span is still placed with the offset valid when it began
        let t0 = 20_000 * MS_PER_DAY + MS_PER_DAY - 2_000; // 23:59:58 UTC, offset 0
        let mut s = Sampler::new();
        s.tick(Some("a.exe".into()), t0, 0, HERE);
        assert_eq!(
            s.tick(Some("a.exe".into()), t0 + 4_000, 3_600_000, HERE),
            vec![
                credit(20_000, "a.exe", 2_000),
                credit(20_001, "a.exe", 2_000)
            ]
        );
    }

    // ---- sleep / resume: gaps are never credited -----------------------------------

    #[test]
    fn a_sleep_resume_gap_is_not_credited_to_the_app_that_was_in_front() {
        let mut s = Sampler::new();
        tick(&mut s, Some("code.exe"), 0);
        assert_eq!(
            tick(&mut s, Some("code.exe"), 5_000),
            vec![credit(0, "code.exe", 5_000)]
        );
        // lid closed, laptop asleep for 8 hours; the next tick is the first after resume
        let resume = 5_000 + 8 * 3_600_000;
        assert!(
            tick(&mut s, Some("browser.exe"), resume).is_empty(),
            "the 8 h gap must not be credited to code.exe"
        );
        // the app in front on resume starts a fresh interval and earns its own time
        assert_eq!(
            tick(&mut s, Some("browser.exe"), resume + 5_000),
            vec![credit(0, "browser.exe", 5_000)]
        );
    }

    #[test]
    fn the_cap_is_two_sample_intervals_and_is_inclusive() {
        assert_eq!(MAX_CREDIT_GAP_MS, 2 * SAMPLE_INTERVAL_MS);
        let mut s = Sampler::new();
        tick(&mut s, Some("a.exe"), 0);
        // a late tick (busy machine) right at the cap is still real time
        assert_eq!(
            tick(&mut s, Some("a.exe"), MAX_CREDIT_GAP_MS),
            vec![credit(0, "a.exe", MAX_CREDIT_GAP_MS)]
        );
        // one millisecond more is treated as a discontinuity
        assert!(tick(&mut s, Some("a.exe"), 2 * MAX_CREDIT_GAP_MS + 1).is_empty());
    }

    #[test]
    fn a_gap_that_would_cross_many_midnights_still_credits_nothing() {
        let mut s = Sampler::new();
        tick(&mut s, Some("a.exe"), 0);
        assert!(tick(&mut s, Some("a.exe"), 3 * MS_PER_DAY).is_empty());
    }

    // ---- idle / locked: nothing is credited while away --------------------------------

    #[test]
    fn presence_decision_is_locked_idle_and_lock_screen_aware() {
        let at = |idle_ms, locked| Presence { idle_ms, locked };
        let t = IDLE_THRESHOLD_MS;
        assert!(is_present(at(0, false), Some("code.exe"), t));
        assert!(
            is_present(at(t - 1, false), Some("code.exe"), t),
            "just under the threshold"
        );
        assert!(
            !is_present(at(t, false), Some("code.exe"), t),
            "exactly at the threshold is away"
        );
        assert!(!is_present(at(t * 4, false), Some("code.exe"), t));
        assert!(!is_present(at(0, true), Some("code.exe"), t), "locked");
        assert!(
            !is_present(at(0, false), Some("LockApp.exe"), t),
            "lock screen app"
        );
        assert!(!is_present(at(0, false), Some("logonui.exe"), t));
        assert!(
            is_present(at(0, false), None, t),
            "no foreground is not away"
        );
        assert!(
            is_present(at(0, false), Some("lockapp-notes.exe"), t),
            "exact names only"
        );
    }

    #[test]
    fn the_input_desktop_name_decides_locked() {
        assert!(is_default_desktop("Default"));
        assert!(is_default_desktop("default"));
        assert!(!is_default_desktop("Winlogon"));
        assert!(!is_default_desktop("Screen-saver"));
        assert!(!is_default_desktop(""));
    }

    #[test]
    fn five_minutes_idle_stops_the_clock_and_return_does_not_backfill() {
        let mut s = Sampler::new();
        let idle = |ms| Presence {
            idle_ms: ms,
            locked: false,
        };
        s.tick(Some("code.exe".into()), 0, 0, idle(1_000));
        // still typing now and then: credited
        assert_eq!(
            s.tick(Some("code.exe".into()), 5_000, 0, idle(2_000)),
            vec![credit(0, "code.exe", 5_000)]
        );
        // 5 minutes without input: this span is not credited, nothing stays open
        assert!(s
            .tick(Some("code.exe".into()), 10_000, 0, idle(IDLE_THRESHOLD_MS))
            .is_empty());
        assert!(s
            .tick(
                Some("code.exe".into()),
                15_000,
                0,
                idle(IDLE_THRESHOLD_MS + 5_000)
            )
            .is_empty());
        // the user comes back: still no back-fill for the idle stretch...
        assert!(s
            .tick(Some("code.exe".into()), 20_000, 0, idle(0))
            .is_empty());
        // ...and time counts again from here
        assert_eq!(
            s.tick(Some("code.exe".into()), 25_000, 0, idle(500)),
            vec![credit(0, "code.exe", 5_000)]
        );
    }

    #[test]
    fn a_locked_session_credits_nothing_until_unlock() {
        let mut s = Sampler::new();
        let locked = Presence {
            idle_ms: 0,
            locked: true,
        };
        s.tick(Some("code.exe".into()), 0, 0, HERE);
        assert!(s.tick(Some("code.exe".into()), 5_000, 0, locked).is_empty());
        assert!(s.tick(None, 10_000, 0, locked).is_empty());
        assert!(s.tick(Some("code.exe".into()), 15_000, 0, HERE).is_empty());
        assert_eq!(
            s.tick(Some("code.exe".into()), 20_000, 0, HERE),
            vec![credit(0, "code.exe", 5_000)]
        );
    }

    #[test]
    fn the_lock_screen_process_itself_is_never_credited_nor_credited_to() {
        let mut s = Sampler::new();
        s.tick(Some("code.exe".into()), 0, 0, HERE);
        // the lock screen shows up as the foreground app before `locked` flips
        assert!(s
            .tick(Some("LockApp.exe".into()), 5_000, 0, HERE)
            .is_empty());
        // and it does not leave an interval open for later
        assert!(s
            .tick(Some("LockApp.exe".into()), 10_000, 0, HERE)
            .is_empty());
        assert!(s.tick(Some("code.exe".into()), 15_000, 0, HERE).is_empty());
    }

    #[test]
    fn custom_limits_are_respected() {
        let mut s = Sampler::with_limits(1_000, 10_000);
        let idle = Presence {
            idle_ms: 10_000,
            locked: false,
        };
        s.tick(Some("a.exe".into()), 0, 0, HERE);
        assert!(
            s.tick(Some("a.exe".into()), 1_001, 0, HERE).is_empty(),
            "over the 1 s cap"
        );
        assert_eq!(
            s.tick(Some("a.exe".into()), 2_001, 0, HERE),
            vec![credit(0, "a.exe", 1_000)]
        );
        assert!(
            s.tick(Some("a.exe".into()), 2_500, 0, idle).is_empty(),
            "at the 10 s idle limit"
        );
    }

    // ---- privacy: the public data carries no titles --------------------------------

    #[test]
    fn credits_hold_only_a_day_an_executable_name_and_a_duration() {
        // Credit has exactly three fields; the app is the bare exe name from the path.
        let c = Credit {
            day: 1,
            app: normalize_process(r"C:\Users\me\Documents\secret plans.docx - Word\winword.exe"),
            ms: 1,
        };
        assert_eq!(c.app, "winword.exe");
    }

    #[cfg(not(all(windows, feature = "native")))]
    #[test]
    fn foreground_and_presence_are_inert_without_native() {
        assert_eq!(foreground_process().unwrap(), None);
        assert_eq!(presence(), Presence::default());
        assert!(is_present(presence(), None, IDLE_THRESHOLD_MS));
    }
}
