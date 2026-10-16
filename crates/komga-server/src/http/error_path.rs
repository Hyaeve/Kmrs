//! Fixes up the `path` field of error JSON: `ApiError` serializes with path left empty and a marker header set;
//! here it is uniformly replaced with the real request path and the marker is removed.

use axum::extract::Request;
use axum::middleware::Next;
use axum::response::Response;

pub const ERROR_MARKER: &str = "x-komga-error-body";

pub async fn error_path_middleware(request: Request, next: Next) -> Response {
    let path = request.uri().path().to_string();
    let response = next.run(request).await;
    if !response.headers().contains_key(ERROR_MARKER) {
        return response;
    }
    let (mut parts, body) = response.into_parts();
    parts.headers.remove(ERROR_MARKER);
    let bytes = axum::body::to_bytes(body, usize::MAX)
        .await
        .unwrap_or_default();
    let bytes = match serde_json::from_slice::<serde_json::Value>(&bytes) {
        Ok(mut json) => {
            if let Some(obj) = json.as_object_mut() {
                obj.insert("path".to_string(), serde_json::Value::String(path));
            }
            serde_json::to_vec(&json).unwrap_or_default()
        }
        Err(_) => bytes.to_vec(),
    };
    Response::from_parts(parts, axum::body::Body::from(bytes))
}
