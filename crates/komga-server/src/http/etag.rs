//! Middleware equivalent of `ShallowEtagHeaderFilter`:
//! for GET responses under `/api/*`, `/opds/*`, `/kobo/*` that are 2xx and not no-store,
//! generates a strong ETag from the body MD5 (`"0<md5hex>"`); an `If-None-Match` hit → 304.
//! File download paths (`*/file/**` of books/series/readlists/kobo) are excluded, and so
//! is the komf proxy namespace (admin-only relays of a live service's dynamic payloads,
//! where a body-hash ETag only costs a full buffer).
//! A response that already carries an ETag (a handler-computed deep ETag, see
//! `stored_thumbnail_response`) passes through untouched.

use crate::error::ApiError;
use axum::body::Body;
use axum::extract::Request;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::Response;
use http_body::Body as _;

pub async fn etag_middleware(request: Request, next: Next) -> Response {
    let path = request.uri().path().to_string();
    let applies =
        (path.starts_with("/api/") || path.starts_with("/opds/") || path.starts_with("/kobo/"))
            && !path.starts_with("/api/v1/komf/")
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
    if response.headers().contains_key(axum::http::header::ETAG) {
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
    // an unknown-size body has no complete form to hash; when it is a stream that never
    // ends (SSE relay), buffering it would hang the response forever
    if response.body().size_hint().exact().is_none() {
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
        if matches_if_none_match(&inm, &etag) {
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

pub fn matches_if_none_match(inm: &str, etag: &str) -> bool {
    inm.split(',').map(str::trim).any(|candidate| {
        candidate == etag
            || candidate == "*"
            || candidate.trim_start_matches("W/") == etag.trim_start_matches("W/")
    })
}

/// Deep ETag for a stored thumbnail row: it changes exactly when the served bytes
/// change, so an `If-None-Match` hit is answered without hashing the body.
/// Blob rows are content-immutable per id (in-place updates only re-point the row at
/// another book/series); URL-backed rows can change under the same URL, so the file's
/// mtime and size go in — a stat is cheap next to a full read. Returns None when the
/// file cannot be stat'ed; the middleware's shallow ETag then still applies.
pub fn stored_thumbnail_etag(
    id: &str,
    file_size: i64,
    blob_present: bool,
    url: Option<&str>,
) -> Option<String> {
    if blob_present {
        return Some(format!("\"krs-b:{id}:{file_size}\""));
    }
    let path = komga_core::dto::url_to_file_path(url?);
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_nanos();
    Some(format!("\"krs-f:{id}:{mtime}:{}\"", meta.len()))
}

/// Serves a stored thumbnail with its deep ETag: an `If-None-Match` hit short-circuits
/// to 304 and `bytes` is never called. With `enabled` off no ETag is set here and the
/// middleware falls back to the body hash, matching the Java behavior.
pub fn stored_thumbnail_response(
    enabled: bool,
    request_headers: &HeaderMap,
    id: &str,
    file_size: i64,
    blob_present: bool,
    url: Option<&str>,
    bytes: impl FnOnce() -> Result<Option<Vec<u8>>, ApiError>,
) -> Result<Response, ApiError> {
    let etag = enabled
        .then(|| stored_thumbnail_etag(id, file_size, blob_present, url))
        .flatten();
    if let Some(etag) = &etag {
        let inm = request_headers
            .get(axum::http::header::IF_NONE_MATCH)
            .and_then(|v| v.to_str().ok());
        if inm.is_some_and(|inm| matches_if_none_match(inm, etag)) {
            let mut response = Response::new(Body::empty());
            *response.status_mut() = StatusCode::NOT_MODIFIED;
            response.headers_mut().insert(
                axum::http::header::ETAG,
                HeaderValue::from_str(etag).unwrap(),
            );
            return Ok(response);
        }
    }
    let Some(bytes) = bytes()? else {
        return Err(ApiError::not_found(""));
    };
    let mut response = Response::new(Body::from(bytes));
    response.headers_mut().insert(
        axum::http::header::CONTENT_TYPE,
        HeaderValue::from_static("image/jpeg"),
    );
    if let Some(etag) = etag {
        response.headers_mut().insert(
            axum::http::header::ETAG,
            HeaderValue::from_str(&etag).unwrap(),
        );
    }
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An SSE-style body never ends: the middleware must pass it through instead of
    /// buffering it for an ETag (the komf job-events relay hangs on `to_bytes` otherwise).
    /// The path stays outside the komf namespace so the size gate is what is exercised.
    #[tokio::test]
    async fn streaming_body_is_not_buffered_for_etag() {
        let app = axum::Router::new()
            .route(
                "/api/v1/events",
                axum::routing::get(|| async {
                    Response::builder()
                        .header(axum::http::header::CONTENT_TYPE, "text/event-stream")
                        .header(axum::http::header::CACHE_CONTROL, "no-cache")
                        .body(Body::from_stream(futures_util::stream::pending::<
                            Result<String, std::convert::Infallible>,
                        >()))
                        .unwrap()
                }),
            )
            .layer(axum::middleware::from_fn(etag_middleware));

        let request = axum::http::Request::get("/api/v1/events")
            .body(Body::empty())
            .unwrap();
        let response = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            tower::ServiceExt::oneshot(app, request),
        )
        .await
        .expect("a streaming response must come back, not be buffered forever")
        .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(!response.headers().contains_key(axum::http::header::ETAG));
    }

    /// The komf proxy namespace is out of ETag scope even for bounded JSON.
    #[tokio::test]
    async fn komf_namespace_gets_no_etag() {
        let app = axum::Router::new()
            .route(
                "/api/v1/komf/jobs",
                axum::routing::get(|| async { axum::Json(serde_json::json!({"content": []})) }),
            )
            .layer(axum::middleware::from_fn(etag_middleware));

        let request = axum::http::Request::get("/api/v1/komf/jobs")
            .body(Body::empty())
            .unwrap();
        let response = tower::ServiceExt::oneshot(app, request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(!response.headers().contains_key(axum::http::header::ETAG));
    }

    /// Bounded bodies keep their shallow ETag (the size gate must not disable it).
    #[tokio::test]
    async fn bounded_body_still_gets_etag() {
        let app = axum::Router::new()
            .route(
                "/api/v1/data",
                axum::routing::get(|| async { axum::Json(serde_json::json!({"a": 1})) }),
            )
            .layer(axum::middleware::from_fn(etag_middleware));

        let request = axum::http::Request::get("/api/v1/data")
            .body(Body::empty())
            .unwrap();
        let response = tower::ServiceExt::oneshot(app, request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let etag = response
            .headers()
            .get(axum::http::header::ETAG)
            .expect("bounded responses keep their ETag");
        assert!(etag.to_str().unwrap().starts_with("\"0"));
    }

    /// A matching If-None-Match short-circuits to 304 with an empty body.
    #[tokio::test]
    async fn if_none_match_hit_returns_304() {
        fn app() -> axum::Router {
            axum::Router::new()
                .route(
                    "/api/v1/data",
                    axum::routing::get(|| async { axum::Json(serde_json::json!({"a": 1})) }),
                )
                .layer(axum::middleware::from_fn(etag_middleware))
        }

        let first = tower::ServiceExt::oneshot(
            app(),
            axum::http::Request::get("/api/v1/data")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        let etag = first
            .headers()
            .get(axum::http::header::ETAG)
            .unwrap()
            .clone();

        let second = tower::ServiceExt::oneshot(
            app(),
            axum::http::Request::get("/api/v1/data")
                .header(axum::http::header::IF_NONE_MATCH, etag)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(second.status(), StatusCode::NOT_MODIFIED);
        let body = axum::body::to_bytes(second.into_body(), usize::MAX)
            .await
            .unwrap();
        assert!(body.is_empty());
    }

    #[test]
    fn if_none_match_variants() {
        let etag = "\"krs-b:t1:42\"";
        assert!(matches_if_none_match(etag, etag));
        assert!(matches_if_none_match("*", etag));
        assert!(matches_if_none_match(&format!("W/{etag}"), etag));
        assert!(matches_if_none_match(&format!("\"other\", {etag}"), etag));
        assert!(!matches_if_none_match("\"krs-b:t1:43\"", etag));
    }

    #[test]
    fn blob_etag_is_id_scoped() {
        let etag = stored_thumbnail_etag("t1", 42, true, None).unwrap();
        assert_eq!(etag, "\"krs-b:t1:42\"");
        // no blob and no url: nothing to key on
        assert!(stored_thumbnail_etag("t1", 42, false, None).is_none());
    }

    #[test]
    fn file_etag_follows_the_file_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("cover.jpg");
        std::fs::write(&file, b"v1").unwrap();
        let url = komga_media::scanner::path_to_url(&file);
        let etag = stored_thumbnail_etag("t1", 2, false, Some(&url)).unwrap();
        // the full entity-tag is quoted and holds only id, mtime nanos and size
        let inner = etag
            .strip_prefix("\"krs-f:t1:")
            .and_then(|s| s.strip_suffix('"'))
            .unwrap();
        let mut parts = inner.split(':');
        assert!(parts.next().unwrap().parse::<u128>().is_ok());
        assert_eq!(parts.next(), Some("2"));
        assert_eq!(parts.next(), None);

        std::fs::write(&file, b"v2-longer").unwrap();
        let etag2 = stored_thumbnail_etag("t1", 2, false, Some(&url)).unwrap();
        assert_ne!(etag, etag2);

        std::fs::remove_file(&file).unwrap();
        assert!(stored_thumbnail_etag("t1", 2, false, Some(&url)).is_none());
    }
}
