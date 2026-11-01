//! Tiny proleptic-Gregorian calendar maths and the local clock, so spoken dates ("friday",
//! "the 5th", "in two weeks") can be resolved without any dependency and tested with a fixed "now".

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

    /// Days since 1970-01-01 (Howard Hinnant's `days_from_civil`).
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

    /// Same day next month, clamped to the end of a shorter month.
    pub fn add_months(self, n: i32) -> Date {
        let total = self.y * 12 + (self.m as i32 - 1) + n;
        let (y, m) = (total.div_euclid(12), (total.rem_euclid(12) + 1) as u32);
        Date {
            y,
            m,
            d: self.d.min(days_in_month(y, m)),
        }
    }

    /// `yyyy-mm-dd`.
    pub fn iso(self) -> String {
        format!("{:04}-{:02}-{:02}", self.y, self.m, self.d)
    }

    /// "Fri 7 Nov" (for feedback lines).
    pub fn short_label(self) -> String {
        format!(
            "{} {} {}",
            &WEEKDAYS[self.weekday() as usize][..3],
            self.d,
            &MONTHS[self.m as usize - 1][..3]
        )
    }
}

/// The local wall clock: today's date, minutes since local midnight, and the seconds into that
/// minute (kept so a relative reminder can be rounded UP and never rings early).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalNow {
    pub date: Date,
    pub minutes: u32,
    /// Seconds into the current minute, 0..=59. `0` when the time is exactly on a minute (and in
    /// every fixed `LocalNow::new` used by tests).
    pub secs: u32,
}

impl LocalNow {
    /// A moment exactly on a minute boundary.
    pub fn new(date: Date, hour: u32, minute: u32) -> LocalNow {
        LocalNow {
            date,
            minutes: hour * 60 + minute,
            secs: 0,
        }
    }

    /// The same moment with `secs` seconds into the minute.
    pub fn with_secs(mut self, secs: u32) -> LocalNow {
        self.secs = secs.min(59);
        self
    }

    /// This moment plus `delta` minutes, for a RELATIVE offset ("in two minutes"): the result is
    /// rounded UP to a whole minute when the current time is not exactly on one, because reminders
    /// are minute-granular and must never fire before the requested duration has passed.
    /// 18:31:57 + 2 min = 18:34 (not 18:33, which would be 57 s early); 18:31:00 + 2 min = 18:33.
    /// The date rolls over midnight.
    pub fn plus_minutes(self, delta: i64) -> LocalNow {
        let round_up = i64::from(self.secs > 0);
        let total = i64::from(self.minutes) + delta + round_up;
        LocalNow {
            date: self.date.add_days(total.div_euclid(1440)),
            minutes: total.rem_euclid(1440) as u32,
            secs: 0,
        }
    }

    /// The real local time. On Windows (with the `native` feature) this is the system local time;
    /// elsewhere it falls back to UTC, which only matters for tests on other platforms.
    pub fn system() -> LocalNow {
        #[cfg(all(windows, feature = "native"))]
        {
            use windows::Win32::System::SystemInformation::GetLocalTime;
            // SAFETY: GetLocalTime has no preconditions and returns a plain struct.
            let t = unsafe { GetLocalTime() };
            if let Some(date) =
                Date::new(i32::from(t.wYear), u32::from(t.wMonth), u32::from(t.wDay))
            {
                return LocalNow::new(date, u32::from(t.wHour), u32::from(t.wMinute))
                    .with_secs(u32::from(t.wSecond));
            }
        }
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        LocalNow {
            date: Date::from_days(secs.div_euclid(86_400)),
            minutes: (secs.rem_euclid(86_400) / 60) as u32,
            secs: (secs.rem_euclid(60)) as u32,
        }
    }
}

/// The local wall-clock time of a Unix timestamp. On Windows (with `native`) this applies the
/// machine's time zone and daylight-saving rules; elsewhere it is UTC.
pub fn local_from_unix(secs: i64) -> LocalNow {
    #[cfg(all(windows, feature = "native"))]
    {
        use windows::Win32::Foundation::{FILETIME, SYSTEMTIME};
        use windows::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};
        // Unix epoch is 11_644_473_600 s after the FILETIME epoch (1601-01-01), in 100 ns ticks.
        let ticks = (secs.saturating_add(11_644_473_600)).max(0) as u64 * 10_000_000;
        let ft = FILETIME {
            dwLowDateTime: (ticks & 0xFFFF_FFFF) as u32,
            dwHighDateTime: (ticks >> 32) as u32,
        };
        let mut utc = SYSTEMTIME::default();
        let mut local = SYSTEMTIME::default();
        // SAFETY: plain structs on the stack; both calls only read/write them.
        let ok = unsafe {
            FileTimeToSystemTime(&ft, &mut utc).is_ok()
                && SystemTimeToTzSpecificLocalTime(None, &utc, &mut local).is_ok()
        };
        if ok {
            if let Some(date) = Date::new(
                i32::from(local.wYear),
                u32::from(local.wMonth),
                u32::from(local.wDay),
            ) {
                return LocalNow::new(date, u32::from(local.wHour), u32::from(local.wMinute))
                    .with_secs(u32::from(local.wSecond));
            }
        }
    }
    LocalNow {
        date: Date::from_days(secs.div_euclid(86_400)),
        minutes: (secs.rem_euclid(86_400) / 60) as u32,
        secs: (secs.rem_euclid(60)) as u32,
    }
}

