//! `/v3/api-docs`: the static OpenAPI document, served like springdoc's endpoint
//! (`writer-with-order-by-keys`, so the document is served key-sorted).

use crate::state::AppState;
use axum::extract::State;
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{routing, Router};

const OPENAPI_JSON: &str = include_str!("../../resources/openapi.json");

pub fn router() -> Router<AppState> {
    Router::new().route("/v3/api-docs", routing::get(api_docs))
}

async fn api_docs(State(_state): State<AppState>) -> Response {
    (
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        )],
        OPENAPI_JSON,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    #[tokio::test]
    async fn serves_openapi_document() {
        let state = crate::api::collections::tests::test_state();
        let response = router()
            .with_state(state)
            .oneshot(
                Request::builder()
                    .uri("/v3/api-docs")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["openapi"], "3.1.0");
        assert!(json["paths"].as_object().unwrap().len() > 100);
    }
}
