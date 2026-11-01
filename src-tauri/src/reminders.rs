//! Reminders & alarms (owner: reminders). docs/CONTRACTS.md §8.
//!
//! A reminder is a calendar event with a time. One background thread (`krovi-reminders`)
//! sleeps until the earliest due reminder, then fires a real Windows toast
//! (`tauri-plugin-notification`, called from Rust: no webview permission involved), plays
//! the system reminder sound if the reminder asks for it, and emits `reminder-fired` for
//! the in-app banner. It works whatever the window mode (pill, panel or hidden).
//!
//! * **Never busy-polls.** It waits on a condvar for `min(time to the next alert, 30 s)`.
//!   The 30 s cap is what picks up clock changes, DST jumps and sleep/resume; the condvar
//!   is what wakes it at once when an event is added, removed, snoozed or dismissed
//!   ([`wake_scheduler`]).
//! * **DST-correct.** Due times come from the wall-clock `date` + `time` and the OS time
//!   zone ([`OsZone`], `SystemTimeToTzSpecificLocalTime`), recomputed every pass, so a 09:00
//!   reminder stays at 09:00 across a DST change and a skipped/repeated hour is handled
//!   (the maths and its tests live in `krovi_core::calendar`).
//! * **Never double-fires.** `Calendar::collect_due` marks a reminder fired in the same
//!   step that hands it out, and the mark is persisted *before* the toast is shown, so a
//!   crash or restart cannot ring it again.
//! * **Missed while closed.** On the first pass after start-up anything already due fires
//!   once, marked `missed`; anything more than 24 h overdue is only marked, not announced;
//!   four or more at once collapse into a single summary toast.
//! * **Toast identity.** `tauri-plugin-notification` sets the AppUserModelID to the app
//!   identifier (`tech.krovi.app`) only when the exe is *not* in a `target\debug|release`
//!   folder. The NSIS installer's Start-menu shortcut carries that AUMID, so installed
//!   builds show as "Krovi". A dev build runs from `target\debug`, so Windows shows the toast
//!   as "Windows PowerShell" (the plugin's deliberate fallback). [`probe_environment`]
//!   reports which applies, whether Windows has toasts enabled for it, and Focus Assist /
//!   Do Not Disturb, in `reminders_status`.
//! * **Toast buttons.** Snooze 5 min / Snooze 10 min / Dismiss, handled in Rust
//!   (`on_action`); clicking the toast body opens the Calendar.
//!
//! This crate's own test binary cannot start on Windows (the `tauri` crate needs the app
//! manifest at load time), so everything decidable without Tauri lives in
//! `krovi_core::calendar` (tested there) and this file stays wiring.

use std::io::Write as _;
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use serde::Serialize;
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_notification::{Action, ActionPerformed, ActionType, NotificationExt};

use krovi_core::calendar::{
    alert_text, format_local_ms, Calendar, DueReminder, ReminderOpts, ZoneOffset,
};
use krovi_store::wire::{calendar_dtos, CalendarDto, ReminderFired};

use crate::commands::{lock_err, now_ms, CmdResult};
use crate::state::AppState;

/// Longest the scheduler sleeps without re-checking the clock.
const MAX_SLEEP_MS: u64 = 30_000;
/// This many (or more) non-stale alerts in one pass share a single summary toast.
const GROUP_AT: usize = 4;
/// Id of the toast button set (Snooze 5 / Snooze 10 / Dismiss).
const ACTION_TYPE: &str = "reminder";
/// Snooze range accepted from the UI / toast (minutes). Fractions are allowed for QA.
const MIN_SNOOZE_MIN: f64 = 0.05;
const MAX_SNOOZE_MIN: f64 = 24.0 * 60.0;

// ---------------------------------------------------------------------------------------
// OS bits (Windows). Non-Windows hosts compile with inert stand-ins so the workspace
// still builds there; Krovi itself is Windows-first.
// ---------------------------------------------------------------------------------------

/// The user's time zone, answered by Windows.
pub struct OsZone;

impl ZoneOffset for OsZone {
    fn offset_ms_at(&self, utc_ms: i64) -> i64 {
        os::offset_ms_at(utc_ms)
    }
}

