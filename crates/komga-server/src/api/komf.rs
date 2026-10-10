//! kmrs-private komf integration management and proxy (admin only). Not part of the
//! Komga API surface, so it stays out of the OpenAPI spec.

use crate::auth::RequireAuth;
use crate::dto::komf::{
    KomfIdentifyRequestDto, KomfIntegrationDto, KomfIntegrationUpdateDto, KomfJobDto,
    KomfJobPageDto, KomfMetadataJobResponseDto, KomfSeriesSearchResultDto, TrackerLinkDto,
    TrackerLinkUpsertDto, TrackerPreferencesDto,
};
use crate::error::{ApiError, Violation};
use crate::service::komf;
use crate::state::AppState;
use axum::body::Body;
use axum::extract::{Path, RawQuery, State};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{routing, Json, Router};
use komga_core::tracker::TrackMode;
use komga_db::dao::komf_integration::{KomfIntegration, KomfIntegrationDao, KomfIntegrationState};
use komga_db::dao::tracker_link::{NewTrackerLink, TrackerLinkDao};
use komga_db::dao::tracker_preferences::TrackerPreferencesDao;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::time::Duration;

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
        // komf builds the OAuth callback under the prefix passed on start, and the
        // relay page accepts it because the path still ends in
        // /api/oauth/{provider}/callback; requires a komf version with
        // redirect_path_prefix support
        .route(
            "/api/v1/komf/api/oauth/{provider}/callback",
            routing::get(oauth_callback),
        )
        // per-user tracker sync: any authenticated user manages their own
        // bindings and platform logins; the kmrs user id is forwarded as
        // komf's X-Tracker-User so every user gets an independent tracker
        // account per platform
        .route(
            "/api/v1/komf/trackers/links",
            routing::get(list_all_tracker_links),
        )
        .route(
            "/api/v1/komf/trackers/links/{seriesId}",
            routing::get(list_tracker_links).put(put_tracker_link),
        )
        .route(
            "/api/v1/komf/trackers/links/{seriesId}/{provider}",
            routing::delete(delete_tracker_link),
        )
        .route(
            "/api/v1/komf/trackers/preferences",
            routing::get(get_tracker_preferences).put(put_tracker_preferences),
        )
        .route("/api/v1/komf/trackers/ledger", routing::get(tracker_ledger))
        .route(
            "/api/v1/komf/trackers/{provider}/search",
            routing::get(tracker_search),
        )
        .route(
            "/api/v1/komf/trackers/{provider}/state",
            routing::get(tracker_state),
        )
        .route(
            "/api/v1/komf/trackers/{provider}/update",
            routing::post(tracker_update),
        )
        .route(
            "/api/v1/komf/trackers/oauth/{provider}/start",
            routing::get(tracker_oauth_start),
        )
        .route(
            "/api/v1/komf/trackers/oauth/{provider}/status",
            routing::get(tracker_oauth_status),
        )
        .route(
            "/api/v1/komf/trackers/oauth/{provider}/logout",
            routing::post(tracker_oauth_logout),
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
    let dao = KomfIntegrationDao::new(state.kmrs_db.clone());
    // absent authKey keeps the stored override so an unrelated URL edit cannot drop it;
    // a blank value clears the override so the config preset applies again
    let auth_key = match body.auth_key {
        None => dao.get()?.and_then(|row| row.auth_key),
        Some(raw) => {
            let trimmed = raw.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        }
    };
    dao.upsert(&url, &base_url, auth_key.as_deref())?;
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
            auth_key_set: false,
            state: None,
            last_error: None,
            komf_reachable: false,
        });
    };
    let komf_reachable = komf::client(state, &row).health().await.is_ok();
    Ok(KomfIntegrationDto {
        configured: true,
        url: Some(row.url),
        base_url: Some(row.base_url),
        auth_key_set: row.auth_key.is_some(),
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
    let response = komf::client(&state, &row)
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
    let response = komf::client(&state, &row)
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
    let response = komf::client(&state, &row)
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
    let response = komf::client(&state, &row)
        .proxy_jobs(Method::GET, &format!("{KOMF_JOBS}/{job_id}"), None)
        .await
        .map_err(komf_unreachable)?;
    json_or_passthrough::<KomfJobDto>(response).await
}

/// SSE responses are relayed as raw bytes: a live event stream cannot be buffered
/// into a typed response.
fn sse_stream_response(response: reqwest::Response) -> Response {
    Response::builder()
        .header(axum::http::header::CONTENT_TYPE, "text/event-stream")
        .header(axum::http::header::CACHE_CONTROL, "no-cache")
        .body(Body::from_stream(response.bytes_stream()))
        .unwrap()
}

async fn get_job_events(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path(job_id): Path<String>,
) -> Result<Response, ApiError> {
    auth.0.require_admin()?;
    let row = connected_integration(&state)?;
    let response = komf::client(&state, &row)
        .proxy_job_events(&format!("{KOMF_JOBS}/{job_id}/events"), None)
        .await
        .map_err(komf_unreachable)?;
    if !response.status().is_success() {
        return Ok(passthrough(response).await);
    }
    Ok(sse_stream_response(response))
}

/// komf-rs ≥ 1.7.0 serves one global job-events firehose at `/jobs/events` (optional
/// `?ids=` filter, RUNNING snapshot replay, JobCreated/JobFinished lifecycle frames),
/// which covers every job regardless of who started it; the query is forwarded
/// untouched.
async fn get_jobs_events(
    State(state): State<AppState>,
    auth: RequireAuth,
    RawQuery(query): RawQuery,
) -> Result<Response, ApiError> {
    auth.0.require_admin()?;
    let row = connected_integration(&state)?;
    let response = komf::client(&state, &row)
        .proxy_job_events(&format!("{KOMF_JOBS}/events"), query.as_deref())
        .await
        .map_err(komf_unreachable)?;
    if !response.status().is_success() {
        return Ok(passthrough(response).await);
    }
    Ok(sse_stream_response(response))
}

const KOMF_OAUTH: &str = "/api/oauth";

/// Passed as `redirect_path_prefix` on start so komf builds the OAuth callback
/// inside the integration namespace instead of its hardcoded root path.
const KOMF_OAUTH_CALLBACK_PREFIX: &str = "/api/v1/komf";

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

/// Tracker bindings only accept the four providers komf implements. Unlike
/// the OAuth proxies (where unknown-but-legal names reach komf and get its
/// own 404), a binding would fail the TRACKER_LINK provider CHECK and
/// surface as a 500, so reject up front.
fn check_tracker_provider(provider: &str) -> Result<(), ApiError> {
    if TRACKER_PROVIDERS.contains(&provider) {
        Ok(())
    } else {
        Err(ApiError::not_found(format!(
            "Tracker provider '{provider}' is not supported"
        )))
    }
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
    let response = komf::client(&state, &row)
        .proxy_oauth(
            Method::GET,
            &format!("{KOMF_OAUTH}/{provider}/start"),
            Some(&format!(
                "redirect_path_prefix={KOMF_OAUTH_CALLBACK_PREFIX}"
            )),
            &headers,
            Duration::from_secs(10),
            None,
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
    // a user-flow start recorded this state's nonce: land the browser back on
    // the trackers page instead of komf's default root (which the webui
    // forwards to the admin komf settings)
    let return_path = query.as_deref().and_then(|q| {
        form_urlencoded::parse(q.as_bytes())
            .find(|(k, _)| k == "state")
            .and_then(|(_, v)| take_oauth_return_path(&v))
    });
    let response = komf::client(&state, &row)
        .proxy_oauth(
            Method::GET,
            &format!("{KOMF_OAUTH}/{provider}/callback"),
            query.as_deref(),
            &headers,
            komf::METADATA_PROXY_TIMEOUT,
            None,
        )
        .await
        .map_err(komf_unreachable)?;
    if let (Some(path), Some(location)) = (
        return_path,
        response.headers().get(axum::http::header::LOCATION),
    ) {
        if response.status().is_redirection() {
            // komf's Location is root-relative ("/?oauth=..."): keep the query,
            // swap the path for the page that started this login
            let suffix = location
                .to_str()
                .ok()
                .and_then(|l| l.split_once('?').map(|(_, query)| query))
                .unwrap_or_default();
            let target = if suffix.is_empty() {
                path.to_string()
            } else {
                format!("{path}?{suffix}")
            };
            return Ok(
                (response.status(), [(axum::http::header::LOCATION, target)]).into_response(),
            );
        }
    }
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
    let response = komf::client(&state, &row)
        .proxy_oauth(
            Method::GET,
            &format!("{KOMF_OAUTH}/{provider}/status"),
            None,
            &headers,
            Duration::from_secs(10),
            None,
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
    let response = komf::client(&state, &row)
        .proxy_oauth(
            Method::POST,
            &format!("{KOMF_OAUTH}/{provider}/logout"),
            None,
            &headers,
            Duration::from_secs(10),
            None,
        )
        .await
        .map_err(komf_unreachable)?;
    Ok(empty_or_passthrough(response).await)
}

/// Per-user tracker bindings live in kmrs.sqlite (not komf): any authenticated
/// user lists, creates, or replaces the bindings of one series for themselves.
/// Listing still requires a connected integration so the webui can rely on
/// 409 to render its not-connected state.
async fn list_all_tracker_links(
    State(state): State<AppState>,
    auth: RequireAuth,
) -> Result<Json<Vec<TrackerLinkDto>>, ApiError> {
    connected_integration(&state)?;
    let links = TrackerLinkDao::new(state.kmrs_db.clone()).list_by_user(&auth.0.user.id)?;
    Ok(Json(links.into_iter().map(TrackerLinkDto::from).collect()))
}

async fn list_tracker_links(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path(series_id): Path<String>,
) -> Result<Json<Vec<TrackerLinkDto>>, ApiError> {
    connected_integration(&state)?;
    let links = TrackerLinkDao::new(state.kmrs_db.clone())
        .list_by_series_and_user(&series_id, &auth.0.user.id)?;
    Ok(Json(links.into_iter().map(TrackerLinkDto::from).collect()))
}

async fn put_tracker_link(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path(series_id): Path<String>,
    Json(body): Json<TrackerLinkUpsertDto>,
) -> Result<Json<TrackerLinkDto>, ApiError> {
    check_tracker_provider(&body.provider)?;
    let track_id = body.track_id.trim();
    if track_id.is_empty() {
        return Err(ApiError::Violations(vec![Violation {
            field_name: "trackId".into(),
            message: "must not be blank".into(),
        }]));
    }
    let track_mode = match body.track_mode.as_deref() {
        None | Some("auto") => TrackMode::Auto,
        Some("chapter") => TrackMode::Chapter,
        Some("volume") => TrackMode::Volume,
        Some(other) => {
            return Err(ApiError::Violations(vec![Violation {
                field_name: "trackMode".into(),
                message: format!("'{other}' is not a valid track mode"),
            }]))
        }
    };
    let chapter_offset = body.chapter_offset.unwrap_or(0);
    if !(-10000..=10000).contains(&chapter_offset) {
        return Err(ApiError::Violations(vec![Violation {
            field_name: "chapterOffset".into(),
            message: "must be between -10000 and 10000".into(),
        }]));
    }
    let dao = TrackerLinkDao::new(state.kmrs_db.clone());
    dao.upsert(&NewTrackerLink {
        series_id: &series_id,
        user_id: &auth.0.user.id,
        provider: &body.provider,
        track_id,
        title: body
            .title
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty()),
        track_mode,
        chapter_offset,
    })?;
    let links = dao.list_by_series_and_user(&series_id, &auth.0.user.id)?;
    let link = links
        .into_iter()
        .find(|l| l.provider == body.provider)
        .expect("upserted binding is readable");
    Ok(Json(TrackerLinkDto::from(link)))
}

async fn delete_tracker_link(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path((series_id, provider)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    check_tracker_provider(&provider)?;
    TrackerLinkDao::new(state.kmrs_db.clone()).delete(&series_id, &auth.0.user.id, &provider)?;
    Ok(StatusCode::NO_CONTENT)
}

/// Per-user display preferences: which libraries show the series-detail
/// tracker module (empty = every library), and the provider preselected
/// in the bind dialog.
async fn get_tracker_preferences(
    State(state): State<AppState>,
    auth: RequireAuth,
) -> Result<Json<TrackerPreferencesDto>, ApiError> {
    let prefs = TrackerPreferencesDao::new(state.kmrs_db.clone()).get(&auth.0.user.id)?;
    Ok(Json(TrackerPreferencesDto {
        libraries: prefs.libraries,
        default_tracker: prefs.default_tracker,
    }))
}

const TRACKER_PROVIDERS: &[&str] = &["anilist", "bangumi", "mal", "mangabaka"];

async fn put_tracker_preferences(
    State(state): State<AppState>,
    auth: RequireAuth,
    Json(body): Json<TrackerPreferencesDto>,
) -> Result<Json<TrackerPreferencesDto>, ApiError> {
    // provider names are komf's own vocabulary; reject anything it would 404
    let default_tracker = body
        .default_tracker
        .as_deref()
        .filter(|d| !d.is_empty())
        .map(str::to_string);
    if let Some(default) = default_tracker.as_deref() {
        if !TRACKER_PROVIDERS.contains(&default) {
            return Err(ApiError::Violations(vec![Violation {
                field_name: "defaultTracker".into(),
                message: format!("'{default}' is not a supported tracker"),
            }]));
        }
    }
    let dao = TrackerPreferencesDao::new(state.kmrs_db.clone());
    dao.set(
        &auth.0.user.id,
        &komga_db::dao::tracker_preferences::TrackerPreferences {
            libraries: body.libraries.clone(),
            default_tracker,
        },
    )?;
    let prefs = dao.get(&auth.0.user.id)?;
    Ok(Json(TrackerPreferencesDto {
        libraries: prefs.libraries,
        default_tracker: prefs.default_tracker,
    }))
}

const KOMF_TRACKER: &str = "/api/tracker";

/// The user's own linked-entry ledger in komf (scoped by X-Tracker-User).
async fn tracker_ledger(
    State(state): State<AppState>,
    auth: RequireAuth,
) -> Result<Response, ApiError> {
    let row = connected_integration(&state)?;
    let response = komf::client(&state, &row)
        .proxy_tracker(
            Method::GET,
            &format!("{KOMF_TRACKER}/links"),
            None,
            None,
            &auth.0.user.id,
        )
        .await
        .map_err(komf_unreachable)?;
    json_or_passthrough::<serde_json::Value>(response).await
}

/// komf owns the search result schema, so the proxy relays the raw JSON.
async fn tracker_search(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path(provider): Path<String>,
    RawQuery(query): RawQuery,
) -> Result<Response, ApiError> {
    check_oauth_provider(&provider)?;
    let row = connected_integration(&state)?;
    let response = komf::client(&state, &row)
        .proxy_tracker(
            Method::GET,
            &format!("{KOMF_TRACKER}/{provider}/search"),
            query.as_deref(),
            None,
            &auth.0.user.id,
        )
        .await
        .map_err(komf_unreachable)?;
    json_or_passthrough::<serde_json::Value>(response).await
}

/// komf owns the state schema, so the proxy relays the raw JSON.
async fn tracker_state(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path(provider): Path<String>,
    RawQuery(query): RawQuery,
) -> Result<Response, ApiError> {
    check_oauth_provider(&provider)?;
    let row = connected_integration(&state)?;
    let response = komf::client(&state, &row)
        .proxy_tracker(
            Method::GET,
            &format!("{KOMF_TRACKER}/{provider}/state"),
            query.as_deref(),
            None,
            &auth.0.user.id,
        )
        .await
        .map_err(komf_unreachable)?;
    json_or_passthrough::<serde_json::Value>(response).await
}

async fn tracker_update(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path(provider): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Result<Response, ApiError> {
    check_oauth_provider(&provider)?;
    let row = connected_integration(&state)?;
    let response = komf::client(&state, &row)
        .proxy_tracker(
            Method::POST,
            &format!("{KOMF_TRACKER}/{provider}/update"),
            None,
            Some(&body),
            &auth.0.user.id,
        )
        .await
        .map_err(komf_unreachable)?;
    Ok(empty_or_passthrough(response).await)
}

/// Per-user OAuth: komf attributes the login to the kmrs user id carried in
/// `?user=` (recorded server-side in komf's pending table and recovered by
/// nonce on callback), so the shared anonymous callback route below serves
/// both the admin flow and every user flow.
/// The kmrs-side landing page for a per-user tracker OAuth login. komf's
/// callback always 302s to its own root (`/?oauth=...`), which the webui
/// forwards to the admin komf page — the nonce recorded at start lets the
/// callback route the browser back here instead.
const TRACKER_OAUTH_RETURN_PATH: &str = "/account/trackers";
const OAUTH_RETURN_PATH_TTL: std::time::Duration = std::time::Duration::from_secs(600);

fn oauth_return_paths(
) -> &'static std::sync::Mutex<std::collections::HashMap<String, (std::time::Instant, &'static str)>>
{
    static MAP: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<String, (std::time::Instant, &'static str)>>,
    > = std::sync::OnceLock::new();
    MAP.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

/// Records which kmrs page a login flow belongs to, keyed by the OAuth nonce
/// komf put into the authorize URL's state. Best-effort: an unparsable
/// location simply falls back to the default landing.
fn record_oauth_return_path(location: &axum::http::HeaderValue, path: &'static str) {
    let Ok(location) = location.to_str() else {
        return;
    };
    let Some((_, query)) = location.split_once('?') else {
        return;
    };
    let Some(state) = form_urlencoded::parse(query.as_bytes())
        .find(|(k, _)| k == "state")
        .map(|(_, v)| v.into_owned())
    else {
        return;
    };
    let Ok(state) = serde_json::from_str::<serde_json::Value>(&state) else {
        return;
    };
    let Some(nonce) = state.get("nonce").and_then(|v| v.as_str()) else {
        return;
    };
    let mut map = oauth_return_paths()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    map.retain(|_, (at, _)| at.elapsed() < OAUTH_RETURN_PATH_TTL);
    map.insert(nonce.to_string(), (std::time::Instant::now(), path));
}

/// Consumes the recorded return path for one OAuth state (single use).
fn take_oauth_return_path(state_raw: &str) -> Option<&'static str> {
    let Ok(state) = serde_json::from_str::<serde_json::Value>(state_raw) else {
        return None;
    };
    let nonce = state.get("nonce").and_then(|v| v.as_str())?;
    oauth_return_paths()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(nonce)
        .map(|(_, path)| path)
}

async fn tracker_oauth_start(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path(provider): Path<String>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    check_oauth_provider(&provider)?;
    let row = connected_integration(&state)?;
    let response = komf::client(&state, &row)
        .proxy_oauth(
            Method::GET,
            &format!("{KOMF_OAUTH}/{provider}/start"),
            Some(&format!(
                "redirect_path_prefix={KOMF_OAUTH_CALLBACK_PREFIX}&user={}",
                auth.0.user.id
            )),
            &headers,
            Duration::from_secs(10),
            Some(&auth.0.user.id),
        )
        .await
        .map_err(komf_unreachable)?;
    if let Some(location) = response.headers().get(axum::http::header::LOCATION) {
        record_oauth_return_path(location, TRACKER_OAUTH_RETURN_PATH);
    }
    Ok(redirect_or_passthrough(response).await)
}

async fn tracker_oauth_status(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path(provider): Path<String>,
) -> Result<Response, ApiError> {
    check_oauth_provider(&provider)?;
    let row = connected_integration(&state)?;
    let response = komf::client(&state, &row)
        .proxy_tracker(
            Method::GET,
            &format!("{KOMF_OAUTH}/{provider}/status"),
            None,
            None,
            &auth.0.user.id,
        )
        .await
        .map_err(komf_unreachable)?;
    json_or_passthrough::<serde_json::Value>(response).await
}

async fn tracker_oauth_logout(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path(provider): Path<String>,
) -> Result<Response, ApiError> {
    check_oauth_provider(&provider)?;
    let row = connected_integration(&state)?;
    let response = komf::client(&state, &row)
        .proxy_tracker(
            Method::POST,
            &format!("{KOMF_OAUTH}/{provider}/logout"),
            None,
            None,
            &auth.0.user.id,
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
    komf::client(state, &row)
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
            serde_json::json!({"configured": false, "authKeySet": false, "komfReachable": false})
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
                "authKeySet": false,
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

    #[tokio::test]
    async fn put_persists_auth_key_and_get_reports_set_without_leaking_it() {
        let app = TestApp::new(router());
        let admin = insert_user(&app.state.db, "admin@x.c", true, true, &[]);
        insert_api_key(&app.state.db, &admin, "k-admin");
        let (komf_url, _) = serve_komf().await;

        let (status, body) = app
            .request_json(
                "PUT",
                "/api/v1/komf/integration",
                "k-admin",
                Some(serde_json::json!({
                    "url": komf_url,
                    "baseUrl": "http://kmrs:25600",
                    "authKey": "row-secret"
                })),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["authKeySet"], true);

        let stored = KomfIntegrationDao::new(app.state.kmrs_db.clone())
            .get()
            .unwrap()
            .unwrap();
        assert_eq!(stored.auth_key.as_deref(), Some("row-secret"));

        // the value itself never leaves the server
        let (_, body) = app.get_json("/api/v1/komf/integration", "k-admin").await;
        assert_eq!(body["authKeySet"], true);
        assert!(body.get("authKey").is_none());
    }

    #[tokio::test]
    async fn put_with_blank_auth_key_clears_the_override() {
        let app = TestApp::new(router());
        let admin = insert_user(&app.state.db, "admin@x.c", true, true, &[]);
        insert_api_key(&app.state.db, &admin, "k-admin");
        let (komf_url, _) = serve_komf().await;
        let body = serde_json::json!({
            "url": komf_url,
            "baseUrl": "http://kmrs:25600",
            "authKey": "row-secret"
        });
        app.request_json(
            "PUT",
            "/api/v1/komf/integration",
            "k-admin",
            Some(body.clone()),
        )
        .await;

        let mut cleared = body.clone();
        cleared["authKey"] = serde_json::json!("   ");
        let (status, response) = app
            .request_json("PUT", "/api/v1/komf/integration", "k-admin", Some(cleared))
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(response["authKeySet"], false);
        assert_eq!(
            KomfIntegrationDao::new(app.state.kmrs_db.clone())
                .get()
                .unwrap()
                .unwrap()
                .auth_key,
            None
        );
    }

    #[tokio::test]
    async fn put_without_auth_key_keeps_the_stored_override() {
        let app = TestApp::new(router());
        let admin = insert_user(&app.state.db, "admin@x.c", true, true, &[]);
        insert_api_key(&app.state.db, &admin, "k-admin");
        let (komf_url, _) = serve_komf().await;
        app.request_json(
            "PUT",
            "/api/v1/komf/integration",
            "k-admin",
            Some(serde_json::json!({
                "url": komf_url,
                "baseUrl": "http://kmrs:25600",
                "authKey": "row-secret"
            })),
        )
        .await;

        // an URL-only re-PUT must not silently drop the stored key
        let (status, body) = app
            .request_json(
                "PUT",
                "/api/v1/komf/integration",
                "k-admin",
                Some(serde_json::json!({"url": komf_url, "baseUrl": "http://kmrs:25600"})),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["authKeySet"], true);
        assert_eq!(
            KomfIntegrationDao::new(app.state.kmrs_db.clone())
                .get()
                .unwrap()
                .unwrap()
                .auth_key
                .as_deref(),
            Some("row-secret")
        );
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
                        ("GET", "/api/jobs/events") => (
                            [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                            "event: JobCreatedEvent\ndata: {\"type\":\"JobCreatedEvent\",\"jobId\":\"job-1\",\"seriesId\":\"ser-1\",\"startedAt\":\"2026-09-27T10:00:00Z\"}\n\nevent: ProviderBookEvent\ndata: {\"type\":\"ProviderBookEvent\",\"provider\":\"MANGADEX\",\"totalBooks\":10,\"bookProgress\":3,\"jobId\":\"job-1\",\"seriesId\":\"ser-1\"}\n\nevent: JobFinishedEvent\ndata: {\"type\":\"JobFinishedEvent\",\"jobId\":\"job-1\",\"seriesId\":\"ser-1\",\"status\":\"COMPLETED\",\"message\":null,\"finishedAt\":\"2026-09-27T10:01:00Z\"}\n\n",
                        )
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
                        // per-user tracker API
                        ("GET", "/api/tracker/links") => {
                            Json(serde_json::json!([{
                                "provider": "anilist",
                                "trackId": "42",
                                "title": "Linked",
                                "url": null,
                                "coverUrl": null,
                                "updatedAt": 1700000000
                            }]))
                            .into_response()
                        }
                        _ if method == "GET" && path.starts_with("/api/tracker/") && path.ends_with("/search") => {
                            Json(serde_json::json!([{
                                "id": "42",
                                "title": "Candidate",
                                "coverUrl": null,
                                "description": null,
                                "tracked": false,
                                "url": null
                            }]))
                            .into_response()
                        }
                        _ if method == "GET" && path.starts_with("/api/tracker/") && path.ends_with("/state") => {
                            Json(serde_json::json!({
                                "status": "reading",
                                "lastReadChapter": 1.0,
                                "totalChapters": 20
                            }))
                            .into_response()
                        }
                        _ if method == "POST" && path.starts_with("/api/tracker/") && path.ends_with("/update") => {
                            StatusCode::NO_CONTENT.into_response()
                        }
                        _ if method == "GET" && oauth_known && path.ends_with("/start") => {
                            // komf's real authorize redirect: state is JSON with the nonce
                            let state = "state=%7B%22redirectUrl%22%3A%22https%3A%2F%2Fkmrs.example%2Fcallback%22%2C%22nonce%22%3A%22test-nonce%22%7D";
                            (
                                StatusCode::FOUND,
                                [(
                                    axum::http::header::LOCATION,
                                    format!("https://provider.example/authorize?client_id=x&{state}"),
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
        dao.upsert(url, "http://kmrs:25600", None).unwrap();
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
        dao.upsert("http://komf:8085", "http://kmrs:25600", None)
            .unwrap();

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
    async fn jobs_events_relays_the_firehose() {
        let app = admin_app();
        let komf = serve_komf_proxy().await;
        seed_connected(&app.state, &komf.url);

        let (status, headers, bytes) = app
            .get_response("/api/v1/komf/jobs/events?ids=job-1,job-2", "k-admin")
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            headers[axum::http::header::CONTENT_TYPE],
            "text/event-stream"
        );
        assert_eq!(headers[axum::http::header::CACHE_CONTROL], "no-cache");
        let body = String::from_utf8(bytes).unwrap();
        assert!(body.contains("event: JobCreatedEvent"));
        assert!(body.contains("event: JobFinishedEvent"));
        assert!(body.contains("\"jobId\":\"job-1\""));

        // a single upstream request, whatever the ids
        let captured = komf.captured();
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0].path, "/api/jobs/events");
        assert_eq!(captured[0].query.as_deref(), Some("ids=job-1,job-2"));
    }

    #[tokio::test]
    async fn jobs_events_allows_missing_ids() {
        let app = admin_app();
        let komf = serve_komf_proxy().await;
        seed_connected(&app.state, &komf.url);

        let (status, _, bytes) = app
            .get_response("/api/v1/komf/jobs/events", "k-admin")
            .await;
        assert_eq!(status, StatusCode::OK);
        let body = String::from_utf8(bytes).unwrap();
        assert!(body.contains("event: JobCreatedEvent"));

        let captured = komf.captured();
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0].path, "/api/jobs/events");
        assert_eq!(captured[0].query, None);
    }

    #[tokio::test]
    async fn jobs_events_relays_upstream_errors() {
        let app = admin_app();
        let komf = serve_komf_proxy().await;
        seed_connected(&app.state, &komf.url);

        let (status, body) = app
            .get_json("/api/v1/komf/jobs/events?ids=broken-1", "k-admin")
            .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body, serde_json::json!({"message": "komf broke"}));
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
            .get_json(
                "/api/v1/komf/api/oauth/anilist/callback?code=x&state=y",
                "k-admin",
            )
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
            "https://provider.example/authorize?client_id=x&state=%7B%22redirectUrl%22%3A%22https%3A%2F%2Fkmrs.example%2Fcallback%22%2C%22nonce%22%3A%22test-nonce%22%7D"
        );

        let captured = komf.captured();
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0].method, "GET");
        assert_eq!(captured[0].path, "/api/oauth/anilist/start");
        assert_eq!(
            captured[0].query.as_deref(),
            Some("redirect_path_prefix=/api/v1/komf")
        );
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
            "/api/v1/komf/api/oauth/anilist/callback?code=auth-code&state=opaque",
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
            "/api/v1/komf/api/oauth/..%2F..%2Fapi%2Fconfig%3F/callback?code=x&state=y",
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

    fn user_app() -> (TestApp, String) {
        let app = TestApp::new(router());
        let user_id = insert_user(&app.state.db, "user@x.c", false, true, &[]);
        insert_api_key(&app.state.db, &user_id, "k-user");
        (app, user_id)
    }

    #[tokio::test]
    async fn tracker_links_crud_is_per_user() {
        let (app, _user_id) = user_app();
        let komf = serve_komf_proxy().await;
        seed_connected(&app.state, &komf.url);
        // empty list initially
        let (status, body) = app
            .get_json("/api/v1/komf/trackers/links/ser-1", "k-user")
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, serde_json::json!([]));

        // create two bindings on different providers
        for (provider, track_id) in [("anilist", "42"), ("mal", "99")] {
            let (status, body) = app
                .request_json(
                    "PUT",
                    "/api/v1/komf/trackers/links/ser-1",
                    "k-user",
                    Some(serde_json::json!({
                        "provider": provider,
                        "trackId": track_id,
                        "title": "Series",
                        "trackMode": "auto",
                        "chapterOffset": 2
                    })),
                )
                .await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(body["provider"], provider);
            assert_eq!(body["trackId"], track_id);
            assert_eq!(body["trackMode"], "auto");
            assert_eq!(body["chapterOffset"], 2);
        }

        // replace one binding: still one row per provider
        let (status, body) = app
            .request_json(
                "PUT",
                "/api/v1/komf/trackers/links/ser-1",
                "k-user",
                Some(serde_json::json!({
                    "provider": "anilist",
                    "trackId": "43",
                    "trackMode": "volume",
                    "chapterOffset": 0
                })),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["trackId"], "43");
        assert_eq!(body["trackMode"], "volume");

        let (status, body) = app
            .get_json("/api/v1/komf/trackers/links/ser-1", "k-user")
            .await;
        assert_eq!(status, StatusCode::OK);
        let links = body.as_array().unwrap();
        assert_eq!(links.len(), 2);
        assert_eq!(links[0]["provider"], "anilist");
        assert_eq!(links[0]["trackId"], "43");

        // another user does not see these bindings
        let other = insert_user(&app.state.db, "other@x.c", false, true, &[]);
        insert_api_key(&app.state.db, &other, "k-other");
        let (status, body) = app
            .get_json("/api/v1/komf/trackers/links/ser-1", "k-other")
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, serde_json::json!([]));

        // the user-wide listing spans series
        app.request_json(
            "PUT",
            "/api/v1/komf/trackers/links/ser-2",
            "k-user",
            Some(serde_json::json!({"provider": "bangumi", "trackId": "7"})),
        )
        .await;
        let (status, body) = app.get_json("/api/v1/komf/trackers/links", "k-user").await;
        assert_eq!(status, StatusCode::OK);
        let links = body.as_array().unwrap();
        assert_eq!(links.len(), 3);
        let (_, body) = app.get_json("/api/v1/komf/trackers/links", "k-other").await;
        assert_eq!(body, serde_json::json!([]));

        // delete only removes the target binding
        let (status, _) = app
            .request_json(
                "DELETE",
                "/api/v1/komf/trackers/links/ser-1/anilist",
                "k-user",
                None,
            )
            .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (_, body) = app
            .get_json("/api/v1/komf/trackers/links/ser-1", "k-user")
            .await;
        let links = body.as_array().unwrap();
        assert_eq!(links.len(), 1);
        assert_eq!(links[0]["provider"], "mal");
    }

    #[tokio::test]
    async fn tracker_link_upsert_validates_input() {
        let (app, user_id) = user_app();

        let (status, body) = app
            .request_json(
                "PUT",
                "/api/v1/komf/trackers/links/ser-1",
                "k-user",
                Some(serde_json::json!({"provider": "anilist", "trackId": "  "})),
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["violations"][0]["fieldName"], "trackId");

        let (status, body) = app
            .request_json(
                "PUT",
                "/api/v1/komf/trackers/links/ser-1",
                "k-user",
                Some(serde_json::json!({"provider": "anilist", "trackId": "1", "trackMode": "issues"})),
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["violations"][0]["fieldName"], "trackMode");

        let (status, _) = app
            .request_json(
                "PUT",
                "/api/v1/komf/trackers/links/ser-1",
                "k-user",
                Some(serde_json::json!({"provider": "AniList", "trackId": "1"})),
            )
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        // an unknown provider never reaches the DAO: no 500 from the CHECK
        let (status, _) = app
            .request_json(
                "PUT",
                "/api/v1/komf/trackers/links/ser-1",
                "k-user",
                Some(serde_json::json!({"provider": "foo", "trackId": "1"})),
            )
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        let (status, _) = app
            .request_json(
                "DELETE",
                "/api/v1/komf/trackers/links/ser-1/foo",
                "k-user",
                None,
            )
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        assert!(TrackerLinkDao::new(app.state.kmrs_db.clone())
            .list_by_series_and_user("ser-1", &user_id)
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn tracker_routes_need_a_connected_integration() {
        let (app, _) = user_app();
        let endpoints: [(&str, &str, Option<serde_json::Value>); 7] = [
            ("GET", "/api/v1/komf/trackers/links", None),
            ("GET", "/api/v1/komf/trackers/links/ser-1", None),
            ("GET", "/api/v1/komf/trackers/anilist/search?name=x", None),
            ("GET", "/api/v1/komf/trackers/anilist/state?trackId=1", None),
            (
                "POST",
                "/api/v1/komf/trackers/anilist/update",
                Some(serde_json::json!({"trackId": "1"})),
            ),
            ("GET", "/api/v1/komf/trackers/ledger", None),
            ("GET", "/api/v1/komf/trackers/oauth/anilist/status", None),
        ];
        for (method, path, body) in endpoints {
            let (status, _) = app.request_json(method, path, "k-user", body).await;
            assert_eq!(status, StatusCode::CONFLICT, "{method} {path}");
        }
    }

    #[tokio::test]
    async fn tracker_proxy_forwards_the_user_identity() {
        let (app, user_id) = user_app();
        let komf = serve_komf_proxy().await;
        seed_connected(&app.state, &komf.url);

        let (status, body) = app
            .get_json("/api/v1/komf/trackers/anilist/search?name=x", "k-user")
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body[0]["title"], "Candidate");

        let (status, body) = app
            .get_json("/api/v1/komf/trackers/anilist/state?trackId=1", "k-user")
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["status"], "reading");

        let (status, _) = app
            .request_json(
                "POST",
                "/api/v1/komf/trackers/anilist/update",
                "k-user",
                Some(serde_json::json!({"trackId": "1", "lastReadChapter": 3})),
            )
            .await;
        assert_eq!(status, StatusCode::NO_CONTENT);

        let (status, body) = app.get_json("/api/v1/komf/trackers/ledger", "k-user").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body[0]["trackId"], "42");

        let (status, body) = app
            .get_json("/api/v1/komf/trackers/oauth/anilist/status", "k-user")
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["logged_in"], true);

        let (status, _) = app
            .request_json(
                "POST",
                "/api/v1/komf/trackers/oauth/anilist/logout",
                "k-user",
                None,
            )
            .await;
        assert_eq!(status, StatusCode::NO_CONTENT);

        // every proxied request carries the kmrs user id as komf's tracker
        // identity; the guard must drop before the next request or the mock's
        // recording handler deadlocks on the same std mutex
        {
            let captured = komf.captured();
            assert_eq!(captured.len(), 6);
            for request in captured.iter() {
                assert_eq!(
                    request
                        .headers
                        .get("x-tracker-user")
                        .and_then(|v| v.to_str().ok()),
                    Some(user_id.as_str()),
                    "{} {}",
                    request.method,
                    request.path
                );
            }
        }

        // the user OAuth start additionally attributes the login via ?user= so
        // komf binds the granted token to this kmrs user on callback
        let (status, _) = app
            .get_json("/api/v1/komf/trackers/oauth/anilist/start", "k-user")
            .await;
        assert_eq!(status, StatusCode::FOUND);
        let captured = komf.captured();
        let start = captured.last().unwrap();
        assert_eq!(start.path, "/api/oauth/anilist/start");
        let query = start.query.as_deref().unwrap_or_default();
        assert!(query.contains("redirect_path_prefix=/api/v1/komf"));
        assert!(query.contains(&format!("user={user_id}")));
        assert_eq!(
            start
                .headers
                .get("x-tracker-user")
                .and_then(|v| v.to_str().ok()),
            Some(user_id.as_str())
        );
    }

    /// After a user-flow login the browser must land back on the trackers
    /// page; komf's own redirect always targets its root, so kmrs rewrites the
    /// Location using the nonce recorded at start.
    #[tokio::test]
    async fn tracker_preferences_roundtrip_is_per_user() {
        let (app, _user_id) = user_app();

        // default: unrestricted, no default tracker
        let (status, body) = app
            .get_json("/api/v1/komf/trackers/preferences", "k-user")
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, serde_json::json!({ "libraries": [] }));

        let (status, body) = app
            .request_json(
                "PUT",
                "/api/v1/komf/trackers/preferences",
                "k-user",
                Some(serde_json::json!({
                    "libraries": ["lib-1"],
                    "defaultTracker": "bangumi"
                })),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            body,
            serde_json::json!({
                "libraries": ["lib-1"],
                "defaultTracker": "bangumi"
            })
        );

        // other users are unaffected
        let other = insert_user(&app.state.db, "other@x.c", false, true, &[]);
        insert_api_key(&app.state.db, &other, "k-other");
        let (_, body) = app
            .get_json("/api/v1/komf/trackers/preferences", "k-other")
            .await;
        assert_eq!(body, serde_json::json!({ "libraries": [] }));
    }

    #[tokio::test]
    async fn tracker_preferences_rejects_unknown_default_tracker() {
        let (app, _) = user_app();

        let (status, body) = app
            .request_json(
                "PUT",
                "/api/v1/komf/trackers/preferences",
                "k-user",
                Some(serde_json::json!({ "defaultTracker": "shikimori" })),
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["violations"][0]["fieldName"], "defaultTracker");

        // the rejected write left nothing behind
        let (_, body) = app
            .get_json("/api/v1/komf/trackers/preferences", "k-user")
            .await;
        assert_eq!(body, serde_json::json!({ "libraries": [] }));
    }

    #[tokio::test]
    async fn user_oauth_callback_lands_on_the_trackers_page() {
        let (app, _) = user_app();
        let komf = serve_komf_proxy().await;
        seed_connected(&app.state, &komf.url);

        let (status, _, _) = app
            .get_response("/api/v1/komf/trackers/oauth/anilist/start", "k-user")
            .await;
        assert_eq!(status, StatusCode::FOUND);

        // the relay page returns through the anonymous callback with komf's state
        let (status, headers, _) = app
            .get_response(
                "/api/v1/komf/api/oauth/anilist/callback?code=abc&state=%7B%22redirectUrl%22%3A%22https%3A%2F%2Fkmrs.example%2Fcallback%22%2C%22nonce%22%3A%22test-nonce%22%7D",
                "k-user",
            )
            .await;
        assert_eq!(status, StatusCode::FOUND);
        assert_eq!(
            headers
                .get(axum::http::header::LOCATION)
                .unwrap()
                .to_str()
                .unwrap(),
            "/account/trackers?oauth=success"
        );
    }

    /// The admin flow records no return path, so its callback relays komf's
    /// Location verbatim and the webui forwards it to the komf settings page.
    #[tokio::test]
    async fn admin_oauth_callback_keeps_the_default_landing() {
        let app = admin_app();
        let komf = serve_komf_proxy().await;
        seed_connected(&app.state, &komf.url);

        let (status, _, _) = app
            .get_response("/api/v1/komf/oauth/anilist/start", "k-admin")
            .await;
        assert_eq!(status, StatusCode::FOUND);

        // a nonce kmrs never recorded: no rewrite
        let (status, headers, _) = app
            .get_response(
                "/api/v1/komf/api/oauth/anilist/callback?code=abc&state=%7B%22nonce%22%3A%22admin-nonce%22%7D",
                "k-admin",
            )
            .await;
        assert_eq!(status, StatusCode::FOUND);
        assert_eq!(
            headers
                .get(axum::http::header::LOCATION)
                .unwrap()
                .to_str()
                .unwrap(),
            "/?oauth=success"
        );
    }
}
