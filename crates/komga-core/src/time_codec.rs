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

/// The system zone offset, read once from the platform `date` command. The `time` crate's
/// `local-offset` feature is intentionally not enabled (it is unsound in multi-threaded programs).
pub fn system_offset() -> time::UtcOffset {
    static OFFSET: std::sync::OnceLock<time::UtcOffset> = std::sync::OnceLock::new();
    *OFFSET.get_or_init(|| {
        std::process::Command::new("date")
            .arg("+%z")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .and_then(|s| {
                let s = s.trim();
                let (sign, rest) = s.split_at_checked(1)?;
                let hours: i32 = rest.get(..2)?.parse().ok()?;
                let minutes: i32 = rest.get(2..4)?.parse().ok()?;
                let seconds = (hours * 3600 + minutes * 60) * if sign == "-" { -1 } else { 1 };
                time::UtcOffset::from_hms(
                    seconds.div_euclid(3600) as i8,
                    (seconds.rem_euclid(3600) / 60) as i8,
                    0,
                )
                .ok()
            })
            .unwrap_or(time::UtcOffset::UTC)
    })
}

/// Jackson `ISO_OFFSET_DATE_TIME`: `yyyy-MM-dd'T'HH:mm:ss[.SSS]±HH:MM`; nanos padded to 9
/// digits, then trailing zeros stripped (verified against Jackson 2.21).
/// `Z` when the offset is zero (Jackson renders +00:00 as Z).
pub fn format_offset_date_time(dt: OffsetDateTime) -> String {
    let nanos = dt.nanosecond();
    let fraction = if nanos == 0 {
        String::new()
    } else {
        let digits = format!("{nanos:09}");
        let trimmed = digits.trim_end_matches('0');
        format!(".{trimmed}")
    };
    let offset = dt.offset();
    let offset_str = if offset.is_utc() {
        "Z".to_string()
    } else {
        let total = offset.whole_seconds();
        let sign = if total < 0 { '-' } else { '+' };
        let abs = total.unsigned_abs();
        format!("{sign}{:02}:{:02}", abs / 3600, (abs % 3600) / 60)
    };
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}{fraction}{offset_str}",
        dt.year(),
        dt.month() as u8,
        dt.day(),
        dt.hour(),
        dt.minute(),
        dt.second(),
    )
}

/// `LocalDateTime.toZonedDateTime()`: reinterpret a UTC timestamp in the system zone
/// (`atZoneSameInstant(ZoneId.systemDefault())`).
pub fn to_zoned_date_time(dt: OffsetDateTime) -> OffsetDateTime {
    dt.to_offset(system_offset())
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
    fn offset_format() {
        use time::UtcOffset;
        let dt = parse_datetime_utc("2024-01-02 03:04:05.999").unwrap();
        assert_eq!(
            format_offset_date_time(dt.to_offset(UtcOffset::UTC)),
            "2024-01-02T03:04:05.999Z"
        );
        let offset = UtcOffset::from_hms(8, 0, 0).unwrap();
        assert_eq!(
            format_offset_date_time(dt.to_offset(offset)),
            "2024-01-02T11:04:05.999+08:00"
        );
        let offset = UtcOffset::from_hms(-5, -30, 0).unwrap();
        assert_eq!(
            format_offset_date_time(dt.to_offset(offset)),
            "2024-01-01T21:34:05.999-05:30"
        );
        // nanos padded to 9 digits, then trailing zeros stripped (Jackson rule)
        let dt = parse_datetime_utc("2024-01-02 03:04:05.9999995").unwrap();
        assert_eq!(
            format_offset_date_time(dt.to_offset(UtcOffset::UTC)),
            "2024-01-02T03:04:05.9999995Z"
        );
        let dt = parse_datetime_utc("2024-01-02 03:04:05.120").unwrap();
        assert_eq!(
            format_offset_date_time(dt.to_offset(UtcOffset::UTC)),
            "2024-01-02T03:04:05.12Z"
        );
    }

    #[test]
    fn date_roundtrip() {
        let d = parse_date("2024-02-29").unwrap();
        assert_eq!(format_date(d), "2024-02-29");
        assert!(parse_date("2023-02-29").is_none());
    }
}
