//! Calendar events and real reminders (SPEC C6/C9, docs/CONTRACTS.md §8).
//!
//! Events carry an ISO date (`yyyy-mm-dd`), a title, and an optional `HH:MM` time
//! (the user's *local* wall clock). Validation is pure (no clock): the date must be a
//! real Gregorian date and the time a real 24h time. Listing is chronological (date,
//! then time with all-day events first, then id) so the UI never has to sort.
//!
//! An event with a time is a **reminder**: it carries `remind` (alert at that time),
//! `sound` (play a short sound with the alert), `fired_at` (when it last alerted, which
//! is what guarantees it never alerts twice), `snoozed_until` (alert again then) and
//! `missed` (it alerted late, e.g. the app was closed at the time).
//!
//! Everything the scheduler decides lives here, pure and testable: turning a local
//! wall-clock time into a UTC instant (including DST gaps and overlaps, through an
//! injected [`ZoneOffset`]), finding the next alert, collecting what is due, and the
//! fire/snooze/dismiss state transitions. The Tauri layer only supplies the clock, the
//! time zone and the toast.

/// Longest accepted title, in characters.
pub const MAX_TITLE_CHARS: usize = 200;

/// A reminder whose time passed more than this long *before it was created* can never
/// ring, so it is stored with its alert off. (Slack covers "in 1 minute" rounding.)
pub const CREATE_GRACE_MS: u64 = 90_000;
/// An alert delivered more than this long after its time counts as *missed*.
pub const MISSED_AFTER_MS: u64 = 60_000;
/// A missed alert older than this is only marked fired, never announced.
pub const STALE_AFTER_MS: u64 = 24 * 60 * 60 * 1_000;

const DAY_MS: i64 = 86_400_000;

/// A single calendar event / reminder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalendarEvent {
    pub id: u64,
    /// ISO date `yyyy-mm-dd`.
    pub date: String,
    pub title: String,
    /// Optional `HH:MM` (24h, local).
    pub time: Option<String>,
    /// Alert at `time`. Always `false` for an all-day event.
    pub remind: bool,
    /// Play a short sound with the alert.
    pub sound: bool,
    /// Epoch ms of the last alert; `None` while an alert is still pending.
    pub fired_at: Option<u64>,
    /// Epoch ms the alert was snoozed to; the next alert is then due at this instant.
    pub snoozed_until: Option<u64>,
    /// The last alert was delivered late (the app was closed or the PC asleep).
    pub missed: bool,
}

impl CalendarEvent {
    /// An event with the default reminder settings: alert on when it has a time, sound on.
    pub fn new(
        id: u64,
        date: impl Into<String>,
        title: impl Into<String>,
        time: Option<String>,
    ) -> Self {
        let remind = time.is_some();
        Self {
            id,
            date: date.into(),
            title: title.into(),
            time,
            remind,
            sound: true,
            fired_at: None,
            snoozed_until: None,
            missed: false,
        }
    }

    /// `true` when this event can alert at all (it has a time and the alert is on).
    pub fn is_reminder(&self) -> bool {
        self.remind && self.time.is_some()
    }

    /// The scheduled wall-clock moment as a UTC instant (epoch ms), ignoring snooze and
    /// whether it already alerted. `None` for an all-day event or unrepresentable time.
    pub fn scheduled_ms(&self, zone: &dyn ZoneOffset) -> Option<u64> {
        let naive = naive_local_ms(&self.date, self.time.as_deref()?)?;
        u64::try_from(local_to_utc_ms(zone, naive)).ok()
    }

    /// When this reminder should alert next: the snooze target if snoozed, else its
    /// scheduled time. `None` when it will not alert (not a reminder, or already fired).
    pub fn next_alert_ms(&self, zone: &dyn ZoneOffset) -> Option<u64> {
        if !self.is_reminder() || self.fired_at.is_some() {
            return None;
        }
        self.snoozed_until.or_else(|| self.scheduled_ms(zone))
    }
}

/// Why an event was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalendarError {
    BadDate,
    BadTime,
    EmptyTitle,
    TitleTooLong,
}

impl std::fmt::Display for CalendarError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            CalendarError::BadDate => "date must be a real yyyy-mm-dd date",
            CalendarError::BadTime => "time must be HH:MM (24h)",
            CalendarError::EmptyTitle => "title is empty",
            CalendarError::TitleTooLong => "title is too long",
        })
    }
}

impl std::error::Error for CalendarError {}

fn is_leap(y: u32) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn days_in_month(y: u32, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(y) => 29,
        2 => 28,
        _ => 0,
    }
}

/// `true` for a real `yyyy-mm-dd` date (year 1970..=9999).
pub fn valid_date(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return false;
    }
    let digits = |r: std::ops::Range<usize>| -> Option<u32> {
        let part = &s[r];
        if part.bytes().all(|c| c.is_ascii_digit()) {
            part.parse().ok()
        } else {
            None
        }
    };
    match (digits(0..4), digits(5..7), digits(8..10)) {
        (Some(y), Some(m), Some(d)) => {
            (1970..=9999).contains(&y)
                && (1..=12).contains(&m)
                && d >= 1
                && d <= days_in_month(y, m)
        }
        _ => false,
    }
}

