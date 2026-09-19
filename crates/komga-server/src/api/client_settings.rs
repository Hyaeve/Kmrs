//! `ClientSettingsController.kt`: per-user and global client settings.

use crate::auth::{MaybeAuth, RequireAuth};
use crate::dto::client_settings::{
    ClientSettingDto, ClientSettingGlobalUpdateDto, ClientSettingUserUpdateDto,
};
use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::State;
use axum::http::StatusCode;
use axum::{routing, Json, Router};
use komga_db::dao::settings::SettingsDao;
use std::collections::{BTreeMap, BTreeSet};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/client-settings/global/list",
            routing::get(get_global_settings),
        )
        .route(
            "/api/v1/client-settings/user/list",
            routing::get(get_user_settings),
        )
        .route(
            "/api/v1/client-settings/global",
            routing::patch(save_global_settings).delete(delete_global_settings),
        )
        .route(
            "/api/v1/client-settings/user",
            routing::patch(save_user_settings).delete(delete_user_settings),
        )
}

/// `^[a-z](?:[a-z0-9_-]*[a-z0-9])*(?:\.[a-z0-9](?:[a-z0-9_-]*[a-z0-9])*)*$`
fn valid_key(key: &str) -> bool {
    fn segment(s: &str, first: bool) -> bool {
        if s.is_empty() {
            return false;
        }
        let mut chars = s.chars();
        if first && !chars.next().unwrap().is_ascii_lowercase() {
            return false;
        }
        if !first {
            let c = chars.next().unwrap();
            if !(c.is_ascii_lowercase() || c.is_ascii_digit()) {
                return false;
            }
        }
        s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
    }
    let mut it = key.split('.');
    let Some(first) = it.next() else { return false };
    segment(first, true) && it.all(|s| segment(s, false))
}

async fn get_global_settings(
    State(state): State<AppState>,
    auth: MaybeAuth,
) -> Result<Json<BTreeMap<String, ClientSettingDto>>, ApiError> {
    let only_unauthorized = auth.0.is_none();
    let settings = SettingsDao::new(state.db.clone())
        .find_all_global(only_unauthorized)?
        .into_iter()
        .map(|s| {
            (
                s.key,
                ClientSettingDto {
                    value: s.value,
                    allow_unauthorized: Some(s.allow_unauthorized),
                },
            )
        })
        .collect();
    Ok(Json(settings))
}

async fn get_user_settings(
    State(state): State<AppState>,
    auth: RequireAuth,
) -> Result<Json<BTreeMap<String, ClientSettingDto>>, ApiError> {
    let settings = SettingsDao::new(state.db.clone())
        .find_all_user(&auth.0.user.id)?
        .into_iter()
        .map(|s| {
            (
                s.key,
                ClientSettingDto {
                    value: s.value,
                    allow_unauthorized: None,
                },
            )
        })
        .collect();
    Ok(Json(settings))
}