#[cfg(windows)]
mod os {
    use windows::core::{w, HSTRING};
    use windows::Win32::Foundation::{ERROR_SUCCESS, FILETIME, HMODULE, SYSTEMTIME};
    use windows::Win32::Media::Audio::{PlaySoundW, SND_ALIAS, SND_ASYNC, SND_NODEFAULT};
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
    use windows::Win32::System::Time::{
        FileTimeToSystemTime, SystemTimeToFileTime, SystemTimeToTzSpecificLocalTime,
    };
    use windows::Win32::UI::Shell::{
        SHQueryUserNotificationState, QUNS_ACCEPTS_NOTIFICATIONS, QUNS_APP, QUNS_BUSY,
        QUNS_NOT_PRESENT, QUNS_PRESENTATION_MODE, QUNS_QUIET_TIME, QUNS_RUNNING_D3D_FULL_SCREEN,
    };
    use windows::UI::Notifications::ToastNotificationManager;

    const EPOCH_1601_TO_1970_MS: i64 = 11_644_473_600_000;

    /// Local offset (ms east of UTC) in effect at a UTC instant, DST included.
    pub fn offset_ms_at(utc_ms: i64) -> i64 {
        let ticks = (utc_ms + EPOCH_1601_TO_1970_MS).saturating_mul(10_000);
        if ticks <= 0 {
            return 0;
        }
        let ticks = ticks as u64;
        let ft = FILETIME {
            dwLowDateTime: ticks as u32,
            dwHighDateTime: (ticks >> 32) as u32,
        };
        let (mut utc, mut local, mut lft) = (
            SYSTEMTIME::default(),
            SYSTEMTIME::default(),
            FILETIME::default(),
        );
        // SAFETY: plain value structs passed by reference for the call's duration.
        let ok = unsafe {
            FileTimeToSystemTime(&ft, &mut utc).is_ok()
                && SystemTimeToTzSpecificLocalTime(None, &utc, &mut local).is_ok()
                && SystemTimeToFileTime(&local, &mut lft).is_ok()
        };
        if !ok {
            return 0;
        }
        let local_ticks = (u64::from(lft.dwHighDateTime) << 32) | u64::from(lft.dwLowDateTime);
        (local_ticks as i64 - ticks as i64) / 10_000
    }

    /// Play Windows' own reminder sound (falls back to the asterisk) without blocking.
    pub fn play_alert_sound() -> &'static str {
        let flags = SND_ALIAS | SND_ASYNC | SND_NODEFAULT;
        // SAFETY: static NUL-terminated wide aliases; no module handle is needed.
        unsafe {
            if PlaySoundW(w!("Notification.Reminder"), HMODULE::default(), flags).as_bool() {
                "Notification.Reminder"
            } else if PlaySoundW(w!("SystemAsterisk"), HMODULE::default(), flags).as_bool() {
                "SystemAsterisk"
            } else {
                "failed"
            }
        }
    }

    /// A DWORD under HKCU, or `None` if the key/value does not exist.
    fn hkcu_dword(subkey: &str, value: &str) -> Option<u32> {
        let (key, name) = (HSTRING::from(subkey), HSTRING::from(value));
        let mut data = 0u32;
        let mut size = std::mem::size_of::<u32>() as u32;
        // SAFETY: `data`/`size` outlive the call and `size` is the byte size of `data`.
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                &key,
                &name,
                RRF_RT_REG_DWORD,
                None,
                Some(std::ptr::addr_of_mut!(data).cast()),
                Some(&mut size),
            )
        };
        (status == ERROR_SUCCESS).then_some(data)
    }

    /// Windows' verdict on toasts for an AppUserModelID: the Settings > Notifications switches
    /// (global, then per app) from the registry, else the WinRT `ToastNotifier.Setting`, which
    /// fails for unpackaged apps with an unregistered id (then `unknown`).
    pub fn toast_setting(aumid: &str) -> &'static str {
        const PUSH: &str = r"Software\Microsoft\Windows\CurrentVersion\PushNotifications";
        const APPS: &str = r"Software\Microsoft\Windows\CurrentVersion\Notifications\Settings";
        if hkcu_dword(PUSH, "ToastEnabled") == Some(0) {
            return "disabled-in-settings";
        }
        // (an id containing backslashes, like the PowerShell fallback, is stored as nested keys,
        // which is exactly what this path spells)
        if hkcu_dword(&format!(r"{APPS}\{aumid}"), "Enabled") == Some(0) {
            return "disabled-for-app";
        }
        let id = HSTRING::from(aumid);
        match ToastNotificationManager::CreateToastNotifierWithId(&id).and_then(|n| n.Setting()) {
            Ok(s) => match s.0 {
                0 => "enabled",
                1 => "disabled-for-app",
                2 => "disabled-for-user",
                3 => "disabled-by-policy",
                4 => "disabled-by-manifest",
                _ => "unknown",
            },
            Err(_) => "unknown",
        }
    }

    #[link(name = "ntdll")]
    extern "system" {
        /// Undocumented but stable since Windows 10: reads a WNF state (here Focus Assist).
        fn NtQueryWnfStateData(
            state_name: *const u64,
            type_id: *const core::ffi::c_void,
            explicit_scope: *const core::ffi::c_void,
            change_stamp: *mut u32,
            buffer: *mut core::ffi::c_void,
            buffer_size: *mut u32,
        ) -> i32;
    }

    /// The active Focus Assist / Do Not Disturb profile: 0 off, 1 priority only (Windows 11
    /// "Do not disturb"), 2 alarms only. `None` if Windows will not say. This is the only
    /// reliable signal: `SHQueryUserNotificationState` keeps answering "accepts notifications"
    /// while Do Not Disturb is on.
    fn dnd_profile() -> Option<u32> {
        // WNF_SHEL_QUIETHOURS_ACTIVE_PROFILE_CHANGED
        let name: u64 = 0x0D83_063E_A3BF_1C75;
        let (mut stamp, mut value, mut size) = (0u32, 0u32, 4u32);
        // SAFETY: all pointers are to locals that outlive the call; `size` is the byte size of `value`.
        let status = unsafe {
            NtQueryWnfStateData(
                &name,
                core::ptr::null(),
                core::ptr::null(),
                &mut stamp,
                core::ptr::addr_of_mut!(value).cast(),
                &mut size,
            )
        };
        (status >= 0 && size == 4).then_some(value)
    }

    /// Focus Assist / Do Not Disturb / full-screen state, best effort.
    pub fn focus_state() -> &'static str {
        match dnd_profile() {
            Some(0) | None => {}
            Some(2) => return "alarms-only",
            Some(_) => return "do-not-disturb",
        }
        // SAFETY: no arguments, returns a plain enum.
        match unsafe { SHQueryUserNotificationState() } {
            Ok(s) if s == QUNS_ACCEPTS_NOTIFICATIONS => "off",
            Ok(s) if s == QUNS_QUIET_TIME => "quiet-time",
            Ok(s) if s == QUNS_BUSY => "busy",
            Ok(s) if s == QUNS_RUNNING_D3D_FULL_SCREEN => "fullscreen",
            Ok(s) if s == QUNS_PRESENTATION_MODE => "presentation",
            Ok(s) if s == QUNS_NOT_PRESENT => "away",
            Ok(s) if s == QUNS_APP => "app",
            _ => "unknown",
        }
    }
}

