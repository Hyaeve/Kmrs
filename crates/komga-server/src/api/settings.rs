//! `SettingsController.kt`: server settings read/update.

use crate::auth::RequireAuth;
use crate::dto::settings::{SettingMultiSource, SettingsDto, SettingsUpdateDto, ThumbnailSizeDto};
use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::State;
use axum::http::StatusCode;
use axum::{routing, Json, Router};
use komga_db::dao::settings::SettingsDao;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/settings", routing::get(get_server_settings))
        .route("/api/v1/settings", routing::patch(update_server_settings))
}

/// Spring Boot's default `spring.servlet.multipart.max-file-size` (komga does not override it)
const DEFAULT_MAX_FILE_SIZE_BYTES: i64 = 1_048_576;

async fn get_server_settings(
    State(state): State<AppState>,
    auth: RequireAuth,
) -> Json<SettingsDto> {
    let s = state.settings.get();
    let config = &state.config;
    let dto = SettingsDto {
        delete_empty_collections: Some(s.delete_empty_collections),
        delete_empty_read_lists: Some(s.delete_empty_readlists),
        remember_me_duration_days: Some((s.remember_me_duration.as_secs() / 86_400) as i64),
        thumbnail_size: Some(ThumbnailSizeDto::from(s.thumbnail_size)),
        task_pool_size: Some(s.task_pool_size as i64),
        server_port: Some(SettingMultiSource {
            configuration_source: Some(config.port),
            database_source: s.server_port,
            effective_value: Some(s.server_port.unwrap_or(config.port)),
        }),
        server_context_path: Some(SettingMultiSource {
            configuration_source: config.server_context_path.clone(),
            database_source: s.server_context_path.clone(),
            effective_value: Some(s.server_context_path.clone().unwrap_or_default()),
        }),
        kobo_proxy: Some(s.kobo_proxy),
        kobo_port: s.kobo_port.map(|p| p as i64),
        kepubify_path: Some(SettingMultiSource {
            configuration_source: config
                .kepubify_path
                .as_ref()
                .map(|p| p.display().to_string()),
            database_source: s.kepubify_path.clone(),
            // Java reports the resolved path (DB first, configuration fallback, probed)
            effective_value: state
                .kepub
                .kepubify_path(&s, config)
                .map(|p| p.display().to_string()),
        }),
        max_upload_file_size_bytes: Some(DEFAULT_MAX_FILE_SIZE_BYTES),
    };
    if auth.0.user.is_admin() {
        Json(dto)
    } else {
        Json(dto.public())
    }
}

async fn update_server_settings(
    State(state): State<AppState>,
    auth: RequireAuth,
    Json(body): Json<SettingsUpdateDto>,
) -> Result<StatusCode, ApiError> {
    auth.0.require_admin()?;
    let dao = SettingsDao::new(state.db.clone());
    let mut violations = vec![];
    if body.task_pool_size.is_some_and(|v| v <= 0) {
        violations.push(violation("taskPoolSize", "must be greater than 0"));
    }
    if body.remember_me_duration_days.is_some_and(|v| v <= 0) {
        violations.push(violation(
            "rememberMeDurationDays",
            "must be greater than 0",
        ));
    }
    for (field, value) in [
        ("serverPort", &body.server_port),
        ("koboPort", &body.kobo_port),
    ] {
        if let Some(Some(v)) = value {
            if *v == 0 {
                violations.push(violation(field, "must be greater than 0"));
            }
        }
    }
    if let Some(Some(path)) = &body.server_context_path {
        if !valid_context_path(path) {
            violations.push(violation(
                "serverContextPath",
                "must match \"^/[\\w-/]*[a-zA-Z0-9]$\"",
            ));
        }
    }
    if !violations.is_empty() {
        return Err(ApiError::Violations(violations));
    }

    if let Some(v) = body.delete_empty_collections {
        dao.save_setting_bool("DELETE_EMPTY_COLLECTIONS", v)?;
    }
    if let Some(v) = body.delete_empty_read_lists {
        dao.save_setting_bool("DELETE_EMPTY_READLISTS", v)?;
    }
    if let Some(v) = body.remember_me_duration_days {
        dao.save_setting_i64("REMEMBER_ME_DURATION", v)?;
    }
    if body.renew_remember_me_key == Some(true) {
        state.settings.renew_remember_me_key();
    }
    if let Some(v) = body.thumbnail_size {
        dao.save_setting("THUMBNAIL_SIZE", v.as_str())?;
    }
    if let Some(v) = body.task_pool_size {
        dao.save_setting_i64("TASK_POOL_SIZE", v)?;
    }
    if let Some(v) = body.server_port {
        save_opt_i64(&dao, "SERVER_PORT", v)?;
    }
    if let Some(v) = &body.server_context_path {
        match v {
            Some(path) => dao.save_setting("SERVER_CONTEXT_PATH", path)?,
            None => dao.delete_setting("SERVER_CONTEXT_PATH")?,
        }
    }
    if let Some(v) = body.kobo_proxy {
        dao.save_setting_bool("KOBO_PROXY", v)?;
    }
    if let Some(v) = body.kobo_port {
        save_opt_i64(&dao, "KOBO_PORT", v)?;
    }
    if let Some(v) = &body.kepubify_path {
        match v {
            Some(path) => dao.save_setting("KEPUBIFY_PATH", path)?,
            None => dao.delete_setting("KEPUBIFY_PATH")?,
        }
    }

    state.settings.reload();
    Ok(StatusCode::NO_CONTENT)
}

