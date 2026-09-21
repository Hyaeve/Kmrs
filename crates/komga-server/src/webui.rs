//! Optional static hosting of a built web UI (e.g. kmweb): `webui.dir` points at a
//! build's dist directory. Unmatched paths outside the backend namespaces fall
//! back to its index.html — the SPA history-mode equivalent of Java's
//! `ResourceNotFoundController` forwarding to `/`.
//!
//! Cache headers mirror `WebMvcConfiguration`: content-hashed build output
//! (css/fonts/img/js/assets) is cached for a year, entry files (index.html, favicon
//! variants, manifest.json) are no-store.

use crate::state::AppState;
use axum::extract::{Request, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use tower::ServiceExt;
use tower_http::services::{ServeDir, ServeFile};

/// First path segments owned by the backend: misses inside them stay 404 instead of
/// falling through to the SPA (Java's forward excludes /api, /opds, /sse the same way).
/// `/login` is deliberately absent — the OAuth2 callback is a registered route, and
/// plain `/login` is a webui route.
const BACKEND_SEGMENTS: &[&str] = &[
    "api", "opds", "sse", "oauth2", "actuator", "kobo", "koreader", "v3", "debug",
];

/// Content-hashed build output, safe to cache long-term like Java's resource handler.
const LONG_CACHE_SEGMENTS: &[&str] = &["css", "fonts", "img", "js", "assets"];

pub async fn fallback(State(state): State<AppState>, request: Request) -> Response {
    let Some(dir) = state.config.webui_dir.clone() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let path = request.uri().path().to_string();
    if path.split('/').any(|s| s == "..") {
        // ServeDir would fall back to index.html here (no leak, but junk paths are 404)
        return StatusCode::NOT_FOUND.into_response();
    }
    let first_segment = path.split('/').nth(1).unwrap_or_default().to_string();
    if BACKEND_SEGMENTS.contains(&first_segment.as_str()) {
        return StatusCode::NOT_FOUND.into_response();
    }

    // `.fallback` (not `not_found_service`, which would force the status to 404):
    // missing files are served as index.html with 200, the SPA history-mode behavior
    let service = ServeDir::new(&dir).fallback(ServeFile::new(dir.join("index.html")));
    let mut response = match service.oneshot(request).await {
        Ok(response) => response.map(axum::body::Body::new),
        Err(e) => {
            tracing::warn!("webui static file error: {e}");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    if response.status().is_success() {
        let cache_control = if LONG_CACHE_SEGMENTS.contains(&first_segment.as_str()) {
            "max-age=31536000, public"
        } else {
            "no-store"
        };
        response.headers_mut().insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static(cache_control),
        );
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth;
    use crate::config::ServerConfig;
    use crate::settings::SettingsProvider;
    use axum::body::Body;
    use axum::http::Request;
    use komga_db::pool::{Database, JournalMode};
    use komga_db::{Migrator, Placeholders};
    use std::sync::Arc;
    use tower::ServiceExt;

    fn test_state(webui_dir: Option<std::path::PathBuf>) -> AppState {
        let db = Database::open_in_memory(true).unwrap();
        Migrator::new(&komga_db::main_migrations(), Placeholders::default())
            .migrate(&db.rw())
            .unwrap();
        let tasks_db = Database::open_in_memory(false).unwrap();
        Migrator::new(&komga_db::tasks_migrations(), Placeholders::default())
            .migrate(&tasks_db.rw())
            .unwrap();
        let db_config = |register_udfs| komga_db::pool::DatabaseConfig {
            file: std::env::temp_dir(),
            register_udfs,
            journal_mode: JournalMode::Wal,
            ..Default::default()
        };
        let config = ServerConfig {
            config_dir: std::env::temp_dir(),
            lucene_dir: std::env::temp_dir(),
            fonts_dir: std::env::temp_dir(),
            port: 0,
            database: db_config(true),
            tasks_db: db_config(false),
            session_timeout: std::time::Duration::from_secs(3600),
            cors_allowed_origins: vec![],
            page_hashing: 3,
            epub_divina_letter_count_threshold: 15,
            kobo_sync_item_limit: 100,
            kepubify_path: None,
            server_context_path: None,
            migration_placeholders: Default::default(),
            oauth2: Default::default(),
            webui_dir,
        };
        AppState {
            sessions: auth::SessionStore::new(config.session_timeout),
            settings: Arc::new(SettingsProvider::load(db.clone())),
            tsid: Arc::new(komga_core::tsid::TsidFactory::new_random_node()),
            events: crate::events::event_bus(),
            task_emitter: Arc::new(crate::service::TaskEmitter::new(
                db.clone(),
                tasks_db.clone(),
                std::sync::Arc::new(tokio::sync::Notify::new()),
            )),
            db,
            tasks_db,
            config: Arc::new(config),
            search_index: crate::state::test_search_index(),
            kepub: crate::service::kepub::KepubConverter::new(tempfile::tempdir().unwrap().keep()),
            kobo_proxy: crate::service::kobo_proxy::KoboProxy::new(),
            shutdown_tx: tokio::sync::watch::channel(false).0,
        }
    }

    fn dist() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("index.html"), "<html>spa</html>").unwrap();
        std::fs::create_dir(dir.path().join("js")).unwrap();
        std::fs::write(dir.path().join("js/app.abc123.js"), "console.log(1)").unwrap();
        std::fs::write(dir.path().join("manifest.json"), "{}").unwrap();
        dir
    }

    async fn get(app: &axum::Router, uri: &str) -> (StatusCode, HeaderMap, String) {
        let response = app
            .clone()
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (
            status,
            headers,
            String::from_utf8(body.to_vec()).unwrap_or_default(),
        )
    }

    use axum::http::HeaderMap;

    #[tokio::test]
    async fn serves_index_and_assets_with_java_cache_policy() {
        let dir = dist();
        let state = test_state(Some(dir.path().to_path_buf()));
        let app = crate::build_router(state);

        let (status, headers, body) = get(&app, "/").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers.get(header::CACHE_CONTROL).unwrap(), "no-store");
        assert_eq!(body, "<html>spa</html>");

        let (status, headers, body) = get(&app, "/js/app.abc123.js").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            headers.get(header::CACHE_CONTROL).unwrap(),
            "max-age=31536000, public"
        );
        assert!(headers
            .get(header::CONTENT_TYPE)
            .unwrap()
            .to_str()
            .unwrap()
            .contains("javascript"));
        assert_eq!(body, "console.log(1)");

        let (status, headers, _) = get(&app, "/manifest.json").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers.get(header::CACHE_CONTROL).unwrap(), "no-store");
    }

    #[tokio::test]
    async fn spa_history_routes_fall_back_to_index() {
        let dir = dist();
        let state = test_state(Some(dir.path().to_path_buf()));
        let app = crate::build_router(state);

        // a webui route like /login or /libraries/<id> is not a backend route
        for uri in ["/login", "/libraries/0ABCDEF", "/book/1/pages/2"] {
            let (status, headers, body) = get(&app, uri).await;
            assert_eq!(status, StatusCode::OK, "{uri}");
            assert_eq!(headers.get(header::CACHE_CONTROL).unwrap(), "no-store");
            assert_eq!(body, "<html>spa</html>", "{uri}");
        }
    }

    #[tokio::test]
    async fn backend_misses_stay_404() {
        let dir = dist();
        let state = test_state(Some(dir.path().to_path_buf()));
        let app = crate::build_router(state);

        for uri in [
            "/api/v1/nope",
            "/opds/v1.2/nope",
            "/sse/v1/nope",
            "/actuator/nope",
            "/oauth2/nope",
            "/koreader/nope",
        ] {
            let (status, _, _) = get(&app, uri).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{uri}");
        }
    }

    #[tokio::test]
    async fn path_traversal_does_not_escape_the_dir() {
        let dir = dist();
        let state = test_state(Some(dir.path().to_path_buf()));
        let app = crate::build_router(state);

        std::fs::write(dir.path().join("..").join("secret.txt"), "top secret").unwrap();
        let (status, _, body) = get(&app, "/../secret.txt").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_ne!(body, "top secret");
    }

    #[tokio::test]
    async fn xhr_401_has_no_basic_challenge() {
        let state = test_state(None);
        let app = crate::build_router(state);

        // the web UI tags every API call with X-Requested-With: a bare 401
        // keeps browsers out of their native sign-in dialog
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v2/users/me")
                    .header("x-requested-with", "XMLHttpRequest")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert!(response.headers().get(header::WWW_AUTHENTICATE).is_none());

        // the same request without the XHR tag keeps the Java challenge
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v2/users/me")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            response.headers().get(header::WWW_AUTHENTICATE).unwrap(),
            "Basic realm=\"Realm\""
        );
    }

    #[tokio::test]
    async fn disabled_by_default() {
        let state = test_state(None);
        let app = crate::build_router(state);
        let (status, _, _) = get(&app, "/").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }
}