/// `true` for a real 24h `HH:MM` time.
pub fn valid_time(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 5 || b[2] != b':' {
        return false;
    }
    let num = |r: std::ops::Range<usize>| -> Option<u32> {
        let part = &s[r];
        if part.bytes().all(|c| c.is_ascii_digit()) {
            part.parse().ok()
        } else {
            None
        }
    };
    matches!((num(0..2), num(3..5)), (Some(h), Some(m)) if h < 24 && m < 60)
}

// ---------------------------------------------------------------------------------------
// Local wall clock -> UTC instant (DST-aware, clock-free)
// ---------------------------------------------------------------------------------------

/// The user's time zone, injected so the maths stays pure: the offset (local minus UTC,
/// in milliseconds) in effect at a UTC instant. The app answers it from the OS
/// (`SystemTimeToTzSpecificLocalTime`), tests from fixed or fake DST zones.
pub trait ZoneOffset {
    fn offset_ms_at(&self, utc_ms: i64) -> i64;
}

/// A zone with a constant offset (no DST), in milliseconds east of UTC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FixedZone(pub i64);

impl ZoneOffset for FixedZone {
    fn offset_ms_at(&self, _utc_ms: i64) -> i64 {
        self.0
    }
}

/// Days since 1970-01-01 for a proleptic Gregorian civil date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400; // [0, 399]
    let mp = (i64::from(m) + 9) % 12; // March = 0
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Inverse of [`days_from_civil`]: the civil `(year, month, day)` for days since 1970-01-01.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

/// A UTC instant shown on the wall clock `offset_ms` east of UTC, as
/// `yyyy-mm-dd HH:MM:SS.mmm` (for logs and diagnostics).
pub fn format_local_ms(utc_ms: i64, offset_ms: i64) -> String {
    let local = utc_ms + offset_ms;
    let days = local.div_euclid(DAY_MS);
    let ms_of_day = local.rem_euclid(DAY_MS);
    let (y, m, d) = civil_from_days(days);
    let (h, mi, s, ms) = (
        ms_of_day / 3_600_000,
        ms_of_day / 60_000 % 60,
        ms_of_day / 1_000 % 60,
        ms_of_day % 1_000,
    );
    format!("{y:04}-{m:02}-{d:02} {h:02}:{mi:02}:{s:02}.{ms:03}")
}

/// A local `date` + `time` read *as if it were UTC*, in epoch ms. `None` if either is invalid.
pub fn naive_local_ms(date: &str, time: &str) -> Option<i64> {
    if !valid_date(date) || !valid_time(time) {
        return None;
    }
    let y: i64 = date[0..4].parse().ok()?;
    let m: u32 = date[5..7].parse().ok()?;
    let d: u32 = date[8..10].parse().ok()?;
    let h: i64 = time[0..2].parse().ok()?;
    let mi: i64 = time[3..5].parse().ok()?;
    Some((days_from_civil(y, m, d) * 86_400 + h * 3_600 + mi * 60) * 1_000)
}

/// The UTC instant (epoch ms) at which the local wall clock first shows `naive`
/// (a local time read as UTC, see [`naive_local_ms`]).
///
/// * An ordinary time maps to exactly one instant.
/// * A time that happens **twice** (clocks go back) resolves to the *first* occurrence.
/// * A time that **never happens** (clocks jump forward) resolves with the pre-jump
///   offset, i.e. it lands just after the jump (02:30 becomes 03:30 when 02:00 -> 03:00).
///
/// A transition affecting `naive` lies within 14h of it, so sampling the offset a day
/// either side gives the offsets before and after; each candidate is then verified.
pub fn local_to_utc_ms(zone: &dyn ZoneOffset, naive: i64) -> i64 {
    let before = zone.offset_ms_at(naive - DAY_MS);
    let after = zone.offset_ms_at(naive + DAY_MS);
    if before == after {
        return naive - before;
    }
    let t_before = naive - before;
    let t_after = naive - after;
    let ok_before = zone.offset_ms_at(t_before) == before;
    let ok_after = zone.offset_ms_at(t_after) == after;
    match (ok_before, ok_after) {
        (true, true) => t_before.min(t_after),
        (true, false) => t_before,
        (false, true) => t_after,
        (false, false) => t_before,
    }
}

// ---------------------------------------------------------------------------------------
// The calendar
// ---------------------------------------------------------------------------------------

/// Reminder settings for a new event; `None` means "use the default" (alert on when the
/// event has a time, sound on).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReminderOpts {
    pub remind: Option<bool>,
    pub sound: Option<bool>,
}

/// A reminder still waiting to alert.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingReminder {
    pub id: u64,
    /// UTC instant (epoch ms) it alerts at.
    pub at: u64,
    /// The instant comes from a snooze rather than the scheduled time.
    pub snoozed: bool,
}