#[cfg(not(windows))]
mod os {
    pub fn offset_ms_at(_utc_ms: i64) -> i64 {
        0
    }
    pub fn play_alert_sound() -> &'static str {
        "unsupported"
    }
    pub fn toast_setting(_aumid: &str) -> &'static str {
        "unknown"
    }
    pub fn focus_state() -> &'static str {
        "unknown"
    }
}

// ---------------------------------------------------------------------------------------
// Logging (stderr always; also a file when KROVI_REMINDER_LOG is set, for QA)
// ---------------------------------------------------------------------------------------

fn stamp(utc_ms: u64) -> String {
    let utc = utc_ms as i64;
    format_local_ms(utc, os::offset_ms_at(utc))
}

fn log(msg: impl AsRef<str>) {
    let line = format!("{} [krovi-reminders] {}", stamp(now_ms()), msg.as_ref());
    eprintln!("{line}");
    if let Some(path) = std::env::var_os("KROVI_REMINDER_LOG") {
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let _ = writeln!(f, "{line}");
        }
    }
}

// ---------------------------------------------------------------------------------------
// Environment: toast identity, Windows' toast setting, Focus Assist
// ---------------------------------------------------------------------------------------

/// The AppUserModelID Windows files our toasts under, mirroring the plugin's own choice.
fn toast_identity(identifier: &str) -> (&'static str, String) {
    const POWERSHELL_AUMID: &str =
        "{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\\WindowsPowerShell\\v1.0\\powershell.exe";
    let dev = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.display().to_string()))
        .is_some_and(|d| d.ends_with("\\target\\debug") || d.ends_with("\\target\\release"));
    if dev {
        ("dev", POWERSHELL_AUMID.to_string())
    } else {
        ("installed", identifier.to_string())
    }
}

