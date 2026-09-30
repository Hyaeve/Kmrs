//! kmrs-private komf integration management and proxy (admin only). Not part of the
//! Komga API surface, so it stays out of the OpenAPI spec.

use crate::auth::RequireAuth;
use crate::dto::komf::{
    KomfIdentifyRequestDto, KomfIntegrationDto, KomfIntegrationUpdateDto, KomfJobDto,
    KomfJobPageDto, KomfMetadataJobResponseDto, KomfSeriesSearchResultDto,
};
use crate::error::{ApiError, Violation};
use crate::service::komf::{self, KomfClient};
use crate::state::AppState;
use axum::body::Body;
use axum::extract::{Path, Query, RawQuery, State};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{routing, Json, Router};
use komga_db::dao::komf_integration::{KomfIntegration, KomfIntegrationDao, KomfIntegrationState};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::convert::Infallible;
use std::time::Duration;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;

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
        .route("/api/v1/komf/jobs", routing::get(get_jobs))
        .route("/api/v1/komf/jobs/events", routing::get(get_jobs_events))
        .route("/api/v1/komf/jobs/{jobId}", routing::get(get_job))
        .route(
            "/api/v1/komf/jobs/{jobId}/events",
            routing::get(get_job_events),
        )
        .route(
            "/api/v1/komf/oauth/{provider}/start",
            routing::get(oauth_start),
        )
        .route(
            "/api/v1/komf/oauth/{provider}/status",
            routing::get(oauth_status),
        )
        .route(
            "/api/v1/komf/oauth/{provider}/logout",
            routing::post(oauth_logout),
        )
        // komf hardcodes this path into the OAuth state's redirectUrl and its relay
        // page only accepts paths ending in /api/oauth/{provider}/callback, so the
        // proxied callback cannot live under /api/v1/komf/
        .route(
            "/api/oauth/{provider}/callback",
            routing::get(oauth_callback),
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

const KOMF_JOBS: &str = "/api/jobs";

async fn get_jobs(
    State(state): State<AppState>,
    auth: RequireAuth,
    RawQuery(query): RawQuery,
) -> Result<Response, ApiError> {
    auth.0.require_admin()?;
    let row = connected_integration(&state)?;
    let response = KomfClient::new(&row.url)
        .proxy_jobs(Method::GET, KOMF_JOBS, query.as_deref())
        .await
        .map_err(komf_unreachable)?;
    json_or_passthrough::<KomfJobPageDto>(response).await
}

async fn get_job(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path(job_id): Path<String>,
) -> Result<Response, ApiError> {
    auth.0.require_admin()?;
    let row = connected_integration(&state)?;
    let response = KomfClient::new(&row.url)
        .proxy_jobs(Method::GET, &format!("{KOMF_JOBS}/{job_id}"), None)
        .await
        .map_err(komf_unreachable)?;
    json_or_passthrough::<KomfJobDto>(response).await
}

/// komf closes the event stream itself when the job finishes, so the raw byte stream
/// is relayed instead of a buffered typed response.
async fn get_job_events(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path(job_id): Path<String>,
) -> Result<Response, ApiError> {
    auth.0.require_admin()?;
    let row = connected_integration(&state)?;
    let response = KomfClient::new(&row.url)
        .proxy_job_events(&format!("{KOMF_JOBS}/{job_id}/events"))
        .await
        .map_err(komf_unreachable)?;
    if !response.status().is_success() {
        return Ok(passthrough(response).await);
    }
    Ok(Response::builder()
        .header(axum::http::header::CONTENT_TYPE, "text/event-stream")
        .header(axum::http::header::CACHE_CONTROL, "no-cache")
        .body(Body::from_stream(response.bytes_stream()))
        .unwrap())
}

/// Bounds the upstream fan-out of one aggregate stream.
const MAX_EVENT_STREAM_IDS: usize = 64;

#[derive(serde::Deserialize)]
struct JobsEventsQuery {
    ids: Option<String>,
}

/// One stream for many jobs: every requested job's komf event stream is relayed with
/// its frames tagged by jobId, so a client tracking several matches keeps a single
/// connection. komf closes a job's stream when the job ends; since the aggregate
/// stays open until every job's stream has closed, that is reported as a synthetic
/// JobStreamClosedEvent. A job whose stream fails to open (komf error, unreachable)
/// gets only the closed event, leaving the final status to the jobs API.
async fn get_jobs_events(
    State(state): State<AppState>,
    auth: RequireAuth,
    Query(query): Query<JobsEventsQuery>,
) -> Result<Response, ApiError> {
    auth.0.require_admin()?;
    let mut ids: Vec<String> = query
        .ids
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .collect();
    let mut seen = std::collections::HashSet::new();
    ids.retain(|id| seen.insert(id.clone()));
    if ids.is_empty() {
        return Err(ApiError::Violations(vec![Violation {
            field_name: "ids".into(),
            message: "must not be blank".into(),
        }]));
    }
    if ids.len() > MAX_EVENT_STREAM_IDS {
        return Err(ApiError::Violations(vec![Violation {
            field_name: "ids".into(),
            message: format!("must contain at most {MAX_EVENT_STREAM_IDS} ids"),
        }]));
    }
    let row = connected_integration(&state)?;
    let (tx, rx) = tokio::sync::mpsc::channel::<String>(256);
    let client = KomfClient::new(&row.url);
    for id in ids {
        let client = client.clone();
        let tx = tx.clone();
        tokio::spawn(async move { relay_job_events(client, id, tx).await });
    }
    drop(tx);
    let stream = ReceiverStream::new(rx).map(Ok::<String, Infallible>);
    Ok(Response::builder()
        .header(axum::http::header::CONTENT_TYPE, "text/event-stream")
        .header(axum::http::header::CACHE_CONTROL, "no-cache")
        .body(Body::from_stream(stream))
        .unwrap())
}

async fn relay_job_events(
    client: KomfClient,
    job_id: String,
    tx: tokio::sync::mpsc::Sender<String>,
) {
    if let Err(e) = relay_job_stream(&client, &job_id, &tx).await {
        tracing::debug!("komf job events relay for {job_id} ended early: {e:#}");
    }
    let _ = tx.send(closed_frame(&job_id)).await;
}

fn closed_frame(job_id: &str) -> String {
    let data = serde_json::json!({"jobId": job_id});
    format!("event: JobStreamClosedEvent\ndata: {data}\n\n")
}

async fn relay_job_stream(
    client: &KomfClient,
    job_id: &str,
    tx: &tokio::sync::mpsc::Sender<String>,
) -> anyhow::Result<()> {
    let response = client
        .proxy_job_events(&format!("{KOMF_JOBS}/{job_id}/events"))
        .await?;
    if !response.status().is_success() {
        return Ok(());
    }
    let mut stream = response.bytes_stream();
    let mut buffer: Vec<u8> = Vec::new();
    loop {
        tokio::select! {
            // the client went away; stop holding an upstream connection for it
            _ = tx.closed() => return Ok(()),
            chunk = stream.next() => {
                let Some(chunk) = chunk else { return Ok(()) };
                buffer.extend_from_slice(&chunk?);
                for frame in split_frames(&mut buffer) {
                    if tx.send(tag_frame(&String::from_utf8_lossy(&frame), job_id)).await.is_err() {
                        return Ok(());
                    }
                }
            }
        }
    }
}

/// Splits complete SSE frames off the buffer; the terminator may be an LF or CRLF
/// pair. Splitting happens on bytes because a multi-byte UTF-8 character may
/// straddle two chunks and must not be decoded mid-sequence.
fn split_frames(buffer: &mut Vec<u8>) -> Vec<Vec<u8>> {
    let mut frames = vec![];
    let mut start = 0;
    loop {
        let lf = find_subslice(&buffer[start..], b"\n\n").map(|p| (p, 2));
        let crlf = find_subslice(&buffer[start..], b"\r\n\r\n").map(|p| (p, 4));
        let Some((pos, len)) = [lf, crlf].into_iter().flatten().min_by_key(|(p, _)| *p) else {
            break;
        };
        frames.push(buffer[start..start + pos].to_vec());
        start += pos + len;
    }
    buffer.drain(..start);
    frames
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Injects the job id into the frame's data payload so the aggregate client can route
/// the event. Frames without an event line are keep-alives; they and frames with
/// unparseable payloads pass through byte-identical.
fn tag_frame(frame: &str, job_id: &str) -> String {
    let mut name = None;
    let mut data = String::new();
    for line in frame.lines() {
        if let Some(value) = line.strip_prefix("event:") {
            name = Some(value.trim().to_string());
        } else if let Some(value) = line.strip_prefix("data:") {
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(value.trim());
        }
    }
    let Some(name) = name else {
        return format!("{frame}\n\n");
    };
    let tagged = if data.is_empty() {
        serde_json::json!({"jobId": job_id})
    } else {
        match serde_json::from_str::<serde_json::Value>(&data) {
            Ok(serde_json::Value::Object(mut map)) => {
                map.insert("jobId".into(), job_id.into());
                serde_json::Value::Object(map)
            }
            _ => return format!("{frame}\n\n"),
        }
    };
    format!("event: {name}\ndata: {tagged}\n\n")
}

const KOMF_OAUTH: &str = "/api/oauth";

/// The provider is interpolated into the upstream URL and the callback route is
/// anonymous: reject anything but lowercase ASCII letters so a percent-encoded
/// segment cannot traverse into komf's unauthenticated API. Legal-but-unknown
/// names still reach komf and get its own 404.
fn check_oauth_provider(provider: &str) -> Result<(), ApiError> {
    if provider.is_empty() || !provider.bytes().all(|b| b.is_ascii_lowercase()) {
        return Err(ApiError::not_found(format!(
            "OAuth provider '{provider}' is not supported"
        )));
    }
    Ok(())
}

async fn oauth_start(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path(provider): Path<String>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    auth.0.require_admin()?;
    check_oauth_provider(&provider)?;
    let row = connected_integration(&state)?;
    let response = KomfClient::new(&row.url)
        .proxy_oauth(
            Method::GET,
            &format!("{KOMF_OAUTH}/{provider}/start"),
            None,
            &headers,
            Duration::from_secs(10),
        )
        .await
        .map_err(komf_unreachable)?;
    Ok(redirect_or_passthrough(response).await)
}

// The browser lands here via a cross-site redirect from komf's relay page, so no
// credentials can be attached, and komf's own callback is unauthenticated too; the
// pending nonce only the admin-only start can create is the actual CSRF guard.
async fn oauth_callback(
    State(state): State<AppState>,
    Path(provider): Path<String>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    check_oauth_provider(&provider)?;
    let row = connected_integration(&state)?;
    let response = KomfClient::new(&row.url)
        .proxy_oauth(
            Method::GET,
            &format!("{KOMF_OAUTH}/{provider}/callback"),
            query.as_deref(),
            &headers,
            komf::METADATA_PROXY_TIMEOUT,
        )
        .await
        .map_err(komf_unreachable)?;
    Ok(redirect_or_passthrough(response).await)
}

/// komf owns the status schema (`{"logged_in": bool, "username": string|null}`), so
/// the proxy relays the raw JSON instead of modeling it.
async fn oauth_status(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path(provider): Path<String>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    auth.0.require_admin()?;
    check_oauth_provider(&provider)?;
    let row = connected_integration(&state)?;
    let response = KomfClient::new(&row.url)
        .proxy_oauth(
            Method::GET,
            &format!("{KOMF_OAUTH}/{provider}/status"),
            None,
            &headers,
            Duration::from_secs(10),
        )
        .await
        .map_err(komf_unreachable)?;
    json_or_passthrough::<serde_json::Value>(response).await
}

async fn oauth_logout(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path(provider): Path<String>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    auth.0.require_admin()?;
    check_oauth_provider(&provider)?;
    let row = connected_integration(&state)?;
    let response = KomfClient::new(&row.url)
        .proxy_oauth(
            Method::POST,
            &format!("{KOMF_OAUTH}/{provider}/logout"),
            None,
            &headers,
            Duration::from_secs(10),
        )
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

/// komf's OAuth dance answers redirects (3xx) whose Location the browser must
/// follow; the redirect is relayed verbatim with an empty body. Anything else is
/// komf's own response and passes through.
async fn redirect_or_passthrough(response: reqwest::Response) -> Response {
    if response.status().is_redirection() {
        if let Some(location) = response.headers().get(axum::http::header::LOCATION) {
            return (
                response.status(),
                [(axum::http::header::LOCATION, location.clone())],
            )
                .into_response();
        }
    }
    passthrough(response).await
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
        headers: HeaderMap,
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
                    let headers = request.headers().clone();
                    let bytes = axum::body::to_bytes(request.into_body(), usize::MAX)
                        .await
                        .unwrap();
                    let body = serde_json::from_slice(&bytes).ok();
                    requests.lock().unwrap().push(CapturedRequest {
                        method: method.clone(),
                        path: path.clone(),
                        query: query.clone(),
                        headers,
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
                    // komf only supports these OAuth providers; anything else gets its own 404
                    let oauth_provider = path
                        .strip_prefix("/api/oauth/")
                        .and_then(|rest| rest.split('/').next());
                    let oauth_known =
                        oauth_provider.is_some_and(|p| ["anilist", "mal", "bangumi"].contains(&p));
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
                        ("GET", "/api/jobs") => Json(serde_json::json!({
                            "content": [{
                                "seriesId": "ser-1",
                                "id": "job-1",
                                "status": "RUNNING",
                                "startedAt": "2026-09-27T10:00:00Z"
                            }],
                            "totalPages": 3,
                            "currentPage": 1
                        }))
                        .into_response(),
                        ("GET", "/api/jobs/job-1") => Json(serde_json::json!({
                            "seriesId": "ser-1",
                            "id": "job-1",
                            "status": "COMPLETED",
                            "message": "done",
                            "startedAt": "2026-09-27T10:00:00Z",
                            "finishedAt": "2026-09-27T10:01:00Z"
                        }))
                        .into_response(),
                        _ if method == "GET"
                            && path.starts_with("/api/jobs/")
                            && path.ends_with("/events") =>
                        {
                            (
                                [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                                "event: ProviderSeriesEvent\ndata: {\"type\":\"ProviderSeriesEvent\",\"provider\":\"MANGADEX\"}\n\nevent: ProviderCompletedEvent\ndata: {\"type\":\"ProviderCompletedEvent\",\"provider\":\"MANGADEX\"}\n\n",
                            )
                                .into_response()
                        }
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
                        _ if method == "GET" && oauth_known && path.ends_with("/start") => {
                            (
                                StatusCode::FOUND,
                                [(
                                    axum::http::header::LOCATION,
                                    "https://provider.example/authorize?client_id=x&state=y",
                                )],
                            )
                                .into_response()
                        }
                        _ if method == "GET" && oauth_known && path.ends_with("/callback") => {
                            (
                                StatusCode::FOUND,
                                [(axum::http::header::LOCATION, "/?oauth=success")],
                            )
                                .into_response()
                        }
                        _ if method == "GET" && oauth_known && path.ends_with("/status") => {
                            Json(serde_json::json!({"logged_in": true, "username": "komf-user"}))
                                .into_response()
                        }
                        _ if method == "POST" && oauth_known && path.ends_with("/logout") => {
                            StatusCode::NO_CONTENT.into_response()
                        }
                        _ if let Some(provider) = oauth_provider => (
                            StatusCode::NOT_FOUND,
                            Json(serde_json::json!({
                                "message": format!("OAuth provider '{provider}' is not supported")
                            })),
                        )
                            .into_response(),
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
        let (status, _) = app.get_json("/api/v1/komf/jobs", "k-user").await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _) = app.get_json("/api/v1/komf/jobs/job-1", "k-user").await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _) = app
            .get_json("/api/v1/komf/jobs/job-1/events", "k-user")
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _) = app
            .get_json("/api/v1/komf/jobs/events?ids=job-1", "k-user")
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _) = app
            .get_json("/api/v1/komf/oauth/anilist/start", "k-user")
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _) = app
            .get_json("/api/v1/komf/oauth/anilist/status", "k-user")
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _) = app
            .request_json("POST", "/api/v1/komf/oauth/anilist/logout", "k-user", None)
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
        let (status, _) = app.get_json("/api/v1/komf/jobs", "k-admin").await;
        assert_eq!(status, StatusCode::CONFLICT);
        let (status, _) = app
            .get_json("/api/v1/komf/jobs/job-1/events", "k-admin")
            .await;
        assert_eq!(status, StatusCode::CONFLICT);
        let (status, _) = app
            .get_json("/api/v1/komf/jobs/events?ids=job-1", "k-admin")
            .await;
        assert_eq!(status, StatusCode::CONFLICT);
        let (status, _) = app
            .get_json("/api/v1/komf/oauth/anilist/start", "k-admin")
            .await;
        assert_eq!(status, StatusCode::CONFLICT);
        let (status, _) = app
            .get_json("/api/v1/komf/oauth/anilist/status", "k-admin")
            .await;
        assert_eq!(status, StatusCode::CONFLICT);
        let (status, _) = app
            .request_json("POST", "/api/v1/komf/oauth/anilist/logout", "k-admin", None)
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

        let (status, _) = app.get_json("/api/v1/komf/jobs", "k-admin").await;
        assert_eq!(status, StatusCode::CONFLICT);
        let (status, _) = app
            .get_json("/api/v1/komf/jobs/events?ids=job-1", "k-admin")
            .await;
        assert_eq!(status, StatusCode::CONFLICT);
        let (status, _) = app
            .get_json("/api/v1/komf/oauth/anilist/start", "k-admin")
            .await;
        assert_eq!(status, StatusCode::CONFLICT);
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
    async fn jobs_forwards_query_and_returns_page() {
        let app = admin_app();
        let komf = serve_komf_proxy().await;
        seed_connected(&app.state, &komf.url);

        let (status, body) = app
            .get_json(
                "/api/v1/komf/jobs?status=RUNNING&pageSize=5&page=1",
                "k-admin",
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            body,
            serde_json::json!({
                "content": [{
                    "seriesId": "ser-1",
                    "id": "job-1",
                    "status": "RUNNING",
                    "startedAt": "2026-09-27T10:00:00Z"
                }],
                "totalPages": 3,
                "currentPage": 1
            })
        );

        let captured = komf.captured();
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0].method, "GET");
        assert_eq!(captured[0].path, "/api/jobs");
        assert_eq!(
            captured[0].query.as_deref(),
            Some("status=RUNNING&pageSize=5&page=1")
        );
    }

    #[tokio::test]
    async fn get_job_returns_job_and_relays_404() {
        let app = admin_app();
        let komf = serve_komf_proxy().await;
        seed_connected(&app.state, &komf.url);

        let (status, body) = app.get_json("/api/v1/komf/jobs/job-1", "k-admin").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            body,
            serde_json::json!({
                "seriesId": "ser-1",
                "id": "job-1",
                "status": "COMPLETED",
                "message": "done",
                "startedAt": "2026-09-27T10:00:00Z",
                "finishedAt": "2026-09-27T10:01:00Z"
            })
        );

        let (status, _) = app
            .get_json("/api/v1/komf/jobs/job-unknown", "k-admin")
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        let captured = komf.captured();
        assert_eq!(captured.len(), 2);
        assert_eq!(captured[0].path, "/api/jobs/job-1");
        assert_eq!(captured[1].path, "/api/jobs/job-unknown");
    }

    #[tokio::test]
    async fn job_events_relays_the_sse_stream() {
        let app = admin_app();
        let komf = serve_komf_proxy().await;
        seed_connected(&app.state, &komf.url);

        let (status, headers, bytes) = app
            .get_response("/api/v1/komf/jobs/job-1/events", "k-admin")
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            headers[axum::http::header::CONTENT_TYPE],
            "text/event-stream"
        );
        assert_eq!(headers[axum::http::header::CACHE_CONTROL], "no-cache");
        let body = String::from_utf8(bytes).unwrap();
        assert!(body.contains("event: ProviderSeriesEvent"));
        assert!(body.contains("event: ProviderCompletedEvent"));
        assert!(body.contains("\"provider\":\"MANGADEX\""));

        let captured = komf.captured();
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0].path, "/api/jobs/job-1/events");
    }

    #[tokio::test]
    async fn jobs_events_aggregates_tagged_frames() {
        let app = admin_app();
        let komf = serve_komf_proxy().await;
        seed_connected(&app.state, &komf.url);

        let (status, headers, bytes) = app
            .get_response("/api/v1/komf/jobs/events?ids=job-1,job-2,job-1", "k-admin")
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            headers[axum::http::header::CONTENT_TYPE],
            "text/event-stream"
        );
        assert_eq!(headers[axum::http::header::CACHE_CONTROL], "no-cache");
        let body = String::from_utf8(bytes).unwrap();
        // each job's frames carry the routing tag; event names pass through unchanged
        assert!(body.contains("event: ProviderSeriesEvent"));
        assert!(body.contains("event: ProviderCompletedEvent"));
        assert!(body.contains("\"jobId\":\"job-1\""));
        assert!(body.contains("\"jobId\":\"job-2\""));
        // one closed event per job, even though job-1 was requested twice
        assert_eq!(body.matches("event: JobStreamClosedEvent").count(), 2);

        let captured = komf.captured();
        let mut paths: Vec<&str> = captured.iter().map(|r| r.path.as_str()).collect();
        paths.sort_unstable();
        assert_eq!(paths, ["/api/jobs/job-1/events", "/api/jobs/job-2/events"]);
    }

    #[tokio::test]
    async fn jobs_events_reports_failed_upstream_as_closed() {
        let app = admin_app();
        let komf = serve_komf_proxy().await;
        seed_connected(&app.state, &komf.url);

        let (status, _, bytes) = app
            .get_response("/api/v1/komf/jobs/events?ids=broken-1", "k-admin")
            .await;
        assert_eq!(status, StatusCode::OK);
        let body = String::from_utf8(bytes).unwrap();
        assert!(body.contains("event: JobStreamClosedEvent"));
        assert!(body.contains("\"jobId\":\"broken-1\""));
        assert!(!body.contains("ProviderSeriesEvent"));
    }

    #[tokio::test]
    async fn jobs_events_keeps_good_jobs_when_others_fail() {
        let app = admin_app();
        let komf = serve_komf_proxy().await;
        seed_connected(&app.state, &komf.url);

        let (status, _, bytes) = app
            .get_response("/api/v1/komf/jobs/events?ids=job-1,broken-1", "k-admin")
            .await;
        assert_eq!(status, StatusCode::OK);
        let body = String::from_utf8(bytes).unwrap();
        assert!(body.contains("event: ProviderSeriesEvent"));
        assert!(body.contains("\"jobId\":\"job-1\""));
        assert!(body.contains("\"jobId\":\"broken-1\""));
        assert_eq!(body.matches("event: JobStreamClosedEvent").count(), 2);
    }

    #[tokio::test]
    async fn jobs_events_validates_ids() {
        let app = admin_app();

        let (status, body) = app.get_json("/api/v1/komf/jobs/events", "k-admin").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["violations"][0]["fieldName"], "ids");
        let (status, _) = app
            .get_json("/api/v1/komf/jobs/events?ids=,,", "k-admin")
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let too_many = (0..=MAX_EVENT_STREAM_IDS)
            .map(|i| format!("j{i}"))
            .collect::<Vec<_>>()
            .join(",");
        let (status, _) = app
            .get_json(
                &format!("/api/v1/komf/jobs/events?ids={too_many}"),
                "k-admin",
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
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

        let (status, body) = app.get_json("/api/v1/komf/jobs/broken", "k-admin").await;
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

        let (status, _) = app
            .get_json("/api/v1/komf/jobs/job-1/events", "k-admin")
            .await;
        assert_eq!(status, StatusCode::BAD_GATEWAY);

        let (status, _) = app
            .get_json("/api/v1/komf/oauth/anilist/start", "k-admin")
            .await;
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        let (status, _) = app
            .get_json("/api/v1/komf/oauth/anilist/status", "k-admin")
            .await;
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        let (status, _) = app
            .request_json("POST", "/api/v1/komf/oauth/anilist/logout", "k-admin", None)
            .await;
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        let (status, _) = app
            .get_json("/api/oauth/anilist/callback?code=x&state=y", "k-admin")
            .await;
        assert_eq!(status, StatusCode::BAD_GATEWAY);
    }

    /// `TestApp`'s helpers always send an X-API-Key header and cannot attach custom
    /// headers, so tests that need either drive a raw router wired the same way.
    fn raw_app(state: &AppState) -> Router {
        router()
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                crate::auth::auth_middleware,
            ))
            .with_state(state.clone())
    }

    async fn raw_request(
        app: &Router,
        method: &str,
        uri: &str,
        headers: &[(&str, &str)],
    ) -> (StatusCode, HeaderMap, Vec<u8>) {
        let mut builder = axum::http::Request::builder().method(method).uri(uri);
        for (name, value) in headers {
            builder = builder.header(*name, *value);
        }
        let response = tower::ServiceExt::oneshot(
            app.clone(),
            builder.body(axum::body::Body::empty()).unwrap(),
        )
        .await
        .unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec();
        (status, headers, bytes)
    }

    #[tokio::test]
    async fn oauth_start_relays_the_redirect_and_forwards_host_headers() {
        let state = TestApp::new(router()).state;
        let admin = insert_user(&state.db, "admin@x.c", true, true, &[]);
        insert_api_key(&state.db, &admin, "k-admin");
        let komf = serve_komf_proxy().await;
        seed_connected(&state, &komf.url);
        let app = raw_app(&state);

        let (status, headers, _) = raw_request(
            &app,
            "GET",
            "/api/v1/komf/oauth/anilist/start",
            &[
                ("X-API-Key", "k-admin"),
                ("Host", "kmrs.example"),
                ("X-Forwarded-Host", "public.example"),
                ("X-Forwarded-Proto", "https"),
            ],
        )
        .await;
        assert_eq!(status, StatusCode::FOUND);
        assert_eq!(
            headers[axum::http::header::LOCATION],
            "https://provider.example/authorize?client_id=x&state=y"
        );

        let captured = komf.captured();
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0].method, "GET");
        assert_eq!(captured[0].path, "/api/oauth/anilist/start");
        assert_eq!(captured[0].headers["host"], "kmrs.example");
        assert_eq!(captured[0].headers["x-forwarded-host"], "public.example");
        assert_eq!(captured[0].headers["x-forwarded-proto"], "https");
    }

    #[tokio::test]
    async fn oauth_callback_is_anonymous_and_relays_the_redirect() {
        let state = TestApp::new(router()).state;
        let komf = serve_komf_proxy().await;
        seed_connected(&state, &komf.url);
        let app = raw_app(&state);

        // no credentials: the browser arrives cross-site from komf's relay page
        let (status, headers, _) = raw_request(
            &app,
            "GET",
            "/api/oauth/anilist/callback?code=auth-code&state=opaque",
            &[],
        )
        .await;
        assert_eq!(status, StatusCode::FOUND);
        assert_eq!(headers[axum::http::header::LOCATION], "/?oauth=success");

        let captured = komf.captured();
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0].method, "GET");
        assert_eq!(captured[0].path, "/api/oauth/anilist/callback");
        assert_eq!(
            captured[0].query.as_deref(),
            Some("code=auth-code&state=opaque")
        );
    }

    #[tokio::test]
    async fn oauth_status_passes_komf_json_through() {
        let app = admin_app();
        let komf = serve_komf_proxy().await;
        seed_connected(&app.state, &komf.url);

        let (status, body) = app
            .get_json("/api/v1/komf/oauth/anilist/status", "k-admin")
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            body,
            serde_json::json!({"logged_in": true, "username": "komf-user"})
        );

        let captured = komf.captured();
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0].method, "GET");
        assert_eq!(captured[0].path, "/api/oauth/anilist/status");
    }

    #[tokio::test]
    async fn oauth_logout_relays_no_content() {
        let app = admin_app();
        let komf = serve_komf_proxy().await;
        seed_connected(&app.state, &komf.url);

        let (status, _) = app
            .request_json("POST", "/api/v1/komf/oauth/anilist/logout", "k-admin", None)
            .await;
        assert_eq!(status, StatusCode::NO_CONTENT);

        let captured = komf.captured();
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0].method, "POST");
        assert_eq!(captured[0].path, "/api/oauth/anilist/logout");
    }

    #[tokio::test]
    async fn oauth_komf_error_is_passed_through() {
        let app = admin_app();
        let komf = serve_komf_proxy().await;
        seed_connected(&app.state, &komf.url);

        let (status, body) = app
            .get_json("/api/v1/komf/oauth/broken/start", "k-admin")
            .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body, serde_json::json!({"message": "komf broke"}));
    }

    #[tokio::test]
    async fn oauth_callback_rejects_provider_traversal() {
        let state = TestApp::new(router()).state;
        let komf = serve_komf_proxy().await;
        seed_connected(&state, &komf.url);
        let app = raw_app(&state);

        // decodes to provider `../../api/config?`: rejected before any proxying
        let (status, _, bytes) = raw_request(
            &app,
            "GET",
            "/api/oauth/..%2F..%2Fapi%2Fconfig%3F/callback?code=x&state=y",
            &[],
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(body["message"]
            .as_str()
            .unwrap()
            .contains("OAuth provider '../../api/config?' is not supported"));
        assert!(komf.captured().is_empty());
    }

    #[tokio::test]
    async fn oauth_start_rejects_provider_traversal() {
        let app = admin_app();
        let komf = serve_komf_proxy().await;
        seed_connected(&app.state, &komf.url);

        let (status, body) = app
            .get_json("/api/v1/komf/oauth/..%2F../start", "k-admin")
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(body["message"]
            .as_str()
            .unwrap()
            .contains("OAuth provider '../..' is not supported"));
        assert!(komf.captured().is_empty());
    }

    #[tokio::test]
    async fn oauth_unknown_provider_relays_komf_not_found() {
        let app = admin_app();
        let komf = serve_komf_proxy().await;
        seed_connected(&app.state, &komf.url);

        let (status, body) = app
            .get_json("/api/v1/komf/oauth/mangabaka/status", "k-admin")
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(
            body,
            serde_json::json!({"message": "OAuth provider 'mangabaka' is not supported"})
        );

        let captured = komf.captured();
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0].path, "/api/oauth/mangabaka/status");
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

    #[test]
    fn split_frames_handles_crlf_and_partial_frames() {
        let mut buf = b"event: A\r\ndata: {}\r\n\r\nevent: B".to_vec();
        let frames = split_frames(&mut buf);
        assert_eq!(frames, vec![b"event: A\r\ndata: {}".to_vec()]);
        assert_eq!(buf, b"event: B".to_vec());
    }

    #[test]
    fn split_frames_keeps_utf8_split_across_chunks_intact() {
        // '日' is E6 97 A5: the frame arrives split inside the character
        let mut buf = b"data: {\"msg\":\"\xe6".to_vec();
        assert!(split_frames(&mut buf).is_empty());
        buf.extend_from_slice(b"\x97\xa5\"}\n\n");
        let frames = split_frames(&mut buf);
        assert_eq!(frames.len(), 1);
        assert_eq!(
            String::from_utf8_lossy(&frames[0]),
            "data: {\"msg\":\"日\"}"
        );
        assert!(buf.is_empty());
    }

    #[test]
    fn tag_frame_injects_job_id() {
        let tagged = tag_frame(
            "event: ProviderSeriesEvent\ndata: {\"provider\":\"MANGADEX\"}",
            "job-1",
        );
        let (event_line, data_line) = tagged.split_once('\n').unwrap();
        assert_eq!(event_line, "event: ProviderSeriesEvent");
        let data: serde_json::Value =
            serde_json::from_str(data_line.strip_prefix("data: ").unwrap().trim()).unwrap();
        assert_eq!(data["provider"], "MANGADEX");
        assert_eq!(data["jobId"], "job-1");
    }

    #[test]
    fn tag_frame_synthesizes_data_for_empty_payloads() {
        let tagged = tag_frame("event: EventStreamNotFoundEvent", "job-1");
        assert_eq!(
            tagged,
            "event: EventStreamNotFoundEvent\ndata: {\"jobId\":\"job-1\"}\n\n"
        );
    }

    #[test]
    fn tag_frame_passes_keep_alives_and_untaggable_frames_through() {
        assert_eq!(tag_frame(": keep-alive", "job-1"), ": keep-alive\n\n");
        let raw = "event: X\ndata: not json";
        assert_eq!(tag_frame(raw, "job-1"), format!("{raw}\n\n"));
    }

    #[test]
    fn closed_frame_escapes_the_job_id() {
        let frame = closed_frame("bad\"\nid");
        let data = frame
            .lines()
            .nth(1)
            .unwrap()
            .strip_prefix("data: ")
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(data).unwrap();
        assert_eq!(parsed["jobId"], "bad\"\nid");
    }
}
