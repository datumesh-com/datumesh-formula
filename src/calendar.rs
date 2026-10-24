//! Dates and instants (SPEC 3.2, 3.3) and the date arithmetic of 15.7: proleptic Gregorian
//! days, ISO weekdays, months with end-of-month clamping, business days and year fractions.

use crate::decimal::{self, Decimal};
use crate::Code;

/// A calendar day, years 0001 to 9999.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date {
    /// Year, 1-9999.
    pub year: i32,
    /// Month, 1-12.
    pub month: u32,
    /// Day of the month.
    pub day: u32,
}

/// Whether `y` is a leap year.
pub fn is_leap(y: i32) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

/// The number of days in month `m` of year `y`.
pub fn days_in_month(y: i32, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => {
            if is_leap(y) {
                29
            } else {
                28
            }
        }
    }
}

fn two(b: &[u8]) -> Option<u32> {
    if b.len() == 2 && b[0].is_ascii_digit() && b[1].is_ascii_digit() {
        Some(u32::from(b[0] - b'0') * 10 + u32::from(b[1] - b'0'))
    } else {
        None
    }
}

impl Date {
    /// A real day in 0001-9999, or `None`.
    pub fn new(year: i64, month: i64, day: i64) -> Option<Date> {
        if !(1..=9999).contains(&year) || !(1..=12).contains(&month) || day < 1 {
            return None;
        }
        let (y, m) = (year as i32, month as u32);
        if day as u64 > u64::from(days_in_month(y, m)) {
            return None;
        }
        Some(Date { year: y, month: m, day: day as u32 })
    }

    /// Parse `YYYY-MM-DD` (SPEC 3.2).
    pub fn parse(s: &str) -> Option<Date> {
        let b = s.as_bytes();
        if b.len() != 10 || b[4] != b'-' || b[7] != b'-' || !b[..4].iter().all(u8::is_ascii_digit) {
            return None;
        }
        let y: i64 = s[..4].parse().ok()?;
        Date::new(y, i64::from(two(&b[5..7])?), i64::from(two(&b[8..10])?))
    }

    /// The days since 1970-01-01 (negative before).
    pub fn ordinal(self) -> i64 {
        // Howard Hinnant's days_from_civil.
        let y = i64::from(self.year) - i64::from(self.month <= 2);
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let m = i64::from(self.month);
        let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(self.day) - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146097 + doe - 719468
    }

    /// The date of a day count since 1970-01-01, if within 0001-9999.
    pub fn from_ordinal(z: i64) -> Option<Date> {
        let z = z + 719468;
        let era = z.div_euclid(146097);
        let doe = z - era * 146097;
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = yoe + era * 400 + i64::from(m <= 2);
        Date::new(y, m, d)
    }

    /// The ISO weekday: Monday 1 to Sunday 7.
    pub fn weekday(self) -> u32 {
        // 1970-01-01 was a Thursday (4).
        ((self.ordinal() + 3).rem_euclid(7) + 1) as u32
    }

    /// The last day of the month.
    pub fn end_of_month(self) -> Date {
        Date { day: days_in_month(self.year, self.month), ..self }
    }

    /// Excel's EDATE: `n` months later, clamped to the month's last day.
    pub fn add_months(self, n: i64) -> Option<Date> {
        let total = i64::from(self.year) * 12 + i64::from(self.month) - 1 + n;
        let y = total.div_euclid(12);
        let m = total.rem_euclid(12) + 1;
        if !(1..=9999).contains(&y) {
            return None;
        }
        let dim = days_in_month(y as i32, m as u32);
        Date::new(y, m, i64::from(self.day.min(dim)))
    }

    /// `YYYY-MM-DD`.
    pub fn text(self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }

    /// Whether this is the last day of February.
    fn is_last_of_february(self) -> bool {
        self.month == 2 && self.day == days_in_month(self.year, 2)
    }
}

/// Microseconds since 1970-01-01T00:00:00Z.
pub type Micros = i64;

/// Microseconds in a day.
pub const DAY_MICROS: i64 = 86_400_000_000;

/// Parse a canonical instant (SPEC 3.3) to microseconds.
pub fn parse_instant(s: &str) -> Option<Micros> {
    let b = s.as_bytes();
    if b.len() != 27 || b[10] != b'T' || b[13] != b':' || b[16] != b':' || b[19] != b'.' || b[26] != b'Z' {
        return None;
    }
    let date = Date::parse(&s[..10])?;
    let h = two(&b[11..13])?;
    let mi = two(&b[14..16])?;
    let se = two(&b[17..19])?;
    if h > 23 || mi > 59 || se > 59 || !b[20..26].iter().all(u8::is_ascii_digit) {
        return None;
    }
    let frac: i64 = s[20..26].parse().ok()?;
    Some(date.ordinal() * DAY_MICROS + (i64::from(h) * 3600 + i64::from(mi) * 60 + i64::from(se)) * 1_000_000 + frac)
}

