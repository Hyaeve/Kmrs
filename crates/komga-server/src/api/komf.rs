//! kmrs-private komf integration management and proxy (admin only). Not part of the
//! Komga API surface, so it stays out of the OpenAPI spec.

use crate::auth::RequireAuth;
use crate::dto::komf::{
    KomfIdentifyRequestDto, KomfIntegrationDto, KomfIntegrationUpdateDto,
    KomfMetadataJobResponseDto, KomfSeriesSearchResultDto,
};
use crate::error::{ApiError, Violation};
use crate::service::komf::{self, KomfClient};
use crate::state::AppState;
use axum::extract::{Path, RawQuery, State};
use axum::http::{Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{routing, Json, Router};
use komga_db::dao::komf_integration::{KomfIntegration, KomfIntegrationDao, KomfIntegrationState};
use serde::de::DeserializeOwned;
use serde::Serialize;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/komf/integration",
            routing::get(get_integration)
                .put(put_integration)
                .delete(delete_integration),
        )
        .route("/api/v1/komf/providers", routing::get(get_providers))
        .route("/api/v1/komf/search", routing::get(search_series))
        .route("/api/v1/komf/identify", routing::post(identify_series))
        .route(
            "/api/v1/komf/match/library/{libraryId}",
            routing::post(match_library),
        )
        .route(
            "/api/v1/komf/match/library/{libraryId}/series/{seriesId}",
            routing::post(match_series),
        )
        .route(
            "/api/v1/komf/reset/library/{libraryId}",
            routing::post(reset_library),
        )
        .route(
            "/api/v1/komf/reset/library/{libraryId}/series/{seriesId}",
            routing::post(reset_series),
        )
        .route(
            "/api/v1/komf/config",
            routing::get(get_config).patch(patch_config),
        )
}

async fn get_integration(
    State(state): State<AppState>,
    auth: RequireAuth,
) -> Result<Json<KomfIntegrationDto>, ApiError> {
    auth.0.require_admin()?;
    Ok(Json(integration_dto(&state).await?))
}

async fn put_integration(
    State(state): State<AppState>,
    auth: RequireAuth,
    Json(body): Json<KomfIntegrationUpdateDto>,
) -> Result<Json<KomfIntegrationDto>, ApiError> {
    auth.0.require_admin()?;
    let mut violations = vec![];
    let url = http_url("url", body.url.as_deref()).unwrap_or_else(|v| {
        violations.push(v);
        String::new()
    });
    let base_url = http_url("baseUrl", body.base_url.as_deref()).unwrap_or_else(|v| {
        violations.push(v);
        String::new()
    });
    if !violations.is_empty() {
        return Err(ApiError::Violations(violations));
    }
    KomfIntegrationDao::new(state.kmrs_db.clone()).upsert(&url, &base_url)?;
    // provisioning failures are recorded on the integration row and surface in the DTO
    let _ = komf::provision(&state, &auth.0.user.id).await;
    Ok(Json(integration_dto(&state).await?))
}

async fn delete_integration(
    State(state): State<AppState>,
    auth: RequireAuth,
) -> Result<StatusCode, ApiError> {
    auth.0.require_admin()?;
    komf::disconnect(&state)
        .await
        .map_err(|e| ApiError::Internal(format!("{e:#}")))?;
    Ok(StatusCode::NO_CONTENT)
}

fn http_url(field: &str, value: Option<&str>) -> Result<String, Violation> {
    let trimmed = value.unwrap_or("").trim();
    if trimmed.is_empty() {
        return Err(Violation {
            field_name: field.into(),
            message: "must not be blank".into(),
        });
    }
    if !(trimmed.starts_with("http://") || trimmed.starts_with("https://")) {
        return Err(Violation {
            field_name: field.into(),
            message: "must start with http:// or https://".into(),
        });
    }
    Ok(trimmed.to_string())
}

