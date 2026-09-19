//! Equivalent of the read part of `LibraryController`: `GET /api/v1/libraries[/{libraryId}]`.

use crate::auth::RequireAuth;
use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::{routing, Json, Router};
use komga_core::dto::library::LibraryDto;
use komga_db::dao::library::LibraryDao;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/libraries", routing::get(get_libraries))
        .route(
            "/api/v1/libraries/{libraryId}",
            routing::get(get_library_by_id),
        )
}

async fn get_libraries(
    State(state): State<AppState>,
    auth: RequireAuth,
) -> Result<Json<Vec<LibraryDto>>, ApiError> {
    let user = &auth.0.user;
    let dao = LibraryDao::new(state.db.clone());
    let mut libraries = if user.can_access_all_libraries() {
        dao.find_all()?
    } else {
        dao.find_all()?
            .into_iter()
            .filter(|l| user.shared_libraries_ids.contains(&l.id))
            .collect()
    };
    libraries.sort_by_key(|l| l.name.to_lowercase());
    Ok(Json(
        libraries
            .iter()
            .map(|l| LibraryDto::of(l, user.is_admin()))
            .collect(),
    ))
}

async fn get_library_by_id(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path(library_id): Path<String>,
) -> Result<Json<LibraryDto>, ApiError> {
    let user = &auth.0.user;
    let library = LibraryDao::new(state.db.clone())
        .find_by_id(&library_id)?
        .ok_or_else(|| ApiError::not_found(""))?;
    if !user.can_access_library(&library.id) {
        return Err(ApiError::forbidden(""));
    }
    Ok(Json(LibraryDto::of(&library, user.is_admin())))
}

#[cfg(test)]
pub(crate) mod test_support {
    //! Shared oneshot-test scaffolding for endpoint tests: in-memory databases with all
    //! migrations applied, an AppState, and fixture inserters. Defined here (first endpoint
    //! module with tests) and reused by sibling endpoint modules.

    use crate::auth;
    use crate::settings::SettingsProvider;
    use crate::state::AppState;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use komga_core::model::user::{ApiKey, KomgaUser, UserRole};
    use komga_core::time_codec::now_utc;
    use komga_db::dao::user::UserDao;
    use komga_db::pool::Database;
    use komga_db::{Migrator, Placeholders};
    use std::path::PathBuf;
    use std::sync::Arc;
    use tower::ServiceExt;

    pub(crate) struct TestApp {
        pub state: AppState,
        app: axum::Router,
    }

    impl TestApp {
        pub(crate) fn new(routes: axum::Router<AppState>) -> Self {
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
            let state = AppState {
                config: Arc::new(test_config()),
                db: db.clone(),
                tasks_db,
                sessions: auth::SessionStore::new(std::time::Duration::from_secs(3600)),
                settings: Arc::new(SettingsProvider::load(db)),
                tsid: Arc::new(komga_core::tsid::TsidFactory::new_random_node()),
            };
            let app = routes
                .layer(axum::middleware::from_fn_with_state(
                    state.clone(),
                    auth::auth_middleware,
                ))
                .with_state(state.clone());
            Self { state, app }
        }

        pub(crate) async fn get(&self, uri: &str, api_key: &str) -> (StatusCode, Vec<u8>) {
            let request = Request::get(uri)
                .header("X-API-Key", api_key)
                .body(Body::empty())
                .unwrap();
            let response = self.app.clone().oneshot(request).await.unwrap();
            let status = response.status();
            let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap()
                .to_vec();
            (status, bytes)
        }

        pub(crate) async fn get_json(
            &self,
            uri: &str,
            api_key: &str,
        ) -> (StatusCode, serde_json::Value) {
            let (status, bytes) = self.get(uri, api_key).await;
            let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
            (status, json)
        }
    }

    fn test_config() -> crate::config::ServerConfig {
        crate::config::ServerConfig {
            config_dir: PathBuf::new(),
            database_file: PathBuf::new(),
            tasks_db_file: PathBuf::new(),
            lucene_dir: PathBuf::new(),
            fonts_dir: PathBuf::new(),
            port: 25600,
            database: Default::default(),
            tasks_db: Default::default(),
            session_timeout: std::time::Duration::from_secs(3600),
            cors_allowed_origins: vec![],
            page_hashing: 3,
            epub_divina_letter_count_threshold: 15,
            kobo_sync_item_limit: 100,
            kepubify_path: None,
        }
    }

