//! Days on the user's clock, how a series repeats, and turning a day and time into a moment
//! (Unix milliseconds) with the computer's own time zone rules.

use serde::{Deserialize, Serialize};
use std::fmt;
use windows::Win32::Foundation::SYSTEMTIME;
use windows::Win32::System::Time::{
    SystemTimeToTzSpecificLocalTime, TzSpecificLocalTimeToSystemTime,
};

const DAY_MS: i64 = 24 * 60 * 60 * 1000;

/// A day on the calendar.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Day {
    pub year: i32,
    pub month: u32,
    pub day: u32,
}

pub fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ if (year.rem_euclid(4) == 0 && year.rem_euclid(100) != 0)
            || year.rem_euclid(400) == 0 =>
        {
            29
        }
        _ => 28,
    }
}

impl Day {
    /// Reads "YYYY-MM-DD"; `None` unless it is a real day from 1970 to 9999.
    pub fn parse(s: &str) -> Option<Day> {
        let b = s.as_bytes();
        if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
            return None;
        }
        let num = |r: std::ops::Range<usize>| -> Option<u32> {
            b[r.clone()].iter().all(u8::is_ascii_digit).then(|| s[r].parse().ok()).flatten()
        };
        let (year, month, day) = (num(0..4)? as i32, num(5..7)?, num(8..10)?);
        let ok = (1970..=9999).contains(&year)
            && (1..=12).contains(&month)
            && day >= 1
            && day <= days_in_month(year, month);
        ok.then_some(Day { year, month, day })
    }

    /// Days since 1 January 1970 (Howard Hinnant's `days_from_civil`).
    pub fn number(self) -> i64 {
        let y = i64::from(self.year) - i64::from(self.month <= 2);
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let m = i64::from(self.month);
        let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + i64::from(self.day) - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }

    /// The day `n` days after 1 January 1970 (`civil_from_days`).
    pub fn from_number(n: i64) -> Day {
        let z = n + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
        let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
        let year = (yoe + era * 400 + i64::from(month <= 2)) as i32;
        Day { year, month, day }
    }

    pub fn add_days(self, n: i64) -> Day {
        Day::from_number(self.number() + n)
    }

    /// 0 is Monday, 6 is Sunday.
    pub fn weekday(self) -> u32 {
        // 1 January 1970 was a Thursday.
        (self.number() + 3).rem_euclid(7) as u32
    }

    fn is_weekend(self) -> bool {
        self.weekday() >= 5
    }

    /// The same day `n` months later; a day the month does not have becomes its last day
    /// (31 January, one month on, is 28 or 29 February).
    pub fn add_months(self, n: i64) -> Day {
        let index = i64::from(self.year) * 12 + i64::from(self.month) - 1 + n;
        let (year, month) = (index.div_euclid(12) as i32, index.rem_euclid(12) as u32 + 1);
        Day { year, month, day: self.day.min(days_in_month(year, month)) }
    }

    fn months_until(self, later: Day) -> i64 {
        (i64::from(later.year) - i64::from(self.year)) * 12 + i64::from(later.month)
            - i64::from(self.month)
    }
}

impl fmt::Display for Day {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

/// How an event or reminder comes back.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Repeat {
    Daily,
    /// Monday to Friday.
    Weekdays,
    Weekly,
    Monthly,
    Yearly,
}

impl Repeat {
    pub fn as_str(self) -> &'static str {
        match self {
            Repeat::Daily => "daily",
            Repeat::Weekdays => "weekdays",
            Repeat::Weekly => "weekly",
            Repeat::Monthly => "monthly",
            Repeat::Yearly => "yearly",
        }
    }

    pub fn parse(s: &str) -> Option<Repeat> {
        match s {
            "daily" => Some(Repeat::Daily),
            "weekdays" => Some(Repeat::Weekdays),
            "weekly" => Some(Repeat::Weekly),
            "monthly" => Some(Repeat::Monthly),
            "yearly" => Some(Repeat::Yearly),
            _ => None,
        }
    }
}

/// The first day a series starting on `start` falls on: `start` itself, except that a
/// weekday series begun at the weekend starts on the Monday.
pub fn first_day(start: Day, repeat: Option<Repeat>) -> Day {
    let mut d = start;
    if repeat == Some(Repeat::Weekdays) {
        while d.is_weekend() {
            d = d.add_days(1);
        }
    }
    d
}