async fn integration_dto(state: &AppState) -> Result<KomfIntegrationDto, ApiError> {
    let Some(row) = KomfIntegrationDao::new(state.kmrs_db.clone()).get()? else {
        // unconfigured: surface the config-file preset so the admin UI can pre-fill the form
        return Ok(KomfIntegrationDto {
            configured: false,
            url: state.config.komf_url.clone(),
            base_url: state.config.komf_base_url.clone(),
            state: None,
            last_error: None,
            komf_reachable: false,
        });
    };
    let komf_reachable = KomfClient::new(&row.url).health().await.is_ok();
    Ok(KomfIntegrationDto {
        configured: true,
        url: Some(row.url),
        base_url: Some(row.base_url),
        state: Some(row.state.as_str().to_string()),
        last_error: row.last_error,
        komf_reachable,
    })
}

const KOMF_METADATA: &str = "/api/komga/metadata";

async fn get_providers(
    State(state): State<AppState>,
    auth: RequireAuth,
    RawQuery(query): RawQuery,
) -> Result<Response, ApiError> {
    auth.0.require_admin()?;
    let response = send_metadata(&state, Method::GET, "/providers", query, None).await?;
    json_or_passthrough::<Vec<String>>(response).await
}

async fn search_series(
    State(state): State<AppState>,
    auth: RequireAuth,
    RawQuery(query): RawQuery,
) -> Result<Response, ApiError> {
    auth.0.require_admin()?;
    let response = send_metadata(&state, Method::GET, "/search", query, None).await?;
    json_or_passthrough::<Vec<KomfSeriesSearchResultDto>>(response).await
}

async fn identify_series(
    State(state): State<AppState>,
    auth: RequireAuth,
    Json(body): Json<KomfIdentifyRequestDto>,
) -> Result<Response, ApiError> {
    auth.0.require_admin()?;
    let body = serde_json::to_value(&body).map_err(|e| ApiError::Internal(e.to_string()))?;
    let response = send_metadata(&state, Method::POST, "/identify", None, Some(body)).await?;
    json_or_passthrough::<KomfMetadataJobResponseDto>(response).await
}

async fn match_library(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path(library_id): Path<String>,
) -> Result<Response, ApiError> {
    auth.0.require_admin()?;
    let response = send_metadata(
        &state,
        Method::POST,
        &format!("/match/library/{library_id}"),
        None,
        None,
    )
    .await?;
    Ok(empty_or_passthrough(response).await)
}