    pub(crate) fn insert_user(
        db: &Database,
        email: &str,
        admin: bool,
        shared_all: bool,
        shared_ids: &[&str],
    ) -> String {
        let user = KomgaUser {
            id: String::new(),
            email: email.into(),
            password: "x".into(),
            roles: if admin {
                [UserRole::Admin].into_iter().collect()
            } else {
                Default::default()
            },
            shared_libraries_ids: shared_ids.iter().map(|s| s.to_string()).collect(),
            shared_all_libraries: shared_all,
            restrictions: Default::default(),
            created_date: now_utc(),
            last_modified_date: now_utc(),
        };
        UserDao::new(db.clone()).insert(&user).unwrap()
    }

    pub(crate) fn insert_api_key(db: &Database, user_id: &str, plain: &str) {
        let key = ApiKey {
            id: String::new(),
            user_id: user_id.into(),
            key: auth::sha512_hex(plain),
            comment: "test".into(),
            created_date: now_utc(),
            last_modified_date: now_utc(),
        };
        UserDao::new(db.clone()).insert_api_key(&key).unwrap();
    }

    pub(crate) fn insert_library(db: &Database, id: &str, name: &str) {
        let library = komga_core::model::library::Library {
            id: id.into(),
            name: name.into(),
            root: format!("file:/data/{}/", name.to_lowercase()),
            import_comicinfo_book: false,
            import_comicinfo_series: false,
            import_comicinfo_collection: false,
            import_comicinfo_readlist: false,
            import_comicinfo_series_append_volume: false,
            import_epub_book: false,
            import_epub_series: false,
            import_mylar_series: false,
            import_local_artwork: false,
            import_barcode_isbn: false,
            scan_force_modified_time: false,
            scan_on_startup: false,
            scan_interval: komga_core::model::library::ScanInterval::Daily,
            scan_cbx: true,
            scan_pdf: true,
            scan_epub: true,
            scan_directory_exclusions: vec![],
            repair_extensions: false,
            convert_to_cbz: false,
            empty_trash_after_scan: false,
            series_cover: komga_core::model::library::SeriesCover::First,
            hash_files: false,
            hash_pages: false,
            hash_koreader: false,
            analyze_dimensions: false,
            oneshots_directory: None,
            unavailable_date: None,
            created_date: now_utc(),
            last_modified_date: now_utc(),
        };
        komga_db::dao::library::LibraryDao::new(db.clone())
            .insert(&library)
            .unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::libraries::test_support::{
        insert_api_key, insert_library, insert_user, TestApp,
    };
    use axum::http::StatusCode;

    fn app() -> TestApp {
        TestApp::new(router())
    }

    #[tokio::test]
    async fn list_sorted_and_root_only_for_admin() {
        let app = app();
        insert_library(&app.state.db, "l1", "Zeta");
        insert_library(&app.state.db, "l2", "alpha");
        let admin = insert_user(&app.state.db, "admin@x.c", true, true, &[]);
        insert_api_key(&app.state.db, &admin, "k-admin");

        let (status, body) = app.get_json("/api/v1/libraries", "k-admin").await;
        assert_eq!(status, StatusCode::OK);
        // sorted by name lowercase
        assert_eq!(
            body.as_array().unwrap().len(),
            2,
            "expected both libraries: {body}"
        );
        assert_eq!(body[0]["name"], "alpha");
        assert_eq!(body[1]["name"], "Zeta");
        assert_eq!(body[0]["root"], "/data/alpha/");

        let user = insert_user(&app.state.db, "user@x.c", false, true, &[]);
        insert_api_key(&app.state.db, &user, "k-user");
        let (status, body) = app.get_json("/api/v1/libraries", "k-user").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body[0]["root"], "");
    }

    #[tokio::test]
    async fn list_filters_shared_libraries() {
        let app = app();
        insert_library(&app.state.db, "l1", "One");
        insert_library(&app.state.db, "l2", "Two");
        let user = insert_user(&app.state.db, "user@x.c", false, false, &["l2"]);
        insert_api_key(&app.state.db, &user, "k-user");

        let (status, body) = app.get_json("/api/v1/libraries", "k-user").await;
        assert_eq!(status, StatusCode::OK);
        let names: Vec<&str> = body
            .as_array()
            .unwrap()
            .iter()
            .map(|l| l["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["Two"]);
    }

    #[tokio::test]
    async fn by_id_not_found_and_forbidden() {
        let app = app();
        insert_library(&app.state.db, "l1", "One");
        let admin = insert_user(&app.state.db, "admin@x.c", true, true, &[]);
        insert_api_key(&app.state.db, &admin, "k-admin");
        let user = insert_user(&app.state.db, "user@x.c", false, false, &[]);
        insert_api_key(&app.state.db, &user, "k-user");

        let (status, body) = app.get_json("/api/v1/libraries/l1", "k-admin").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["name"], "One");

        let (status, _) = app.get_json("/api/v1/libraries/nope", "k-admin").await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        let (status, _) = app.get_json("/api/v1/libraries/l1", "k-user").await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }
}