/// What is known about whether an alert will actually be seen.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Environment {
    /// `installed` (AUMID = app identifier, from the Start-menu shortcut, shows as "Krovi") or
    /// `dev` (runs from `target\debug|release`: Windows shows "Windows PowerShell").
    pub toast_identity: &'static str,
    pub app_user_model_id: String,
    /// Windows' setting for this app's toasts: `enabled`, `disabled-in-settings` (Settings >
    /// Notifications is off), `disabled-for-app`, `disabled-for-user`, `disabled-by-policy`,
    /// `disabled-by-manifest`, or `unknown` (Windows would not say; the toast is still sent).
    pub toasts: &'static str,
    /// Focus Assist / Do Not Disturb: `off`, `do-not-disturb`, `alarms-only`, `quiet-time`,
    /// `busy`, `fullscreen`, `presentation`, `away`, `app` or `unknown`. Everything from
    /// `do-not-disturb` to `presentation` means Windows holds the toast back (no pop-up, no
    /// sound) and files it in the notification centre instead.
    pub focus: &'static str,
    /// The toast may not pop up on screen (see `toasts` / `focus`); the in-app banner and
    /// the Calendar still show the alert.
    pub toast_may_be_hidden: bool,
}

/// Focus states in which the user asked Windows for quiet: no alert sound is played.
fn focus_wants_quiet(focus: &str) -> bool {
    matches!(focus, "do-not-disturb" | "alarms-only" | "quiet-time")
}

pub fn probe_environment(app: &AppHandle) -> Environment {
    let (identity, aumid) = toast_identity(&app.config().identifier);
    let toasts = os::toast_setting(&aumid);
    let focus = os::focus_state();
    let toast_may_be_hidden = (toasts != "enabled" && toasts != "unknown")
        || focus_wants_quiet(focus)
        || matches!(focus, "busy" | "fullscreen" | "presentation");
    Environment {
        toast_identity: identity,
        app_user_model_id: aumid,
        toasts,
        focus,
        toast_may_be_hidden,
    }
}

// ---------------------------------------------------------------------------------------
// Hub: the wake-up channel and the status the QA command reports
// ---------------------------------------------------------------------------------------

fn lk<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// One delivered (or test-fired) alert, as reported by `reminders_status`.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct FireRecord {
    pub id: u64,
    pub title: String,
    /// When it was scheduled for, and when it was actually delivered (epoch ms).
    pub due_at: u64,
    pub fired_at: u64,
    pub late_ms: u64,
    pub missed: bool,
    pub test: bool,
    /// The instant just before the toast was handed to Windows (epoch ms), and how far past
    /// `due_at` that was. This is the number to hold against "AT that time".
    pub shown_at: u64,
    pub shown_late_ms: i64,
    /// `requested` (handed to Windows), `grouped` (covered by a summary toast) or `error: ..`.
    pub toast: String,
    /// `off`, `Notification.Reminder`, `SystemAsterisk`, `skipped (quiet time)` or `failed`.
    pub sound: String,
    pub scheduled_local: String,
    pub fired_local: String,
    pub shown_local: String,
}

#[derive(Default)]
struct Info {
    started_at: u64,
    passes: u64,
    fires: u64,
    last_fire: Option<FireRecord>,
    last_error: Option<String>,
    /// A fired mark could not be saved; retried on the next pass.
    unsaved: bool,
}

/// Wake-up channel + diagnostics. Managed state (`app.state::<Hub>()`).
pub struct Hub {
    generation: Mutex<u64>,
    cv: Condvar,
    info: Mutex<Info>,
}

impl Hub {
    fn new() -> Self {
        Self {
            generation: Mutex::new(0),
            cv: Condvar::new(),
            info: Mutex::new(Info::default()),
        }
    }

    fn wake(&self) {
        *lk(&self.generation) += 1;
        self.cv.notify_all();
    }

    fn generation(&self) -> u64 {
        *lk(&self.generation)
    }

    /// Sleep up to `timeout`, returning early if [`Hub::wake`] ran after `seen`.
    fn wait(&self, seen: u64, timeout: Duration) {
        let guard = lk(&self.generation);
        let _ = self
            .cv
            .wait_timeout_while(guard, timeout, |g| *g == seen)
            .unwrap_or_else(PoisonError::into_inner);
    }
}

/// Tell the scheduler the set of reminders changed (added, removed, snoozed, ...): it
/// re-evaluates immediately instead of at its next timed wake. Safe to call from any
/// thread; a no-op before the scheduler exists.
pub fn wake_scheduler(app: &AppHandle) {
    if let Some(hub) = app.try_state::<Hub>() {
        hub.wake();
    }
}

// ---------------------------------------------------------------------------------------
// Persistence helpers
// ---------------------------------------------------------------------------------------

fn persist(state: &AppState, cal: &Calendar) -> CmdResult<()> {
    let mut store = state.store.lock().map_err(lock_err)?;
    store.save_calendar(cal.list()).map_err(|e| e.to_string())
}

