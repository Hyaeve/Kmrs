//! Login/logout:
//! - `GET /api/v1/login/set-cookie`: materializes the X-Auth-Token session into a cookie (204).
//! - `GET|POST /api/logout`: clears the session cookie and invalidates the session (204).

use crate::auth::{MaybeAuth, RequireAuth, SESSION_COOKIE_NAME, SESSION_HEADER_NAME};
use crate::state::AppState;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{routing, Router};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/login/set-cookie", routing::get(set_cookie))
        .route("/api/logout", routing::get(logout).post(logout))
}

async fn set_cookie(_auth: RequireAuth, headers: HeaderMap) -> Response {
    match headers
        .get(SESSION_HEADER_NAME)
        .and_then(|v| v.to_str().ok())
    {
        Some(session_id) => (
            StatusCode::NO_CONTENT,
            [(
                axum::http::header::SET_COOKIE,
                format!("{SESSION_COOKIE_NAME}={session_id}; Path=/; HttpOnly; SameSite=Lax"),
            )],
        )
            .into_response(),
        None => StatusCode::NO_CONTENT.into_response(),
    }
}

async fn logout(State(state): State<AppState>, auth: MaybeAuth, headers: HeaderMap) -> Response {
    let session_id = headers
        .get(SESSION_HEADER_NAME)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
        .or_else(|| {
            headers
                .get(axum::http::header::COOKIE)
                .and_then(|v| v.to_str().ok())
                .and_then(|header| {
                    header
                        .split(';')
                        .filter_map(|pair| {
                            let pair = pair.trim();
                            pair.split_once('=')
                                .filter(|(k, _)| *k == SESSION_COOKIE_NAME)
                                .map(|(_, v)| v.to_string())
                        })
                        .next()
                })
        });
    if let Some(id) = session_id {
        // the client discards the cookie either way; a failed delete is left to the
        // hourly cleanup rather than failing the logout
        if let Err(e) = state.sessions.invalidate(&id) {
            tracing::warn!("failed to delete session on logout: {e}");
        }
    }
    let _ = auth;
    (
        StatusCode::NO_CONTENT,
        [(
            axum::http::header::SET_COOKIE,
            format!("{SESSION_COOKIE_NAME}=; Path=/; Max-Age=0; HttpOnly; SameSite=Lax"),
        )],
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::collections::tests::{call, insert_user, test_state};
    use axum::body::Body;
    use axum::http::Request;

    #[tokio::test]
    async fn set_cookie_requires_auth_and_echoes_session() {
        let state = test_state();
        insert_user(&state.db, "u@b.c", &[], &[], Default::default(), "k");

        let (status, _, _) = call(
            &state,
            router(),
            Request::get("/api/v1/login/set-cookie")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        let (status, headers, _) = call(
            &state,
            router(),
            Request::get("/api/v1/login/set-cookie")
                .header("X-API-Key", "k")
                .header(SESSION_HEADER_NAME, "session-1")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let cookie = headers[axum::http::header::SET_COOKIE].to_str().unwrap();
        assert!(
            cookie.contains(&format!("{SESSION_COOKIE_NAME}=session-1")),
            "{cookie}"
        );
    }
}