async fn save_global_settings(
    State(state): State<AppState>,
    auth: RequireAuth,
    Json(body): Json<BTreeMap<String, ClientSettingGlobalUpdateDto>>,
) -> Result<StatusCode, ApiError> {
    auth.0.require_admin()?;
    if let Some(key) = body.keys().find(|k| !valid_key(k)) {
        return Err(ApiError::Violations(vec![crate::error::Violation {
            field_name: key.clone(),
            message:
                "must match \"^[a-z](?:[a-z0-9_-]*[a-z0-9])*(?:\\.[a-z0-9](?:[a-z0-9_-]*[a-z0-9])*)*$\""
                    .to_string(),
        }]));
    }
    let dao = SettingsDao::new(state.db.clone());
    for (key, setting) in body {
        if setting.value.is_empty() {
            return Err(ApiError::Violations(vec![crate::error::Violation {
                field_name: key,
                message: "must not be blank".into(),
            }]));
        }
        dao.save_global(&key, &setting.value, setting.allow_unauthorized)?;
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn save_user_settings(
    State(state): State<AppState>,
    auth: RequireAuth,
    Json(body): Json<BTreeMap<String, ClientSettingUserUpdateDto>>,
) -> Result<StatusCode, ApiError> {
    if let Some(key) = body.keys().find(|k| !valid_key(k)) {
        return Err(ApiError::Violations(vec![crate::error::Violation {
            field_name: key.clone(),
            message:
                "must match \"^[a-z](?:[a-z0-9_-]*[a-z0-9])*(?:\\.[a-z0-9](?:[a-z0-9_-]*[a-z0-9])*)*$\""
                    .to_string(),
        }]));
    }
    let dao = SettingsDao::new(state.db.clone());
    for (key, setting) in body {
        if setting.value.is_empty() {
            return Err(ApiError::Violations(vec![crate::error::Violation {
                field_name: key,
                message: "must not be blank".into(),
            }]));
        }
        dao.save_for_user(&auth.0.user.id, &key, &setting.value)?;
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_global_settings(
    State(state): State<AppState>,
    auth: RequireAuth,
    Json(body): Json<BTreeSet<String>>,
) -> Result<StatusCode, ApiError> {
    auth.0.require_admin()?;
    if let Some(key) = body.iter().find(|k| !valid_key(k)) {
        return Err(ApiError::Violations(vec![crate::error::Violation {
            field_name: key.clone(),
            message:
                "must match \"^[a-z](?:[a-z0-9_-]*[a-z0-9])*(?:\\.[a-z0-9](?:[a-z0-9_-]*[a-z0-9])*)*$\""
                    .to_string(),
        }]));
    }
    SettingsDao::new(state.db.clone())
        .delete_global_by_keys(&body.into_iter().collect::<Vec<_>>())?;
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_user_settings(
    State(state): State<AppState>,
    auth: RequireAuth,
    Json(body): Json<BTreeSet<String>>,
) -> Result<StatusCode, ApiError> {
    if let Some(key) = body.iter().find(|k| !valid_key(k)) {
        return Err(ApiError::Violations(vec![crate::error::Violation {
            field_name: key.clone(),
            message:
                "must match \"^[a-z](?:[a-z0-9_-]*[a-z0-9])*(?:\\.[a-z0-9](?:[a-z0-9_-]*[a-z0-9])*)*$\""
                    .to_string(),
        }]));
    }
    SettingsDao::new(state.db.clone())
        .delete_by_user_id_and_keys(&auth.0.user.id, &body.into_iter().collect::<Vec<_>>())?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::collections::tests::{call, get, insert_user, test_state};
    use axum::body::Body;
    use axum::http::{Request, StatusCode};

    fn seed_users(state: &AppState) {
        insert_user(
            &state.db,
            "a@b.c",
            &[komga_core::model::user::UserRole::Admin],
            &[],
            Default::default(),
            "admin",
        );
        insert_user(&state.db, "u@b.c", &[], &[], Default::default(), "user");
    }

    fn seed_setting(state: &AppState, key: &str, value: &str, allow: bool) {
        SettingsDao::new(state.db.clone())
            .save_global(key, value, allow)
            .unwrap();
    }

    #[tokio::test]
    async fn global_list_visibility() {
        let state = test_state();
        seed_users(&state);
        seed_setting(&state, "app.public", "v1", true);
        seed_setting(&state, "app.secret", "v2", false);

        // anonymous: only allowUnauthorized=true
        let (status, json) = {
            let (s, _, b) = call(
                &state,
                router(),
                get("/api/v1/client-settings/global/list", "no-such-key"),
            )
            .await;
            (s, serde_json::from_slice::<serde_json::Value>(&b).unwrap())
        };
        assert_eq!(status, StatusCode::OK);
        let map = json.as_object().unwrap();
        assert_eq!(map.len(), 1);
        assert_eq!(map["app.public"]["value"], "v1");
        assert_eq!(map["app.public"]["allowUnauthorized"], true);

        // authenticated: everything
        let (status, json) = {
            let (s, _, b) = call(
                &state,
                router(),
                get("/api/v1/client-settings/global/list", "user"),
            )
            .await;
            (s, serde_json::from_slice::<serde_json::Value>(&b).unwrap())
        };
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json.as_object().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn global_save_and_delete() {
        let state = test_state();
        seed_users(&state);

        let body = serde_json::json!({
            "app.key1": {"value": "a string value", "allowUnauthorized": true},
            "app.key2": {"value": "{\"json\":\"object\"}", "allowUnauthorized": false}
        });
        let (status, _, _) = call(
            &state,
            router(),
            Request::patch("/api/v1/client-settings/global")
                .header("X-API-Key", "admin")
                .header("Content-Type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);

        let map = {
            let (_, _, b) = call(
                &state,
                router(),
                get("/api/v1/client-settings/global/list", "admin"),
            )
            .await;
            serde_json::from_slice::<serde_json::Value>(&b).unwrap()
        };
        assert_eq!(map["app.key1"]["value"], "a string value");
        assert_eq!(map["app.key1"]["allowUnauthorized"], true);
        assert_eq!(map["app.key2"]["allowUnauthorized"], false);

        // non-admin → 403
        let (status, _, _) = call(
            &state,
            router(),
            Request::patch("/api/v1/client-settings/global")
                .header("X-API-Key", "user")
                .header("Content-Type", "application/json")
                .body(Body::from(
                    r#"{"app.x": {"value": "y", "allowUnauthorized": false}}"#,
                ))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);

        // invalid key → violations
        let (status, json) = {
            let (s, _, b) = call(
                &state,
                router(),
                Request::patch("/api/v1/client-settings/global")
                    .header("X-API-Key", "admin")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        r#"{"Bad.Key": {"value": "y", "allowUnauthorized": false}}"#,
                    ))
                    .unwrap(),
            )
            .await;
            (s, serde_json::from_slice::<serde_json::Value>(&b).unwrap())
        };
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(json["violations"].is_array());

        // blank value → violations
        let (status, _, _) = call(
            &state,
            router(),
            Request::patch("/api/v1/client-settings/global")
                .header("X-API-Key", "admin")
                .header("Content-Type", "application/json")
                .body(Body::from(
                    r#"{"app.ok": {"value": "", "allowUnauthorized": false}}"#,
                ))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        // delete
        let (status, _, _) = call(
            &state,
            router(),
            Request::delete("/api/v1/client-settings/global")
                .header("X-API-Key", "admin")
                .header("Content-Type", "application/json")
                .body(Body::from(r#"["app.key1"]"#))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let map = {
            let (_, _, b) = call(
                &state,
                router(),
                get("/api/v1/client-settings/global/list", "admin"),
            )
            .await;
            serde_json::from_slice::<serde_json::Value>(&b).unwrap()
        };
        assert!(map.get("app.key1").is_none());
        assert!(map.get("app.key2").is_some());
    }

    #[tokio::test]
    async fn user_save_and_delete() {
        let state = test_state();
        seed_users(&state);

        let body = serde_json::json!({
            "app.key1": {"value": "a string value"},
            "app.key2": {"value": "{\"json\":\"object\"}"}
        });
        let (status, _, _) = call(
            &state,
            router(),
            Request::patch("/api/v1/client-settings/user")
                .header("X-API-Key", "user")
                .header("Content-Type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);

        let map = {
            let (_, _, b) = call(
                &state,
                router(),
                get("/api/v1/client-settings/user/list", "user"),
            )
            .await;
            serde_json::from_slice::<serde_json::Value>(&b).unwrap()
        };
        assert_eq!(map["app.key1"]["value"], "a string value");
        assert!(map["app.key1"].get("allowUnauthorized").is_none());

        // user settings are per-user: admin sees nothing
        let map = {
            let (_, _, b) = call(
                &state,
                router(),
                get("/api/v1/client-settings/user/list", "admin"),
            )
            .await;
            serde_json::from_slice::<serde_json::Value>(&b).unwrap()
        };
        assert_eq!(map.as_object().unwrap().len(), 0);

        // invalid key → violations
        let (status, _, _) = call(
            &state,
            router(),
            Request::patch("/api/v1/client-settings/user")
                .header("X-API-Key", "user")
                .header("Content-Type", "application/json")
                .body(Body::from(r#"{"1bad": {"value": "y"}}"#))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        // delete
        let (status, _, _) = call(
            &state,
            router(),
            Request::delete("/api/v1/client-settings/user")
                .header("X-API-Key", "user")
                .header("Content-Type", "application/json")
                .body(Body::from(r#"["app.key1"]"#))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let map = {
            let (_, _, b) = call(
                &state,
                router(),
                get("/api/v1/client-settings/user/list", "user"),
            )
            .await;
            serde_json::from_slice::<serde_json::Value>(&b).unwrap()
        };
        assert!(map.get("app.key1").is_none());
        assert!(map.get("app.key2").is_some());
    }
}
