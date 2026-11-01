//! Spoken dates and times ("tomorrow at nine", "friday", "the 5th", "in two hours") resolved
//! against a given local "now". Pure: no clock, no I/O. It works on the parser's normalized word
//! list and reports which words it used so the caller can leave the rest as the reminder title.

use crate::civil::{Date, LocalNow};
use crate::parser::parse_quantity;

/// What was found in an utterance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Extracted {
    pub date: Option<Date>,
    /// Minutes since midnight.
    pub time: Option<u32>,
    /// Words that belong to the date/time phrase (including their prepositions).
    pub consumed: Vec<bool>,
}

const MORNING: u32 = 9 * 60;
const AFTERNOON: u32 = 15 * 60;
const EVENING: u32 = 18 * 60;
const NIGHT: u32 = 20 * 60;

fn weekday_index(w: &str) -> Option<u32> {
    Some(match w {
        "monday" | "mon" => 0,
        "tuesday" | "tue" | "tues" => 1,
        "wednesday" | "wed" => 2,
        "thursday" | "thu" | "thur" | "thurs" => 3,
        "friday" | "fri" => 4,
        "saturday" => 5,
        "sunday" => 6,
        _ => return None,
    })
}

fn month_index(w: &str) -> Option<u32> {
    Some(match w {
        "january" | "jan" => 1,
        "february" | "feb" => 2,
        "march" => 3,
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

fn daypart(w: &str) -> Option<u32> {
    match w {
        "morning" => Some(MORNING),
        "afternoon" => Some(AFTERNOON),
        "evening" => Some(EVENING),
        "night" | "tonight" => Some(NIGHT),
        _ => None,
    }
}

fn ordinal_word(w: &str) -> Option<u32> {
    Some(match w {
        "first" => 1,
        "second" => 2,
        "third" => 3,
        "fourth" => 4,
        "fifth" => 5,
        "sixth" => 6,
        "seventh" => 7,
        "eighth" => 8,
        "ninth" => 9,
        "tenth" => 10,
        "eleventh" => 11,
        "twelfth" => 12,
        "thirteenth" => 13,
        "fourteenth" => 14,
        "fifteenth" => 15,
        "sixteenth" => 16,
        "seventeenth" => 17,
        "eighteenth" => 18,
        "nineteenth" => 19,
        "twentieth" => 20,
        "thirtieth" => 30,
        _ => return None,
    })
}

fn is_suffix(w: &str) -> bool {
    matches!(w, "st" | "nd" | "rd" | "th")
}

/// An ordinal day of month starting at `i`: "5th" (`5`,`th`), "fifth", "twenty fifth", "5".
/// `needs_marker`: a bare number only counts when it carries an ordinal suffix.
fn parse_day(w: &[&str], i: usize, needs_marker: bool) -> Option<(u32, usize)> {
    let t = *w.get(i)?;
    if t.starts_with(|c: char| c.is_ascii_digit()) && !t.contains('.') {
        let n: u32 = t.parse().ok()?;
        if !(1..=31).contains(&n) {
            return None;
        }
        if w.get(i + 1).is_some_and(|s| is_suffix(s)) {
            return Some((n, 2));
        }
        return (!needs_marker).then_some((n, 1));
    }
    if let Some(n) = ordinal_word(t) {
        return Some((n, 1));
    }
    // "twenty first" .. "thirty first"
    if matches!(t, "twenty" | "thirty") {
        let base = if t == "twenty" { 20 } else { 30 };
        if let Some(u) = w.get(i + 1).and_then(|n| ordinal_word(n)) {
            if u <= 9 && base + u <= 31 {
                return Some((base + u, 2));
            }
        }
    }
    None
}

/// Next occurrence of `day` of the month on or after `today`.
fn upcoming_day_of_month(day: u32, today: Date) -> Option<Date> {
    for add in 0..13 {
        let probe = Date { d: 1, ..today }.add_months(add);
        if let Some(cand) = Date::new(probe.y, probe.m, day) {
            if cand >= today {
                return Some(cand);
            }
        }
    }
    None
}

/// First occurrence of month/day on or after `today` (or in the explicit `year`).
fn month_day(month: u32, day: u32, year: Option<i32>, today: Date) -> Option<Date> {
    if let Some(y) = year {
        return Date::new(y, month, day);
    }
    let this = Date::new(today.y, month, day);
    match this {
        Some(d) if d >= today => Some(d),
        _ => Date::new(today.y + 1, month, day),
    }
}

/// How many words of a "year" (a four digit 19xx/20xx number) sit at `i`.
fn parse_year(w: &[&str], i: usize) -> Option<i32> {
    let t = *w.get(i)?;
    if t.len() == 4 && t.bytes().all(|b| b.is_ascii_digit()) {
        let y: i32 = t.parse().ok()?;
        return (1970..=2100).contains(&y).then_some(y);
    }
    None
}

#[derive(Clone, Copy)]
enum Mer {
    Am,
    Pm,
}

/// "am"/"pm", spoken or spelled ("a m" / "p m"), at `i`. Returns (meridiem, words used).
fn parse_meridiem(w: &[&str], i: usize) -> Option<(Mer, usize)> {
    match w.get(i).copied()? {
        "am" => Some((Mer::Am, 1)),
        "pm" => Some((Mer::Pm, 1)),
        "a" if w.get(i + 1) == Some(&"m") => Some((Mer::Am, 2)),
        "p" if w.get(i + 1) == Some(&"m") => Some((Mer::Pm, 2)),
        _ => None,
    }
}

/// A whole number from a digit string or number words, with how many words it took.
fn whole(w: &[&str], i: usize) -> Option<(u32, usize)> {
    let (q, used) = parse_quantity(w, i)?;
    if q.fract() != 0.0
        || !(0.0..=2400.0).contains(&q)
        || matches!(w[i], "a" | "an" | "half" | "quarter")
    {
        return None;
    }
    Some((q as u32, used))
}

/// Minutes after the hour: two digits ("30", "05") or words ("thirty", "oh five", "forty five").
fn parse_minute(w: &[&str], i: usize) -> Option<(u32, usize)> {
    let t = *w.get(i)?;
    if t == "oh" || t == "o" {
        let (n, used) = whole(w, i + 1)?;
        return (n <= 9).then_some((n, used + 1));
    }
    if t.starts_with(|c: char| c.is_ascii_digit()) {
        return (t.len() == 2)
            .then(|| t.parse::<u32>().ok().filter(|m| *m < 60).map(|m| (m, 1)))
            .flatten();
    }
    let (n, used) = whole(w, i)?;
    (10..60).contains(&n).then_some((n, used))
}

fn to_24h(hour: u32, mer: Option<Mer>) -> Option<u32> {
    match (hour, mer) {
        (13..=23, None) => Some(hour),
        (13..=23, Some(_)) | (24.., _) => None,
        (0, None) => Some(0),
        (0, Some(_)) => None,
        (12, Some(Mer::Am)) => Some(0),
        (12, _) => Some(12),
        (h, Some(Mer::Pm)) => Some(h + 12),
        (h, Some(Mer::Am)) => Some(h),
        // No am/pm: 1-6 almost always means the afternoon ("at 3"), 7-11 the morning ("at 9").
        (h @ 1..=6, None) => Some(h + 12),
        (h, None) => Some(h),
    }
}

/// A clock time starting at `i` (the hour or "half"/"quarter"/"noon"/"midnight"). `anchored` means
/// a time preposition ("at", "by") came right before, which lets a bare hour count as a time.
/// Returns (minutes since midnight, words used).
fn parse_time(w: &[&str], i: usize, anchored: bool) -> Option<(u32, usize, bool)> {
    match *w.get(i)? {
        "noon" | "midday" => return Some((12 * 60, 1, false)),
        "midnight" => return Some((0, 1, false)),
        "half" if w.get(i + 1) == Some(&"past") => {
            let (h, used) = whole(w, i + 2)?;
            return Some((to_24h(h, None)? % 24 * 60 + 30, 2 + used, false));
        }
        "quarter" if matches!(w.get(i + 1), Some(&"past") | Some(&"to")) => {
            let past = w[i + 1] == "past";
            let (h, used) = whole(w, i + 2)?;
            let base = to_24h(h, None)? % 24 * 60;
            return Some(if past {
                (base + 15, 2 + used, false)
            } else {
                ((base + 24 * 60 - 15) % (24 * 60), 2 + used, false)
            });
        }
        _ => {}
    }

    let (hour, mut used) = whole(w, i)?;
    if hour > 24 {
        return None;
    }
    let mut minute = 0;
    let mut has_minute = false;
    if let Some((m, mu)) = parse_minute(w, i + used) {
        minute = m;
        used += mu;
        has_minute = true;
    }
    let mut mer = None;
    let mut explicit = false;
    if let Some((m, mu)) = parse_meridiem(w, i + used) {
        mer = Some(m);
        used += mu;
        explicit = true;
    } else if w.get(i + used) == Some(&"oclock")
        || (w.get(i + used) == Some(&"o") && w.get(i + used + 1) == Some(&"clock"))
    {
        used += if w[i + used] == "oclock" { 1 } else { 2 };
        explicit = true;
    } else if w.get(i + used) == Some(&"in") && w.get(i + used + 1) == Some(&"the") {
        if let Some(part) = w.get(i + used + 2).and_then(|d| match *d {
            "morning" => Some(Mer::Am),
            "afternoon" | "evening" => Some(Mer::Pm),
            _ => None,
        }) {
            mer = Some(part);
            used += 3;
            explicit = true;
        }
    } else if w.get(i + used) == Some(&"at") && w.get(i + used + 1) == Some(&"night") {
        mer = Some(Mer::Pm);
        used += 2;
        explicit = true;
    }
    // A time needs an anchor: "at 9", "9 am", "9 o'clock", "17 00" after "at". A lone number is not a time.
    if !(anchored || explicit) {
        return None;
    }
    if has_minute && !anchored && !explicit {
        return None;
    }
    let h24 = to_24h(hour, mer)?;
    // "8" with no am/pm is open to a daypart word elsewhere in the phrase ("tonight at 8").
    let ambiguous = mer.is_none() && (1..=11).contains(&hour);
    Some((h24 % 24 * 60 + minute, used, ambiguous))
}

fn mark(consumed: &mut [bool], from: usize, to: usize) {
    for c in &mut consumed[from..to] {
        *c = true;
    }
}

/// Consume a directly preceding preposition so it does not linger in the title.
fn eat_before(w: &[&str], consumed: &mut [bool], at: usize, preps: &[&str]) {
    if at > 0 && !consumed[at - 1] && preps.contains(&w[at - 1]) {
        consumed[at - 1] = true;
    }
}

/// Find the date and time phrases in `w`, resolved against `now`.
pub fn extract(w: &[&str], now: &LocalNow) -> Extracted {
    let today = now.date;
    let mut consumed = vec![false; w.len()];
    let mut date: Option<Date> = None;
    let mut time: Option<u32> = None;
    let mut daypart_time: Option<u32> = None;
    let mut time_only_roll = false;
    // The clock time had no am/pm, so a daypart word can still decide it.
    let mut time_ambiguous = false;

    let mut i = 0;
    while i < w.len() {
        if consumed[i] {
            i += 1;
            continue;
        }
        let t = w[i];

        // ---- relative: "in 30 minutes", "in two hours", "in 3 days", "in a week" ----
        if t == "in" {
            // "in half an hour", "in a quarter of an hour"
            let fraction: Option<(i64, usize)> = match &w[i + 1..] {
                ["half", "an", "hour", ..] | ["half", "a", "hour", ..] => Some((30, 3)),
                ["a", "quarter", "of", "an", "hour", ..] => Some((15, 5)),
                ["quarter", "of", "an", "hour", ..] => Some((15, 4)),
                ["three", "quarters", "of", "an", "hour", ..] => Some((45, 5)),
                _ => None,
            };
            if let Some((minutes, words)) = fraction {
                let at = now.plus_minutes(minutes);
                date.get_or_insert(at.date);
                time.get_or_insert(at.minutes);
                mark(&mut consumed, i, i + 1 + words);
                i += 1 + words;
                continue;
            }
            let quantity = if matches!(w.get(i + 1), Some(&"a") | Some(&"an")) {
                Some((1.0, 1))
            } else {
                parse_quantity(w, i + 1)
            };
            if let Some((q, used)) = quantity {
                let unit = w.get(i + 1 + used).copied().unwrap_or("");
                let n = q.round() as i64;
                let (applied, span) = match unit {
                    "minute" | "minutes" | "min" | "mins" if n >= 1 => {
                        let at = now.plus_minutes(n);
                        date.get_or_insert(at.date);
                        time.get_or_insert(at.minutes);
                        (true, 1)
                    }
                    "hour" | "hours" | "hr" | "hrs" if n >= 1 => {
                        // "in an hour and a half", "in two hours and a quarter"
                        let after = i + 2 + used;
                        let (extra, extra_words) =
                            match (w.get(after), w.get(after + 1), w.get(after + 2)) {
                                (Some(&"and"), Some(&"a"), Some(&"half")) => (30, 3),
                                (Some(&"and"), Some(&"a"), Some(&"quarter")) => (15, 3),
                                _ => (0, 0),
                            };
                        let at = now.plus_minutes(n * 60 + extra);
                        date.get_or_insert(at.date);
                        time.get_or_insert(at.minutes);
                        (true, 1 + extra_words)
                    }
                    "day" | "days" if n >= 1 => {
                        date.get_or_insert(today.add_days(n));
                        (true, 1)
                    }
                    "week" | "weeks" if n >= 1 => {
                        date.get_or_insert(today.add_days(7 * n));
                        (true, 1)
                    }
                    "month" | "months" if n >= 1 => {
                        date.get_or_insert(today.add_months(n as i32));
                        (true, 1)
                    }
                    _ => (false, 0),
                };
                if applied {
                    mark(&mut consumed, i, i + 1 + used + span);
                    i += 1 + used + span;
                    continue;
                }
            }
        }

        // ---- today / tonight / tomorrow / the day after tomorrow ----
        if t == "today" {
            date.get_or_insert(today);
            mark(&mut consumed, i, i + 1);
            i += 1;
            continue;
        }
        if t == "tonight" {
            date.get_or_insert(today);
            daypart_time = Some(NIGHT);
            mark(&mut consumed, i, i + 1);
            i += 1;
            continue;
        }
        if t == "tomorrow" {
            date.get_or_insert(today.add_days(1));
            mark(&mut consumed, i, i + 1);
            i += 1;
            continue;
        }
        if t == "day" && w.get(i + 1) == Some(&"after") && w.get(i + 2) == Some(&"tomorrow") {
            date.get_or_insert(today.add_days(2));
            let start = if i > 0 && w[i - 1] == "the" && !consumed[i - 1] {
                i - 1
            } else {
                i
            };
            mark(&mut consumed, start, i + 3);
            i += 3;
            continue;
        }

        // ---- "this morning", "this evening" ----
        if t == "this" {
            if let Some(dp) = w.get(i + 1).and_then(|d| daypart(d)) {
                date.get_or_insert(today);
                daypart_time = Some(dp);
                mark(&mut consumed, i, i + 2);
                i += 2;
                continue;
            }
        }

        // ---- weekdays: "friday", "this friday", "next friday", "on friday" ----
        if let Some(wd) = weekday_index(t) {
            let qualifier = if i > 0 && !consumed[i - 1] {
                Some(w[i - 1])
            } else {
                None
            };
            let today_wd = today.weekday();
            let ahead = i64::from((wd + 7 - today_wd) % 7);
            let days = match qualifier {
                Some("this") => ahead,
                Some("next") => {
                    // The weekday in the FOLLOWING Monday-Sunday week.
                    let to_next_monday = 7 - i64::from(today_wd);
                    to_next_monday + i64::from(wd)
                }
                _ if ahead == 0 => 7,
                _ => ahead,
            };
            date.get_or_insert(today.add_days(days));
            let start = if matches!(qualifier, Some("this") | Some("next") | Some("coming")) {
                i - 1
            } else {
                i
            };
            mark(&mut consumed, start, i + 1);
            eat_before(
                w,
                &mut consumed,
                start,
                &["on", "by", "for", "until", "till", "due"],
            );
            i += 1;
            continue;
        }

        // ---- "next week", "next month" ----
        if t == "next" && matches!(w.get(i + 1), Some(&"week") | Some(&"month")) {
            let d = if w[i + 1] == "week" {
                today.add_days(7)
            } else {
                today.add_months(1)
            };
            date.get_or_insert(d);
            mark(&mut consumed, i, i + 2);
            eat_before(
                w,
                &mut consumed,
                i,
                &["on", "by", "for", "until", "till", "due"],
            );
            i += 2;
            continue;
        }

        // ---- month + day: "march 5th", "november 12 2027" ----
        if let Some(month) = month_index(t) {
            if let Some((day, used)) = parse_day(w, i + 1, false) {
                let year = parse_year(w, i + 1 + used);
                if let Some(d) = month_day(month, day, year, today) {
                    date.get_or_insert(d);
                    let end = i + 1 + used + usize::from(year.is_some());
                    mark(&mut consumed, i, end);
                    eat_before(
                        w,
                        &mut consumed,
                        i,
                        &["on", "by", "for", "until", "till", "due"],
                    );
                    i = end;
                    continue;
                }
            }
        }

        // ---- day + month: "the 5th of march", "5th march" ----
        if let Some((day, used)) = parse_day(w, i, true) {
            let mut j = i + used;
            if w.get(j) == Some(&"of") {
                j += 1;
            }
            if let Some(month) = w.get(j).and_then(|m| month_index(m)) {
                let year = parse_year(w, j + 1);
                if let Some(d) = month_day(month, day, year, today) {
                    date.get_or_insert(d);
                    let end = j + 1 + usize::from(year.is_some());
                    let start = if i > 0 && w[i - 1] == "the" && !consumed[i - 1] {
                        i - 1
                    } else {
                        i
                    };
                    mark(&mut consumed, start, end);
                    eat_before(
                        w,
                        &mut consumed,
                        start,
                        &["on", "by", "for", "until", "till", "due"],
                    );
                    i = end;
                    continue;
                }
            }
            // bare "the 5th": the next time that day of the month comes round
            if i > 0 && (w[i - 1] == "the" || w[i - 1] == "on") {
                if let Some(d) = upcoming_day_of_month(day, today) {
                    date.get_or_insert(d);
                    let mut start = i;
                    if w[i - 1] == "the" && !consumed[i - 1] {
                        start = i - 1;
                    }
                    mark(&mut consumed, start, i + used);
                    eat_before(
                        w,
                        &mut consumed,
                        start,
                        &["on", "by", "for", "until", "till", "due"],
                    );
                    i += used;
                    continue;
                }
            }
        }

        // ---- clock times ----
        let anchored = i > 0
            && !consumed[i - 1]
            && matches!(
                w[i - 1],
                "at" | "around" | "by" | "until" | "till" | "from" | "before" | "after"
            );
        let starts_time = matches!(t, "noon" | "midday" | "midnight" | "half" | "quarter")
            || t.starts_with(|c: char| c.is_ascii_digit())
            || whole(w, i).is_some();
        if starts_time && time.is_none() {
            if let Some((minutes, used, ambiguous)) = parse_time(w, i, anchored) {
                time = Some(minutes);
                time_ambiguous = ambiguous;
                mark(&mut consumed, i, i + used);
                eat_before(
                    w,
                    &mut consumed,
                    i,
                    &["at", "around", "by", "until", "till"],
                );
                time_only_roll = true;
                i += used;
                continue;
            }
        }

        // ---- a lone daypart after a date: "tomorrow morning", "friday afternoon" ----
        if let Some(dp) = daypart(t) {
            let after_date = i > 0 && consumed[i - 1];
            if after_date || matches!(t, "tonight") {
                daypart_time = Some(dp);
                mark(&mut consumed, i, i + 1);
                i += 1;
                continue;
            }
        }
        i += 1;
    }

    // "tonight at 8" / "this evening at 7" / "friday afternoon at 3": with no am/pm, the daypart
    // word picks the half of the day for the spoken hour ("tomorrow morning at 3" is 3 AM).
    if let (Some(t), Some(dp), true) = (time, daypart_time, time_ambiguous) {
        let h12 = t / 60 % 12;
        let hour = if dp >= 12 * 60 { h12 + 12 } else { h12 };
        time = Some(hour % 24 * 60 + t % 60);
    }
    // An explicit clock time beats a daypart's default; a daypart alone supplies one.
    let time = time.or(daypart_time);

    // "remind me at 9" with no date: today if that is still ahead, otherwise tomorrow.
    let date = match (date, time) {
        (None, Some(t)) if time_only_roll => Some(if t > now.minutes {
            now.date
        } else {
            now.date.add_days(1)
        }),
        (d, _) => d,
    };
    Extracted {
        date,
        time,
        consumed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sunday 2026-11-01, 10:30.
    fn now() -> LocalNow {
        LocalNow::new(Date::new(2026, 11, 1).unwrap(), 10, 30)
    }

    /// Runs the real tokenizer first, so "5th" arrives as "5","th" and "o'clock" as "oclock".
    fn run_at(s: &str, at: &LocalNow) -> (Option<String>, Option<String>) {
        let words = crate::parser::norm_words(s);
        let refs: Vec<&str> = words.iter().map(String::as_str).collect();
        let e = extract(&refs, at);
        (e.date.map(Date::iso), e.time.map(crate::civil::hhmm))
    }

    fn run(s: &str) -> (Option<String>, Option<String>) {
        run_at(s, &now())
    }

    fn d(s: &str) -> Option<String> {
        Some(s.to_string())
    }

    #[test]
    fn relative_days() {
        assert_eq!(run("today"), (d("2026-11-01"), None));
        assert_eq!(run("tomorrow"), (d("2026-11-02"), None));
        assert_eq!(run("the day after tomorrow"), (d("2026-11-03"), None));
        assert_eq!(run("in 3 days"), (d("2026-11-04"), None));
        assert_eq!(run("in two weeks"), (d("2026-11-15"), None));
        assert_eq!(run("in a week"), (d("2026-11-08"), None));
        assert_eq!(run("in 2 months"), (d("2027-01-01"), None));
        assert_eq!(run("next week"), (d("2026-11-08"), None));
        assert_eq!(run("next month"), (d("2026-12-01"), None));
    }

    #[test]
    fn weekdays_resolve_forward() {
        // today is Sunday
        assert_eq!(run("monday"), (d("2026-11-02"), None));
        assert_eq!(run("friday"), (d("2026-11-06"), None));
        assert_eq!(run("on friday"), (d("2026-11-06"), None));
        assert_eq!(
            run("sunday"),
            (d("2026-11-08"), None),
            "today's weekday means next week's"
        );
        assert_eq!(run("this sunday"), (d("2026-11-01"), None));
        assert_eq!(run("this friday"), (d("2026-11-06"), None));
        assert_eq!(
            run("next monday"),
            (d("2026-11-02"), None),
            "Monday after this week"
        );
        assert_eq!(run("next friday"), (d("2026-11-06"), None));
    }

    #[test]
    fn next_weekday_means_the_following_week_midweek() {
        // Wednesday 2026-11-04
        let wed = LocalNow::new(Date::new(2026, 11, 4).unwrap(), 8, 0);
        let go = |s: &str| run_at(s, &wed).0;
        assert_eq!(go("friday"), d("2026-11-06"));
        assert_eq!(go("this friday"), d("2026-11-06"));
        assert_eq!(go("next friday"), d("2026-11-13"));
        assert_eq!(go("next monday"), d("2026-11-09"));
        assert_eq!(go("wednesday"), d("2026-11-11"));
    }

    #[test]
    fn clock_times() {
        assert_eq!(
            run("tomorrow at 9"),
            (d("2026-11-02"), Some("09:00".into()))
        );
        assert_eq!(
            run("tomorrow at nine"),
            (d("2026-11-02"), Some("09:00".into()))
        );
        assert_eq!(
            run("tomorrow at 9 am"),
            (d("2026-11-02"), Some("09:00".into()))
        );
        assert_eq!(
            run("tomorrow at 9 30 pm"),
            (d("2026-11-02"), Some("21:30".into()))
        );
        assert_eq!(
            run("tomorrow at nine thirty"),
            (d("2026-11-02"), Some("09:30".into()))
        );
        assert_eq!(
            run("today at 5"),
            (d("2026-11-01"), Some("17:00".into())),
            "1-6 default to the afternoon"
        );
        assert_eq!(
            run("today at 7"),
            (d("2026-11-01"), Some("07:00".into())),
            "7-11 default to the morning"
        );
        assert_eq!(
            run("friday at 17 00"),
            (d("2026-11-06"), Some("17:00".into()))
        );
        assert_eq!(
            run("friday at noon"),
            (d("2026-11-06"), Some("12:00".into()))
        );
        assert_eq!(
            run("friday at midnight"),
            (d("2026-11-06"), Some("00:00".into()))
        );
        assert_eq!(
            run("friday at half past three"),
            (d("2026-11-06"), Some("15:30".into()))
        );
        assert_eq!(
            run("friday at quarter past 3"),
            (d("2026-11-06"), Some("15:15".into()))
        );
        assert_eq!(
            run("friday at quarter to 5"),
            (d("2026-11-06"), Some("16:45".into()))
        );
        assert_eq!(
            run("friday at 3 o'clock"),
            (d("2026-11-06"), Some("15:00".into()))
        );
        assert_eq!(
            run("friday at 8 in the evening"),
            (d("2026-11-06"), Some("20:00".into()))
        );
        assert_eq!(run("friday 5 pm"), (d("2026-11-06"), Some("17:00".into())));
        assert_eq!(
            run("friday at 12 a m"),
            (d("2026-11-06"), Some("00:00".into()))
        );
    }

    #[test]
    fn dayparts() {
        assert_eq!(
            run("tomorrow morning"),
            (d("2026-11-02"), Some("09:00".into()))
        );
        assert_eq!(
            run("tomorrow afternoon"),
            (d("2026-11-02"), Some("15:00".into()))
        );
        assert_eq!(
            run("tomorrow evening"),
            (d("2026-11-02"), Some("18:00".into()))
        );
        assert_eq!(
            run("tomorrow night"),
            (d("2026-11-02"), Some("20:00".into()))
        );
        assert_eq!(run("tonight"), (d("2026-11-01"), Some("20:00".into())));
        assert_eq!(run("this evening"), (d("2026-11-01"), Some("18:00".into())));
        assert_eq!(
            run("friday afternoon"),
            (d("2026-11-06"), Some("15:00".into()))
        );
        assert_eq!(
            run("tomorrow morning at 8"),
            (d("2026-11-02"), Some("08:00".into())),
            "explicit hour beats the daypart default"
        );
    }

    #[test]
    fn relative_clock() {
        assert_eq!(
            run("in 30 minutes"),
            (d("2026-11-01"), Some("11:00".into()))
        );
        assert_eq!(run("in an hour"), (d("2026-11-01"), Some("11:30".into())));
        assert_eq!(run("in two hours"), (d("2026-11-01"), Some("12:30".into())));
        let late = LocalNow::new(Date::new(2026, 11, 1).unwrap(), 23, 40);
        let e = extract(&["in", "30", "minutes"], &late);
        assert_eq!(
            (e.date.unwrap().iso(), e.time),
            ("2026-11-02".to_string(), Some(10))
        );
    }

    /// A relative reminder must never ring EARLY: with seconds on the clock, round up to the next
    /// whole minute. (Real bug: "in two minutes" at 18:31:57 became 18:33, 57 s early.)
    #[test]
    fn relative_offsets_round_up_when_the_clock_has_seconds() {
        let day = Date::new(2026, 11, 1).unwrap();
        let at = |h, m, s| LocalNow::new(day, h, m).with_secs(s);
        let t = |s: &str, now: &LocalNow| run_at(s, now);
        let today = d("2026-11-01");
        // 18:31:57 + 2 min -> 18:34; on the minute -> 18:33
        assert_eq!(
            t("in two minutes", &at(18, 31, 57)),
            (today.clone(), Some("18:34".into()))
        );
        assert_eq!(
            t("in 2 minutes", &at(18, 31, 57)),
            (today.clone(), Some("18:34".into()))
        );
        assert_eq!(
            t("in two minutes", &at(18, 31, 0)),
            (today.clone(), Some("18:33".into()))
        );
        assert_eq!(
            t("in two minutes", &at(18, 31, 1)),
            (today.clone(), Some("18:34".into()))
        );
        // hours and the fractions
        assert_eq!(
            t("in an hour", &at(18, 31, 30)),
            (today.clone(), Some("19:32".into()))
        );
        assert_eq!(
            t("in an hour", &at(18, 31, 0)),
            (today.clone(), Some("19:31".into()))
        );
        assert_eq!(
            t("in half an hour", &at(18, 31, 30)),
            (today.clone(), Some("19:02".into()))
        );
        assert_eq!(
            t("in half an hour", &at(18, 31, 0)),
            (today.clone(), Some("19:01".into()))
        );
        assert_eq!(
            t("in a quarter of an hour", &at(18, 31, 5)),
            (today.clone(), Some("18:47".into()))
        );
        assert_eq!(
            t("in an hour and a half", &at(18, 31, 5)),
            (today.clone(), Some("20:02".into()))
        );
        assert_eq!(
            t("in 90 minutes", &at(18, 31, 5)),
            (today.clone(), Some("20:02".into()))
        );
        // date rollover: 23:59:30 + 1 min -> 00:01 tomorrow (00:00 would be 30 s early)
        assert_eq!(
            t("in a minute", &at(23, 59, 30)),
            (d("2026-11-02"), Some("00:01".into()))
        );
        assert_eq!(
            t("in 1 minute", &at(23, 59, 0)),
            (d("2026-11-02"), Some("00:00".into()))
        );
        // absolute clock times are unaffected by the seconds
        assert_eq!(
            t("at 5 pm", &at(16, 59, 59)),
            (today.clone(), Some("17:00".into()))
        );
        assert_eq!(
            t("at 18:32", &at(18, 31, 57)),
            (today, Some("18:32".into()))
        );
        // day offsets have no time of day, so there is nothing to round
        assert_eq!(t("in 2 days", &at(18, 31, 57)), (d("2026-11-03"), None));
    }

    #[test]
    fn calendar_dates() {
        assert_eq!(run("on the 5th"), (d("2026-11-05"), None));
        let late = LocalNow::new(Date::new(2026, 11, 20).unwrap(), 9, 0);
        assert_eq!(
            run_at("on the 5th", &late).0,
            d("2026-12-05"),
            "the 5th has passed this month"
        );
        assert_eq!(
            run_at("on the 20th", &late).0,
            d("2026-11-20"),
            "today counts"
        );
        assert_eq!(run("on the 20th"), (d("2026-11-20"), None));
        assert_eq!(run("on the first"), (d("2026-11-01"), None));
        assert_eq!(run("on march 5th"), (d("2027-03-05"), None));
        assert_eq!(run("november 12"), (d("2026-11-12"), None));
        assert_eq!(run("december twenty fifth"), (d("2026-12-25"), None));
        assert_eq!(run("the 5th of march"), (d("2027-03-05"), None));
        assert_eq!(run("5th march 2028"), (d("2028-03-05"), None));
        assert_eq!(run("on the twenty first"), (d("2026-11-21"), None));
    }

    #[test]
    fn time_only_rolls_to_tomorrow_when_passed() {
        assert_eq!(
            run("at 9 am"),
            (d("2026-11-02"), Some("09:00".into())),
            "09:00 already passed at 10:30"
        );
        assert_eq!(run("at 5 pm"), (d("2026-11-01"), Some("17:00".into())));
    }

    #[test]
    fn plain_numbers_and_words_are_not_dates() {
        assert_eq!(run("buy 9 apples"), (None, None));
        assert_eq!(run("call mom"), (None, None));
        assert_eq!(
            run("may i have a word"),
            (None, None),
            "'may' alone is not a month"
        );
        assert_eq!(run("the march of progress"), (None, None));
        assert_eq!(run("we sat down on sun"), (None, None));
    }

    #[test]
    fn consumed_marks_prepositions_with_their_phrase() {
        let w = ["call", "mom", "on", "friday", "at", "9", "am"];
        let e = extract(&w, &now());
        assert_eq!(e.time, Some(540));
        assert_eq!(e.consumed, vec![false, false, true, true, true, true, true]);
    }
}
