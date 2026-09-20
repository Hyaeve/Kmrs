//! Equivalent of `UserController`: /api/v2/users/**.

use crate::api::claim::is_valid_email;
use crate::auth::RequireAuth;
use crate::dto::common::{Page, Pageable};
use crate::dto::user::*;
use crate::error::{ApiError, Violation};
use crate::http::pagination::{QueryExt, QueryPageable};
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::{routing, Json, Router};
use komga_core::model::user::{
    AgeRestriction, AllowExclude, ContentRestrictions, KomgaUser, UserRole,
};
use komga_core::time_codec::now_utc;
use komga_db::dao::user::UserDao;
use std::collections::BTreeSet;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v2/users/me", routing::get(get_me))
        .route(
            "/api/v2/users/me/password",
            routing::patch(update_my_password),
        )
        .route("/api/v2/users", routing::get(list_users).post(create_user))
        .route(
            "/api/v2/users/{id}",
            routing::patch(update_user).delete(delete_user),
        )
        .route(
            "/api/v2/users/{id}/password",
            routing::patch(update_password_by_id),
        )
        .route(
            "/api/v2/users/me/authentication-activity",
            routing::get(my_authentication_activity),
        )
        .route(
            "/api/v2/users/authentication-activity",
            routing::get(authentication_activity),
        )
        .route(
            "/api/v2/users/{id}/authentication-activity/latest",
            routing::get(latest_authentication_activity),
        )
        .route(
            "/api/v2/users/me/api-keys",
            routing::get(my_api_keys).post(create_api_key),
        )
        .route(
            "/api/v2/users/me/api-keys/{keyId}",
            routing::delete(delete_api_key),
        )
}

fn user_dao(state: &AppState) -> UserDao {
    UserDao::new(state.db.clone())
}

async fn get_me(auth: RequireAuth) -> Json<UserDto> {
    Json(UserDto::from(&auth.0.user))
}

fn validate_password(password: &str) -> Result<(), ApiError> {
    if password.trim().is_empty() {
        return Err(ApiError::Violations(vec![Violation {
            field_name: "password".into(),
            message: "must not be blank".into(),
        }]));
    }
    Ok(())
}

