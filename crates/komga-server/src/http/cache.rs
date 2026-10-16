//! Equivalent of `WebContentInterceptor`: `/api/**` and `/opds/**` uniformly get
//! `Cache-Control: max-age=0, must-revalidate, private` (Spring's `CacheControl.toString()`
//! renders max-age first, then private, then must-revalidate).
//! The 1h exception for collection/readlist thumbnails is overridden at the specific endpoints.

use axum::extract::Request;
use axum::http::HeaderValue;
use axum::middleware::Next;
use axum::response::Response;

pub async fn cache_control_middleware(request: Request, next: Next) -> Response {
    let path = request.uri().path().to_string();
    let applies = path.starts_with("/api/") || path.starts_with("/opds/");
    let mut response = next.run(request).await;
    // endpoints that set their own Cache-Control (collection/readlist thumbnails use 1h) keep it
    if applies
        && !response
            .headers()
            .contains_key(axum::http::header::CACHE_CONTROL)
    {
        response.headers_mut().insert(
            axum::http::header::CACHE_CONTROL,
            HeaderValue::from_static("max-age=0, must-revalidate, private"),
        );
    }
    response
}
