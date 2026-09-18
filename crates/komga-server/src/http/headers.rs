//! Shared HTTP helpers for endpoints: `ContentDisposition` encoding, Last-Modified/304
//! negotiation, HTTP-date codecs, and the `author` / `search_regex` query parameter parsers.

use axum::http::HeaderMap;
use komga_core::model::common::Author;

/// Spring `ContentDisposition` with a UTF-8 filename, as built by
/// `ContentDisposition.builder(kind).filename(name, UTF_8)`: always the `filename*=UTF-8''` form,
/// with RFC 5987 attr-char pass-through and uppercase percent-encoding for the rest.
pub fn content_disposition(kind: &str, filename: &str) -> String {
    let mut encoded = String::with_capacity(filename.len() * 2);
    for &b in filename.as_bytes() {
        let c = b as char;
        let attr_char = c.is_ascii_alphanumeric()
            || matches!(
                c,
                '!' | '#'
                    | '$'
                    | '&'
                    | '+'
                    | '-'
                    | '.'
                    | '^'
                    | '_'
                    | '`'
                    | '|'
                    | '~'
            );
        if attr_char {
            encoded.push(c);
        } else {
            encoded.push_str(&format!("%{b:02X}"));
        }
    }
    format!("{kind}; filename*=UTF-8''{encoded}")
}

/// `EEE, dd MMM yyyy HH:mm:ss 'GMT'` (RFC 1123), the format of `Last-Modified` and
/// `If-Modified-Since`. Input is epoch seconds.
pub fn format_http_date(epoch_seconds: i64) -> String {
    const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let dt = time::OffsetDateTime::from_unix_timestamp(epoch_seconds)
        .unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
    let weekday = DAYS[dt.weekday().number_days_from_sunday() as usize];
    format!(
        "{weekday}, {:02} {} {:04} {:02}:{:02}:{:02} GMT",
        dt.day(),
        MONTHS[dt.month() as usize - 1],
        dt.year(),
        dt.hour(),
        dt.minute(),
        dt.second(),
    )
}

/// Parses an HTTP-date (RFC 1123 IMF-fixdate) into epoch seconds; anything else yields None.
pub fn parse_http_date(s: &str) -> Option<i64> {
    // "Sun, 06 Nov 1994 08:49:37 GMT"
    let s = s.trim();
    let (weekday, rest) = s.split_once(',')?;
    if weekday.trim().len() != 3 {
        return None;
    }
    let mut it = rest.trim().split(' ');
    let day: u8 = it.next()?.parse().ok()?;
    let month = match it.next()? {
        "Jan" => 1,
        "Feb" => 2,
        "Mar" => 3,
        "Apr" => 4,
        "May" => 5,
        "Jun" => 6,
        "Jul" => 7,
        "Aug" => 8,
        "Sep" => 9,
        "Oct" => 10,
        "Nov" => 11,
        "Dec" => 12,
        _ => return None,
    };
    let year: i32 = it.next()?.parse().ok()?;
    let hms = it.next()?;
    let mut hms_it = hms.split(':');
    let hour: u8 = hms_it.next()?.parse().ok()?;
    let minute: u8 = hms_it.next()?.parse().ok()?;
    let second: u8 = hms_it.next()?.parse().ok()?;
    let date = time::Date::from_calendar_date(year, time::Month::try_from(month).ok()?, day).ok()?;
    let time = time::Time::from_hms(hour, minute, second).ok()?;
    Some(time::PrimitiveDateTime::new(date, time).assume_utc().unix_timestamp())
}

/// Spring `WebRequest.checkNotModified(lastModifiedTimestamp)`: true when the resource was not
/// modified since `If-Modified-Since` (millisecond timestamp truncated to seconds).
pub fn check_not_modified(last_modified_millis: i64, headers: &HeaderMap) -> bool {
    let Some(if_modified_since) = headers
        .get(axum::http::header::IF_MODIFIED_SINCE)
        .and_then(|v| v.to_str().ok())
        .and_then(parse_http_date)
    else {
        return false;
    };
    if_modified_since >= last_modified_millis / 1000
}

/// `AuthorsHandlerMethodArgumentResolver`: `author=name,role` (last-comma split); entries without
/// a comma are dropped; a single blank parameter means "no filter".
pub fn parse_authors(values: &[String]) -> Vec<Author> {
    if values.len() == 1 && values[0].trim().is_empty() {
        return vec![];
    }
    values
        .iter()
        .filter(|v| v.contains(','))
        .map(|v| Author {
            name: v.rsplit_once(',').map(|(n, _)| n.to_string()).unwrap_or_default(),
            role: v.rsplit_once(',').map(|(_, r)| r.to_string()).unwrap_or_default(),
        })
        .collect()
}

/// `DelimitedPairHandlerMethodArgumentResolver`: first value, last-comma split; absent/blank or
/// comma-less yields None.
pub fn parse_delimited_pair(values: &[String]) -> Option<(String, String)> {
    let first = values.first()?;
    if values.len() == 1 && first.trim().is_empty() {
        return None;
    }
    first
        .rsplit_once(',')
        .map(|(a, b)| (a.to_string(), b.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_disposition_encodes_like_spring() {
        assert_eq!(
            content_disposition("attachment", "Berserk v01.cbz"),
            "attachment; filename*=UTF-8''Berserk%20v01.cbz"
        );
        assert_eq!(
            content_disposition("inline", "ページ-1.jpeg"),
            "inline; filename*=UTF-8''%E3%83%9A%E3%83%BC%E3%82%B8-1.jpeg"
        );
    }

    #[test]
    fn http_date_roundtrip() {
        let s = "Sun, 06 Nov 1994 08:49:37 GMT";
        let epoch = parse_http_date(s).unwrap();
        assert_eq!(epoch, 784111777);
        assert_eq!(format_http_date(epoch), s);
        assert!(parse_http_date("not a date").is_none());
    }

    #[test]
    fn not_modified_semantics() {
        let mut headers = HeaderMap::new();
        // last modified 1994-11-06 08:49:37.500 UTC -> truncated to 784111777
        headers.insert(
            axum::http::header::IF_MODIFIED_SINCE,
            "Sun, 06 Nov 1994 08:49:37 GMT".parse().unwrap(),
        );
        assert!(check_not_modified(784_111_777_500, &headers));
        // one second earlier: modified
        assert!(!check_not_modified(784_111_778_500, &headers));
        assert!(!check_not_modified(784_111_777_500, &HeaderMap::new()));
    }

    #[test]
    fn authors_parsing() {
        let parsed = parse_authors(&["Kentaro Miura,writer".to_string(), "no-comma".to_string()]);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].name, "Kentaro Miura");
        assert_eq!(parsed[0].role, "writer");
        assert!(parse_authors(&["".to_string()]).is_empty());
        // last-comma split keeps commas in the name
        let parsed = parse_authors(&["Doe, John,writer".to_string()]);
        assert_eq!(parsed[0].name, "Doe, John");
    }

    #[test]
    fn delimited_pair_parsing() {
        assert_eq!(
            parse_delimited_pair(&["^ber,title".to_string()]),
            Some(("^ber".to_string(), "title".to_string()))
        );
        assert_eq!(parse_delimited_pair(&["nocomma".to_string()]), None);
        assert_eq!(parse_delimited_pair(&["".to_string()]), None);
        assert_eq!(parse_delimited_pair(&[]), None);
    }
}