/// A reminder that came due and was just marked fired.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DueReminder {
    /// The event as it is *after* being marked (`fired_at`, `missed` set).
    pub event: CalendarEvent,
    /// The instant it was due.
    pub due_at: u64,
    /// How late it is being delivered.
    pub late_ms: u64,
    /// More than [`MISSED_AFTER_MS`] late.
    pub missed: bool,
    /// More than [`STALE_AFTER_MS`] late: mark only, do not announce.
    pub silent: bool,
    /// This alert is a snoozed repeat rather than the first one.
    pub snoozed: bool,
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// `2026-11-01` -> `1 Nov`; anything unparseable is returned as is.
fn short_date(date: &str) -> String {
    if !valid_date(date) {
        return date.to_string();
    }
    let m: usize = date[5..7].parse().unwrap_or(0);
    let d: u32 = date[8..10].parse().unwrap_or(0);
    match MONTHS.get(m.wrapping_sub(1)) {
        Some(name) => format!("{d} {name}"),
        None => date.to_string(),
    }
}

/// The two lines of text for an alert's notification: the reminder itself, then why and
/// when. `missed` / `snoozed` / `test` pick the wording; the clock time is the event's
/// own `HH:MM` so it reads the same in the toast and in the panel.
pub fn alert_text(
    event: &CalendarEvent,
    missed: bool,
    snoozed: bool,
    test: bool,
) -> (String, String) {
    let at = event.time.as_deref().unwrap_or("");
    let body = if test {
        format!("Test reminder \u{b7} {at}")
    } else if missed {
        format!(
            "Missed reminder \u{b7} was due {at} on {}",
            short_date(&event.date)
        )
    } else if snoozed {
        format!("Snoozed reminder \u{b7} {at}")
    } else {
        format!("Reminder \u{b7} {at}")
    };
    (event.title.clone(), body)
}

/// The persisted calendar.
#[derive(Debug, Clone, Default)]
pub struct Calendar {
    next_id: u64,
    events: Vec<CalendarEvent>,
}

impl Calendar {
    pub fn new() -> Self {
        Self {
            next_id: 1,
            events: Vec::new(),
        }
    }

    /// Rebuild from persisted events; new ids continue after the largest one.
    pub fn from_events(events: Vec<CalendarEvent>) -> Self {
        let next_id = events.iter().map(|e| e.id).max().unwrap_or(0) + 1;
        let mut c = Self { next_id, events };
        c.sort();
        c
    }

    fn sort(&mut self) {
        self.events.sort_by(|a, b| {
            a.date
                .cmp(&b.date)
                .then_with(|| {
                    a.time
                        .as_deref()
                        .unwrap_or("")
                        .cmp(b.time.as_deref().unwrap_or(""))
                })
                .then_with(|| a.id.cmp(&b.id))
        });
    }

    /// Validate and add an event with the default reminder settings; returns its id.
    pub fn add(
        &mut self,
        date: &str,
        title: &str,
        time: Option<&str>,
    ) -> Result<u64, CalendarError> {
        self.add_with(date, title, time, ReminderOpts::default())
    }

    /// Validate and add an event with explicit reminder settings; returns its id. An
    /// event without a time never alerts, whatever `opts.remind` says.
    pub fn add_with(
        &mut self,
        date: &str,
        title: &str,
        time: Option<&str>,
        opts: ReminderOpts,
    ) -> Result<u64, CalendarError> {
        if !valid_date(date) {
            return Err(CalendarError::BadDate);
        }
        let title = title.trim();
        if title.is_empty() {
            return Err(CalendarError::EmptyTitle);
        }
        if title.chars().count() > MAX_TITLE_CHARS {
            return Err(CalendarError::TitleTooLong);
        }
        let time = match time.map(str::trim).filter(|t| !t.is_empty()) {
            Some(t) if valid_time(t) => Some(t.to_string()),
            Some(_) => return Err(CalendarError::BadTime),
            None => None,
        };
        let id = self.next_id.max(1);
        self.next_id = id + 1;
        let mut event = CalendarEvent::new(id, date, title, time);
        event.remind = event.time.is_some() && opts.remind.unwrap_or(true);
        event.sound = opts.sound.unwrap_or(true);
        self.events.push(event);
        self.sort();
        Ok(id)
    }

    /// Remove by id. Returns `false` if unknown.
    pub fn remove(&mut self, id: u64) -> bool {
        match self.events.iter().position(|e| e.id == id) {
            Some(pos) => {
                self.events.remove(pos);
                true
            }
            None => false,
        }
    }

    /// All events, chronological.
    pub fn list(&self) -> &[CalendarEvent] {
        &self.events
    }

    pub fn get(&self, id: u64) -> Option<&CalendarEvent> {
        self.events.iter().find(|e| e.id == id)
    }