/// The first day on or after `day` that the series starting on `start` falls on. A series
/// that does not repeat falls only on `start`.
pub fn next_on_or_after(start: Day, repeat: Option<Repeat>, day: Day) -> Option<Day> {
    let start = first_day(start, repeat);
    if day <= start {
        return Some(start);
    }
    let gap = day.number() - start.number();
    Some(match repeat? {
        Repeat::Daily => day,
        Repeat::Weekly => start.add_days((gap + 6) / 7 * 7),
        Repeat::Weekdays => first_day(day, Some(Repeat::Weekdays)),
        Repeat::Monthly | Repeat::Yearly => {
            let step = if repeat == Some(Repeat::Monthly) { 1 } else { 12 };
            // Counted from `start` each time, so a 31st stays the 31st where a month has one.
            let n = start.months_until(day) / step;
            let candidate = start.add_months(n * step);
            if candidate >= day {
                candidate
            } else {
                start.add_months((n + 1) * step)
            }
        }
    })
}

/// Every day from `from` to `to` (both included) that the series falls on, at most `limit`.
pub fn days_between(
    start: Day,
    repeat: Option<Repeat>,
    from: Day,
    to: Day,
    limit: usize,
) -> Vec<Day> {
    let mut days = Vec::new();
    let mut next = next_on_or_after(start, repeat, from);
    while let Some(d) = next.filter(|d| *d <= to && days.len() < limit) {
        days.push(d);
        next = if repeat.is_some() { next_on_or_after(start, repeat, d.add_days(1)) } else { None };
    }
    days
}

/// Minutes into the day of "HH:MM".
pub fn minutes(time: &str) -> Option<u32> {
    let (h, m) = time.split_once(':')?;
    let (h, m): (u32, u32) = (h.parse().ok()?, m.parse().ok()?);
    (h < 24 && m < 60).then_some(h * 60 + m)
}

fn systemtime(day: Day, ms_of_day: i64) -> SYSTEMTIME {
    SYSTEMTIME {
        wYear: day.year as u16,
        wMonth: day.month as u16,
        wDayOfWeek: 0,
        wDay: day.day as u16,
        wHour: (ms_of_day / 3_600_000) as u16,
        wMinute: (ms_of_day / 60_000 % 60) as u16,
        wSecond: (ms_of_day / 1000 % 60) as u16,
        wMilliseconds: (ms_of_day % 1000) as u16,
    }
}

fn parts(t: &SYSTEMTIME) -> (Day, i64) {
    let day = Day { year: i32::from(t.wYear), month: u32::from(t.wMonth), day: u32::from(t.wDay) };
    let ms = i64::from(t.wHour) * 3_600_000
        + i64::from(t.wMinute) * 60_000
        + i64::from(t.wSecond) * 1000
        + i64::from(t.wMilliseconds);
    (day, ms)
}

/// The moment `day` at `minute` minutes past midnight on the computer's clock, in Unix
/// milliseconds. In the hour skipped when clocks go forward, Windows picks a moment nearby.
pub fn local_ms(day: Day, minute: u32) -> Option<i64> {
    let local = systemtime(day, i64::from(minute) * 60_000);
    let mut utc = SYSTEMTIME::default();
    // SAFETY: both pointers are to live SYSTEMTIMEs; `None` means the current time zone.
    unsafe { TzSpecificLocalTimeToSystemTime(None, &local, &mut utc) }.ok()?;
    let (day, ms) = parts(&utc);
    Some(day.number() * DAY_MS + ms)
}

