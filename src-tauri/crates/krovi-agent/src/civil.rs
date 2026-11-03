//! Tiny proleptic-Gregorian calendar maths and the local wall clock derived from a UTC instant
//! plus a fixed UTC offset. No clock access: callers pass `now` in, which keeps every
//! reminder-resolution test deterministic.

/// A calendar date.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Date {
    pub y: i32,
    pub m: u32,
    pub d: u32,
}

pub const WEEKDAYS: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];
pub const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

pub fn is_leap(y: i32) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

pub fn days_in_month(y: i32, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(y) => 29,
        2 => 28,
        _ => 0,
    }
}

impl Date {
    /// `None` unless this is a real date.
    pub fn new(y: i32, m: u32, d: u32) -> Option<Date> {
        ((1..=12).contains(&m) && d >= 1 && d <= days_in_month(y, m)).then_some(Date { y, m, d })
    }

    /// Parse `yyyy-mm-dd` (exactly that shape).
    pub fn parse_iso(s: &str) -> Option<Date> {
        let b = s.as_bytes();
        if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
            return None;
        }
        let digits = |r: std::ops::Range<usize>| -> Option<u32> {
            let part = &s[r];
            if part.bytes().all(|c| c.is_ascii_digit()) {
                part.parse().ok()
            } else {
                None
            }
        };
        Date::new(digits(0..4)? as i32, digits(5..7)?, digits(8..10)?)
    }

    /// Days since 1970-01-01.
    pub fn days(self) -> i64 {
        let y = i64::from(if self.m <= 2 { self.y - 1 } else { self.y });
        let era = y.div_euclid(400);
        let yoe = y.rem_euclid(400);
        let mp = i64::from((self.m + 9) % 12);
        let doy = (153 * mp + 2) / 5 + i64::from(self.d) - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }

    pub fn from_days(z: i64) -> Date {
        let z = z + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
        let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
        let y = (yoe + era * 400) as i32 + i32::from(m <= 2);
        Date { y, m, d }
    }

    pub fn add_days(self, n: i64) -> Date {
        Date::from_days(self.days() + n)
    }

    /// Monday = 0 .. Sunday = 6.
    pub fn weekday(self) -> u32 {
        (self.days() + 3).rem_euclid(7) as u32
    }

    /// `yyyy-mm-dd`.
    pub fn iso(self) -> String {
        format!("{:04}-{:02}-{:02}", self.y, self.m, self.d)
    }

    /// "Fri 7 Nov".
    pub fn short_label(self) -> String {
        format!(
            "{} {} {}",
            &WEEKDAYS[self.weekday() as usize][..3],
            self.d,
            &MONTHS[self.m as usize - 1][..3]
        )
    }
}

/// The local wall clock at one instant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalClock {
    pub date: Date,
    /// Minutes since local midnight, 0..1440.
    pub minutes: u32,
    /// Seconds into the current minute, 0..60.
    pub secs: u32,
}

impl LocalClock {
    /// The wall clock at `utc_ms` in a zone `offset_minutes` east of UTC (IST = 330).
    pub fn from_utc_ms(utc_ms: i64, offset_minutes: i32) -> LocalClock {
        let local = utc_ms.div_euclid(1000) + i64::from(offset_minutes) * 60;
        LocalClock::from_local_secs(local)
    }

    /// The wall clock for seconds since the local epoch (1970-01-01 00:00 local).
    pub fn from_local_secs(local: i64) -> LocalClock {
        LocalClock {
            date: Date::from_days(local.div_euclid(86_400)),
            minutes: (local.rem_euclid(86_400) / 60) as u32,
            secs: local.rem_euclid(60) as u32,
        }
    }

    /// Seconds since the local epoch.
    pub fn local_secs(self) -> i64 {
        self.date.days() * 86_400 + i64::from(self.minutes) * 60 + i64::from(self.secs)
    }
}

/// UTC milliseconds of a local date and minutes-since-midnight in a zone `offset_minutes` east
/// of UTC. A fixed offset: the host's reminder scheduler re-derives DST from date + time.
pub fn local_to_utc_ms(date: Date, minutes: u32, offset_minutes: i32) -> i64 {
    (date.days() * 86_400 + i64::from(minutes) * 60 - i64::from(offset_minutes) * 60) * 1000
}