    fn get_mut(&mut self, id: u64) -> Option<&mut CalendarEvent> {
        self.events.iter_mut().find(|e| e.id == id)
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    // ---- reminders -------------------------------------------------------------------

    /// Change an event's alert and/or sound. Turning the alert **on** re-arms it (clears
    /// `fired_at`, the snooze and the missed mark); turning it off cancels any snooze.
    /// An all-day event cannot be given an alert. Returns `false` if the id is unknown.
    pub fn set_alert(&mut self, id: u64, remind: Option<bool>, sound: Option<bool>) -> bool {
        let Some(e) = self.get_mut(id) else {
            return false;
        };
        if let Some(on) = remind {
            let on = on && e.time.is_some();
            if on && !e.remind {
                e.fired_at = None;
                e.snoozed_until = None;
                e.missed = false;
            }
            if !on {
                e.snoozed_until = None;
            }
            e.remind = on;
        }
        if let Some(s) = sound {
            e.sound = s;
        }
        true
    }

    /// If the event's alert is armed but its time passed more than [`CREATE_GRACE_MS`]
    /// ago, switch the alert off (a reminder in the past can never ring) and return
    /// `true`. Call it right after creating or re-arming an event.
    pub fn disarm_if_past(&mut self, id: u64, zone: &dyn ZoneOffset, now_ms: u64) -> bool {
        let Some(e) = self.get_mut(id) else {
            return false;
        };
        if e.next_alert_ms(zone)
            .is_some_and(|at| at + CREATE_GRACE_MS < now_ms)
        {
            e.remind = false;
            e.snoozed_until = None;
            return true;
        }
        false
    }

    /// Every reminder still waiting to alert, soonest first.
    pub fn pending(&self, zone: &dyn ZoneOffset) -> Vec<PendingReminder> {
        let mut out: Vec<PendingReminder> = self
            .events
            .iter()
            .filter_map(|e| {
                e.next_alert_ms(zone).map(|at| PendingReminder {
                    id: e.id,
                    at,
                    snoozed: e.snoozed_until.is_some(),
                })
            })
            .collect();
        out.sort_by_key(|p| (p.at, p.id));
        out
    }

    /// The soonest instant any reminder alerts at.
    pub fn next_alert_ms(&self, zone: &dyn ZoneOffset) -> Option<u64> {
        self.events
            .iter()
            .filter_map(|e| e.next_alert_ms(zone))
            .min()
    }

    /// Mark every reminder due at `now_ms` as fired and return them, oldest first. Marking
    /// happens here, atomically with detection, so a reminder is handed out **once**: a
    /// second call returns nothing for it.
    pub fn collect_due(&mut self, zone: &dyn ZoneOffset, now_ms: u64) -> Vec<DueReminder> {
        let mut due: Vec<(u64, usize)> = self
            .events
            .iter()
            .enumerate()
            .filter_map(|(i, e)| {
                let at = e.next_alert_ms(zone)?;
                (at <= now_ms).then_some((at, i))
            })
            .collect();
        due.sort_by_key(|&(at, i)| (at, self.events[i].id));
        due.into_iter()
            .map(|(at, i)| {
                let late_ms = now_ms - at;
                let missed = late_ms > MISSED_AFTER_MS;
                let e = &mut self.events[i];
                let snoozed = e.snoozed_until.is_some();
                e.fired_at = Some(now_ms);
                e.snoozed_until = None;
                e.missed = missed;
                DueReminder {
                    event: e.clone(),
                    due_at: at,
                    late_ms,
                    missed,
                    silent: late_ms > STALE_AFTER_MS,
                    snoozed,
                }
            })
            .collect()
    }

    /// Alert again at `until_ms`. Makes the event pending again (clears `fired_at`) and
    /// switches its alert on. Returns `false` for an unknown id or an all-day event.
    pub fn snooze(&mut self, id: u64, until_ms: u64) -> bool {
        match self.get_mut(id) {
            Some(e) if e.time.is_some() => {
                e.remind = true;
                e.fired_at = None;
                e.snoozed_until = Some(until_ms);
                e.missed = false;
                true
            }
            _ => false,
        }
    }

    /// Acknowledge: cancel a snooze and any pending alert without alerting. A reminder
    /// that never fired is marked fired at `now_ms`. Returns `false` if the id is unknown.
    pub fn dismiss(&mut self, id: u64, now_ms: u64) -> bool {
        match self.get_mut(id) {
            Some(e) => {
                e.snoozed_until = None;
                if e.fired_at.is_none() {
                    e.fired_at = Some(now_ms);
                }
                true
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const UTC: FixedZone = FixedZone(0);
    const IST: FixedZone = FixedZone(19_800_000); // +05:30
    const MIN: u64 = 60_000;

    /// UTC epoch ms of `date` `time` (read as UTC).
    fn at(date: &str, time: &str) -> u64 {
        naive_local_ms(date, time).unwrap() as u64
    }

    #[test]
    fn date_validation() {
        assert!(valid_date("2026-11-01"));
        assert!(valid_date("2024-02-29")); // leap
        assert!(!valid_date("2026-02-29"));
        assert!(!valid_date("2026-13-01"));
        assert!(!valid_date("2026-00-10"));
        assert!(!valid_date("2026-04-31"));
        assert!(!valid_date("26-11-01"));
        assert!(!valid_date("2026/11/01"));
        assert!(!valid_date("2026-1-01"));
        assert!(!valid_date("abcd-ef-gh"));
        assert!(!valid_date(""));
        assert!(!valid_date("1969-12-31"));
    }

    #[test]
    fn time_validation() {
        assert!(valid_time("00:00"));
        assert!(valid_time("23:59"));
        assert!(!valid_time("24:00"));
        assert!(!valid_time("12:60"));
        assert!(!valid_time("1:30"));
        assert!(!valid_time("12-30"));
        assert!(!valid_time(""));
    }

    #[test]
    fn add_validates_and_trims() {
        let mut c = Calendar::new();
        assert_eq!(c.add("nope", "x", None), Err(CalendarError::BadDate));
        assert_eq!(
            c.add("2026-11-01", "   ", None),
            Err(CalendarError::EmptyTitle)
        );
        assert_eq!(
            c.add("2026-11-01", &"x".repeat(MAX_TITLE_CHARS + 1), None),
            Err(CalendarError::TitleTooLong)
        );
        assert_eq!(
            c.add("2026-11-01", "ok", Some("25:00")),
            Err(CalendarError::BadTime)
        );
        let id = c.add("2026-11-01", "  Dentist  ", Some("09:30")).unwrap();
        assert_eq!(id, 1);
        assert_eq!(c.list()[0].title, "Dentist");
        assert_eq!(c.list()[0].time.as_deref(), Some("09:30"));
        // blank time means all-day
        c.add("2026-11-01", "All day", Some("  ")).unwrap();
        assert_eq!(c.list()[0].time, None);
    }

    #[test]
    fn list_is_chronological_all_day_first() {
        let mut c = Calendar::new();
        c.add("2026-11-02", "later day", None).unwrap();
        c.add("2026-11-01", "evening", Some("18:00")).unwrap();
        c.add("2026-11-01", "all day", None).unwrap();
        c.add("2026-11-01", "morning", Some("08:00")).unwrap();
        let titles: Vec<_> = c.list().iter().map(|e| e.title.as_str()).collect();
        assert_eq!(titles, vec!["all day", "morning", "evening", "later day"]);
    }

    #[test]
    fn remove_and_restore() {
        let mut c = Calendar::new();
        let a = c.add("2026-11-01", "a", None).unwrap();
        let b = c.add("2026-11-02", "b", None).unwrap();
        assert!(c.remove(a));
        assert!(!c.remove(a));
        let mut restored = Calendar::from_events(c.list().to_vec());
        assert_eq!(restored.len(), 1);
        assert_eq!(restored.list()[0].id, b);
        assert_eq!(restored.add("2026-11-03", "c", None).unwrap(), b + 1);
    }

    // ---- reminder defaults ----------------------------------------------------------

    #[test]
    fn a_timed_event_is_a_reminder_with_sound_by_default() {
        let mut c = Calendar::new();
        let id = c.add("2026-11-01", "Call", Some("17:00")).unwrap();
        let e = c.get(id).unwrap();
        assert!(e.remind && e.sound && e.is_reminder());
        assert_eq!((e.fired_at, e.snoozed_until, e.missed), (None, None, false));
    }

    #[test]
    fn an_all_day_event_never_alerts_even_if_asked() {
        let mut c = Calendar::new();
        let opts = ReminderOpts {
            remind: Some(true),
            sound: None,
        };
        let id = c.add_with("2026-11-01", "Holiday", None, opts).unwrap();
        assert!(!c.get(id).unwrap().remind);
        assert!(!c.set_alert(99, Some(true), None)); // unknown id
        assert!(c.set_alert(id, Some(true), None));
        assert!(!c.get(id).unwrap().remind, "no time, no alert");
        assert!(c.pending(&UTC).is_empty());
    }

    #[test]
    fn explicit_options_are_honoured() {
        let mut c = Calendar::new();
        let opts = ReminderOpts {
            remind: Some(false),
            sound: Some(false),
        };
        let id = c
            .add_with("2026-11-01", "Quiet", Some("08:00"), opts)
            .unwrap();
        let e = c.get(id).unwrap();
        assert!(!e.remind && !e.sound && !e.is_reminder());
        assert_eq!(e.next_alert_ms(&UTC), None);
    }

    // ---- wall clock -> UTC ----------------------------------------------------------

    #[test]
    fn civil_dates_convert_to_the_unix_epoch() {
        assert_eq!(naive_local_ms("1970-01-01", "00:00"), Some(0));
        assert_eq!(
            naive_local_ms("1970-01-02", "00:01"),
            Some(86_400_000 + 60_000)
        );
        // 2026-11-01T12:00:00Z, cross-checked against a known epoch value
        assert_eq!(
            naive_local_ms("2026-11-01", "12:00"),
            Some(1_793_534_400_000)
        );
        assert_eq!(
            naive_local_ms("2024-02-29", "23:59"),
            Some(1_709_251_140_000)
        );
        assert_eq!(naive_local_ms("2026-02-30", "10:00"), None);
        assert_eq!(naive_local_ms("2026-02-28", "25:00"), None);
    }

    #[test]
    fn civil_days_round_trip_and_log_stamps_read_like_a_wall_clock() {
        for (y, m, d) in [
            (1970, 1, 1),
            (2000, 2, 29),
            (2024, 12, 31),
            (2026, 11, 1),
            (2100, 3, 1),
            (9999, 12, 31),
        ] {
            assert_eq!(civil_from_days(days_from_civil(y, m, d)), (y, m, d));
        }
        // 12:00:00.123Z is 17:30:00.123 in India
        let t = naive_local_ms("2026-11-01", "12:00").unwrap() + 123;
        assert_eq!(format_local_ms(t, 19_800_000), "2026-11-01 17:30:00.123");
        assert_eq!(format_local_ms(t, 0), "2026-11-01 12:00:00.123");
        // crosses midnight forwards and backwards
        assert_eq!(
            format_local_ms(t, 13 * 3_600_000),
            "2026-11-02 01:00:00.123"
        );
        assert_eq!(format_local_ms(0, -1), "1969-12-31 23:59:59.999");
    }

    #[test]
    fn a_fixed_zone_shifts_by_its_offset() {
        // 17:30 in India is 12:00 UTC
        let naive = naive_local_ms("2026-11-01", "17:30").unwrap();
        assert_eq!(
            local_to_utc_ms(&IST, naive),
            naive_local_ms("2026-11-01", "12:00").unwrap()
        );
        let e = CalendarEvent::new(1, "2026-11-01", "x", Some("17:30".into()));
        assert_eq!(e.scheduled_ms(&IST), Some(at("2026-11-01", "12:00")));
        assert_eq!(e.scheduled_ms(&UTC), Some(at("2026-11-01", "17:30")));
        // all-day and pre-epoch results have no instant
        assert_eq!(
            CalendarEvent::new(2, "2026-11-01", "x", None).scheduled_ms(&IST),
            None
        );
        let early = CalendarEvent::new(3, "1970-01-01", "x", Some("01:00".into()));
        assert_eq!(early.scheduled_ms(&IST), None);
    }

    /// US-Eastern-like zone: EST (-5h) with EDT (-4h) from `start` (inclusive) to `end`.
    struct Dst {
        start: i64,
        end: i64,
    }
    impl ZoneOffset for Dst {
        fn offset_ms_at(&self, t: i64) -> i64 {
            if t >= self.start && t < self.end {
                -4 * 3_600_000
            } else {
                -5 * 3_600_000
            }
        }
    }
    fn us() -> Dst {
        // 2026: DST starts Sun 8 Mar 07:00Z (02:00 EST -> 03:00 EDT), ends Sun 1 Nov 06:00Z.
        Dst {
            start: naive_local_ms("2026-03-08", "07:00").unwrap(),
            end: naive_local_ms("2026-11-01", "06:00").unwrap(),
        }
    }
    fn utc_of(z: &Dst, date: &str, time: &str) -> u64 {
        local_to_utc_ms(z, naive_local_ms(date, time).unwrap()) as u64
    }

    #[test]
    fn ordinary_times_use_the_offset_in_effect() {
        let z = us();
        assert_eq!(utc_of(&z, "2026-03-08", "01:30"), at("2026-03-08", "06:30")); // EST
        assert_eq!(utc_of(&z, "2026-07-04", "12:00"), at("2026-07-04", "16:00")); // EDT
        assert_eq!(utc_of(&z, "2026-12-25", "09:00"), at("2026-12-25", "14:00"));
        // EST again
    }

    #[test]
    fn a_time_skipped_by_the_spring_jump_lands_just_after_it() {
        let z = us();
        // 02:30 never happens on 8 Mar; it rings at 03:30 EDT (= 07:30Z), not an hour early.
        assert_eq!(utc_of(&z, "2026-03-08", "02:30"), at("2026-03-08", "07:30"));
        assert_eq!(utc_of(&z, "2026-03-08", "03:30"), at("2026-03-08", "07:30"));
        // and the very edge of the gap
        assert_eq!(utc_of(&z, "2026-03-08", "02:00"), at("2026-03-08", "07:00"));
    }

    #[test]
    fn a_time_repeated_by_the_autumn_fall_back_rings_once_at_the_first_occurrence() {
        let z = us();
        // 01:30 happens twice on 1 Nov: 05:30Z (EDT) then 06:30Z (EST). Ring at the first.
        assert_eq!(utc_of(&z, "2026-11-01", "01:30"), at("2026-11-01", "05:30"));
        assert_eq!(utc_of(&z, "2026-11-01", "00:30"), at("2026-11-01", "04:30")); // before it
        assert_eq!(utc_of(&z, "2026-11-01", "02:30"), at("2026-11-01", "07:30"));
        // after it
    }

    #[test]
    fn a_reminder_set_across_the_jump_stays_on_wall_clock_time() {
        // 09:00 every morning is 09:00 on the wall, so the UTC gap between 08 Mar 09:00 and
        // 07 Mar 09:00 is 23h, not 24h.
        let z = us();
        let a = utc_of(&z, "2026-03-07", "09:00");
        let b = utc_of(&z, "2026-03-08", "09:00");
        assert_eq!(b - a, 23 * 3_600_000);
    }

    // ---- scheduling -----------------------------------------------------------------

    #[test]
    fn next_alert_picks_the_earliest_pending_and_snooze_wins() {
        let mut c = Calendar::new();
        let a = c.add("2026-11-01", "late", Some("18:00")).unwrap();
        let b = c.add("2026-11-01", "early", Some("09:00")).unwrap();
        c.add("2026-11-01", "all day", None).unwrap();
        assert_eq!(c.next_alert_ms(&UTC), Some(at("2026-11-01", "09:00")));
        let order: Vec<_> = c.pending(&UTC).iter().map(|p| p.id).collect();
        assert_eq!(order, vec![b, a]);
        // snoozing "early" past "late" reorders the queue and flags it
        assert!(c.snooze(b, at("2026-11-01", "20:00")));
        let p = c.pending(&UTC);
        assert_eq!((p[0].id, p[0].snoozed), (a, false));
        assert_eq!((p[1].id, p[1].snoozed), (b, true));
        assert_eq!(c.next_alert_ms(&UTC), Some(at("2026-11-01", "18:00")));
    }

    #[test]
    fn nothing_is_due_before_its_time_and_it_fires_exactly_on_it() {
        let mut c = Calendar::new();
        let id = c.add("2026-11-01", "Call", Some("17:00")).unwrap();
        let t = at("2026-11-01", "17:00");
        assert!(c.collect_due(&UTC, t - 1).is_empty());
        let due = c.collect_due(&UTC, t);
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].event.id, id);
        assert_eq!((due[0].due_at, due[0].late_ms), (t, 0));
        assert!(!due[0].missed && !due[0].silent);
        let e = c.get(id).unwrap();
        assert_eq!(e.fired_at, Some(t));
        assert!(!e.missed);
    }

    #[test]
    fn a_reminder_is_handed_out_once_and_never_again() {
        let mut c = Calendar::new();
        c.add("2026-11-01", "Once", Some("17:00")).unwrap();
        let t = at("2026-11-01", "17:00");
        assert_eq!(c.collect_due(&UTC, t).len(), 1);
        assert!(c.collect_due(&UTC, t).is_empty());
        assert!(c.collect_due(&UTC, t + 10 * MIN).is_empty());
        assert_eq!(c.next_alert_ms(&UTC), None);
        assert!(c.pending(&UTC).is_empty());
        // ... and a restart (persisted fired_at) does not bring it back
        let mut restored = Calendar::from_events(c.list().to_vec());
        assert!(restored.collect_due(&UTC, t + 99 * MIN).is_empty());
    }

    #[test]
    fn late_delivery_is_marked_missed_and_very_late_is_silent() {
        let mut c = Calendar::new();
        let on_time = c.add("2026-11-01", "slightly late", Some("10:00")).unwrap();
        let missed = c
            .add("2026-11-01", "closed at the time", Some("11:00"))
            .unwrap();
        let stale = c.add("2026-10-30", "ancient", Some("11:00")).unwrap();
        // 25h05 after "ancient": it is silent + missed; the 1 Nov ones are not yet due
        let due = c.collect_due(&UTC, at("2026-10-30", "11:00") + 25 * 3_600_000 + 5 * MIN);
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].event.id, stale);
        assert!(due[0].missed && due[0].silent);
        assert!(c.get(stale).unwrap().missed);

        let due = c.collect_due(&UTC, at("2026-11-01", "10:00") + 30_000);
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].event.id, on_time);
        assert!(!due[0].missed, "30s late is on time");

        let due = c.collect_due(&UTC, at("2026-11-01", "11:00") + 2 * MIN);
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].event.id, missed);
        assert!(due[0].missed && !due[0].silent);
        assert_eq!(due[0].late_ms, 2 * MIN);
    }

    #[test]
    fn several_due_at_once_come_back_oldest_first() {
        let mut c = Calendar::new();
        let b = c.add("2026-11-01", "b", Some("10:05")).unwrap();
        let a = c.add("2026-11-01", "a", Some("10:00")).unwrap();
        let ids: Vec<_> = c
            .collect_due(&UTC, at("2026-11-01", "11:00"))
            .iter()
            .map(|d| d.event.id)
            .collect();
        assert_eq!(ids, vec![a, b]);
    }

    #[test]
    fn a_disabled_or_all_day_event_never_fires() {
        let mut c = Calendar::new();
        let off = c
            .add_with(
                "2026-11-01",
                "off",
                Some("10:00"),
                ReminderOpts {
                    remind: Some(false),
                    sound: None,
                },
            )
            .unwrap();
        c.add("2026-11-01", "all day", None).unwrap();
        assert!(!c.get(off).unwrap().remind);
        assert!(c.collect_due(&UTC, at("2026-11-02", "00:00")).is_empty());
    }

    #[test]
    fn snooze_rings_again_at_the_new_time_then_not_again() {
        let mut c = Calendar::new();
        let id = c.add("2026-11-01", "Tea", Some("10:00")).unwrap();
        let t = at("2026-11-01", "10:00");
        assert_eq!(c.collect_due(&UTC, t).len(), 1);

        assert!(c.snooze(id, t + 5 * MIN));
        let e = c.get(id).unwrap();
        assert_eq!((e.fired_at, e.snoozed_until), (None, Some(t + 5 * MIN)));
        assert!(c.collect_due(&UTC, t + 5 * MIN - 1).is_empty());
        let again = c.collect_due(&UTC, t + 5 * MIN);
        assert_eq!(again.len(), 1);
        assert_eq!(again[0].due_at, t + 5 * MIN);
        assert!(!again[0].missed);
        assert_eq!(c.get(id).unwrap().snoozed_until, None);
        assert!(c.collect_due(&UTC, t + 60 * MIN).is_empty());
    }

    #[test]
    fn snooze_can_be_repeated_and_needs_a_timed_event() {
        let mut c = Calendar::new();
        let id = c.add("2026-11-01", "Tea", Some("10:00")).unwrap();
        let all_day = c.add("2026-11-01", "Holiday", None).unwrap();
        assert!(c.snooze(id, 100));
        assert!(c.snooze(id, 200));
        assert_eq!(c.get(id).unwrap().snoozed_until, Some(200));
        assert!(!c.snooze(all_day, 300));
        assert!(!c.snooze(404, 300));
    }

    #[test]
    fn dismiss_cancels_a_pending_alert_and_a_snooze() {
        let mut c = Calendar::new();
        let id = c.add("2026-11-01", "Tea", Some("10:00")).unwrap();
        let t = at("2026-11-01", "10:00");
        assert!(c.dismiss(id, t - 5 * MIN));
        assert_eq!(c.get(id).unwrap().fired_at, Some(t - 5 * MIN));
        assert!(c.collect_due(&UTC, t + MIN).is_empty());

        // dismissing a snoozed reminder stops the repeat
        let id2 = c.add("2026-11-01", "Tea 2", Some("11:00")).unwrap();
        let t2 = at("2026-11-01", "11:00");
        c.collect_due(&UTC, t2);
        c.snooze(id2, t2 + 5 * MIN);
        assert!(c.dismiss(id2, t2 + MIN));
        let e = c.get(id2).unwrap();
        assert_eq!(e.snoozed_until, None);
        assert!(e.fired_at.is_some());
        assert!(c.collect_due(&UTC, t2 + 10 * MIN).is_empty());
        assert!(!c.dismiss(404, 0));
    }

    #[test]
    fn toggling_the_alert_off_and_on_rearms_it() {
        let mut c = Calendar::new();
        let id = c.add("2026-11-01", "Tea", Some("10:00")).unwrap();
        let t = at("2026-11-01", "10:00");
        c.collect_due(&UTC, t);
        c.set_alert(id, Some(false), None);
        assert!(c.next_alert_ms(&UTC).is_none());
        c.set_alert(id, Some(true), None);
        let e = c.get(id).unwrap();
        assert!(e.remind);
        assert_eq!((e.fired_at, e.snoozed_until, e.missed), (None, None, false));
        assert_eq!(c.next_alert_ms(&UTC), Some(t));
        // sound toggles independently and does not re-arm
        c.collect_due(&UTC, t);
        c.set_alert(id, None, Some(false));
        let e = c.get(id).unwrap();
        assert!(!e.sound && e.fired_at.is_some());
    }

    #[test]
    fn a_reminder_created_in_the_past_is_disarmed_but_one_a_moment_late_still_rings() {
        let mut c = Calendar::new();
        let now = at("2026-11-01", "10:00");
        let past = c.add("2026-11-01", "past", Some("09:00")).unwrap();
        let just = c.add("2026-11-01", "just missed", Some("09:59")).unwrap(); // 60s ago
        let future = c.add("2026-11-01", "future", Some("10:30")).unwrap();
        assert!(c.disarm_if_past(past, &UTC, now));
        assert!(!c.get(past).unwrap().remind);
        assert!(!c.disarm_if_past(just, &UTC, now));
        assert!(c.get(just).unwrap().remind);
        assert!(!c.disarm_if_past(future, &UTC, now));
        assert!(!c.disarm_if_past(404, &UTC, now));
        // within the grace the late one is delivered immediately
        let due = c.collect_due(&UTC, now);
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].event.id, just);
    }

    #[test]
    fn a_snoozed_repeat_is_flagged_as_such() {
        let mut c = Calendar::new();
        let id = c.add("2026-11-01", "Tea", Some("10:00")).unwrap();
        let t = at("2026-11-01", "10:00");
        let first = c.collect_due(&UTC, t);
        assert!(!first[0].snoozed);
        c.snooze(id, t + 5 * MIN);
        let again = c.collect_due(&UTC, t + 5 * MIN);
        assert!(again[0].snoozed);
    }

    #[test]
    fn alert_text_names_the_reminder_and_why_it_is_here() {
        let e = CalendarEvent::new(1, "2026-11-01", "Call mum", Some("17:00".into()));
        assert_eq!(
            alert_text(&e, false, false, false),
            ("Call mum".to_string(), "Reminder \u{b7} 17:00".to_string())
        );
        assert_eq!(
            alert_text(&e, true, false, false).1,
            "Missed reminder \u{b7} was due 17:00 on 1 Nov"
        );
        assert_eq!(
            alert_text(&e, false, true, false).1,
            "Snoozed reminder \u{b7} 17:00"
        );
        assert_eq!(
            alert_text(&e, false, false, true).1,
            "Test reminder \u{b7} 17:00"
        );
        // a nonsense date is shown as stored rather than panicking
        let odd = CalendarEvent::new(2, "nope", "x", Some("09:05".into()));
        assert_eq!(
            alert_text(&odd, true, false, false).1,
            "Missed reminder \u{b7} was due 09:05 on nope"
        );
        assert_eq!(short_date("2026-12-25"), "25 Dec");
    }

    #[test]
    fn disarm_leaves_a_snoozed_future_alert_alone() {
        let mut c = Calendar::new();
        let id = c.add("2026-11-01", "Tea", Some("08:00")).unwrap();
        let now = at("2026-11-01", "10:00");
        c.snooze(id, now + 5 * MIN); // scheduled time long gone, but the snooze is ahead
        assert!(!c.disarm_if_past(id, &UTC, now));
        assert!(c.get(id).unwrap().remind);
    }
}
