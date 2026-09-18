//! Middleware equivalent of `ShallowEtagHeaderFilter`:
//! for GET responses under `/api/*`, `/opds/*`, `/kobo/*` that are 2xx and not no-store,
//! generates a strong ETag from the body MD5 (`"0<md5hex>"`); an `If-None-Match` hit → 304.
//! File download paths (`*/file/**` of books/series/readlists/kobo) are excluded.

use axum::extract::Request;
use axum::http::{HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::Response;

pub async fn etag_middleware(request: Request, next: Next) -> Response {
    let path = request.uri().path().to_string();
    let applies =
        (path.starts_with("/api/") || path.starts_with("/opds/") || path.starts_with("/kobo/"))
            && !is_file_download(&path);
    let if_none_match = request
        .headers()
        .get(axum::http::header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let method = request.method().clone();

    let response = next.run(request).await;
    if !applies || method != axum::http::Method::GET || !response.status().is_success() {
        return response;
    }
    let cache_control = response
        .headers()
        .get(axum::http::header::CACHE_CONTROL)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    if cache_control.contains("no-store") {
        return response;
    }

    let (mut parts, body) = response.into_parts();
    let bytes = axum::body::to_bytes(body, usize::MAX)
        .await
        .unwrap_or_default();
    let etag = format!("\"0{:x}\"", md5::compute(&bytes));
    parts.headers.insert(
        axum::http::header::ETAG,
        HeaderValue::from_str(&etag).unwrap(),
    );

    if let Some(inm) = if_none_match {
        let matches = inm.split(',').map(str::trim).any(|candidate| {
            candidate == etag
                || candidate == "*"
                || candidate.trim_start_matches("W/") == etag.trim_start_matches("W/")
        });
        if matches {
            parts.status = StatusCode::NOT_MODIFIED;
            return Response::from_parts(parts, axum::body::Body::empty());
        }
    }
    Response::from_parts(parts, axum::body::Body::from(bytes))
}

fn is_file_download(path: &str) -> bool {
    // books/{id}/file, series/{id}/file, readlists/{id}/file, kobo .../file/epub
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    segments.contains(&"file")
}
