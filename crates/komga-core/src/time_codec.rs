//! Time codecs, round-trip compatible with the formats jOOQ/xerial writes to SQLite.
//!
//! - DB `datetime`: TEXT `yyyy-MM-dd HH:mm:ss[.f…]` (`java.sql.Timestamp.toString()` semantics:
//!   `.0` is appended when nanos=0, otherwise 9-digit zero-padded with trailing zeros stripped), UTC.
//!   Reads also accept ISO-8601 (`T` separator, `Z`/offset suffix): external tools writing into
//!   komga's SQLite file directly produce that shape.
//! - DB `date`: TEXT `yyyy-MM-dd`.
//! - DTO `datetime`: `yyyy-MM-dd'T'HH:mm:ss'Z'` (second precision, UTC with no offset).
//! - DTO `date`: `yyyy-MM-dd`.

use time::{Date, Month, OffsetDateTime, PrimitiveDateTime, Time, UtcOffset};

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

/// Parses a DB datetime into its wall-clock part: jOOQ's write format (`yyyy-MM-dd HH:mm:ss[.f…]`,
/// including `CURRENT_TIMESTAMP`'s fraction-less form), plus ISO-8601 variants (`T` separator,
/// `Z`/`±HH:MM` suffix). The offset designator is validated but not applied.
pub fn parse_datetime(s: &str) -> Option<PrimitiveDateTime> {
    parse_db_datetime(s).map(|(dt, _)| dt)
}

/// `parse_datetime` as a UTC instant: an explicit offset is applied, otherwise UTC is assumed.
pub fn parse_datetime_utc(s: &str) -> Option<OffsetDateTime> {
    parse_db_datetime(s).map(|(dt, offset)| match offset {
        Some(offset) => dt.assume_offset(offset).to_offset(UtcOffset::UTC),
        None => dt.assume_utc(),
    })
}

fn parse_db_datetime(s: &str) -> Option<(PrimitiveDateTime, Option<UtcOffset>)> {
    let s = s.trim();
    let sep = s.find([' ', 'T', 't'])?;
    let date = parse_date(&s[..sep])?;
    let rest = &s[sep + 1..];
    // the date part is already consumed, so a '+'/'-' in the rest can only be the offset sign
    let (time_part, offset) = match rest.find(['+', '-']) {
        Some(i) => (&rest[..i], Some(parse_utc_offset(&rest[i..])?)),
        None => match rest.strip_suffix('Z').or_else(|| rest.strip_suffix('z')) {
            Some(t) => (t, Some(UtcOffset::UTC)),
            None => (rest, None),
        },
    };
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
    Some((PrimitiveDateTime::new(date, time), offset))
}

/// `±HH:MM`, `±HHMM` or `±HH`.
fn parse_utc_offset(s: &str) -> Option<UtcOffset> {
    let (sign, digits) = match s.as_bytes().first()? {
        b'+' => (1i8, &s[1..]),
        b'-' => (-1i8, &s[1..]),
        _ => return None,
    };
    if !digits.is_ascii() {
        return None;
    }
    let (hour, minute): (i8, i8) = match digits.split_once(':') {
        Some((h, m)) => (h.parse().ok()?, m.parse().ok()?),
        None if digits.len() == 4 => (digits[..2].parse().ok()?, digits[2..].parse().ok()?),
        None if digits.len() == 2 => (digits.parse().ok()?, 0),
        _ => return None,
    };
    UtcOffset::from_hms(sign * hour, sign * minute, 0).ok()
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

/// The system zone, resolved once through jiff: offsets then follow IANA DST rules per
/// timestamp, instead of a fixed offset snapshot. The `time` crate's own `local-offset`
/// feature stays disabled (it is unsound in multi-threaded programs).
pub fn system_time_zone() -> &'static jiff::tz::TimeZone {
    static TZ: std::sync::OnceLock<jiff::tz::TimeZone> = std::sync::OnceLock::new();
    TZ.get_or_init(jiff::tz::TimeZone::system)
}

fn to_utc_offset(offset: jiff::tz::Offset) -> time::UtcOffset {
    time::UtcOffset::from_whole_seconds(offset.seconds()).unwrap_or(time::UtcOffset::UTC)
}

/// Offset of the system zone in effect at the instant `dt` refers to.
pub fn system_offset_at(dt: OffsetDateTime) -> time::UtcOffset {
    jiff::Timestamp::from_second(dt.unix_timestamp())
        .map(|ts| to_utc_offset(ts.to_zoned(system_time_zone().clone()).offset()))
        .unwrap_or(time::UtcOffset::UTC)
}

/// Offset of the system zone at the wall-clock time `dt` shows (`LocalDateTime.atZone`).
/// jiff's compatible disambiguation matches Java: gaps shift forward, folds keep the
/// pre-transition offset.
pub fn system_offset_for_wall_clock(dt: OffsetDateTime) -> time::UtcOffset {
    jiff::civil::DateTime::new(
        dt.year() as i16,
        dt.month() as i8,
        dt.day() as i8,
        dt.hour() as i8,
        dt.minute() as i8,
        dt.second() as i8,
        0,
    )
    .ok()
    .and_then(|c| system_time_zone().to_zoned(c).ok())
    .map(|z| to_utc_offset(z.offset()))
    .unwrap_or(time::UtcOffset::UTC)
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
    dt.to_offset(system_offset_at(dt))
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

    #[test]
    fn parse_iso8601_variants() {
        // JS `Date.toISOString()` / `Instant.toString()` style, seen in komga databases
        // written to directly by external tools
        let dt = parse_datetime_utc("2026-09-21T05:33:30.327Z").unwrap();
        assert_eq!(format_datetime(dt), "2026-09-21 05:33:30.327");

        let dt = parse_datetime_utc("2026-09-21T05:33:30.327").unwrap();
        assert_eq!(format_datetime(dt), "2026-09-21 05:33:30.327");

        // an explicit offset is applied, not dropped
        let dt = parse_datetime_utc("2026-09-21T13:33:30.327+08:00").unwrap();
        assert_eq!(format_datetime(dt), "2026-09-21 05:33:30.327");
        let dt = parse_datetime_utc("2026-09-21T05:33:30-05:30").unwrap();
        assert_eq!(format_datetime(dt), "2026-09-21 11:03:30.0");

        assert!(parse_datetime("2026-09-21T05:33:30.327Z").is_some());

        assert!(parse_datetime_utc("2026-09-21T05:33:30.1234567890Z").is_none());
        assert!(parse_datetime_utc("2026-09-21T05:33:30+0x:00").is_none());
        assert!(parse_datetime_utc("2026-09-21T05:33:30.327ZZ").is_none());
        assert!(parse_datetime_utc("2026-09-21T05:33:30+080").is_none());
    }
}