/// "Sun 1 Nov 10:30 AM" for a file timestamp, in local time.
pub fn describe_system_time(t: std::time::SystemTime) -> Option<String> {
    let secs = match t.duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => d.as_secs() as i64,
        Err(e) => -(e.duration().as_secs() as i64),
    };
    let local = local_from_unix(secs);
    Some(format!(
        "{} {}",
        local.date.short_label(),
        clock_label(local.minutes)
    ))
}

/// `HH:MM` for a minutes-since-midnight value.
pub fn hhmm(minutes: u32) -> String {
    format!("{:02}:{:02}", (minutes / 60) % 24, minutes % 60)
}

/// "9:00 AM" style label.
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
    fn epoch_and_known_dates() {
        assert_eq!(d(1970, 1, 1).days(), 0);
        assert_eq!(d(1970, 1, 1).weekday(), 3, "1970-01-01 was a Thursday");
        assert_eq!(d(2026, 11, 1).weekday(), 6, "2026-11-01 is a Sunday");
        assert_eq!(d(2000, 2, 29).days(), 11_016);
        assert_eq!(d(2024, 3, 1).weekday(), 4, "2024-03-01 was a Friday");
    }

    #[test]
    fn days_roundtrip_over_a_wide_range() {
        for n in (-800_000..800_000).step_by(997) {
            assert_eq!(Date::from_days(n).days(), n);
        }
        assert_eq!(Date::from_days(d(2028, 2, 28).days() + 1), d(2028, 2, 29));
        assert_eq!(Date::from_days(d(2027, 2, 28).days() + 1), d(2027, 3, 1));
    }

    #[test]
    fn arithmetic_crosses_boundaries() {
        assert_eq!(d(2026, 12, 31).add_days(1), d(2027, 1, 1));
        assert_eq!(d(2026, 1, 1).add_days(-1), d(2025, 12, 31));
        assert_eq!(
            d(2026, 1, 31).add_months(1),
            d(2026, 2, 28),
            "clamps to month end"
        );
        assert_eq!(d(2026, 11, 15).add_months(2), d(2027, 1, 15));
        assert_eq!(d(2026, 3, 15).add_months(-3), d(2025, 12, 15));
    }

    #[test]
    fn rejects_unreal_dates() {
        assert!(Date::new(2026, 2, 29).is_none());
        assert!(Date::new(2026, 13, 1).is_none());
        assert!(Date::new(2026, 4, 31).is_none());
        assert!(Date::new(2028, 2, 29).is_some());
    }

    #[test]
    fn formatting() {
        assert_eq!(d(2026, 11, 7).iso(), "2026-11-07");
        assert_eq!(d(2026, 11, 7).short_label(), "Sat 7 Nov");
        assert_eq!(hhmm(9 * 60 + 5), "09:05");
        assert_eq!(clock_label(0), "12:00 AM");
        assert_eq!(clock_label(12 * 60), "12:00 PM");
        assert_eq!(clock_label(17 * 60 + 30), "5:30 PM");
    }

    #[test]
    fn plus_minutes_rounds_up_so_a_relative_time_is_never_early() {
        let day = d(2026, 11, 1);
        let hm = |n: LocalNow| (n.date, n.minutes / 60, n.minutes % 60, n.secs);
        // 18:31:57 + 2 min = 18:34 (18:33 would be 57 s early)
        let n = LocalNow::new(day, 18, 31).with_secs(57);
        assert_eq!(hm(n.plus_minutes(2)), (day, 18, 34, 0));
        // exactly on the minute: no rounding
        let n = LocalNow::new(day, 18, 31);
        assert_eq!(hm(n.plus_minutes(2)), (day, 18, 33, 0));
        // one second past the minute is enough to round up
        let n = LocalNow::new(day, 18, 31).with_secs(1);
        assert_eq!(hm(n.plus_minutes(2)), (day, 18, 34, 0));
        // zero offset from a non-minute moment is the next whole minute, never the past
        let n = LocalNow::new(day, 18, 31).with_secs(30);
        assert_eq!(hm(n.plus_minutes(0)), (day, 18, 32, 0));
        // across midnight: 23:59:30 + 1 min = 00:01 the next day (00:00 would be 30 s early)
        let n = LocalNow::new(day, 23, 59).with_secs(30);
        assert_eq!(hm(n.plus_minutes(1)), (d(2026, 11, 2), 0, 1, 0));
        // seconds are clamped to a valid value
        assert_eq!(LocalNow::new(day, 1, 1).with_secs(99).secs, 59);
    }

    #[test]
    fn plus_minutes_rolls_the_date() {
        let n = LocalNow::new(d(2026, 11, 1), 23, 50);
        let later = n.plus_minutes(30);
        assert_eq!((later.date, later.minutes), (d(2026, 11, 2), 20));
    }

    #[test]
    fn system_clock_is_plausible() {
        let n = LocalNow::system();
        assert!(n.date.y >= 2025 && n.minutes < 1440);
    }
}