async fn update_my_password(
    State(state): State<AppState>,
    auth: RequireAuth,
    Json(body): Json<PasswordUpdateDto>,
) -> Result<StatusCode, ApiError> {
    validate_password(&body.password)?;
    let dao = user_dao(&state);
    let mut user = dao
        .find_by_email_ignore_case(&auth.0.user.email)?
        .ok_or_else(|| ApiError::not_found(""))?;
    user.password =
        bcrypt::hash(&body.password, 10).map_err(|e| ApiError::Internal(e.to_string()))?;
    dao.update(&user)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_users(
    State(state): State<AppState>,
    auth: RequireAuth,
) -> Result<Json<Vec<UserDto>>, ApiError> {
    auth.0.require_admin()?;
    let users = user_dao(&state).find_all()?;
    Ok(Json(users.iter().map(UserDto::from).collect()))
}

/// `UserRoles.valuesOf`: invalid role names are silently ignored.
fn values_of(roles: &[String]) -> BTreeSet<UserRole> {
    roles
        .iter()
        .filter_map(|r| r.parse::<UserRole>().ok())
        .collect()
}

fn age_restriction_of(dto: Option<AgeRestrictionUpdateDto>) -> Option<AgeRestriction> {
    match dto {
        None
        | Some(AgeRestrictionUpdateDto {
            restriction: AllowExcludeDto::None,
            ..
        }) => None,
        Some(d) => Some(AgeRestriction {
            age: d.age,
            restriction: match d.restriction {
                AllowExcludeDto::AllowOnly => AllowExclude::AllowOnly,
                AllowExcludeDto::Exclude => AllowExclude::Exclude,
                AllowExcludeDto::None => unreachable!(),
            },
        }),
    }
}

async fn create_user(
    State(state): State<AppState>,
    auth: RequireAuth,
    Json(body): Json<UserCreationDto>,
) -> Result<(StatusCode, Json<UserDto>), ApiError> {
    auth.0.require_admin()?;
    let mut violations = Vec::new();
    if !is_valid_email(&body.email) {
        violations.push(Violation {
            field_name: "email".into(),
            message: "must be a well-formed email address".into(),
        });
    }
    if body.password.trim().is_empty() {
        violations.push(Violation {
            field_name: "password".into(),
            message: "must not be blank".into(),
        });
    }
    if let Some(ar) = &body.age_restriction {
        if ar.age < 0 {
            violations.push(Violation {
                field_name: "ageRestriction.age".into(),
                message: "must be greater than or equal to 0".into(),
            });
        }
    }
    if !violations.is_empty() {
        return Err(ApiError::Violations(violations));
    }

    let dao = user_dao(&state);
    if dao.exists_by_email_ignore_case(&body.email)? {
        return Err(ApiError::bad_request(
            "A user with this email already exists",
        ));
    }
    // legacy behavior: when sharedLibraries is not provided, all libraries are shared by default
    let (shared_all, shared_ids) = match &body.shared_libraries {
        None => (true, BTreeSet::new()),
        Some(sl) if sl.all => (true, BTreeSet::new()),
        Some(sl) => (false, sl.library_ids.clone()),
    };
    let user = KomgaUser {
        id: String::new(),
        email: body.email.clone(),
        password: bcrypt::hash(&body.password, 10)
            .map_err(|e| ApiError::Internal(e.to_string()))?,
        roles: values_of(&body.roles),
        shared_all_libraries: shared_all,
        shared_libraries_ids: shared_ids,
        restrictions: ContentRestrictions::new(
            age_restriction_of(body.age_restriction),
            body.labels_allow.unwrap_or_default(),
            body.labels_exclude.unwrap_or_default(),
        ),
        created_date: now_utc(),
        last_modified_date: now_utc(),
    };
    let id = dao.insert(&user)?;
    let created = dao
        .find_by_id(&id)?
        .ok_or_else(|| ApiError::Internal("user not found after insert".into()))?;
    Ok((StatusCode::CREATED, Json(UserDto::from(&created))))
}

async fn update_user(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path(id): Path<String>,
    Json(patch): Json<UserUpdateDto>,
) -> Result<StatusCode, ApiError> {
    auth.0.require_admin()?;
    if auth.0.user.id == id {
        return Err(ApiError::forbidden(""));
    }
    let dao = user_dao(&state);
    let mut existing = dao
        .find_by_id(&id)?
        .ok_or_else(|| ApiError::not_found(""))?;

    if let Some(roles) = &patch.roles {
        // komga NPEs on explicit null (roles!!); aligned here as a 500
        let roles = roles
            .as_ref()
            .ok_or_else(|| ApiError::Internal("null value for roles".into()))?;
        existing.roles = values_of(&roles.iter().cloned().collect::<Vec<_>>());
    }
    if let Some(shared) = &patch.shared_libraries {
        let shared = shared
            .as_ref()
            .ok_or_else(|| ApiError::Internal("null value for sharedLibraries".into()))?;
        existing.shared_all_libraries = shared.all;
        existing.shared_libraries_ids = if shared.all {
            BTreeSet::new()
        } else {
            shared.library_ids.clone()
        };
    }
    let restrictions = &mut existing.restrictions;
    if let Some(age_restriction) = &patch.age_restriction {
        restrictions.age_restriction = age_restriction_of(*age_restriction);
    }
    if let Some(labels) = &patch.labels_allow {
        restrictions.labels_allow =
            komga_core::model::user::lower_not_blank(labels.clone().unwrap_or_default());
    }
    if let Some(labels) = &patch.labels_exclude {
        restrictions.labels_exclude =
            komga_core::model::user::lower_not_blank(labels.clone().unwrap_or_default());
    }
    // ContentRestrictions construction semantics: allow minus exclude
    existing.restrictions = ContentRestrictions::new(
        existing.restrictions.age_restriction,
        existing.restrictions.labels_allow.clone(),
        existing.restrictions.labels_exclude.clone(),
    );

    dao.update(&existing)?;
    // permission/sharing changes invalidate all sessions of this user (KomgaUserLifecycle semantics)
    state.sessions.invalidate_user(&id);
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_user(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    auth.0.require_admin()?;
    if auth.0.user.id == id {
        return Err(ApiError::forbidden(""));
    }
    let dao = user_dao(&state);
    dao.find_by_id(&id)?
        .ok_or_else(|| ApiError::not_found(""))?;
    dao.delete(&id)?;
    state.sessions.invalidate_user(&id);
    Ok(StatusCode::NO_CONTENT)
}

async fn update_password_by_id(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path(id): Path<String>,
    Json(body): Json<PasswordUpdateDto>,
) -> Result<StatusCode, ApiError> {
    validate_password(&body.password)?;
    if !auth.0.is_admin() && auth.0.user.id != id {
        return Err(ApiError::forbidden(""));
    }
    let dao = user_dao(&state);
    let mut user = dao
        .find_by_id(&id)?
        .ok_or_else(|| ApiError::not_found(""))?;
    user.password =
        bcrypt::hash(&body.password, 10).map_err(|e| ApiError::Internal(e.to_string()))?;
    dao.update(&user)?;
    // changing someone else's password invalidates their sessions; changing your own does not
    if auth.0.user.id != id {
        state.sessions.invalidate_user(&id);
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn my_authentication_activity(
    State(state): State<AppState>,
    auth: RequireAuth,
    query: QueryPageable,
) -> Result<Json<Page<AuthenticationActivityDto>>, ApiError> {
    let (items, total) = activity_page(&state, &query.pageable, Some(&auth.0.user))?;
    Ok(Json(Page::of(
        items.iter().map(AuthenticationActivityDto::from).collect(),
        total,
        &query.pageable,
    )))
}

async fn authentication_activity(
    State(state): State<AppState>,
    auth: RequireAuth,
    query: QueryPageable,
) -> Result<Json<Page<AuthenticationActivityDto>>, ApiError> {
    auth.0.require_admin()?;
    let (items, total) = activity_page(&state, &query.pageable, None)?;
    Ok(Json(Page::of(
        items.iter().map(AuthenticationActivityDto::from).collect(),
        total,
        &query.pageable,
    )))
}

/// Defaults to dateTime desc; returns everything when unpaged.
fn activity_page(
    state: &AppState,
    pageable: &Pageable,
    user: Option<&KomgaUser>,
) -> Result<(Vec<komga_core::model::user::AuthenticationActivity>, u64), ApiError> {
    let dao = user_dao(state);
    let (limit, offset) = if pageable.unpaged {
        (None, 0)
    } else {
        (Some(pageable.size), pageable.offset() as u32)
    };
    let (items, total) = match user {
        Some(user) => dao.find_activities_by_user(&user.id, &user.email, limit, offset)?,
        None => dao.find_all_activities(limit, offset)?,
    };
    Ok((items, total as u64))
}

async fn latest_authentication_activity(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path(id): Path<String>,
    query: QueryPageable,
) -> Result<Json<AuthenticationActivityDto>, ApiError> {
    if !auth.0.is_admin() && auth.0.user.id != id {
        return Err(ApiError::forbidden(""));
    }
    let dao = user_dao(&state);
    let user = dao
        .find_by_id(&id)?
        .ok_or_else(|| ApiError::not_found(""))?;
    let api_key_id = query.params.first("apikey_id").map(str::to_string);
    let activity = dao
        .find_most_recent_activity_by_user(&user.id, &user.email, api_key_id.as_deref())?
        .ok_or_else(|| ApiError::not_found(""))?;
    Ok(Json(AuthenticationActivityDto::from(&activity)))
}

async fn my_api_keys(
    auth: RequireAuth,
    State(state): State<AppState>,
) -> Result<Json<Vec<ApiKeyDto>>, ApiError> {
    let keys = user_dao(&state).find_api_keys_by_user_id(&auth.0.user.id)?;
    Ok(Json(keys.iter().map(ApiKeyDto::of_redacted).collect()))
}

async fn create_api_key(
    State(state): State<AppState>,
    auth: RequireAuth,
    Json(body): Json<ApiKeyRequestDto>,
) -> Result<Json<ApiKeyDto>, ApiError> {
    if body.comment.trim().is_empty() {
        return Err(ApiError::Violations(vec![Violation {
            field_name: "comment".into(),
            message: "must not be blank".into(),
        }]));
    }
    let dao = user_dao(&state);
    if dao.exists_api_key_by_comment_and_user_id(&body.comment, &auth.0.user.id)? {
        return Err(ApiError::bad_request(komga_core::error::codes::ERR_1034));
    }
    // komga retries generation up to 10 times (guards against unique key conflicts)
    for _ in 0..10 {
        let plain = uuid::Uuid::new_v4().simple().to_string();
        let api_key = komga_core::model::user::ApiKey {
            id: String::new(),
            user_id: auth.0.user.id.clone(),
            key: crate::auth::sha512_hex(&plain),
            comment: body.comment.clone(),
            created_date: now_utc(),
            last_modified_date: now_utc(),
        };
        match dao.insert_api_key(&api_key) {
            Ok(id) => {
                let mut dto = ApiKeyDto::of(&api_key);
                dto.id = id;
                dto.key = plain; // the plaintext is returned only this once
                return Ok(Json(dto));
            }
            Err(_) => continue,
        }
    }
    Err(ApiError::Status {
        status: StatusCode::SERVICE_UNAVAILABLE,
        message: "Failed to generate API key".into(),
    })
}

async fn delete_api_key(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path(key_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let dao = user_dao(&state);
    if !dao.exists_api_key_by_id_and_user_id(&key_id, &auth.0.user.id)? {
        return Err(ApiError::not_found(""));
    }
    dao.delete_api_key_by_id_and_user_id(&key_id, &auth.0.user.id)?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::SettingsProvider;
    use axum::body::Body;
    use axum::http::Request;
    use komga_core::model::user::ApiKey;
    use komga_db::pool::Database;
    use komga_db::{Migrator, Placeholders};
    use std::sync::Arc;
    use tower::ServiceExt;

    fn test_state() -> (AppState, tokio::sync::watch::Receiver<bool>) {
        let db = Database::open_in_memory(true).unwrap();
        let migrations = komga_db::main_migrations();
        Migrator::new(&migrations, Placeholders::default())
            .migrate(&db.rw())
            .unwrap();
        let tasks_db = Database::open_in_memory(false).unwrap();
        let tasks_migrations = komga_db::tasks_migrations();
        Migrator::new(&tasks_migrations, Placeholders::default())
            .migrate(&tasks_db.rw())
            .unwrap();
        let config = crate::config::ServerConfig::from_env();
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let state = AppState {
            config: Arc::new(config.clone()),
            settings: Arc::new(SettingsProvider::load(db.clone())),
            task_emitter: Arc::new(crate::service::TaskEmitter::new(
                db.clone(),
                tasks_db.clone(),
                std::sync::Arc::new(tokio::sync::Notify::new()),
            )),
            db,
            tasks_db,
            sessions: crate::auth::SessionStore::new(config.session_timeout),
            tsid: Arc::new(komga_core::tsid::TsidFactory::new_random_node()),
            events: crate::events::event_bus(),
            search_index: crate::state::test_search_index(),
            kepub: crate::service::kepub::KepubConverter::new(tempfile::tempdir().unwrap().keep()),
            kobo_proxy: crate::service::kobo_proxy::KoboProxy::new(),
            shutdown_tx,
        };
        (state, shutdown_rx)
    }

    fn test_router(state: AppState) -> Router {
        Router::new()
            .merge(router())
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                crate::auth::auth_middleware,
            ))
            .with_state(state)
    }

    fn seed_user(state: &AppState, email: &str) -> String {
        let dao = UserDao::new(state.db.clone());
        let user_id = dao
            .insert(&KomgaUser {
                id: String::new(),
                email: email.to_string(),
                password: bcrypt::hash("pass", 10).unwrap(),
                roles: [UserRole::Admin].into_iter().collect(),
                shared_libraries_ids: BTreeSet::new(),
                shared_all_libraries: true,
                restrictions: ContentRestrictions::default(),
                created_date: now_utc(),
                last_modified_date: now_utc(),
            })
            .unwrap();
        dao.insert_api_key(&ApiKey {
            id: String::new(),
            user_id: user_id.clone(),
            key: crate::auth::sha512_hex("secret"),
            comment: "test".into(),
            created_date: now_utc(),
            last_modified_date: now_utc(),
        })
        .unwrap();
        user_id
    }

    /// Activity is persisted on a spawned task; poll until it lands.
    async fn wait_activity(
        state: &AppState,
        user_id: &str,
        email: &str,
    ) -> komga_core::model::user::AuthenticationActivity {
        let dao = UserDao::new(state.db.clone());
        for _ in 0..100 {
            if let Ok(Some(activity)) = dao.find_most_recent_activity_by_user(user_id, email, None)
            {
                return activity;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        panic!("authentication activity was not recorded");
    }

    #[tokio::test]
    async fn api_key_success_records_user_id_and_email() {
        let (state, _rx) = test_state();
        let user_id = seed_user(&state, "Admin@Example.com");
        let app = test_router(state.clone());
        let request = Request::builder()
            .method("GET")
            .uri("/api/v2/users/me")
            .header("X-API-Key", "secret")
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let activity = wait_activity(&state, &user_id, "Admin@Example.com").await;
        assert_eq!(activity.user_id.as_deref(), Some(user_id.as_str()));
        assert_eq!(activity.email.as_deref(), Some("Admin@Example.com"));
        assert!(activity.success);
        assert_eq!(activity.source.as_deref(), Some("ApiKey"));
    }

    #[tokio::test]
    async fn password_success_records_user_id_and_canonical_email() {
        let (state, _rx) = test_state();
        let user_id = seed_user(&state, "Admin@Example.com");
        let app = test_router(state.clone());
        // credentials in a different case than the stored email
        let credentials = base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            "admin@example.com:pass",
        );
        let request = Request::builder()
            .method("GET")
            .uri("/api/v2/users/me")
            .header("Authorization", format!("Basic {credentials}"))
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        // LoginListener records the canonical user.email, not the submitted principal
        let activity = wait_activity(&state, &user_id, "Admin@Example.com").await;
        assert_eq!(activity.user_id.as_deref(), Some(user_id.as_str()));
        assert_eq!(activity.email.as_deref(), Some("Admin@Example.com"));
        assert!(activity.success);
        assert_eq!(activity.source.as_deref(), Some("Password"));
    }
}
