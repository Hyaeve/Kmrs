//! REST DTOs, serde-compatible with komga's `interfaces/api/rest/dto`.
//!
//! Datetime fields use the DTO format `yyyy-MM-dd'T'HH:mm:ss'Z'` (see `time_codec`); date fields
//! use `yyyy-MM-dd`. None of these DTOs carry Jackson `@JsonInclude(NON_NULL)`: null fields are
//! serialized as JSON null. The only exception is `R2Locator`, which is `NON_EMPTY`.

pub mod book;
pub mod collection;
pub mod common;
pub mod library;
pub mod progression;
pub mod readlist;
pub mod series;
pub mod tachiyomi;
pub mod thumbnail;

pub use common::{AlternateTitleDto, AuthorDto, GroupCountDto, WebLinkDto};

use serde::{Deserialize, Deserializer, Serializer};
use time::{Date, OffsetDateTime};

/// `#[serde(with = "dto_datetime")]` for `OffsetDateTime` fields
pub mod dto_datetime {
    use super::*;

    pub fn serialize<S: Serializer>(dt: &OffsetDateTime, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&crate::time_codec::format_dto_datetime(*dt))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<OffsetDateTime, D::Error> {
        let s = String::deserialize(deserializer)?;
        parse_dto_datetime(&s)
            .ok_or_else(|| serde::de::Error::custom(format!("invalid datetime: {s}")))
    }
}

/// `#[serde(with = "dto_datetime_opt")]` for `Option<OffsetDateTime>` fields
pub mod dto_datetime_opt {
    use super::*;

    pub fn serialize<S: Serializer>(
        dt: &Option<OffsetDateTime>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match dt {
            Some(dt) => serializer.serialize_str(&crate::time_codec::format_dto_datetime(*dt)),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<OffsetDateTime>, D::Error> {
        let s: Option<String> = Option::deserialize(deserializer)?;
        match s {
            Some(s) => parse_dto_datetime(&s)
                .map(Some)
                .ok_or_else(|| serde::de::Error::custom(format!("invalid datetime: {s}"))),
            None => Ok(None),
        }
    }
}

/// `#[serde(with = "dto_date")]` for `Option<Date>` fields
pub mod dto_date {
    use super::*;

    pub fn serialize<S: Serializer>(d: &Option<Date>, serializer: S) -> Result<S::Ok, S::Error> {
        match d {
            Some(d) => serializer.serialize_str(&crate::time_codec::format_date(*d)),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<Date>, D::Error> {
        let s: Option<String> = Option::deserialize(deserializer)?;
        match s {
            Some(s) => crate::time_codec::parse_date(&s)
                .map(Some)
                .ok_or_else(|| serde::de::Error::custom(format!("invalid date: {s}"))),
            None => Ok(None),
        }
    }
}

/// Parses the DTO datetime format `yyyy-MM-dd'T'HH:mm:ss'Z'`
pub fn parse_dto_datetime(s: &str) -> Option<OffsetDateTime> {
    let format =
        time::macros::format_description!("[year]-[month]-[day]T[hour]:[minute]:[second]Z");
    Some(time::PrimitiveDateTime::parse(s, format).ok()?.assume_utc())
}

/// `BinaryByteUnit.format` (jakewharton/byteunits 0.9.1): `#,##0.#` number format over
/// 1024-based units, rounding half-even to at most one decimal.
pub fn format_binary_bytes(bytes: i64) -> String {
    assert!(bytes >= 0, "bytes < 0: {bytes}");
    const UNITS: [&str; 6] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
    let mut count = bytes as f64;
    let mut unit_index = 0;
    while count >= 1024.0 && unit_index < UNITS.len() - 1 {
        count /= 1024.0;
        unit_index += 1;
    }
    format!("{} {}", format_decimal(count), UNITS[unit_index])
}

/// Java `DecimalFormat("#,##0.#")`: thousands grouping, at most one fraction digit,
/// round-half-even on the exact binary value.
fn format_decimal(value: f64) -> String {
    // Rust's `{:.1}` is correctly rounded half-even on the exact value, same as DecimalFormat
    let rounded = format!("{value:.1}");
    let (int_part, frac_part) = rounded.split_once('.').unwrap_or((&rounded[..], ""));
    let mut grouped = String::new();
    for (i, c) in int_part.chars().enumerate() {
        if i > 0 && (int_part.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(c);
    }
    if frac_part == "0" {
        grouped
    } else {
        format!("{grouped}.{frac_part}")
    }
}

/// `URL.toURI().toPath().pathString` for komga's `file:/…` URLs: strips the scheme
/// and percent-decodes (UTF-8). Java's Path normalizes away the trailing `/` of
/// directory URLs (`Paths.get("/a/")` renders as `/a`).
pub fn url_to_file_path(url: &str) -> String {
    let path = url.strip_prefix("file:").unwrap_or(url);
    // `file:///path` and `file:/path` both denote a local path with an empty authority
    let path = path.strip_prefix("//").map_or(path, |p| {
        // after the authority marker, the path starts at the next '/'
        match p.find('/') {
            Some(idx) => &p[idx..],
            None => "/",
        }
    });
    let decoded = percent_decode(path);
    if decoded.len() > 1 {
        decoded.trim_end_matches('/').to_string()
    } else {
        decoded
    }
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let hex = |b: u8| -> Option<u8> {
        match b {
            b'0'..=b'9' => Some(b - b'0'),
            b'a'..=b'f' => Some(b - b'a' + 10),
            b'A'..=b'F' => Some(b - b'A' + 10),
            _ => None,
        }
    };
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push(h << 4 | l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_bytes_format() {
        assert_eq!(format_binary_bytes(0), "0 B");
        assert_eq!(format_binary_bytes(999), "999 B");
        assert_eq!(format_binary_bytes(1024), "1 KiB");
        assert_eq!(format_binary_bytes(1536), "1.5 KiB");
        assert_eq!(format_binary_bytes(1_048_576), "1 MiB");
        assert_eq!(format_binary_bytes(1_234_567), "1.2 MiB");
        assert_eq!(format_binary_bytes(1_040_000), "1,015.6 KiB");
        assert_eq!(format_binary_bytes(2_000_000_000), "1.9 GiB");
        assert_eq!(format_binary_bytes(4_294_967_296), "4 GiB");
        assert_eq!(format_binary_bytes(5_497_558_138_880), "5 TiB");
    }

    #[test]
    fn url_to_path() {
        // Java's `toPath().pathString` drops the trailing slash of directory URLs
        assert_eq!(url_to_file_path("file:/data/berserk/"), "/data/berserk");
        assert_eq!(url_to_file_path("file:///data/berserk/"), "/data/berserk");
        assert_eq!(url_to_file_path("file:/data/my%20book/"), "/data/my book");
        assert_eq!(url_to_file_path("file:/data/%E3%81%82/"), "/data/あ");
        assert_eq!(url_to_file_path("file:/"), "/");
    }

    #[test]
    fn dto_datetime_roundtrip() {
        let dt = parse_dto_datetime("2024-01-02T03:04:05Z").unwrap();
        assert_eq!(
            crate::time_codec::format_dto_datetime(dt),
            "2024-01-02T03:04:05Z"
        );
        assert!(parse_dto_datetime("2024-01-02 03:04:05").is_none());
    }
}