/// Mutate the calendar under its lock, persist it, wake the scheduler, and return the new
/// chronological list. `f` failing leaves nothing changed or saved.
fn mutate<T>(
    app: &AppHandle,
    f: impl FnOnce(&mut Calendar) -> CmdResult<T>,
) -> CmdResult<(T, Vec<CalendarDto>)> {
    let state = app.state::<AppState>();
    let (out, list) = {
        let mut c = state.calendar.lock().map_err(lock_err)?;
        let out = f(&mut c)?;
        persist(&state, &c)?;
        (out, calendar_dtos(c.list()))
    };
    wake_scheduler(app);
    Ok((out, list))
}

fn emit_calendar_changed(app: &AppHandle, list: &[CalendarDto]) {
    let _ = app.emit("calendar-changed", list);
}

/// The one way an event is created (UI, voice): validates, applies the reminder options
/// (a timed event is a reminder by default), switches the alert off if its time is already
/// past, persists, and wakes the scheduler. Returns the whole, ordered list.
pub(crate) fn add_event(
    app: &AppHandle,
    date: &str,
    title: &str,
    time: Option<&str>,
    opts: ReminderOpts,
) -> CmdResult<Vec<CalendarDto>> {
    let ((), list) = mutate(app, |c| {
        let id = c
            .add_with(date, title, time, opts)
            .map_err(|e| e.to_string())?;
        c.disarm_if_past(id, &OsZone, now_ms());
        Ok(())
    })?;
    Ok(list)
}

// ---------------------------------------------------------------------------------------
// Delivery: toast + sound + event
// ---------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum ToastMode {
    /// This alert gets its own toast.
    Own,
    /// A summary toast covers it.
    Grouped,
}

fn show_toast(app: &AppHandle, id: u64, title: &str, body: &str) -> Result<(), String> {
    app.notification()
        .builder()
        .id(i32::try_from(id).unwrap_or(i32::MAX))
        .title(title)
        .body(body)
        .action_type_id(ACTION_TYPE)
        .show()
        .map_err(|e| e.to_string())
}

fn play_sound(env: &Environment, ignore_focus: bool) -> String {
    if !ignore_focus && focus_wants_quiet(env.focus) {
        // The user asked Windows for quiet (Do Not Disturb): the toast still lands in the
        // notification centre, and the in-app banner still shows.
        return format!("skipped ({})", env.focus);
    }
    os::play_alert_sound().to_string()
}

/// How one alert is delivered.
#[derive(Clone, Copy)]
struct Delivery {
    /// A QA test-fire: the stored reminder is untouched.
    test: bool,
    toast: ToastMode,
    /// Play the sound (a burst plays it once).
    play: bool,
    /// Play it even under Do Not Disturb (QA only).
    ignore_focus: bool,
}

impl Delivery {
    fn alert(toast: ToastMode, play: bool) -> Self {
        Self {
            test: false,
            toast,
            play,
            ignore_focus: false,
        }
    }
}

fn deliver(
    app: &AppHandle,
    hub: &Hub,
    d: &DueReminder,
    how: Delivery,
    env: &Environment,
) -> FireRecord {
    let Delivery {
        test,
        toast,
        play,
        ignore_focus,
    } = how;
    let e = &d.event;
    let fired_at = if test {
        now_ms()
    } else {
        e.fired_at.unwrap_or_else(now_ms)
    };
    let (title, body) = alert_text(e, d.missed, d.snoozed, test);

    let shown_at = now_ms();
    let toast_result = match toast {
        ToastMode::Grouped => "grouped".to_string(),
        ToastMode::Own => match show_toast(app, e.id, &title, &body) {
            Ok(()) => "requested".to_string(),
            Err(err) => format!("error: {err}"),
        },
    };
    let sound_result = if !e.sound {
        "off".to_string()
    } else if !play {
        "grouped".to_string()
    } else {
        play_sound(env, ignore_focus)
    };

    let payload = ReminderFired {
        id: e.id,
        title: e.title.clone(),
        date: e.date.clone(),
        time: e.time.clone(),
        missed: d.missed,
        fired_at,
        sound: e.sound,
        test,
    };
    let _ = app.emit("reminder-fired", &payload);

    let rec = FireRecord {
        id: e.id,
        title: e.title.clone(),
        due_at: d.due_at,
        fired_at,
        late_ms: d.late_ms,
        missed: d.missed,
        test,
        shown_at,
        shown_late_ms: shown_at as i64 - d.due_at as i64,
        toast: toast_result,
        sound: sound_result,
        scheduled_local: stamp(d.due_at),
        fired_local: stamp(fired_at),
        shown_local: stamp(shown_at),
    };
    log(format!(
        "{} id={} {:?} scheduled={} shown={} late={}ms marked={} missed={} toast={} sound={} identity={}({}) toasts={} focus={}",
        if test { "TEST-FIRE" } else { "FIRE" },
        rec.id,
        rec.title,
        rec.scheduled_local,
        rec.shown_local,
        rec.shown_late_ms,
        rec.fired_local,
        rec.missed,
        rec.toast,
        rec.sound,
        env.toast_identity,
        env.app_user_model_id,
        env.toasts,
        env.focus,
    ));
    if rec.toast.starts_with("error") {
        lk(&hub.info).last_error = Some(rec.toast.clone());
    }
    {
        let mut info = lk(&hub.info);
        info.fires += 1;
        info.last_fire = Some(rec.clone());
    }
    rec
}

