//! Equivalent of `TaskController`: `DELETE /api/v1/tasks` (clear the task queue).

use crate::auth::RequireAuth;
use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::State;
use axum::{routing, Json, Router};
use komga_db::dao::tasks::TasksDao;

pub fn router() -> Router<AppState> {
    Router::new().route("/api/v1/tasks", routing::delete(empty_task_queue))
}

async fn empty_task_queue(
    State(state): State<AppState>,
    auth: RequireAuth,
) -> Result<Json<i64>, ApiError> {
    auth.0.require_admin()?;
    let deleted = TasksDao::new(state.tasks_db.clone()).delete_all_without_owner()?;
    Ok(Json(deleted))
}

#[cfg(test)]
mod tests {
    use crate::api::libraries::test_support::{insert_api_key, insert_user, TestApp};
    use axum::http::StatusCode;
    use komga_core::task::{BookTaskKind, Task, DEFAULT_PRIORITY};
    use komga_db::dao::tasks::TasksDao;

    #[tokio::test]
    async fn clear_returns_deleted_count() {
        let app = TestApp::new(super::router());
        let dao = TasksDao::new(app.state.tasks_db.clone());
        dao.save(&Task::book(
            BookTaskKind::HashBook,
            "b1",
            DEFAULT_PRIORITY,
            None,
        ))
        .unwrap();
        dao.save(&Task::book(
            BookTaskKind::HashBook,
            "b2",
            DEFAULT_PRIORITY,
            None,
        ))
        .unwrap();
        // claimed tasks are not cleared
        dao.take_first("worker-1").unwrap();

        let admin = insert_user(&app.state.db, "admin@x.c", true, true, &[]);
        insert_api_key(&app.state.db, &admin, "k-admin");
        let user = insert_user(&app.state.db, "user@x.c", false, true, &[]);
        insert_api_key(&app.state.db, &user, "k-user");

        let (status, _) = app
            .request_json("DELETE", "/api/v1/tasks", "k-user", None)
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN);

        let (status, body) = app
            .request_json("DELETE", "/api/v1/tasks", "k-admin", None)
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, serde_json::json!(1));
        assert_eq!(dao.count().unwrap(), 1);
    }
}
