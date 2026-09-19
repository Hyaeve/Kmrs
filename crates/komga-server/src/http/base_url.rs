//! Base URL construction for OPDS/WebPub links, mirroring
//! `ServletUriComponentsBuilder.fromCurrentContextPath()` with `forward-headers-strategy:
//! framework`: X-Forwarded-Proto/Host take precedence over the request's own scheme and Host;
//! the optional `server.servlet.context-path` setting is appended.

use crate::settings::SettingsProvider;
use axum::http::request::Parts;

/// `{proto}://{host}[/prefix]` without a trailing slash.
pub fn base_url(parts: &Parts, settings: &SettingsProvider) -> String {
    base_url_from_headers(&parts.headers, settings)
}

/// Header-only variant for handlers that extract `HeaderMap` instead of the request parts.
pub fn base_url_from_headers(
    headers: &axum::http::HeaderMap,
    settings: &SettingsProvider,
) -> String {
    let proto = first_header(headers, "x-forwarded-proto")
        .map(|v| v.split(',').next().unwrap().trim().to_string())
        .unwrap_or_else(|| "http".to_string());
    let host = first_header(headers, "x-forwarded-host")
        .map(|v| v.split(',').next().unwrap().trim().to_string())
        .or_else(|| first_header(headers, axum::http::header::HOST.as_str()))
        .unwrap_or_else(|| "localhost".to_string());
    let prefix = settings.get().server_context_path.unwrap_or_default();
    let prefix = prefix.trim_matches('/');
    if prefix.is_empty() {
        format!("{proto}://{host}")
    } else {
        format!("{proto}://{host}/{prefix}")
    }
}

fn first_header(headers: &axum::http::HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
        .filter(|v| !v.is_empty())
}

/// Appends path segments (Spring's `pathSegment`): each segment is percent-encoded.
pub fn path_segment(url: &mut String, segments: &[&str]) {
    for segment in segments {
        if segment.is_empty() {
            continue;
        }
        url.push('/');
        url.push_str(&encode_path_segment(segment));
    }
}

/// Spring's `UriUtils.encodePathSegment`: RFC 3986 path-segment rules (space as %20).
pub fn encode_path_segment(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len());
    for c in segment.chars() {
        let unreserved = c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | '~');
        let sub_delim = matches!(
            c,
            '!' | '$' | '&' | '\'' | '(' | ')' | '*' | '+' | ',' | ';' | '='
        );
        if unreserved || sub_delim || c == ':' || c == '@' {
            out.push(c);
        } else {
            let mut buf = [0u8; 4];
            for b in c.encode_utf8(&mut buf).as_bytes() {
                out.push_str(&format!("%{b:02X}"));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_segments_encoding() {
        let mut url = "http://h".to_string();
        path_segment(&mut url, &["opds", "v2", "series/a b", "page#1"]);
        assert_eq!(url, "http://h/opds/v2/series%2Fa%20b/page%231");
    }
}
