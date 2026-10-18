//! kmrs-native server endpoints (`/api/v1/server/**`): what the webui needs from the
//! server itself — version info, scheduled scans, sessions, the log file and shutdown.
//! kmrs-private like `api::stats` (kept out of the OpenAPI spec). Unlike the actuator
//! parity surface, everything but `/info` is ADMIN-gated, shutdown included.

use crate::auth::RequireAuth;
use crate::error::ApiError;
use crate::state::AppState;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::header::{CONTENT_LENGTH, CONTENT_TYPE};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{routing, Json, Router};
use serde::Serialize;
use tokio_util::io::ReaderStream;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/server/info", routing::get(get_info))
        .route(
            "/api/v1/server/scheduled-tasks",
            routing::get(get_scheduled_tasks),
        )
        .route("/api/v1/server/sessions", routing::get(list_sessions))
        .route(
            "/api/v1/server/sessions/{id}",
            routing::delete(delete_session),
        )
        .route("/api/v1/server/logfile", routing::get(get_logfile))
        .route("/api/v1/server/shutdown", routing::post(post_shutdown))
}

// region info

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ServerInfoDto {
    version: String,
    git_branch: String,
    git_commit_id: String,
}

/// No admin gate: the sidebar footer shows the version to every logged-in user.
async fn get_info(_auth: RequireAuth) -> Json<ServerInfoDto> {
    Json(ServerInfoDto {
        version: env!("CARGO_PKG_VERSION").to_string(),
        git_branch: env!("GIT_BRANCH").to_string(),
        git_commit_id: env!("GIT_COMMIT_ID").to_string(),
    })
}

// endregion

// region scheduled-tasks

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ScheduledTasksDto {
    tasks: Vec<ScheduledTaskDto>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ScheduledTaskDto {
    library_id: String,
    library_name: String,
    interval_seconds: u64,
}

async fn get_scheduled_tasks(auth: RequireAuth) -> Result<Json<ScheduledTasksDto>, ApiError> {
    auth.0.require_admin()?;
    let tasks = crate::service::scheduler::ScanScheduler::scheduled_tasks()
        .into_iter()
        .map(|registration| ScheduledTaskDto {
            library_id: registration.library_id,
            library_name: registration.library_name,
            interval_seconds: registration.period.as_secs(),
        })
        .collect();
    Ok(Json(ScheduledTasksDto { tasks }))
}

// endregion

// region sessions

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionsDto {
    sessions: Vec<SessionDto>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionDto {
    id: String,
    creation_time: String,
    last_accessed_time: String,
    max_inactive_interval: u64,
    expired: bool,
}

async fn list_sessions(
    State(state): State<AppState>,
    auth: RequireAuth,
) -> Result<Json<SessionsDto>, ApiError> {
    auth.0.require_admin()?;
    let sessions = state
        .sessions
        .all()?
        .into_iter()
        .map(|(id, data)| session_dto(&id, &data, &state))
        .collect();
    Ok(Json(SessionsDto { sessions }))
}

async fn delete_session(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    auth.0.require_admin()?;
    state.sessions.invalidate(&id)?;
    Ok(StatusCode::NO_CONTENT)
}

/// `expired` is always false: the store only ever returns live sessions.
fn session_dto(id: &str, data: &crate::auth::session::SessionData, state: &AppState) -> SessionDto {
    let instant = |millis: u64| {
        komga_core::time_codec::format_offset_date_time(
            time::OffsetDateTime::from_unix_timestamp_nanos(millis as i128 * 1_000_000).unwrap(),
        )
    };
    SessionDto {
        id: id.to_string(),
        creation_time: instant(data.created_at),
        last_accessed_time: instant(data.last_accessed_at),
        max_inactive_interval: state.config.session_timeout.as_secs(),
        expired: false,
    }
}

// endregion

// region logfile

async fn get_logfile(
    auth: RequireAuth,
    State(state): State<AppState>,
) -> Result<Response, ApiError> {
    auth.0.require_admin()?;
    let logs_dir = state.config.config_dir.join("logs");
    let Some(path) = crate::api::actuator::latest_log_file(&logs_dir) else {
        return Ok(([(CONTENT_TYPE, "text/plain")], "").into_response());
    };
    let file = tokio::fs::File::open(&path)
        .await
        .map_err(|e| ApiError::Internal(format!("open {}: {e}", path.display())))?;
    let content_length = file.metadata().await.ok().map(|m| m.len().to_string());
    let body = Body::from_stream(ReaderStream::new(file));
    let mut response = ([(CONTENT_TYPE, "text/plain")], body).into_response();
    if let Some(len) = content_length {
        response
            .headers_mut()
            .insert(CONTENT_LENGTH, len.parse().unwrap());
    }
    Ok(response)
}

// endregion

// region shutdown

async fn post_shutdown(
    State(state): State<AppState>,
    auth: RequireAuth,
) -> Result<StatusCode, ApiError> {
    auth.0.require_admin()?;
    schedule_shutdown(state.shutdown_tx.clone());
    Ok(StatusCode::NO_CONTENT)
}

/// The response goes out first; the shutdown fires after a delay so the connection
/// can complete.
fn schedule_shutdown(tx: tokio::sync::watch::Sender<bool>) {
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        let _ = tx.send(true);
    });
}