/// `HH:MM` for minutes since midnight.
pub fn hhmm(minutes: u32) -> String {
    format!("{:02}:{:02}", (minutes / 60) % 24, minutes % 60)
}

/// "5:00 PM".
pub fn clock_label(minutes: u32) -> String {
    let (h, m) = ((minutes / 60) % 24, minutes % 60);
    let h12 = if h % 12 == 0 { 12 } else { h % 12 };
    format!("{h12}:{m:02} {}", if h < 12 { "AM" } else { "PM" })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> Date {
        Date::new(y, m, day).unwrap()
    }

    #[test]
    fn known_dates_and_weekdays() {
        assert_eq!(d(1970, 1, 1).days(), 0);
        assert_eq!(d(1970, 1, 1).weekday(), 3, "1970-01-01 was a Thursday");
        assert_eq!(d(2026, 11, 2).weekday(), 0, "2026-11-02 is a Monday");
        assert_eq!(d(2026, 11, 1).weekday(), 6, "2026-11-01 is a Sunday");
        assert_eq!(d(2000, 2, 29).days(), 11_016);
    }

    #[test]
    fn days_roundtrip() {
        for n in (-800_000..800_000).step_by(997) {
            assert_eq!(Date::from_days(n).days(), n);
        }
        assert_eq!(d(2028, 2, 28).add_days(1), d(2028, 2, 29));
        assert_eq!(d(2027, 2, 28).add_days(1), d(2027, 3, 1));
        assert_eq!(d(2026, 12, 31).add_days(1), d(2027, 1, 1));
        assert_eq!(d(2026, 1, 1).add_days(-1), d(2025, 12, 31));
    }

    #[test]
    fn rejects_unreal_dates() {
        assert!(Date::new(2026, 2, 29).is_none());
        assert!(Date::new(2026, 13, 1).is_none());
        assert!(Date::new(2026, 4, 31).is_none());
        assert!(Date::new(2028, 2, 29).is_some());
    }

    #[test]
    fn iso_roundtrip_and_rejects() {
        assert_eq!(Date::parse_iso("2026-11-07"), Some(d(2026, 11, 7)));
        assert_eq!(d(2026, 11, 7).iso(), "2026-11-07");
        for bad in [
            "2026-2-7",
            "2026-02-30",
            "26-11-07",
            "2026/11/07",
            "",
            "2026-11-0x",
        ] {
            assert!(Date::parse_iso(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn local_clock_applies_the_offset() {
        // 2026-11-02 12:00:30 UTC is 17:30:30 in IST (+330)
        let utc = d(2026, 11, 2).days() * 86_400_000 + (12 * 3600 + 30) * 1000;
        let c = LocalClock::from_utc_ms(utc, 330);
        assert_eq!(c.date, d(2026, 11, 2));
        assert_eq!((c.minutes, c.secs), (17 * 60 + 30, 30));
        // a negative offset can move the date back: 01:00:30 UTC is 20:00:30 the day before
        let c = LocalClock::from_utc_ms(utc - 11 * 3_600_000, -300);
        assert_eq!(c.date, d(2026, 11, 1));
        assert_eq!((c.minutes, c.secs), (20 * 60, 30));
    }

    #[test]
    fn local_to_utc_inverts_from_utc() {
        for off in [-480, -300, 0, 60, 330, 545, 840] {
            let ms = local_to_utc_ms(d(2026, 11, 7), 17 * 60, off);
            let back = LocalClock::from_utc_ms(ms, off);
            assert_eq!(
                (back.date, back.minutes, back.secs),
                (d(2026, 11, 7), 17 * 60, 0)
            );
        }
    }

    #[test]
    fn labels() {
        assert_eq!(d(2026, 11, 7).short_label(), "Sat 7 Nov");
        assert_eq!(hhmm(9 * 60 + 5), "09:05");
        assert_eq!(clock_label(0), "12:00 AM");
        assert_eq!(clock_label(12 * 60), "12:00 PM");
        assert_eq!(clock_label(17 * 60 + 30), "5:30 PM");
    }
}