/// One toast for a burst of alerts (e.g. several reminders missed while Krovi was closed).
fn show_summary(app: &AppHandle, due: &[&DueReminder]) -> String {
    let n = due.len();
    let title = format!("{n} reminders");
    let mut lines: Vec<String> = due.iter().take(3).map(|d| d.event.title.clone()).collect();
    if n > 3 {
        lines.push(format!("+{} more", n - 3));
    }
    let any_missed = due.iter().any(|d| d.missed);
    let body = format!(
        "{}{}",
        if any_missed {
            "Missed while Krovi was closed: "
        } else {
            ""
        },
        lines.join(" \u{b7} ")
    );
    match app.notification().builder().title(title).body(body).show() {
        Ok(()) => "requested".into(),
        Err(e) => format!("error: {e}"),
    }
}

// ---------------------------------------------------------------------------------------
// The scheduler
// ---------------------------------------------------------------------------------------

/// Collect what is due, persist the marks, then deliver. Returns `true` if anything changed.
fn pass(app: &AppHandle, hub: &Hub) -> bool {
    let state = app.state::<AppState>();
    let now = now_ms();
    let mut changed = false;

    let due = {
        // a poisoned lock must not stop alerts: the calendar data itself is still sound
        let mut cal = lk(&state.calendar);
        let due = cal.collect_due(&OsZone, now);
        let retry = lk(&hub.info).unsaved;
        if !due.is_empty() || retry {
            // persist BEFORE showing anything: a crash now can lose a toast, never repeat one
            match persist(&state, &cal) {
                Ok(()) => lk(&hub.info).unsaved = false,
                Err(e) => {
                    log(format!("could not save fired marks (will retry): {e}"));
                    let mut info = lk(&hub.info);
                    info.unsaved = true;
                    info.last_error = Some(e);
                }
            }
            changed = true;
        }
        due
    };
    if due.is_empty() {
        return changed;
    }

    let (loud, quiet): (Vec<&DueReminder>, Vec<&DueReminder>) = due.iter().partition(|d| !d.silent);
    for d in &quiet {
        log(format!(
            "id={} {:?} was {} h overdue: marked fired, not announced",
            d.event.id,
            d.event.title,
            d.late_ms / 3_600_000
        ));
    }
    if loud.is_empty() {
        return changed;
    }

    let env = probe_environment(app);
    if loud.len() >= GROUP_AT {
        let summary = show_summary(app, &loud);
        log(format!(
            "{} alerts at once -> one summary toast ({summary})",
            loud.len()
        ));
        let mut sound_played = false;
        for d in &loud {
            let play = !sound_played && d.event.sound;
            sound_played |= play;
            deliver(app, hub, d, Delivery::alert(ToastMode::Grouped, play), &env);
        }
    } else {
        for d in &loud {
            let how = Delivery::alert(ToastMode::Own, d.event.sound);
            deliver(app, hub, d, how, &env);
        }
    }
    changed
}

/// How long to sleep: until the next alert, capped, never zero.
fn next_wait_ms(state: &AppState) -> u64 {
    let next = lk(&state.calendar).next_alert_ms(&OsZone);
    let wait = match next {
        Some(at) => at.saturating_sub(now_ms()),
        None => MAX_SLEEP_MS,
    };
    wait.clamp(1, MAX_SLEEP_MS)
}

fn run(app: AppHandle) {
    let hub = app.state::<Hub>();
    let state = app.state::<AppState>();
    lk(&hub.info).started_at = now_ms();
    let offset_min = os::offset_ms_at(now_ms() as i64) / 60_000;
    let (pending, total) = {
        let c = lk(&state.calendar);
        (c.pending(&OsZone).len(), c.len())
    };
    log(format!(
        "scheduler up: utc offset {offset_min:+} min, {pending} pending of {total} events, persistence={}",
        state
            .live
            .persistence_error
            .lock()
            .ok()
            .and_then(|e| e.clone())
            .unwrap_or_else(|| "ok".into())
    ));

    loop {
        // read the generation BEFORE looking, so a change made during the pass is not lost
        let seen = hub.generation();
        let changed = pass(&app, &hub);
        lk(&hub.info).passes += 1;
        if changed {
            let list = calendar_dtos(lk(&state.calendar).list());
            emit_calendar_changed(&app, &list);
        }
        let wait = next_wait_ms(&state);
        hub.wait(seen, Duration::from_millis(wait));
    }
}

