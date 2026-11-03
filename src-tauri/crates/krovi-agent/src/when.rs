//! Time expressions for reminders: "in 20 minutes", "at 5pm", "tomorrow at 9", "friday at 3pm",
//! "next monday", "march 15", "tonight at 8". Everything resolves against a supplied local
//! clock, so it is deterministic and testable. The whole phrase must be understood (no leftover
//! words), which is what lets the reminder parser tell a time from a title.
//!
//! Rules worth knowing (all shown to the user on the confirmation card as an absolute time):
//!
//! * Relative times ("in 20 minutes") are rounded UP to the next whole minute, never early.
//! * A time with no date means the next time that clock reading occurs: today if it is still
//!   ahead, otherwise tomorrow.
//! * A bare hour with no am/pm ("at 5") takes the earliest reading still ahead today; on a later
//!   day it means 7-11 = morning and 12, 1-6 = afternoon/evening.
//! * A date with no time means 09:00 (flagged `default_time`).
//! * A bare weekday is its next occurrence (today only if the time is still ahead); "next
//!   friday" is that weekday in the following Monday-to-Sunday week.
//! * A month-and-day that already passed this year means next year.

use crate::civil::{Date, LocalClock};
use crate::text::{parse_duration, parse_int};

/// The default reminder time when only a date was given.
pub const DEFAULT_MINUTES: u32 = 9 * 60;

/// An absolute local date and time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Resolved {
    pub date: Date,
    /// Minutes since local midnight.
    pub minutes: u32,
    /// No time was said; [`DEFAULT_MINUTES`] was used.
    pub default_time: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Daypart {
    Morning,
    Afternoon,
    Evening,
    Night,
}