/// The smallest instant, 0001-01-01T00:00:00.000000Z.
pub fn min_instant() -> Micros {
    Date { year: 1, month: 1, day: 1 }.ordinal() * DAY_MICROS
}

/// The greatest instant, 9999-12-31T23:59:59.999999Z.
pub fn max_instant() -> Micros {
    (Date { year: 9999, month: 12, day: 31 }.ordinal() + 1) * DAY_MICROS - 1
}

/// The canonical text of an instant, if within 0001-9999.
pub fn instant_text(us: Micros) -> Option<String> {
    if us < min_instant() || us > max_instant() {
        return None;
    }
    let day = us.div_euclid(DAY_MICROS);
    let rest = us.rem_euclid(DAY_MICROS);
    let d = Date::from_ordinal(day)?;
    let secs = rest / 1_000_000;
    let frac = rest % 1_000_000;
    Some(format!("{}T{:02}:{:02}:{:02}.{:06}Z", d.text(), secs / 3600, (secs / 60) % 60, secs % 60, frac))
}

/// The UTC date of an instant.
pub fn date_of(us: Micros) -> Option<Date> {
    Date::from_ordinal(us.div_euclid(DAY_MICROS))
}

/// A weekend rule of 15.7.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Weekend {
    /// Saturday and Sunday (the default).
    SatSun,
    /// Friday and Saturday.
    FriSat,
    /// Sunday only.
    Sun,
    /// No weekend.
    None,
}

impl Weekend {
    /// The rule named by its literal.
    pub fn from_name(s: &str) -> Option<Weekend> {
        match s {
            "sat-sun" => Some(Weekend::SatSun),
            "fri-sat" => Some(Weekend::FriSat),
            "sun" => Some(Weekend::Sun),
            "none" => Some(Weekend::None),
            _ => None,
        }
    }

    /// Whether `d` falls on the weekend.
    pub fn is_weekend(self, d: Date) -> bool {
        let w = d.weekday();
        match self {
            Weekend::SatSun => w >= 6,
            Weekend::FriSat => w == 5 || w == 6,
            Weekend::Sun => w == 7,
            Weekend::None => false,
        }
    }
}

/// Business-day rules: a weekend and a holiday set (the calendar slice's dates).
pub struct BusinessDays<'a> {
    /// The weekend.
    pub weekend: Weekend,
    /// The holidays, ascending.
    pub holidays: &'a [Date],
}

impl BusinessDays<'_> {
    /// Whether `d` is a business day.
    pub fn is_business(&self, d: Date) -> bool {
        !self.weekend.is_weekend(d) && self.holidays.binary_search(&d).is_err()
    }

    /// Excel's WORKDAY: `n` business days after (or before) `d`, and the days stepped over.
    pub fn add(&self, d: Date, n: i64, fuel: &mut dyn FnMut(u64) -> Result<(), Code>) -> Result<Date, Code> {
        let mut cur = d.ordinal();
        let mut left = n.unsigned_abs();
        let step = if n < 0 { -1 } else { 1 };
        while left > 0 {
            cur += step;
            fuel(1)?;
            let day = Date::from_ordinal(cur).ok_or(Code::Range)?;
            if self.is_business(day) {
                left -= 1;
            }
        }
        Date::from_ordinal(cur).ok_or(Code::Range)
    }

    /// A roll convention applied to `d`.
    pub fn roll(&self, d: Date, roll: Roll, fuel: &mut dyn FnMut(u64) -> Result<(), Code>) -> Result<Date, Code> {
        if self.is_business(d) {
            return Ok(d);
        }
        let seek = |dir: i64, fuel: &mut dyn FnMut(u64) -> Result<(), Code>| -> Result<Date, Code> {
            let mut cur = d.ordinal();
            loop {
                cur += dir;
                fuel(1)?;
                let day = Date::from_ordinal(cur).ok_or(Code::Range)?;
                if self.is_business(day) {
                    return Ok(day);
                }
            }
        };
        match roll {
            Roll::Following => seek(1, fuel),
            Roll::Preceding => seek(-1, fuel),
            Roll::ModifiedFollowing => {
                let f = seek(1, fuel)?;
                if f.month == d.month && f.year == d.year {
                    Ok(f)
                } else {
                    seek(-1, fuel)
                }
            }
        }
    }

    /// Excel's NETWORKDAYS: business days from `a` to `b`, both inclusive, negative when
    /// reversed; one unit of fuel per day stepped over.
    pub fn between(&self, a: Date, b: Date, fuel: &mut dyn FnMut(u64) -> Result<(), Code>) -> Result<i64, Code> {
        let (lo, hi, sign) = if a <= b { (a, b, 1) } else { (b, a, -1) };
        let mut n = 0i64;
        for o in lo.ordinal()..=hi.ordinal() {
            fuel(1)?;
            let day = Date::from_ordinal(o).ok_or(Code::Range)?;
            if self.is_business(day) {
                n += 1;
            }
        }
        Ok(sign * n)
    }
}