pub fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let handle = app.handle().clone();
    app.manage(Hub::new());
    register_toast_actions(&handle);
    std::thread::Builder::new()
        .name("krovi-reminders".into())
        .spawn(move || run(handle))?;
    Ok(())
}

// ---------------------------------------------------------------------------------------
// Toast buttons
// ---------------------------------------------------------------------------------------

fn register_toast_actions(app: &AppHandle) {
    let n = app.notification();
    let ty = ActionType::builder(ACTION_TYPE)
        .actions(vec![
            Action::builder("snooze5", "Snooze 5 min").build(),
            Action::builder("snooze10", "Snooze 10 min").build(),
            Action::builder("dismiss", "Dismiss").build(),
        ])
        .build();
    if let Err(e) = n.register_action_types(vec![ty]) {
        log(format!("could not register toast buttons: {e}"));
    }
    let handle = app.clone();
    if let Err(e) = n.on_action(move |p| on_toast_action(&handle, p)) {
        log(format!("could not listen for toast buttons: {e}"));
    }
}

fn on_toast_action(app: &AppHandle, performed: &ActionPerformed) {
    let Some(id) = performed
        .notification()
        .map(|n| n.id())
        .filter(|id| *id > 0)
    else {
        return;
    };
    let id = id as u64;
    let action = performed.action_id();
    let result = match action {
        "snooze5" => snooze_event(app, id, 5.0),
        "snooze10" => snooze_event(app, id, 10.0),
        "dismiss" => dismiss_event(app, id),
        "tap" => {
            // The body of the toast was clicked: show the Calendar.
            let _ = app.emit("shell-open-tool", json!({ "tool": "calendar" }));
            log(format!("toast clicked: opening the Calendar (id={id})"));
            return;
        }
        _ => return,
    };
    match result {
        Ok(list) => {
            log(format!("toast button {action:?} applied to id={id}"));
            emit_calendar_changed(app, &list);
        }
        Err(e) => log(format!("toast button {action:?} failed for id={id}: {e}")),
    }
}

// ---------------------------------------------------------------------------------------
// Shared actions (commands and toast buttons)
// ---------------------------------------------------------------------------------------

fn snooze_event(app: &AppHandle, id: u64, minutes: f64) -> CmdResult<Vec<CalendarDto>> {
    if !minutes.is_finite() {
        return Err("snooze minutes must be a number".into());
    }
    let minutes = minutes.clamp(MIN_SNOOZE_MIN, MAX_SNOOZE_MIN);
    let until = now_ms() + (minutes * 60_000.0) as u64;
    let (_, list) = mutate(app, |c| {
        if c.snooze(id, until) {
            Ok(())
        } else {
            Err(format!("cannot snooze reminder {id}: unknown or all-day"))
        }
    })?;
    log(format!(
        "snoozed id={id} until {} ({minutes} min)",
        stamp(until)
    ));
    Ok(list)
}

fn dismiss_event(app: &AppHandle, id: u64) -> CmdResult<Vec<CalendarDto>> {
    let (_, list) = mutate(app, |c| {
        if c.dismiss(id, now_ms()) {
            Ok(())
        } else {
            Err(format!("unknown reminder id: {id}"))
        }
    })?;
    log(format!("dismissed id={id}"));
    Ok(list)
}