// endregion

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::collections::tests::{call, get, insert_user, test_state};
    use axum::http::Request;
    use komga_core::model::user::UserRole;

    fn seed_admin(state: &AppState) -> String {
        insert_user(
            &state.db,
            "admin@x.c",
            &[UserRole::Admin],
            &[],
            Default::default(),
            "k-admin",
        );
        "k-admin".to_string()
    }

    fn seed_user(state: &AppState) -> String {
        insert_user(
            &state.db,
            "user@x.c",
            &[],
            &[],
            Default::default(),
            "k-user",
        );
        "k-user".to_string()
    }

    /// `get()` always sends an API key; anonymous and non-GET requests need a builder.
    fn request(method: &str, uri: &str, api_key: Option<&str>) -> Request<Body> {
        let mut builder = Request::builder().method(method).uri(uri);
        if let Some(key) = api_key {
            builder = builder.header("X-API-Key", key);
        }
        builder.body(Body::empty()).unwrap()
    }

    fn json(bytes: &[u8]) -> serde_json::Value {
        serde_json::from_slice(bytes).unwrap()
    }

    #[tokio::test]
    async fn info_shape_and_any_authenticated_user() {
        let state = test_state();
        let user_key = seed_user(&state);

        let (status, _, body) = call(&state, router(), get("/api/v1/server/info", &user_key)).await;
        assert_eq!(status, StatusCode::OK);
        let body = json(&body);
        assert_eq!(body["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(body["gitBranch"], env!("GIT_BRANCH"));
        assert_eq!(body["gitCommitId"], env!("GIT_COMMIT_ID"));

        let (status, _, _) = call(
            &state,
            router(),
            request("GET", "/api/v1/server/info", None),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn scheduled_tasks_empty_list_and_admin_gate() {
        let state = test_state();
        let admin_key = seed_admin(&state);
        let user_key = seed_user(&state);

        let (status, _, _) = call(
            &state,
            router(),
            get("/api/v1/server/scheduled-tasks", &user_key),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _, _) = call(
            &state,
            router(),
            request("GET", "/api/v1/server/scheduled-tasks", None),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        let (status, _, body) = call(
            &state,
            router(),
            get("/api/v1/server/scheduled-tasks", &admin_key),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json(&body), serde_json::json!({"tasks": []}));
    }

    #[tokio::test]
    async fn sessions_list_shape_and_delete_round_trip() {
        let state = test_state();
        let admin_key = seed_admin(&state);
        let user_key = seed_user(&state);

        let (status, _, _) =
            call(&state, router(), get("/api/v1/server/sessions", &user_key)).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _, _) = call(
            &state,
            router(),
            request("GET", "/api/v1/server/sessions", None),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        let user_id = insert_user(
            &state.db,
            "someone@x.c",
            &[],
            &[],
            Default::default(),
            "k-s",
        );
        let session_id = state.sessions.create(&user_id).unwrap();

        let (status, _, body) =
            call(&state, router(), get("/api/v1/server/sessions", &admin_key)).await;
        assert_eq!(status, StatusCode::OK);
        let sessions = json(&body);
        let sessions = sessions["sessions"].as_array().unwrap();
        assert_eq!(sessions.len(), 1);
        let session = &sessions[0];
        assert_eq!(session["id"], session_id);
        assert!(session["creationTime"].as_str().unwrap().ends_with('Z'));
        assert!(session["lastAccessedTime"].as_str().unwrap().ends_with('Z'));
        assert_eq!(session["maxInactiveInterval"], 604_800);
        assert_eq!(session["expired"], false);
        assert!(session.get("attributeNames").is_none());

        let (status, _, _) = call(
            &state,
            router(),
            request(
                "DELETE",
                &format!("/api/v1/server/sessions/{session_id}"),
                Some(&admin_key),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (status, _, body) =
            call(&state, router(), get("/api/v1/server/sessions", &admin_key)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json(&body)["sessions"], serde_json::json!([]));

        let (status, _, _) = call(
            &state,
            router(),
            request(
                "DELETE",
                "/api/v1/server/sessions/whatever",
                Some(&user_key),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn logfile_empty_200_without_logs_dir() {
        let state = test_state();
        let admin_key = seed_admin(&state);
        let user_key = seed_user(&state);
        // point at an empty config dir so the host's real logs can't leak into the test
        let dir = tempfile::tempdir().unwrap();
        let mut config = (*state.config).clone();
        config.config_dir = dir.path().to_path_buf();
        let state = AppState {
            config: std::sync::Arc::new(config),
            ..state
        };

        let (status, _, _) = call(&state, router(), get("/api/v1/server/logfile", &user_key)).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _, _) = call(
            &state,
            router(),
            request("GET", "/api/v1/server/logfile", None),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        let (status, headers, body) =
            call(&state, router(), get("/api/v1/server/logfile", &admin_key)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers.get(CONTENT_TYPE).unwrap(), "text/plain");
        assert!(body.is_empty());
    }

    /// Only the gate is asserted: the 204 path would send on `shutdown_tx` for real.
    #[tokio::test]
    async fn shutdown_requires_admin() {
        let state = test_state();
        let user_key = seed_user(&state);

        let (status, _, _) = call(
            &state,
            router(),
            request("POST", "/api/v1/server/shutdown", Some(&user_key)),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _, _) = call(
            &state,
            router(),
            request("POST", "/api/v1/server/shutdown", None),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }
}
