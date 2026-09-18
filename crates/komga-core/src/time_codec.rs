//! Time codecs, round-trip compatible with the formats jOOQ/xerial writes to SQLite.
//!
//! - DB `datetime`: TEXT `yyyy-MM-dd HH:mm:ss[.f…]` (`java.sql.Timestamp.toString()` semantics:
//!   `.0` is appended when nanos=0, otherwise 9-digit zero-padded with trailing zeros stripped), UTC.
//! - DB `date`: TEXT `yyyy-MM-dd`.
//! - DTO `datetime`: `yyyy-MM-dd'T'HH:mm:ss'Z'` (second precision, UTC with no offset).
//! - DTO `date`: `yyyy-MM-dd`.

use time::{Date, Month, OffsetDateTime, PrimitiveDateTime, Time};

pub fn now_utc() -> OffsetDateTime {
    OffsetDateTime::now_utc()
}

/// The nanos part of `Timestamp.toString()`: 0 → "0"; otherwise 9-digit zero-padded with trailing zeros stripped.
fn format_nanos(nanos: u32) -> String {
    if nanos == 0 {
        return "0".to_string();
    }
    let mut s = format!("{nanos:09}");
    while s.ends_with('0') {
        s.pop();
    }
    s
}

/// DB datetime write format.
pub fn format_datetime(dt: OffsetDateTime) -> String {
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{}",
        dt.year(),
        dt.month() as u8,
        dt.day(),
        dt.hour(),
        dt.minute(),
        dt.second(),
        format_nanos(dt.nanosecond()),
    )
}

/// Parses a DB datetime: tolerates no fraction, the fraction-less form produced by `CURRENT_TIMESTAMP`, and up to 9 fraction digits.
pub fn parse_datetime(s: &str) -> Option<PrimitiveDateTime> {
    let s = s.trim();
    let (date_part, time_part) = s.split_once(' ')?;
    let date = parse_date(date_part)?;
    let (hms, nanos) = match time_part.split_once('.') {
        Some((hms, frac)) => {
            if frac.is_empty() || frac.len() > 9 || !frac.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            let mut n: u32 = frac.parse().ok()?;
            for _ in 0..(9 - frac.len()) {
                n *= 10;
            }
            (hms, n)
        }
        None => (time_part, 0),
    };
    let mut it = hms.split(':');
    let hour: u8 = it.next()?.parse().ok()?;
    let minute: u8 = it.next()?.parse().ok()?;
    let second: u8 = it.next()?.parse().ok()?;
    if it.next().is_some() {
        return None;
    }
    let time = Time::from_hms_nano(hour, minute, second, nanos).ok()?;
    Some(PrimitiveDateTime::new(date, time))
}

pub fn parse_datetime_utc(s: &str) -> Option<OffsetDateTime> {
    parse_datetime(s).map(|dt| dt.assume_utc())
}

/// DB datetime truncated to milliseconds (komga's mtime comparison semantics, `LanguageUtils.kt`).
pub fn truncate_to_millis(dt: OffsetDateTime) -> OffsetDateTime {
    let nanos = dt.nanosecond();
    dt.replace_nanosecond(nanos - nanos % 1_000_000).unwrap()
}

pub fn format_date(d: Date) -> String {
    format!("{:04}-{:02}-{:02}", d.year(), d.month() as u8, d.day())
}

pub fn parse_date(s: &str) -> Option<Date> {
    let s = s.trim();
    let mut it = s.split('-');
    let year: i32 = it.next()?.parse().ok()?;
    let month: u8 = it.next()?.parse().ok()?;
    let day: u8 = it.next()?.parse().ok()?;
    if it.next().is_some() {
        return None;
    }
    Date::from_calendar_date(year, Month::try_from(month).ok()?, day).ok()
}

/// DTO datetime: `yyyy-MM-dd'T'HH:mm:ss'Z'`.
pub fn format_dto_datetime(dt: OffsetDateTime) -> String {
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        dt.year(),
        dt.month() as u8,
        dt.day(),
        dt.hour(),
        dt.minute(),
        dt.second(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamp_to_string_semantics() {
        let dt = parse_datetime_utc("2020-01-02 03:04:05").unwrap();
        assert_eq!(format_datetime(dt), "2020-01-02 03:04:05.0");

        let dt = parse_datetime_utc("2020-01-02 03:04:05.123456789").unwrap();
        assert_eq!(format_datetime(dt), "2020-01-02 03:04:05.123456789");

        let dt = parse_datetime_utc("2020-01-02 03:04:05.120").unwrap();
        assert_eq!(format_datetime(dt), "2020-01-02 03:04:05.12");

        let dt = parse_datetime_utc("2020-01-02 03:04:05.100000000").unwrap();
        assert_eq!(format_datetime(dt), "2020-01-02 03:04:05.1");
    }

    #[test]
    fn parse_tolerates_variants() {
        assert!(parse_datetime_utc("2020-01-02 03:04:05").is_some());
        assert!(parse_datetime_utc("2020-01-02 03:04:05.0").is_some());
        assert!(parse_datetime_utc("2020-01-02 03:04:05.123456789").is_some());
        assert!(parse_datetime_utc("2020-01-02").is_none());
        assert!(parse_datetime_utc("2020-01-02 03:04:05.1234567890").is_none());
    }

    #[test]
    fn millis_truncation() {
        let dt = parse_datetime_utc("2020-01-02 03:04:05.123456789").unwrap();
        assert_eq!(
            format_datetime(truncate_to_millis(dt)),
            "2020-01-02 03:04:05.123"
        );
    }

    #[test]
    fn dto_format() {
        let dt = parse_datetime_utc("2020-01-02 03:04:05.999").unwrap();
        assert_eq!(format_dto_datetime(dt), "2020-01-02T03:04:05Z");
    }

    #[test]
    fn date_roundtrip() {
        let d = parse_date("2024-02-29").unwrap();
        assert_eq!(format_date(d), "2024-02-29");
        assert!(parse_date("2023-02-29").is_none());
    }
}
