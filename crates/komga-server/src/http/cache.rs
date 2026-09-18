//! Equivalent of `WebContentInterceptor`: `/api/**` and `/opds/**` uniformly get
//! `Cache-Control: private, max-age=0, must-revalidate` (and Spring's built-in cacheControl header is disabled).
//! The 1h exception for collection/readlist thumbnails is overridden at the specific endpoints.

use axum::extract::Request;
use axum::http::HeaderValue;
use axum::middleware::Next;
use axum::response::Response;

pub async fn cache_control_middleware(request: Request, next: Next) -> Response {
    let path = request.uri().path().to_string();
    let applies = path.starts_with("/api/") || path.starts_with("/opds/");
    let mut response = next.run(request).await;
    if applies {
        response.headers_mut().insert(
            axum::http::header::CACHE_CONTROL,
            HeaderValue::from_static("private, max-age=0, must-revalidate"),
        );
    }
    response
}