impl Daypart {
    fn of(w: &str) -> Option<Daypart> {
        Some(match w {
            "morning" => Daypart::Morning,
            "afternoon" => Daypart::Afternoon,
            "evening" => Daypart::Evening,
            "night" => Daypart::Night,
            _ => return None,
        })
    }
    fn minutes(self) -> u32 {
        match self {
            Daypart::Morning => 9 * 60,
            Daypart::Afternoon => 15 * 60,
            Daypart::Evening => 18 * 60,
            Daypart::Night => 21 * 60,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClockSpec {
    Fixed(u32),
    /// "5", "5:30" with no am/pm: `hour12` 1..=12.
    Ambiguous {
        hour12: u32,
        minute: u32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DateSpec {
    Fixed(Date),
    Weekday { wd: u32, next: bool },
    DayOnly(u32),
}

/// Resolve a time phrase (lower-case words) against `now`. `None` unless every word is part of
/// a time expression.
pub fn parse_when(w: &[&str], now: &LocalClock) -> Option<Resolved> {
    if w.is_empty() {
        return None;
    }
    relative(w, now).or_else(|| absolute(w, now))
}

fn relative(w: &[&str], now: &LocalClock) -> Option<Resolved> {
    let (start, lead) = if w[0] == "in" || w[0] == "after" {
        (1, true)
    } else {
        (0, false)
    };
    let (secs, used) = parse_duration(&w[start..])?;
    let mut end = start + used;
    let rest = &w[end..];
    let trailing = if rest == ["from", "now"] {
        end += 2;
        true
    } else if rest == ["later"] {
        end += 1;
        true
    } else {
        false
    };
    if end != w.len() || !(lead || trailing) || secs == 0 {
        return None;
    }
    let target = now.local_secs() + secs as i64;
    // reminders are minute-granular and must never ring early: round up to a whole minute
    let target = (target + 59).div_euclid(60) * 60;
    let c = LocalClock::from_local_secs(target);
    Some(Resolved {
        date: c.date,
        minutes: c.minutes,
        default_time: false,
    })
}

// ---------------------------------------------------------------------------------------------
// date words
// ---------------------------------------------------------------------------------------------

fn month_of(w: &str) -> Option<u32> {
    Some(match w {
        "january" | "jan" => 1,
        "february" | "feb" => 2,
        "march" | "mar" => 3,
        "april" | "apr" => 4,
        "may" => 5,
        "june" | "jun" => 6,
        "july" | "jul" => 7,
        "august" | "aug" => 8,
        "september" | "sep" | "sept" => 9,
        "october" | "oct" => 10,
        "november" | "nov" => 11,
        "december" | "dec" => 12,
        _ => return None,
    })
}

fn weekday_of(w: &str) -> Option<u32> {
    Some(match w {
        "monday" => 0,
        "tuesday" => 1,
        "wednesday" => 2,
        "thursday" => 3,
        "friday" => 4,
        "saturday" => 5,
        "sunday" => 6,
        _ => return None,
    })
}

/// "5", "5th", "22nd": day number 1..=31 and whether an ordinal suffix was present.
fn day_ordinal(w: &str) -> Option<(u32, bool)> {
    let (digits, suffix) = match w.len().checked_sub(2).map(|i| w.split_at(i)) {
        Some((d, s)) if matches!(s, "st" | "nd" | "rd" | "th") && !d.is_empty() => (d, true),
        _ => (w, false),
    };
    if digits.is_empty() || digits.len() > 2 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let n: u32 = digits.parse().ok()?;
    (1..=31).contains(&n).then_some((n, suffix))
}

fn month_day(m: u32, d: u32, today: Date) -> Option<Date> {
    match Date::new(today.y, m, d) {
        Some(dt) if dt >= today => Some(dt),
        _ => Date::new(today.y + 1, m, d),
    }
}

/// A date word at the start of `w`: the date, how many words it used, and a part of day it
/// implies ("tonight", "this afternoon").
fn date_item(w: &[&str], now: &LocalClock) -> Option<(DateSpec, usize, Option<Daypart>)> {
    let today = now.date;
    match w[0] {
        "today" => return Some((DateSpec::Fixed(today), 1, None)),
        "tonight" => return Some((DateSpec::Fixed(today), 1, Some(Daypart::Night))),
        "tomorrow" | "tmrw" => return Some((DateSpec::Fixed(today.add_days(1)), 1, None)),
        "day" => {
            if w.get(1) == Some(&"after") && w.get(2) == Some(&"tomorrow") {
                return Some((DateSpec::Fixed(today.add_days(2)), 3, None));
            }
            return None;
        }
        "this" => {
            let n = w.get(1)?;
            if let Some(dp) = Daypart::of(n) {
                return Some((DateSpec::Fixed(today), 2, Some(dp)));
            }
            return weekday_of(n).map(|wd| (DateSpec::Weekday { wd, next: false }, 2, None));
        }
        "next" => {
            return weekday_of(w.get(1)?).map(|wd| (DateSpec::Weekday { wd, next: true }, 2, None));
        }
        "coming" => {
            return weekday_of(w.get(1)?)
                .map(|wd| (DateSpec::Weekday { wd, next: false }, 2, None));
        }
        "in" => {
            // "in 3 days", "in a week" (a time of day may follow); hours/minutes are relative()
            let (secs, used) = parse_duration(&w[1..])?;
            return (secs > 0 && secs % 86_400 == 0).then(|| {
                (
                    DateSpec::Fixed(today.add_days((secs / 86_400) as i64)),
                    1 + used,
                    None,
                )
            });
        }
        _ => {}
    }
    if let Some(wd) = weekday_of(w[0]) {
        return Some((DateSpec::Weekday { wd, next: false }, 1, None));
    }
    // "march 15", "march 15th", "march the 15th"
    if let Some(m) = month_of(w[0]) {
        let mut i = 1;
        if w.get(i) == Some(&"the") {
            i += 1;
        }
        if let Some((d, _)) = w.get(i).and_then(|x| day_ordinal(x)) {
            return month_day(m, d, today).map(|dt| (DateSpec::Fixed(dt), i + 1, None));
        }
    }
    // "15 march", "15th of march", "the 15th of march", "the 5th"
    let j = usize::from(w[0] == "the");
    let (d, suffix) = day_ordinal(w.get(j)?)?;
    let mut k = j + 1;
    if w.get(k) == Some(&"of") {
        k += 1;
    }
    if let Some(m) = w.get(k).and_then(|x| month_of(x)) {
        return month_day(m, d, today).map(|dt| (DateSpec::Fixed(dt), k + 1, None));
    }
    suffix.then_some((DateSpec::DayOnly(d), j + 1, None))
}

// ---------------------------------------------------------------------------------------------
// clock words
// ---------------------------------------------------------------------------------------------

fn spec(hour: u32, minute: u32, mer: Option<bool>, leading_zero: bool) -> Option<ClockSpec> {
    match mer {
        Some(pm) => {
            if !(1..=12).contains(&hour) {
                return None;
            }
            Some(ClockSpec::Fixed(
                (hour % 12 + if pm { 12 } else { 0 }) * 60 + minute,
            ))
        }
        None => {
            if hour > 23 {
                return None;
            }
            if hour == 0 || hour >= 13 || leading_zero {
                Some(ClockSpec::Fixed(hour * 60 + minute))
            } else {
                Some(ClockSpec::Ambiguous {
                    hour12: hour,
                    minute,
                })
            }
        }
    }
}

fn meridiem(w: Option<&&str>) -> Option<bool> {
    match w.map(|s| s.replace('.', "")).as_deref() {
        Some("am") => Some(false),
        Some("pm") => Some(true),
        _ => None,
    }
}

fn is_oclock(w: Option<&&str>) -> bool {
    matches!(
        w.map(|s| s.replace('.', "")).as_deref(),
        Some("oclock") | Some("o'clock")
    )
}

/// A clock reading at the start of `w`. A bare hour ("5", "five") only counts right after
/// "at"/"around"/"by" (`after_at`), so a stray number in a title is not mistaken for a time.
fn clock_item(w: &[&str], after_at: bool) -> Option<(ClockSpec, usize)> {
    let t0 = w[0];
    match t0 {
        "noon" | "midday" => return Some((ClockSpec::Fixed(12 * 60), 1)),
        "midnight" => return Some((ClockSpec::Fixed(0), 1)),
        _ => {}
    }
    // "half past five", "quarter past 5", "quarter to six"
    if matches!(t0, "half" | "quarter") && w.len() >= 3 && matches!(w[1], "past" | "to") {
        let (h, used) = parse_int(&w[2..]).filter(|(h, _)| (1..=12).contains(h))?;
        let h = h as u32;
        let (hour12, minute) = match (t0, w[1]) {
            ("half", "past") => (h, 30),
            ("quarter", "past") => (h, 15),
            ("quarter", "to") => (if h == 1 { 12 } else { h - 1 }, 45),
            _ => return None,
        };
        return Some((ClockSpec::Ambiguous { hour12, minute }, 2 + used));
    }

    let digits_first = t0.as_bytes().first().is_some_and(u8::is_ascii_digit);
    if digits_first {
        let t = t0.replace('.', "");
        let (mer_inline, body) = if let Some(b) = t.strip_suffix("am") {
            (Some(false), b)
        } else if let Some(b) = t.strip_suffix("pm") {
            (Some(true), b)
        } else {
            (None, t.as_str())
        };
        let (h_str, m_str) = match body.split_once(':') {
            Some((h, m)) => (h, Some(m)),
            None => (body, None),
        };
        let all_digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
        if h_str.len() > 2 || !all_digits(h_str) {
            return None;
        }
        let hour: u32 = h_str.parse().ok()?;
        let minute: u32 = match m_str {
            Some(m) => {
                if m.len() != 2 || !all_digits(m) {
                    return None;
                }
                m.parse().ok()?
            }
            None => 0,
        };
        if minute > 59 {
            return None;
        }
        let (mut mer, mut used, mut oclock) = (mer_inline, 1usize, false);
        if mer.is_none() {
            if let Some(pm) = meridiem(w.get(1)) {
                mer = Some(pm);
                used = 2;
            } else if m_str.is_none() && is_oclock(w.get(1)) {
                oclock = true;
                used = 2;
            }
        }
        if mer.is_none() && m_str.is_none() && !oclock && !after_at {
            return None;
        }
        let leading_zero = h_str.len() == 2 && h_str.starts_with('0');
        return spec(hour, minute, mer, leading_zero).map(|s| (s, used));
    }

    // spoken: "five", "five thirty", "five pm", "ten o'clock"
    let (h, hn) = parse_int(w)?;
    if h > 23 {
        return None;
    }
    let mut used = hn;
    let mut minute = 0;
    if meridiem(w.get(used)).is_none() {
        if let Some((m, mn)) = parse_int(&w[used..]).filter(|(m, _)| (1..=59).contains(m)) {
            minute = m as u32;
            used += mn;
        }
    }
    let mut mer = None;
    let mut oclock = false;
    if let Some(pm) = meridiem(w.get(used)) {
        mer = Some(pm);
        used += 1;
    } else if is_oclock(w.get(used)) {
        oclock = true;
        used += 1;
    }
    if mer.is_none() && !oclock && !after_at {
        return None;
    }
    spec(h as u32, minute, mer, false).map(|s| (s, used))
}

// ---------------------------------------------------------------------------------------------
// putting it together
// ---------------------------------------------------------------------------------------------

fn absolute(w: &[&str], now: &LocalClock) -> Option<Resolved> {
    let mut i = 0;
    let mut date: Option<DateSpec> = None;
    let mut clock: Option<ClockSpec> = None;
    let mut part: Option<Daypart> = None;
    let mut after_at = false;
    while i < w.len() {
        let t = w[i];
        if matches!(t, "at" | "around" | "by") {
            after_at = true;
            i += 1;
            continue;
        }
        if matches!(t, "on" | "for") {
            i += 1;
            continue;
        }
        if date.is_none() {
            if let Some((d, used, dp)) = date_item(&w[i..], now) {
                date = Some(d);
                part = part.or(dp);
                i += used;
                after_at = false;
                continue;
            }
        }
        if part.is_none() {
            // "in the morning", "morning"
            let (dp, used) = if t == "in" && w.get(i + 1) == Some(&"the") {
                (w.get(i + 2).and_then(|x| Daypart::of(x)), 3)
            } else {
                (Daypart::of(t), 1)
            };
            if let Some(dp) = dp {
                part = Some(dp);
                i += used;
                continue;
            }
        }
        if clock.is_none() {
            if let Some((c, used)) = clock_item(&w[i..], after_at) {
                clock = Some(c);
                i += used;
                after_at = false;
                continue;
            }
        }
        return None;
    }
    if date.is_none() && clock.is_none() && part.is_none() {
        return None;
    }
    resolve(date, clock, part, now)
}

/// For an hour with no am/pm on a day other than today: 7-11 is morning, 12 and 1-6 afternoon.
fn ambiguous_default(am: u32) -> u32 {
    let h = am / 60;
    let h12 = if h == 0 { 12 } else { h };
    if (7..=11).contains(&h12) {
        am
    } else {
        am + 12 * 60
    }
}

fn resolve(
    date: Option<DateSpec>,
    clock: Option<ClockSpec>,
    part: Option<Daypart>,
    now: &LocalClock,
) -> Option<Resolved> {
    let (cands, default_time): (Vec<u32>, bool) = match clock {
        Some(ClockSpec::Fixed(m)) => (vec![m], false),
        Some(ClockSpec::Ambiguous { hour12, minute }) => {
            let am = (hour12 % 12) * 60 + minute;
            match part {
                Some(Daypart::Morning) => (vec![am], false),
                Some(_) => (vec![am + 12 * 60], false),
                None => (vec![am, am + 12 * 60], false),
            }
        }
        None => match part {
            Some(p) => (vec![p.minutes()], false),
            None => (vec![DEFAULT_MINUTES], true),
        },
    };
    let now_in_day = now.minutes * 60 + now.secs;
    let earliest_today = || cands.iter().copied().filter(|m| m * 60 > now_in_day).min();
    let for_other_day = || {
        if cands.len() == 2 {
            ambiguous_default(cands[0])
        } else {
            cands[0]
        }
    };
    let today = now.date;

    let (day, minutes) = match date {
        None => match earliest_today() {
            Some(m) => (today, m),
            None => (today.add_days(1), for_other_day()),
        },
        Some(DateSpec::Fixed(d)) if d == today => {
            (today, earliest_today().unwrap_or_else(for_other_day))
        }
        Some(DateSpec::Fixed(d)) => (d, for_other_day()),
        Some(DateSpec::Weekday { wd, next: true }) => {
            let monday_next = today.add_days(7 - i64::from(today.weekday()));
            (monday_next.add_days(i64::from(wd)), for_other_day())
        }
        Some(DateSpec::Weekday { wd, next: false }) => {
            let ahead = (wd + 7 - today.weekday()) % 7;
            if ahead == 0 {
                match earliest_today() {
                    Some(m) => (today, m),
                    None => (today.add_days(7), for_other_day()),
                }
            } else {
                (today.add_days(i64::from(ahead)), for_other_day())
            }
        }
        Some(DateSpec::DayOnly(day)) => {
            let (mut y, mut m) = (today.y, today.m);
            if day < today.d {
                m += 1;
            }
            let mut found = None;
            for _ in 0..14 {
                if m > 12 {
                    m = 1;
                    y += 1;
                }
                if let Some(d) = Date::new(y, m, day) {
                    found = Some(d);
                    break;
                }
                m += 1;
            }
            let d = found?;
            if d == today {
                (today, earliest_today().unwrap_or_else(for_other_day))
            } else {
                (d, for_other_day())
            }
        }
    };
    Some(Resolved {
        date: day,
        minutes,
        default_time,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> Date {
        Date::new(y, m, day).unwrap()
    }

    /// Monday 2026-11-02, 10:00:30 local.
    fn now() -> LocalClock {
        LocalClock {
            date: d(2026, 11, 2),
            minutes: 10 * 60,
            secs: 30,
        }
    }

    fn at(phrase: &str) -> Option<(Date, u32, bool)> {
        at_from(phrase, &now())
    }

    fn at_from(phrase: &str, clock: &LocalClock) -> Option<(Date, u32, bool)> {
        let w: Vec<String> = phrase.split_whitespace().map(str::to_lowercase).collect();
        let r: Vec<&str> = w.iter().map(String::as_str).collect();
        parse_when(&r, clock).map(|r| (r.date, r.minutes, r.default_time))
    }

    fn hm(h: u32, m: u32) -> u32 {
        h * 60 + m
    }

    #[test]
    fn relative_times_round_up_and_never_ring_early() {
        for (p, want) in [
            ("in 20 minutes", (d(2026, 11, 2), hm(10, 21))),
            ("in an hour", (d(2026, 11, 2), hm(11, 1))),
            ("in half an hour", (d(2026, 11, 2), hm(10, 31))),
            ("in 1 hour 30 minutes", (d(2026, 11, 2), hm(11, 31))),
            ("in 90 seconds", (d(2026, 11, 2), hm(10, 2))),
            ("in 2 days", (d(2026, 11, 4), hm(10, 1))),
            ("10 minutes from now", (d(2026, 11, 2), hm(10, 11))),
            ("in 15 hours", (d(2026, 11, 3), hm(1, 1))),
            ("after 5 minutes", (d(2026, 11, 2), hm(10, 6))),
        ] {
            let (date, minutes, def) = at(p).unwrap_or_else(|| panic!("{p}"));
            assert_eq!((date, minutes), want, "{p}");
            assert!(!def);
        }
        // exactly on a minute: no rounding
        let exact = LocalClock {
            date: d(2026, 11, 2),
            minutes: hm(10, 0),
            secs: 0,
        };
        assert_eq!(at_from("in 20 minutes", &exact).unwrap().1, hm(10, 20));
    }

    #[test]
    fn relative_rolls_over_midnight_and_month() {
        let late = LocalClock {
            date: d(2026, 12, 31),
            minutes: hm(23, 50),
            secs: 0,
        };
        assert_eq!(
            at_from("in 20 minutes", &late).unwrap(),
            (d(2027, 1, 1), hm(0, 10), false)
        );
    }

    #[test]
    fn clock_times_with_no_date() {
        for (p, want) in [
            ("at 5pm", (d(2026, 11, 2), hm(17, 0))),
            ("at 5 pm", (d(2026, 11, 2), hm(17, 0))),
            ("5pm", (d(2026, 11, 2), hm(17, 0))),
            ("at 5:30pm", (d(2026, 11, 2), hm(17, 30))),
            ("at 5:30 pm", (d(2026, 11, 2), hm(17, 30))),
            ("at 5 p.m.", (d(2026, 11, 2), hm(17, 0))),
            ("at 17:00", (d(2026, 11, 2), hm(17, 0))),
            ("17:45", (d(2026, 11, 2), hm(17, 45))),
            ("at 9am", (d(2026, 11, 3), hm(9, 0))),
            ("at 11am", (d(2026, 11, 2), hm(11, 0))),
            ("at 12pm", (d(2026, 11, 2), hm(12, 0))),
            ("at 12am", (d(2026, 11, 3), hm(0, 0))),
            ("at noon", (d(2026, 11, 2), hm(12, 0))),
            ("at midnight", (d(2026, 11, 3), hm(0, 0))),
            ("at 5", (d(2026, 11, 2), hm(17, 0))),
            ("at 9", (d(2026, 11, 2), hm(21, 0))),
            ("at 11", (d(2026, 11, 2), hm(11, 0))),
            ("at 5 o'clock", (d(2026, 11, 2), hm(17, 0))),
            ("at 09:30", (d(2026, 11, 3), hm(9, 30))),
            ("at five pm", (d(2026, 11, 2), hm(17, 0))),
            ("at five thirty pm", (d(2026, 11, 2), hm(17, 30))),
            ("at ten fifteen", (d(2026, 11, 2), hm(10, 15))),
            ("at half past five", (d(2026, 11, 2), hm(17, 30))),
            ("at quarter past 5", (d(2026, 11, 2), hm(17, 15))),
            ("at quarter to six", (d(2026, 11, 2), hm(17, 45))),
        ] {
            let (date, minutes, def) = at(p).unwrap_or_else(|| panic!("{p}"));
            assert_eq!((date, minutes), want, "{p}");
            assert!(!def, "{p}");
        }
    }

    #[test]
    fn dated_times() {
        for (p, want, def) in [
            ("tomorrow at 9", (d(2026, 11, 3), hm(9, 0)), false),
            ("tomorrow at 9am", (d(2026, 11, 3), hm(9, 0)), false),
            ("tomorrow at 5", (d(2026, 11, 3), hm(17, 0)), false),
            ("tomorrow at 6:30", (d(2026, 11, 3), hm(18, 30)), false),
            ("tomorrow at 11", (d(2026, 11, 3), hm(11, 0)), false),
            ("tomorrow at 12", (d(2026, 11, 3), hm(12, 0)), false),
            ("at 9 tomorrow", (d(2026, 11, 3), hm(9, 0)), false),
            ("tomorrow", (d(2026, 11, 3), hm(9, 0)), true),
            ("tomorrow morning", (d(2026, 11, 3), hm(9, 0)), false),
            ("tomorrow afternoon", (d(2026, 11, 3), hm(15, 0)), false),
            ("tomorrow evening", (d(2026, 11, 3), hm(18, 0)), false),
            ("tomorrow night", (d(2026, 11, 3), hm(21, 0)), false),
            (
                "tomorrow at 9 in the morning",
                (d(2026, 11, 3), hm(9, 0)),
                false,
            ),
            (
                "tomorrow at 7 in the evening",
                (d(2026, 11, 3), hm(19, 0)),
                false,
            ),
            ("tonight", (d(2026, 11, 2), hm(21, 0)), false),
            ("tonight at 8", (d(2026, 11, 2), hm(20, 0)), false),
            ("this afternoon", (d(2026, 11, 2), hm(15, 0)), false),
            ("today at 5pm", (d(2026, 11, 2), hm(17, 0)), false),
            (
                "day after tomorrow at noon",
                (d(2026, 11, 4), hm(12, 0)),
                false,
            ),
            ("on friday", (d(2026, 11, 6), hm(9, 0)), true),
            ("friday at 3pm", (d(2026, 11, 6), hm(15, 0)), false),
            ("on friday at 3", (d(2026, 11, 6), hm(15, 0)), false),
            ("this friday", (d(2026, 11, 6), hm(9, 0)), true),
            ("coming friday", (d(2026, 11, 6), hm(9, 0)), true),
            ("next friday", (d(2026, 11, 13), hm(9, 0)), true),
            ("next monday", (d(2026, 11, 9), hm(9, 0)), true),
            ("monday", (d(2026, 11, 9), hm(9, 0)), true),
            ("monday at 3pm", (d(2026, 11, 2), hm(15, 0)), false),
            ("monday at 9am", (d(2026, 11, 9), hm(9, 0)), false),
            ("sunday at 10", (d(2026, 11, 8), hm(10, 0)), false),
            ("in 3 days at 9am", (d(2026, 11, 5), hm(9, 0)), false),
            ("in a week", (d(2026, 11, 9), hm(10, 1)), false),
            ("march 15", (d(2027, 3, 15), hm(9, 0)), true),
            ("on march 15th at 3pm", (d(2027, 3, 15), hm(15, 0)), false),
            ("15 march", (d(2027, 3, 15), hm(9, 0)), true),
            ("the 15th of march", (d(2027, 3, 15), hm(9, 0)), true),
            ("december 25", (d(2026, 12, 25), hm(9, 0)), true),
            ("on december the 25th", (d(2026, 12, 25), hm(9, 0)), true),
            ("november 20 at 8pm", (d(2026, 11, 20), hm(20, 0)), false),
            ("the 5th", (d(2026, 11, 5), hm(9, 0)), true),
            ("5th at 4pm", (d(2026, 11, 5), hm(16, 0)), false),
            ("the 1st", (d(2026, 12, 1), hm(9, 0)), true),
            ("the 31st", (d(2026, 12, 31), hm(9, 0)), true),
        ] {
            let got = at(p).unwrap_or_else(|| panic!("{p}"));
            assert_eq!(got, (want.0, want.1, def), "{p}");
        }
    }

    #[test]
    fn today_in_the_past_is_returned_as_is() {
        // the planner is what blocks it; the resolver just reports the truth
        assert_eq!(
            at("today at 9am").unwrap(),
            (d(2026, 11, 2), hm(9, 0), false)
        );
        assert_eq!(
            at("today at 5").unwrap(),
            (d(2026, 11, 2), hm(17, 0), false)
        );
        let evening = LocalClock {
            date: d(2026, 11, 2),
            minutes: hm(18, 0),
            secs: 0,
        };
        assert_eq!(
            at_from("today at 5", &evening).unwrap(),
            (d(2026, 11, 2), hm(17, 0), false)
        );
    }

    #[test]
    fn same_weekday_rolls_to_next_week_when_the_time_has_passed() {
        assert_eq!(at("monday at 9am").unwrap().0, d(2026, 11, 9));
        assert_eq!(at("monday at 11am").unwrap().0, d(2026, 11, 2));
    }

    #[test]
    fn year_rolls_over() {
        let dec = LocalClock {
            date: d(2026, 12, 30),
            minutes: hm(8, 0),
            secs: 0,
        };
        assert_eq!(at_from("january 2", &dec).unwrap().0, d(2027, 1, 2));
        assert_eq!(at_from("the 2nd", &dec).unwrap().0, d(2027, 1, 2));
        assert_eq!(at_from("tomorrow", &dec).unwrap().0, d(2026, 12, 31));
        assert_eq!(at_from("in 3 days", &dec).unwrap().0, d(2027, 1, 2));
    }

    #[test]
    fn a_bare_hour_needs_at() {
        for bad in ["5", "five", "tomorrow 5", "call mom five"] {
            assert!(at(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn not_a_time_at_all() {
        for bad in [
            "",
            "soon",
            "at",
            "for",
            "on",
            "call mom",
            "tomorrow foo",
            "at 25",
            "at 13pm",
            "at 0pm",
            "at 5:75",
            "at 5:7",
            "at 123",
            "february 30",
            "next week",
            "in soon",
            "in 2 hours at 5pm",
            "10 minutes",
            "the sun",
            "a minute",
            "march",
            "next",
            "this",
            "day after",
            "half past",
            "quarter to 13",
            "at five apples",
            "in the morning of doom",
        ] {
            assert!(at(bad).is_none(), "{bad:?}");
        }
    }

    #[test]
    fn two_dates_or_two_times_are_not_one_time() {
        assert!(at("tomorrow friday").is_none());
        assert!(at("at 5pm at 6pm").is_none());
    }
}