/// The day and minute of the day on the computer's clock at `ms` (Unix milliseconds).
pub fn local_day(ms: i64) -> Option<(Day, u32)> {
    let utc = systemtime(Day::from_number(ms.div_euclid(DAY_MS)), ms.rem_euclid(DAY_MS));
    let mut local = SYSTEMTIME::default();
    // SAFETY: as in `local_ms`.
    unsafe { SystemTimeToTzSpecificLocalTime(None, &utc, &mut local) }.ok()?;
    let (day, ms) = parts(&local);
    Some((day, (ms / 60_000) as u32))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> Day {
        Day::parse(s).unwrap()
    }

    #[test]
    fn days_must_be_real() {
        for ok in ["2026-10-30", "2024-02-29", "2000-02-29", "1970-01-01", "9999-12-31"] {
            assert_eq!(Day::parse(ok).unwrap().to_string(), ok);
        }
        for bad in [
            "2026-02-29",
            "1900-02-29",
            "2026-13-01",
            "2026-00-10",
            "2026-04-31",
            "2026-1-05",
            "26-10-30",
            "2026/10/30",
            "",
            "2026-10-3x",
            "1969-12-31",
            "+026-10-30",
        ] {
            assert_eq!(Day::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn day_numbers_round_trip_and_know_the_weekday() {
        assert_eq!(d("1970-01-01").number(), 0);
        assert_eq!(d("2000-03-01").number(), 11_017);
        for n in [-1, 0, 59, 365, 11_016, 20_000, 2_932_896] {
            assert_eq!(Day::from_number(n).number(), n);
        }
        assert_eq!(d("2026-10-30").weekday(), 4, "a Friday");
        assert_eq!(d("2026-11-02").weekday(), 0, "a Monday");
        assert_eq!(d("2026-12-31").add_days(1), d("2027-01-01"));
        assert_eq!(d("2028-03-01").add_days(-1), d("2028-02-29"));
    }

    #[test]
    fn months_keep_the_day_or_take_the_last_one() {
        assert_eq!(d("2026-01-31").add_months(1), d("2026-02-28"));
        assert_eq!(d("2028-01-31").add_months(1), d("2028-02-29"));
        assert_eq!(d("2026-11-15").add_months(2), d("2027-01-15"));
        assert_eq!(d("2026-01-15").add_months(-1), d("2025-12-15"));
    }

    #[test]
    fn a_series_that_does_not_repeat_is_one_day() {
        let start = d("2026-10-30");
        assert_eq!(next_on_or_after(start, None, d("2026-10-01")), Some(start));
        assert_eq!(next_on_or_after(start, None, start), Some(start));
        assert_eq!(next_on_or_after(start, None, d("2026-10-31")), None);
    }

    #[test]
    fn each_repeat_finds_its_next_day() {
        let fri = d("2026-10-30");
        let next = |r, day: &str| next_on_or_after(fri, Some(r), d(day)).unwrap().to_string();
        assert_eq!(next(Repeat::Daily, "2026-11-04"), "2026-11-04");
        assert_eq!(next(Repeat::Weekly, "2026-10-31"), "2026-11-06");
        assert_eq!(next(Repeat::Weekly, "2026-11-06"), "2026-11-06");
        assert_eq!(next(Repeat::Weekdays, "2026-10-31"), "2026-11-02", "the weekend is skipped");
        assert_eq!(next(Repeat::Monthly, "2026-10-31"), "2026-11-30");
        assert_eq!(next(Repeat::Monthly, "2026-12-01"), "2026-12-30");
        assert_eq!(next(Repeat::Yearly, "2026-10-31"), "2027-10-30");
        assert_eq!(next(Repeat::Daily, "2026-01-01"), "2026-10-30", "never before the start");
    }

    #[test]
    fn monthly_on_the_31st_comes_back_to_the_31st() {
        let start = d("2026-01-31");
        let days: Vec<_> = days_between(start, Some(Repeat::Monthly), start, d("2026-05-31"), 99)
            .iter()
            .map(Day::to_string)
            .collect();
        assert_eq!(days, ["2026-01-31", "2026-02-28", "2026-03-31", "2026-04-30", "2026-05-31"]);
    }

    #[test]
    fn a_birthday_on_29_february() {
        let start = d("2028-02-29");
        let days: Vec<_> = days_between(start, Some(Repeat::Yearly), start, d("2032-12-31"), 99)
            .iter()
            .map(Day::to_string)
            .collect();
        assert_eq!(days, ["2028-02-29", "2029-02-28", "2030-02-28", "2031-02-28", "2032-02-29"]);
    }

    #[test]
    fn a_weekday_series_begun_at_the_weekend_starts_on_monday() {
        let sat = d("2026-10-31");
        assert_eq!(first_day(sat, Some(Repeat::Weekdays)), d("2026-11-02"));
        assert_eq!(first_day(sat, Some(Repeat::Weekly)), sat);
        let days = days_between(sat, Some(Repeat::Weekdays), sat, d("2026-11-09"), 99);
        assert_eq!(days.len(), 6);
        assert!(days.iter().all(|d| d.weekday() < 5));
    }

    #[test]
    fn days_between_stops_at_the_limit() {
        let start = d("2026-01-01");
        assert_eq!(days_between(start, Some(Repeat::Daily), start, d("2026-12-31"), 10).len(), 10);
        assert!(days_between(start, None, d("2026-01-02"), d("2026-12-31"), 10).is_empty());
    }

    #[test]
    fn times_are_minutes_into_the_day() {
        assert_eq!(minutes("00:00"), Some(0));
        assert_eq!(minutes("15:30"), Some(930));
        assert_eq!(minutes("24:00"), None);
        assert_eq!(minutes("noon"), None);
    }

    #[test]
    fn the_computers_clock_round_trips() {
        let day = d("2026-10-30");
        let ms = local_ms(day, 15 * 60 + 5).unwrap();
        assert_eq!(local_day(ms), Some((day, 15 * 60 + 5)));
        assert_eq!(local_ms(day, 16 * 60).unwrap() - ms, 55 * 60_000);
        // Whatever the time zone, a moment is within a day of the same clock time in UTC.
        assert!((ms - (day.number() * DAY_MS + 905 * 60_000)).abs() < DAY_MS);
    }
}