async fn match_series(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path((library_id, series_id)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    auth.0.require_admin()?;
    let response = send_metadata(
        &state,
        Method::POST,
        &format!("/match/library/{library_id}/series/{series_id}"),
        None,
        None,
    )
    .await?;
    json_or_passthrough::<KomfMetadataJobResponseDto>(response).await
}

async fn reset_library(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path(library_id): Path<String>,
    RawQuery(query): RawQuery,
) -> Result<Response, ApiError> {
    auth.0.require_admin()?;
    let response = send_metadata(
        &state,
        Method::POST,
        &format!("/reset/library/{library_id}"),
        query,
        None,
    )
    .await?;
    Ok(empty_or_passthrough(response).await)
}

async fn reset_series(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path((library_id, series_id)): Path<(String, String)>,
    RawQuery(query): RawQuery,
) -> Result<Response, ApiError> {
    auth.0.require_admin()?;
    let response = send_metadata(
        &state,
        Method::POST,
        &format!("/reset/library/{library_id}/series/{series_id}"),
        query,
        None,
    )
    .await?;
    Ok(empty_or_passthrough(response).await)
}

/// The config schema is komf's own, so the proxy stays transparent and relays the
/// raw JSON instead of modeling it.
async fn get_config(
    State(state): State<AppState>,
    auth: RequireAuth,
) -> Result<Response, ApiError> {
    auth.0.require_admin()?;
    let row = connected_integration(&state)?;
    let response = KomfClient::new(&row.url)
        .proxy_config(Method::GET, None)
        .await
        .map_err(komf_unreachable)?;
    json_or_passthrough::<serde_json::Value>(response).await
}

/// The komga connection fields are owned by the integration (it provisions and
/// rotates the API key), so admin edits must go through the integration endpoint.
async fn patch_config(
    State(state): State<AppState>,
    auth: RequireAuth,
    Json(body): Json<serde_json::Value>,
) -> Result<Response, ApiError> {
    auth.0.require_admin()?;
    if let Some(komga) = body.get("komga").and_then(|v| v.as_object()) {
        let violations: Vec<Violation> = ["baseUri", "komgaUser", "komgaApiKey", "komgaPassword"]
            .into_iter()
            .filter(|key| komga.contains_key(*key))
            .map(|key| Violation {
                field_name: format!("komga.{key}"),
                message: "is managed by /api/v1/komf/integration".into(),
            })
            .collect();
        if !violations.is_empty() {
            return Err(ApiError::Violations(violations));
        }
    }
    let row = connected_integration(&state)?;
    let response = KomfClient::new(&row.url)
        .proxy_config(Method::PATCH, Some(&body))
        .await
        .map_err(komf_unreachable)?;
    Ok(empty_or_passthrough(response).await)
}

/// Proxying only makes sense once provisioning succeeded; anything earlier is a
/// conflict with the integration lifecycle, not a komf failure.
fn connected_integration(state: &AppState) -> Result<KomfIntegration, ApiError> {
    match KomfIntegrationDao::new(state.kmrs_db.clone()).get()? {
        None => Err(ApiError::conflict("komf integration is not configured")),
        Some(row) if row.state != KomfIntegrationState::Connected => {
            Err(ApiError::conflict(format!(
                "komf integration is not connected (state: {})",
                row.state.as_str()
            )))
        }
        Some(row) => Ok(row),
    }
}

async fn send_metadata(
    state: &AppState,
    method: Method,
    path: &str,
    query: Option<String>,
    body: Option<serde_json::Value>,
) -> Result<reqwest::Response, ApiError> {
    let row = connected_integration(state)?;
    KomfClient::new(&row.url)
        .proxy_metadata(
            method,
            &format!("{KOMF_METADATA}{path}"),
            query.as_deref(),
            body.as_ref(),
        )
        .await
        .map_err(komf_unreachable)
}

fn komf_unreachable(e: anyhow::Error) -> ApiError {
    ApiError::bad_gateway(format!("failed to reach komf: {e:#}"))
}

/// A 2xx carries komf's JSON, relayed typed; anything else is komf's own error
/// contract and goes back untouched.
async fn json_or_passthrough<T: DeserializeOwned + Serialize>(
    response: reqwest::Response,
) -> Result<Response, ApiError> {
    if !response.status().is_success() {
        return Ok(passthrough(response).await);
    }
    let dto = response.json::<T>().await.map_err(|e| {
        ApiError::bad_gateway(format!("komf returned an unexpected response: {e:#}"))
    })?;
    Ok(Json(dto).into_response())
}

/// Success statuses with no meaningful body (202, 204) are relayed as-is.
async fn empty_or_passthrough(response: reqwest::Response) -> Response {
    if !response.status().is_success() {
        return passthrough(response).await;
    }
    response.status().into_response()
}

/// komf answered for itself: its status and body go back to the caller untouched.
async fn passthrough(response: reqwest::Response) -> Response {
    let status = response.status();
    let body = response.bytes().await.unwrap_or_default();
    (
        status,
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        body,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::libraries::test_support::{insert_api_key, insert_user, test_config, TestApp};
    use axum::routing::{get, patch};
    use std::sync::{Arc, Mutex};

    /// Minimal komf: health at the root, 204 on PATCH /api/config, captured bodies.
    async fn serve_komf() -> (String, Arc<Mutex<Vec<serde_json::Value>>>) {
        let patches = Arc::new(Mutex::new(vec![]));
        let app = {
            let patches = patches.clone();
            Router::new().route("/", get(|| async { "komf-rs" })).route(
                "/api/config",
                patch(move |Json(body): Json<serde_json::Value>| {
                    let patches = patches.clone();
                    async move {
                        patches.lock().unwrap().push(body);
                        StatusCode::NO_CONTENT
                    }
                }),
            )
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (format!("http://{addr}"), patches)
    }

    #[tokio::test]
    async fn endpoints_are_admin_only() {
        let app = TestApp::new(router());
        let user = insert_user(&app.state.db, "user@x.c", false, true, &[]);
        insert_api_key(&app.state.db, &user, "k-user");

        let (status, _) = app.get_json("/api/v1/komf/integration", "k-user").await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _) = app
            .request_json(
                "PUT",
                "/api/v1/komf/integration",
                "k-user",
                Some(
                    serde_json::json!({"url": "http://komf:8085", "baseUrl": "http://kmrs:25600"}),
                ),
            )
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _) = app
            .request_json("DELETE", "/api/v1/komf/integration", "k-user", None)
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn get_reports_unconfigured() {
        let app = TestApp::new(router());
        let admin = insert_user(&app.state.db, "admin@x.c", true, true, &[]);
        insert_api_key(&app.state.db, &admin, "k-admin");

        let (status, body) = app.get_json("/api/v1/komf/integration", "k-admin").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            body,
            serde_json::json!({"configured": false, "komfReachable": false})
        );
    }

    #[tokio::test]
    async fn get_returns_preset_as_form_defaults_when_unconfigured() {
        let mut config = test_config();
        config.komf_url = Some("http://komf:8085".into());
        config.komf_base_url = Some("http://kmrs:25600".into());
        let app = TestApp::with_config(router(), config);
        let admin = insert_user(&app.state.db, "admin@x.c", true, true, &[]);
        insert_api_key(&app.state.db, &admin, "k-admin");

        let (status, body) = app.get_json("/api/v1/komf/integration", "k-admin").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            body,
            serde_json::json!({
                "configured": false,
                "url": "http://komf:8085",
                "baseUrl": "http://kmrs:25600",
                "komfReachable": false
            })
        );
    }

    #[tokio::test]
    async fn put_validates_urls() {
        let app = TestApp::new(router());
        let admin = insert_user(&app.state.db, "admin@x.c", true, true, &[]);
        insert_api_key(&app.state.db, &admin, "k-admin");

        let (status, body) = app
            .request_json(
                "PUT",
                "/api/v1/komf/integration",
                "k-admin",
                Some(serde_json::json!({"url": "", "baseUrl": "ftp://kmrs"})),
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["violations"].as_array().unwrap().len(), 2);
        assert!(KomfIntegrationDao::new(app.state.kmrs_db.clone())
            .get()
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn put_provisions_and_delete_tears_down() {
        let app = TestApp::new(router());
        let admin = insert_user(&app.state.db, "admin@x.c", true, true, &[]);
        insert_api_key(&app.state.db, &admin, "k-admin");
        let (komf_url, patches) = serve_komf().await;

        let (status, body) = app
            .request_json(
                "PUT",
                "/api/v1/komf/integration",
                "k-admin",
                Some(serde_json::json!({"url": komf_url, "baseUrl": "http://kmrs:25600"})),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["configured"], true);
        assert_eq!(body["state"], "connected");
        assert_eq!(body["komfReachable"], true);
        assert_eq!(patches.lock().unwrap().len(), 1);

        let (status, _) = app
            .request_json("DELETE", "/api/v1/komf/integration", "k-admin", None)
            .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert!(KomfIntegrationDao::new(app.state.kmrs_db.clone())
            .get()
            .unwrap()
            .is_none());

        let (status, body) = app.get_json("/api/v1/komf/integration", "k-admin").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["configured"], false);
    }

    #[derive(Debug)]
    struct CapturedRequest {
        method: String,
        path: String,
        query: Option<String>,
        body: Option<serde_json::Value>,
    }

    struct MockKomf {
        url: String,
        requests: Arc<Mutex<Vec<CapturedRequest>>>,
    }

    impl MockKomf {
        fn captured(&self) -> std::sync::MutexGuard<'_, Vec<CapturedRequest>> {
            self.requests.lock().unwrap()
        }
    }

    /// komf stand-in for the proxy endpoints: records every request and answers with
    /// komf's real response shapes. Anything mentioning "broken" gets komf's 422.
    async fn serve_komf_proxy() -> MockKomf {
        let requests = Arc::new(Mutex::new(vec![]));
        let app = Router::new().fallback({
            let requests = requests.clone();
            move |request: axum::extract::Request| {
                let requests = requests.clone();
                async move {
                    let method = request.method().as_str().to_string();
                    let path = request.uri().path().to_string();
                    let query = request.uri().query().map(str::to_string);
                    let bytes = axum::body::to_bytes(request.into_body(), usize::MAX)
                        .await
                        .unwrap();
                    let body = serde_json::from_slice(&bytes).ok();
                    requests.lock().unwrap().push(CapturedRequest {
                        method: method.clone(),
                        path: path.clone(),
                        query: query.clone(),
                        body,
                    });
                    let broken = path.contains("broken")
                        || query.as_deref().is_some_and(|q| q.contains("broken"));
                    if broken {
                        return (
                            StatusCode::UNPROCESSABLE_ENTITY,
                            Json(serde_json::json!({"message": "komf broke"})),
                        )
                            .into_response();
                    }
                    match (method.as_str(), path.as_str()) {
                        ("GET", "/api/komga/metadata/providers") => {
                            Json(serde_json::json!(["MANGA_BAKA", "MANGADEX"])).into_response()
                        }
                        ("GET", "/api/komga/metadata/search") => Json(serde_json::json!([{
                            "url": "https://mangadex.org/title/abc",
                            "imageUrl": "https://mangadex.org/covers/abc.jpg",
                            "title": "Solo Leveling",
                            "provider": "MANGADEX",
                            "resultId": "abc",
                            "mediaType": "MANGA",
                            "language": "en"
                        }]))
                        .into_response(),
                        ("POST", "/api/komga/metadata/identify") => {
                            Json(serde_json::json!({"id": "job-1"})).into_response()
                        }
                        ("GET", "/api/config") => Json(serde_json::json!({
                            "komga": {
                                "baseUri": "http://kmrs:25600",
                                "eventListener": {"enabled": true}
                            },
                            "server": {"port": 8085}
                        }))
                        .into_response(),
                        ("PATCH", "/api/config") => StatusCode::NO_CONTENT.into_response(),
                        _ if method == "POST"
                            && path.starts_with("/api/komga/metadata/match/")
                            && path.contains("/series/") =>
                        {
                            Json(serde_json::json!({"id": "job-2"})).into_response()
                        }
                        _ if method == "POST" && path.starts_with("/api/komga/metadata/match/") => {
                            StatusCode::ACCEPTED.into_response()
                        }
                        _ if method == "POST" && path.starts_with("/api/komga/metadata/reset/") => {
                            StatusCode::NO_CONTENT.into_response()
                        }
                        _ => StatusCode::NOT_FOUND.into_response(),
                    }
                }
            }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        MockKomf {
            url: format!("http://{addr}"),
            requests,
        }
    }

    fn admin_app() -> TestApp {
        let app = TestApp::new(router());
        let admin = insert_user(&app.state.db, "admin@x.c", true, true, &[]);
        insert_api_key(&app.state.db, &admin, "k-admin");
        app
    }

    fn seed_connected(state: &AppState, url: &str) {
        let dao = KomfIntegrationDao::new(state.kmrs_db.clone());
        dao.upsert(url, "http://kmrs:25600").unwrap();
        dao.mark_connected("user-1", "key-1").unwrap();
    }

    #[tokio::test]
    async fn proxy_endpoints_are_admin_only() {
        let app = TestApp::new(router());
        let user = insert_user(&app.state.db, "user@x.c", false, true, &[]);
        insert_api_key(&app.state.db, &user, "k-user");

        let (status, _) = app.get_json("/api/v1/komf/providers", "k-user").await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _) = app.get_json("/api/v1/komf/search?name=x", "k-user").await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _) = app.get_json("/api/v1/komf/config", "k-user").await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _) = app
            .request_json(
                "POST",
                "/api/v1/komf/identify",
                "k-user",
                Some(serde_json::json!({"seriesId": "s", "provider": "MANGADEX", "providerSeriesId": "p"})),
            )
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _) = app
            .request_json(
                "POST",
                "/api/v1/komf/match/library/l/series/s",
                "k-user",
                None,
            )
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _) = app
            .request_json("POST", "/api/v1/komf/reset/library/l", "k-user", None)
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _) = app
            .request_json(
                "PATCH",
                "/api/v1/komf/config",
                "k-user",
                Some(serde_json::json!({})),
            )
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn proxy_without_integration_is_conflict() {
        let app = admin_app();

        let (status, _) = app.get_json("/api/v1/komf/providers", "k-admin").await;
        assert_eq!(status, StatusCode::CONFLICT);
        let (status, _) = app.get_json("/api/v1/komf/config", "k-admin").await;
        assert_eq!(status, StatusCode::CONFLICT);
        let (status, _) = app
            .request_json("POST", "/api/v1/komf/match/library/l", "k-admin", None)
            .await;
        assert_eq!(status, StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn proxy_with_unconnected_integration_is_conflict() {
        let app = admin_app();
        let dao = KomfIntegrationDao::new(app.state.kmrs_db.clone());
        dao.upsert("http://komf:8085", "http://kmrs:25600").unwrap();

        let (status, body) = app.get_json("/api/v1/komf/providers", "k-admin").await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert!(body["message"].as_str().unwrap().contains("pending"));

        dao.mark_error("boom").unwrap();
        let (status, body) = app.get_json("/api/v1/komf/providers", "k-admin").await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert!(body["message"].as_str().unwrap().contains("error"));
    }

    #[tokio::test]
    async fn providers_forwards_query_and_returns_list() {
        let app = admin_app();
        let komf = serve_komf_proxy().await;
        seed_connected(&app.state, &komf.url);

        let (status, body) = app
            .get_json("/api/v1/komf/providers?libraryId=lib-1", "k-admin")
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, serde_json::json!(["MANGA_BAKA", "MANGADEX"]));

        let captured = komf.captured();
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0].method, "GET");
        assert_eq!(captured[0].path, "/api/komga/metadata/providers");
        assert_eq!(captured[0].query.as_deref(), Some("libraryId=lib-1"));
    }

    #[tokio::test]
    async fn search_forwards_query_and_returns_results() {
        let app = admin_app();
        let komf = serve_komf_proxy().await;
        seed_connected(&app.state, &komf.url);

        let (status, body) = app
            .get_json("/api/v1/komf/search?name=Solo&libraryId=lib-1", "k-admin")
            .await;
        assert_eq!(status, StatusCode::OK);
        let results = body.as_array().unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["title"], "Solo Leveling");
        assert_eq!(results[0]["provider"], "MANGADEX");
        assert_eq!(results[0]["resultId"], "abc");
        assert_eq!(results[0]["mediaType"], "MANGA");

        let captured = komf.captured();
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0].path, "/api/komga/metadata/search");
        assert_eq!(
            captured[0].query.as_deref(),
            Some("name=Solo&libraryId=lib-1")
        );
    }

    #[tokio::test]
    async fn identify_forwards_body_and_returns_job() {
        let app = admin_app();
        let komf = serve_komf_proxy().await;
        seed_connected(&app.state, &komf.url);
        let identify = serde_json::json!({
            "libraryId": "lib-1",
            "seriesId": "ser-1",
            "provider": "MANGADEX",
            "providerSeriesId": "abc"
        });

        let (status, body) = app
            .request_json(
                "POST",
                "/api/v1/komf/identify",
                "k-admin",
                Some(identify.clone()),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, serde_json::json!({"id": "job-1"}));

        let captured = komf.captured();
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0].path, "/api/komga/metadata/identify");
        assert_eq!(captured[0].body, Some(identify));
    }

    #[tokio::test]
    async fn match_forwards_to_same_path() {
        let app = admin_app();
        let komf = serve_komf_proxy().await;
        seed_connected(&app.state, &komf.url);

        let (status, body) = app
            .request_json(
                "POST",
                "/api/v1/komf/match/library/lib-1/series/ser-1",
                "k-admin",
                None,
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, serde_json::json!({"id": "job-2"}));

        let (status, _) = app
            .request_json("POST", "/api/v1/komf/match/library/lib-1", "k-admin", None)
            .await;
        assert_eq!(status, StatusCode::ACCEPTED);

        let captured = komf.captured();
        assert_eq!(captured.len(), 2);
        assert_eq!(
            captured[0].path,
            "/api/komga/metadata/match/library/lib-1/series/ser-1"
        );
        assert_eq!(captured[1].path, "/api/komga/metadata/match/library/lib-1");
    }

    #[tokio::test]
    async fn reset_forwards_to_same_path_with_query() {
        let app = admin_app();
        let komf = serve_komf_proxy().await;
        seed_connected(&app.state, &komf.url);

        let (status, _) = app
            .request_json(
                "POST",
                "/api/v1/komf/reset/library/lib-1/series/ser-1?removeComicInfo=true",
                "k-admin",
                None,
            )
            .await;
        assert_eq!(status, StatusCode::NO_CONTENT);

        let (status, _) = app
            .request_json("POST", "/api/v1/komf/reset/library/lib-1", "k-admin", None)
            .await;
        assert_eq!(status, StatusCode::NO_CONTENT);

        let captured = komf.captured();
        assert_eq!(captured.len(), 2);
        assert_eq!(
            captured[0].path,
            "/api/komga/metadata/reset/library/lib-1/series/ser-1"
        );
        assert_eq!(captured[0].query.as_deref(), Some("removeComicInfo=true"));
        assert_eq!(captured[1].path, "/api/komga/metadata/reset/library/lib-1");
        assert_eq!(captured[1].query, None);
    }

    #[tokio::test]
    async fn komf_error_is_passed_through_untouched() {
        let app = admin_app();
        let komf = serve_komf_proxy().await;
        seed_connected(&app.state, &komf.url);

        let (status, body) = app
            .request_json(
                "POST",
                "/api/v1/komf/reset/library/broken-lib",
                "k-admin",
                None,
            )
            .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body, serde_json::json!({"message": "komf broke"}));

        let (status, body) = app
            .get_json("/api/v1/komf/providers?libraryId=broken", "k-admin")
            .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body, serde_json::json!({"message": "komf broke"}));
    }

    #[tokio::test]
    async fn unreachable_komf_is_bad_gateway() {
        let app = admin_app();
        // nothing listens on port 1
        seed_connected(&app.state, "http://127.0.0.1:1");

        let (status, _) = app.get_json("/api/v1/komf/providers", "k-admin").await;
        assert_eq!(status, StatusCode::BAD_GATEWAY);
    }

    #[tokio::test]
    async fn get_config_returns_komf_json_verbatim() {
        let app = admin_app();
        let komf = serve_komf_proxy().await;
        seed_connected(&app.state, &komf.url);

        let (status, body) = app.get_json("/api/v1/komf/config", "k-admin").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["komga"]["baseUri"], "http://kmrs:25600");
        assert_eq!(body["server"]["port"], 8085);

        let captured = komf.captured();
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0].method, "GET");
        assert_eq!(captured[0].path, "/api/config");
    }

    #[tokio::test]
    async fn patch_config_forwards_other_komga_sections() {
        let app = admin_app();
        let komf = serve_komf_proxy().await;
        seed_connected(&app.state, &komf.url);
        let patch = serde_json::json!({
            "komga": {
                "eventListener": {"enabled": false},
                "metadataUpdate": {"default": {"updateModes": ["API"]}}
            }
        });

        let (status, _) = app
            .request_json(
                "PATCH",
                "/api/v1/komf/config",
                "k-admin",
                Some(patch.clone()),
            )
            .await;
        assert_eq!(status, StatusCode::NO_CONTENT);

        let captured = komf.captured();
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0].method, "PATCH");
        assert_eq!(captured[0].path, "/api/config");
        assert_eq!(captured[0].body, Some(patch));
    }

    #[tokio::test]
    async fn patch_config_rejects_connection_fields_without_forwarding() {
        let app = admin_app();
        let komf = serve_komf_proxy().await;
        seed_connected(&app.state, &komf.url);

        for key in ["baseUri", "komgaUser", "komgaApiKey", "komgaPassword"] {
            let (status, body) = app
                .request_json(
                    "PATCH",
                    "/api/v1/komf/config",
                    "k-admin",
                    Some(serde_json::json!({"komga": {key: "x", "eventListener": {"enabled": true}}})),
                )
                .await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "key {key}");
            let violations = body["violations"].as_array().unwrap();
            assert_eq!(violations.len(), 1);
            assert_eq!(violations[0]["fieldName"], format!("komga.{key}"));
        }
        assert!(komf.captured().is_empty());
    }
}