// ---------------------------------------------------------------------------------------
// Commands ([reminders] block in lib.rs)
// ---------------------------------------------------------------------------------------

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PendingDto {
    pub id: u64,
    pub title: String,
    pub date: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time: Option<String>,
    /// UTC instant it alerts at (epoch ms), and how far away that is now.
    pub due_at: u64,
    pub in_ms: i64,
    pub due_local: String,
    pub sound: bool,
    pub snoozed: bool,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SchedulerDto {
    pub running: bool,
    pub started_at: u64,
    pub passes: u64,
    pub fires: u64,
    pub last_fire: Option<FireRecord>,
    pub last_error: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RemindersStatus {
    pub now: u64,
    pub now_local: String,
    /// Minutes east of UTC right now (IST = 330).
    pub utc_offset_minutes: i64,
    /// Reminders still waiting to alert, soonest first.
    pub pending: Vec<PendingDto>,
    pub next: Option<PendingDto>,
    pub scheduler: SchedulerDto,
    pub environment: Environment,
    /// Set when the database could not be opened and reminders live in memory only.
    pub persistence_error: Option<String>,
}

/// Upcoming reminders plus scheduler diagnostics and the toast environment.
#[tauri::command]
pub fn reminders_status(
    app: AppHandle,
    state: State<AppState>,
    hub: State<Hub>,
) -> CmdResult<RemindersStatus> {
    let now = now_ms();
    let pending: Vec<PendingDto> = {
        let c = state.calendar.lock().map_err(lock_err)?;
        c.pending(&OsZone)
            .into_iter()
            .filter_map(|p| {
                let e = c.get(p.id)?;
                Some(PendingDto {
                    id: p.id,
                    title: e.title.clone(),
                    date: e.date.clone(),
                    time: e.time.clone(),
                    due_at: p.at,
                    in_ms: p.at as i64 - now as i64,
                    due_local: stamp(p.at),
                    sound: e.sound,
                    snoozed: p.snoozed,
                })
            })
            .collect()
    };
    let info = lk(&hub.info);
    Ok(RemindersStatus {
        now,
        now_local: stamp(now),
        utc_offset_minutes: os::offset_ms_at(now as i64) / 60_000,
        next: pending.first().cloned(),
        pending,
        scheduler: SchedulerDto {
            running: info.started_at > 0,
            started_at: info.started_at,
            passes: info.passes,
            fires: info.fires,
            last_fire: info.last_fire.clone(),
            last_error: info.last_error.clone(),
        },
        environment: probe_environment(&app),
        persistence_error: state
            .live
            .persistence_error
            .lock()
            .ok()
            .and_then(|e| e.clone()),
    })
}

/// Add an event with explicit reminder settings (`remind` defaults to on when `time` is
/// given, `sound` to on). A reminder whose time already passed is stored with its alert off.
#[tauri::command]
pub fn reminder_add(
    app: AppHandle,
    date: String,
    title: String,
    time: Option<String>,
    remind: Option<bool>,
    sound: Option<bool>,
) -> CmdResult<Vec<CalendarDto>> {
    add_event(
        &app,
        &date,
        &title,
        time.as_deref(),
        ReminderOpts { remind, sound },
    )
}

/// Switch an event's alert and/or sound. Turning the alert on re-arms it (and switches it
/// off again if its time already passed).
#[tauri::command]
pub fn reminder_set(
    app: AppHandle,
    id: u64,
    remind: Option<bool>,
    sound: Option<bool>,
) -> CmdResult<Vec<CalendarDto>> {
    let (_, list) = mutate(&app, |c| {
        if !c.set_alert(id, remind, sound) {
            return Err(format!("unknown calendar event id: {id}"));
        }
        if remind == Some(true) {
            c.disarm_if_past(id, &OsZone, now_ms());
        }
        Ok(())
    })?;
    Ok(list)
}

/// Alert again in `minutes` (5 and 10 in the UI; fractions are accepted for tests).
#[tauri::command]
pub fn reminder_snooze(app: AppHandle, id: u64, minutes: f64) -> CmdResult<Vec<CalendarDto>> {
    snooze_event(&app, id, minutes)
}

/// Acknowledge: cancel a snooze / pending alert without ringing.
#[tauri::command]
pub fn reminder_dismiss(app: AppHandle, id: u64) -> CmdResult<Vec<CalendarDto>> {
    dismiss_event(&app, id)
}

/// QA: ring this reminder right now (real toast, sound and `reminder-fired`) without
/// changing what is stored. `ignore_focus` plays the sound even under Do Not Disturb.
#[tauri::command]
pub fn reminder_test_fire(
    app: AppHandle,
    state: State<AppState>,
    hub: State<Hub>,
    id: u64,
    ignore_focus: Option<bool>,
) -> CmdResult<FireRecord> {
    let event = {
        let c = state.calendar.lock().map_err(lock_err)?;
        c.get(id)
            .cloned()
            .ok_or_else(|| format!("unknown calendar event id: {id}"))?
    };
    let now = now_ms();
    let due = DueReminder {
        event,
        due_at: now,
        late_ms: 0,
        missed: false,
        silent: false,
        snoozed: false,
    };
    let env = probe_environment(&app);
    let how = Delivery {
        test: true,
        toast: ToastMode::Own,
        play: due.event.sound,
        ignore_focus: ignore_focus.unwrap_or(false),
    };
    Ok(deliver(&app, &hub, &due, how, &env))
}