/// A business-day roll of 15.7.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Roll {
    /// The next business day.
    Following,
    /// The next business day, unless it is in the next month: then the previous one.
    ModifiedFollowing,
    /// The previous business day.
    Preceding,
}

impl Roll {
    /// The roll named by its literal.
    pub fn from_name(s: &str) -> Option<Roll> {
        match s {
            "following" => Some(Roll::Following),
            "modified_following" => Some(Roll::ModifiedFollowing),
            "preceding" => Some(Roll::Preceding),
            _ => None,
        }
    }
}

/// The conventions of `year_fraction`.
pub const CONVENTIONS: &[&str] = &["act/360", "act/365f", "act/act_isda", "30/360", "30e/360", "30/360_us"];

/// `year_fraction(a, b, convention)`: the order of the dates does not matter.
pub fn year_fraction(a: Date, b: Date, convention: &str) -> Result<Decimal, Code> {
    let (d1, d2) = if a <= b { (a, b) } else { (b, a) };
    let int = Decimal::from_i64;
    let days = d2.ordinal() - d1.ordinal();
    match convention {
        "act/360" => decimal::div(&int(days), &int(360)),
        "act/365f" => decimal::div(&int(days), &int(365)),
        "act/act_isda" => {
            // Days in non-leap years over 365 plus days in leap years over 366, as one fraction.
            let mut d365 = 0i64;
            let mut d366 = 0i64;
            let mut y = d1.year;
            let mut from = d1.ordinal();
            let end = d2.ordinal();
            while from < end {
                let next_year = if y < 9999 { Date { year: y + 1, month: 1, day: 1 }.ordinal() } else { end };
                let to = next_year.min(end);
                if is_leap(y) {
                    d366 += to - from;
                } else {
                    d365 += to - from;
                }
                from = to;
                y += 1;
            }
            let num = int(d365 * 366 + d366 * 365);
            decimal::div(&num, &int(365 * 366))
        }
        "30/360" | "30e/360" | "30/360_us" => {
            let (mut dd1, mut dd2) = (i64::from(d1.day), i64::from(d2.day));
            match convention {
                "30/360" => {
                    if dd1 == 31 {
                        dd1 = 30;
                    }
                    if dd2 == 31 && dd1 >= 30 {
                        dd2 = 30;
                    }
                }
                "30e/360" => {
                    if dd1 == 31 {
                        dd1 = 30;
                    }
                    if dd2 == 31 {
                        dd2 = 30;
                    }
                }
                _ => {
                    if d1.is_last_of_february() && d2.is_last_of_february() {
                        dd2 = 30;
                    }
                    if d1.is_last_of_february() {
                        dd1 = 30;
                    }
                    if dd2 == 31 && dd1 >= 30 {
                        dd2 = 30;
                    }
                    if dd1 == 31 {
                        dd1 = 30;
                    }
                }
            }
            let n = (i64::from(d2.year) - i64::from(d1.year)) * 360
                + (i64::from(d2.month) - i64::from(d1.month)) * 30
                + (dd2 - dd1);
            decimal::div(&int(n), &int(360))
        }
        _ => Err(Code::Enum),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinals() {
        for s in ["0001-01-01", "1970-01-01", "2024-02-29", "9999-12-31", "2000-03-01"] {
            let d = Date::parse(s).unwrap();
            assert_eq!(Date::from_ordinal(d.ordinal()).unwrap(), d);
            assert_eq!(d.text(), s);
        }
        assert_eq!(Date::parse("1970-01-01").unwrap().ordinal(), 0);
        assert_eq!(Date::parse("2024-03-15").unwrap().weekday(), 5);
        assert!(Date::parse("2023-02-29").is_none());
        assert!(Date::parse("0000-01-01").is_none());
        let t = parse_instant("2026-03-01T10:00:00.250000Z").unwrap();
        assert_eq!(instant_text(t).unwrap(), "2026-03-01T10:00:00.250000Z");
        assert_eq!(instant_text(max_instant()).unwrap(), "9999-12-31T23:59:59.999999Z");
        assert_eq!(instant_text(min_instant()).unwrap(), "0001-01-01T00:00:00.000000Z");
    }
}
