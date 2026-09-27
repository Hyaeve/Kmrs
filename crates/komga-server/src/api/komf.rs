//! kmrs-private komf integration management (admin only). Not part of the Komga API
//! surface, so it stays out of the OpenAPI spec.

use crate::auth::RequireAuth;
use crate::dto::komf::{KomfIntegrationDto, KomfIntegrationUpdateDto};
use crate::error::{ApiError, Violation};
use crate::service::komf::{self, KomfClient};
use crate::state::AppState;
use axum::extract::State;
use axum::http::StatusCode;
use axum::{routing, Json, Router};
use komga_db::dao::komf_integration::KomfIntegrationDao;

pub fn router() -> Router<AppState> {
    Router::new().route(
        "/api/v1/komf/integration",
        routing::get(get_integration)
            .put(put_integration)
            .delete(delete_integration),
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
        return Ok(KomfIntegrationDto {
            configured: false,
            url: None,
            base_url: None,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::libraries::test_support::{insert_api_key, insert_user, TestApp};
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
}
