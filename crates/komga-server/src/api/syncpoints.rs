//! `SyncPointController.kt`: `DELETE /api/v1/syncpoints/me`.

use crate::auth::RequireAuth;
use crate::error::ApiError;
use crate::http::pagination::QueryExt;
use crate::state::AppState;
use axum::extract::State;
use axum::http::StatusCode;
use axum::{routing, Router};
use komga_db::dao::sync_point::SyncPointDao;

pub fn router() -> Router<AppState> {
    Router::new().route(
        "/api/v1/syncpoints/me",
        routing::delete(delete_sync_points_for_current_user),
    )
}

async fn delete_sync_points_for_current_user(
    State(state): State<AppState>,
    auth: RequireAuth,
    query: crate::http::pagination::QueryPageable,
) -> Result<StatusCode, ApiError> {
    let dao = SyncPointDao::new(state.db.clone());
    let key_ids = query.params.all("key_id");
    if key_ids.is_empty() {
        dao.delete_by_user_id(&auth.0.user.id)?;
    } else {
        dao.delete_by_user_id_and_api_key_ids(&auth.0.user.id, key_ids)?;
    }
    Ok(StatusCode::NO_CONTENT)
}