fn save_opt_i64(dao: &SettingsDao, key: &str, value: Option<u16>) -> komga_db::Result<()> {
    match value {
        Some(v) => dao.save_setting_i64(key, v as i64),
        None => dao.delete_setting(key),
    }
}

fn violation(field: &str, message: &str) -> crate::error::Violation {
    crate::error::Violation {
        field_name: field.into(),
        message: message.into(),
    }
}

fn valid_context_path(path: &str) -> bool {
    if !path.starts_with('/') || path.len() < 2 {
        return false;
    }
    let last = path.chars().last().unwrap();
    last.is_ascii_alphanumeric()
        && path[1..]
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '/')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::collections::tests::{call, get, insert_user, test_state};
    use axum::body::Body;
    use axum::http::{Request, StatusCode};

    fn seed_admin(state: &AppState) -> String {
        insert_user(
            &state.db,
            "a@b.c",
            &[komga_core::model::user::UserRole::Admin],
            &[],
            Default::default(),
            "k",
        );
        "k".to_string()
    }

    #[tokio::test]
    async fn get_settings_admin_and_public() {
        let state = test_state();
        let key = seed_admin(&state);
        let (status, json) = {
            let (s, _, b) = call(&state, router(), get("/api/v1/settings", &key)).await;
            (s, serde_json::from_slice::<serde_json::Value>(&b).unwrap())
        };
        assert_eq!(status, StatusCode::OK);
        // migration placeholders default both to true
        assert_eq!(json["deleteEmptyCollections"], true);
        assert_eq!(json["deleteEmptyReadLists"], true);
        assert_eq!(json["rememberMeDurationDays"], 365);
        assert_eq!(json["thumbnailSize"], "DEFAULT");
        assert_eq!(json["taskPoolSize"], 1);
        assert_eq!(json["serverPort"]["effectiveValue"], 25600);
        assert_eq!(json["serverPort"]["configurationSource"], 25600);
        assert_eq!(json["koboProxy"], false);
        assert_eq!(json["maxUploadFileSizeBytes"], 1_048_576);

        // non-admin: only maxUploadFileSizeBytes survives
        insert_user(&state.db, "u@b.c", &[], &[], Default::default(), "u");
        let (status, json) = {
            let (s, _, b) = call(&state, router(), get("/api/v1/settings", "u")).await;
            (s, serde_json::from_slice::<serde_json::Value>(&b).unwrap())
        };
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["maxUploadFileSizeBytes"], 1_048_576);
        assert!(json.get("taskPoolSize").is_none());
        assert!(json.get("serverPort").is_none());
        assert!(json.get("deleteEmptyCollections").is_none());
    }

    async fn get_kepubify(state: &AppState, key: &str) -> serde_json::Value {
        let (status, json) = {
            let (s, _, b) = call(state, router(), get("/api/v1/settings", key)).await;
            (s, serde_json::from_slice::<serde_json::Value>(&b).unwrap())
        };
        assert_eq!(status, StatusCode::OK);
        json["kepubifyPath"].clone()
    }

    #[tokio::test]
    async fn kepubify_effective_value_resolves_db_then_configuration() {
        let dir = tempfile::tempdir().unwrap();
        let make_script = |name: &str| {
            let script = dir.path().join(name);
            std::fs::write(&script, "#!/bin/sh\n").unwrap();
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
            script
        };
        let config_script = make_script("kepubify-config");
        let db_script = make_script("kepubify-db");
        let config_str = config_script.to_str().unwrap().to_string();

        let mut state = test_state();
        let mut config = (*state.config).clone();
        config.kepubify_path = Some(config_script);
        state.config = std::sync::Arc::new(config);
        let key = seed_admin(&state);

        // no DB value: the configuration value becomes effective (the docker image case)
        let kepubify = get_kepubify(&state, &key).await;
        assert_eq!(kepubify["configurationSource"], config_str);
        assert_eq!(kepubify["databaseSource"], serde_json::Value::Null);
        assert_eq!(kepubify["effectiveValue"], config_str);

        // a valid DB value wins
        let dao = SettingsDao::new(state.db.clone());
        dao.save_setting("KEPUBIFY_PATH", db_script.to_str().unwrap())
            .unwrap();
        state.settings.reload();
        let kepubify = get_kepubify(&state, &key).await;
        assert_eq!(kepubify["databaseSource"], db_script.to_str().unwrap());
        assert_eq!(kepubify["effectiveValue"], db_script.to_str().unwrap());

        // an invalid DB value falls back to configuration
        dao.save_setting("KEPUBIFY_PATH", "/nonexistent/kepubify")
            .unwrap();
        state.settings.reload();
        let kepubify = get_kepubify(&state, &key).await;
        assert_eq!(kepubify["effectiveValue"], config_str);
    }

    #[tokio::test]
    async fn patch_updates_and_reload() {
        let state = test_state();
        seed_admin(&state);
        let body = serde_json::json!({
            "deleteEmptyCollections": true,
            "taskPoolSize": 4,
            "thumbnailSize": "LARGE",
            "serverPort": 8080,
            "serverContextPath": "/komga",
            "koboProxy": true
        });
        let (status, _, _) = call(
            &state,
            router(),
            Request::patch("/api/v1/settings")
                .header("X-API-Key", "k")
                .header("Content-Type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let s = state.settings.get();
        assert!(s.delete_empty_collections);
        assert_eq!(s.task_pool_size, 4);
        assert_eq!(s.thumbnail_size.as_str(), "LARGE");
        assert_eq!(s.server_port, Some(8080));
        assert_eq!(s.server_context_path.as_deref(), Some("/komga"));
        assert!(s.kobo_proxy);

        // isSet semantics: explicit null clears the DB value
        let body = serde_json::json!({"serverPort": null, "serverContextPath": null});
        let (status, _, _) = call(
            &state,
            router(),
            Request::patch("/api/v1/settings")
                .header("X-API-Key", "k")
                .header("Content-Type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let s = state.settings.get();
        assert_eq!(s.server_port, None);
        assert_eq!(s.server_context_path, None);
    }

    #[tokio::test]
    async fn patch_violations_and_renew_key() {
        let state = test_state();
        seed_admin(&state);

        let body = serde_json::json!({"taskPoolSize": 0, "serverContextPath": "komga"});
        let (status, json) = {
            let (s, _, b) = call(
                &state,
                router(),
                Request::patch("/api/v1/settings")
                    .header("X-API-Key", "k")
                    .header("Content-Type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await;
            (s, serde_json::from_slice::<serde_json::Value>(&b).unwrap())
        };
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let violations = json["violations"].as_array().unwrap();
        assert_eq!(violations.len(), 2);
        assert_eq!(violations[0]["fieldName"], "taskPoolSize");

        // renewRememberMeKey generates a new key
        let old_key = state.settings.get().remember_me_key;
        let body = serde_json::json!({"renewRememberMeKey": true});
        let (status, _, _) = call(
            &state,
            router(),
            Request::patch("/api/v1/settings")
                .header("X-API-Key", "k")
                .header("Content-Type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let new_key = state.settings.get().remember_me_key;
        assert_ne!(old_key, new_key);
        assert_eq!(new_key.len(), 32);
    }

    #[tokio::test]
    async fn patch_requires_admin() {
        let state = test_state();
        insert_user(&state.db, "u@b.c", &[], &[], Default::default(), "k");
        let (status, _, _) = call(
            &state,
            router(),
            Request::patch("/api/v1/settings")
                .header("X-API-Key", "k")
                .header("Content-Type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }
}
