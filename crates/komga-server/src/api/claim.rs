//! Equivalent of `ClaimController`: GET/POST /api/v1/claim (anonymous).

use crate::auth::MaybeAuth;
use crate::dto::user::UserDto;
use crate::error::{ApiError, Violation};
use crate::state::AppState;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::{routing, Json, Router};
use komga_core::model::user::{ContentRestrictions, KomgaUser, UserRole};
use komga_core::time_codec::now_utc;
use komga_db::dao::user::UserDao;
use serde::Serialize;
use std::collections::BTreeSet;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaimStatus {
    pub is_claimed: bool,
}

pub fn router() -> Router<AppState> {
    Router::new().route("/api/v1/claim", routing::get(get_status).post(claim))
}

async fn get_status(State(state): State<AppState>) -> Json<ClaimStatus> {
    let count = UserDao::new(state.db.clone()).count().unwrap_or(0);
    Json(ClaimStatus {
        is_claimed: count > 0,
    })
}

async fn claim(
    State(state): State<AppState>,
    _auth: MaybeAuth,
    headers: HeaderMap,
) -> Result<(axum::http::StatusCode, Json<UserDto>), ApiError> {
    let email = headers
        .get("X-Komga-Email")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let password = headers
        .get("X-Komga-Password")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let mut violations = Vec::new();
    if !is_valid_email(&email) {
        violations.push(Violation {
            field_name: "claimServer.email".into(),
            message: "must be a well-formed email address".into(),
        });
    }
    if password.trim().is_empty() {
        violations.push(Violation {
            field_name: "claimServer.password".into(),
            message: "must not be blank".into(),
        });
    }
    if !violations.is_empty() {
        return Err(ApiError::Violations(violations));
    }

    let user_dao = UserDao::new(state.db.clone());
    if user_dao.count()? > 0 {
        return Err(ApiError::bad_request(
            "This server has already been claimed",
        ));
    }

    let user = KomgaUser {
        id: String::new(),
        email: email.clone(),
        password: bcrypt::hash(&password, 10).map_err(|e| ApiError::Internal(e.to_string()))?,
        roles: [
            UserRole::Admin,
            UserRole::FileDownload,
            UserRole::PageStreaming,
            UserRole::KoboSync,
            UserRole::KoreaderSync,
        ]
        .into_iter()
        .collect(),
        shared_all_libraries: true,
        shared_libraries_ids: BTreeSet::new(),
        restrictions: ContentRestrictions::default(),
        created_date: now_utc(),
        last_modified_date: now_utc(),
    };
    let id = user_dao.insert(&user)?;
    let created = user_dao
        .find_by_id(&id)?
        .ok_or_else(|| ApiError::Internal("user not found after insert".into()))?;
    Ok((axum::http::StatusCode::OK, Json(UserDto::from(&created))))
}

pub(crate) fn is_valid_email(email: &str) -> bool {
    // `.+@.+\..+`
    let Some((local, rest)) = email.split_once('@') else {
        return false;
    };
    !local.is_empty() && rest.contains('.') && !rest.starts_with('.') && !rest.ends_with('.')
}
