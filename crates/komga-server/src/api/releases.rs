//! `ReleaseController.kt`: GitHub releases proxy with a 1h cache.

use crate::auth::RequireAuth;
use crate::error::ApiError;
use crate::service::update_check::{fetch_releases, github_client, GithubReleaseDto, UpdateInfo};
use crate::state::AppState;
use axum::extract::State;
use axum::{routing, Json, Router};
use serde::{Deserialize, Serialize};

pub fn router() -> Router<AppState> {
    router_with_base("https://api.github.com/repos/kmworks/kmrs/releases")
}

pub(crate) fn router_with_base(base_url: &'static str) -> Router<AppState> {
    Router::new()
        .route("/api/v1/releases", routing::get(get_releases))
        // a layer applies only to the routes above it: the update check has no use for the GitHub client
        .layer(axum::Extension(ReleasesClient::new(base_url)))
        .route("/api/v1/update-check", routing::get(get_update_check))
}

#[derive(Clone)]
struct ReleasesClient {
    base_url: &'static str,
    http: reqwest::Client,
    /// Caffeine `expireAfterAccess(1h)`: fetch at most once per hour
    cache: moka::sync::Cache<String, Vec<GithubReleaseDto>>,
}

impl ReleasesClient {
    fn new(base_url: &'static str) -> Self {
        Self {
            base_url,
            http: github_client(),
            cache: moka::sync::Cache::builder()
                .time_to_idle(std::time::Duration::from_secs(3600))
                .build(),
        }
    }

    async fn cached_releases(&self) -> Option<Vec<GithubReleaseDto>> {
        if let Some(releases) = self.cache.get("releases") {
            return Some(releases);
        }
        let releases = match self.fetch().await {
            Ok(releases) => releases,
            Err(e) => {
                tracing::warn!("failed to fetch releases from GitHub: {e}");
                return None;
            }
        };
        self.cache.insert("releases".to_string(), releases.clone());
        Some(releases)
    }

    async fn fetch(&self) -> Result<Vec<GithubReleaseDto>, reqwest::Error> {
        fetch_releases(&self.http, self.base_url).await
    }
}

async fn get_releases(
    State(_state): State<AppState>,
    axum::Extension(client): axum::Extension<ReleasesClient>,
    auth: RequireAuth,
) -> Result<Json<Vec<ReleaseDto>>, ApiError> {
    auth.0.require_admin()?;
    let Some(releases) = client.cached_releases().await else {
        return Err(ApiError::not_found(""));
    };
    let dtos = releases
        .iter()
        .enumerate()
        .map(|(index, gh)| ReleaseDto {
            // the web UI flags the running release by comparing version to
            // build.version (CARGO_PKG_VERSION, no v prefix), while kmrs tags
            // carry one — strip it so the comparison can match
            version: gh
                .tag_name
                .strip_prefix('v')
                .unwrap_or(&gh.tag_name)
                .to_string(),
            release_date: gh.published_at,
            url: gh.html_url.clone(),
            latest: index == 0,
            pre_release: gh.prerelease,
            description: gh.body.clone(),
        })
        .collect();
    Ok(Json(dtos))
}

/// kmrs enhancement: the periodic update check's last outcome. Always 200: `update` is
/// null while the running version is up to date, `checkedAt` is null until the first
/// successful check.
async fn get_update_check(
    State(state): State<AppState>,
    auth: RequireAuth,
) -> Result<Json<UpdateCheckDto>, ApiError> {
    auth.0.require_admin()?;
    let outcome = state.latest_update.get();
    Ok(Json(UpdateCheckDto {
        enabled: state.config.update_check_enabled,
        checked_at: outcome.as_ref().map(|o| o.checked_at),
        update: outcome.and_then(|o| o.update),
    }))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheckDto {
    pub enabled: bool,
    #[serde(with = "komga_core::dto::progression::zoned_date_time_opt")]
    pub checked_at: Option<time::OffsetDateTime>,
    pub update: Option<UpdateInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseDto {
    pub version: String,
    #[serde(with = "komga_core::dto::progression::zoned_date_time")]
    pub release_date: time::OffsetDateTime,
    pub url: String,
    pub latest: bool,
    #[serde(rename = "preRelease")]
    pub pre_release: bool,
    pub description: String,
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::api::collections::tests::{call, get, insert_user, test_state};
    use crate::service::update_check::CheckOutcome;
    use axum::http::StatusCode;

    const RELEASES: &str = r#"[
        {"html_url": "https://github.com/kmworks/kmrs/releases/tag/v0.6.0", "tag_name": "v0.6.0",
         "published_at": "2023-12-15T00:00:00Z", "body": "new features", "prerelease": false},
        {"html_url": "https://github.com/kmworks/kmrs/releases/tag/v0.6.0-beta.1", "tag_name": "v0.6.0-beta.1",
         "published_at": "2023-12-01T00:00:00Z", "body": "beta", "prerelease": true}
    ]"#;

    pub(crate) async fn serve(body: &'static str) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                let (socket, _) = listener.accept().await.unwrap();
                let body = body.to_string();
                tokio::spawn(async move {
                    use tokio::io::{AsyncReadExt, AsyncWriteExt};
                    let mut socket = socket;
                    let mut request = Vec::new();
                    let mut chunk = [0u8; 1024];
                    loop {
                        let n = socket.read(&mut chunk).await.unwrap_or(0);
                        if n == 0 {
                            break;
                        }
                        request.extend_from_slice(&chunk[..n]);
                        if request.windows(4).any(|w| w == b"\r\n\r\n") {
                            break;
                        }
                    }
                    // GitHub rejects requests without a User-Agent with 403
                    let request = String::from_utf8_lossy(&request).to_lowercase();
                    let (status, body) = if request.contains("user-agent: kmrs/") {
                        ("200 OK", body)
                    } else {
                        ("403 Forbidden", "Forbidden".to_string())
                    };
                    let response = format!(
                        "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                });
            }
        });
        format!("http://127.0.0.1:{port}/releases")
    }

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
    async fn releases_list() {
        let state = test_state();
        let key = seed_admin(&state);
        let url = serve(RELEASES).await;
        let base: &'static str = Box::leak(url.into_boxed_str());

        let (status, json) = {
            let (s, _, b) = call(
                &state,
                router_with_base(base),
                get("/api/v1/releases", &key),
            )
            .await;
            (s, serde_json::from_slice::<serde_json::Value>(&b).unwrap())
        };
        assert_eq!(status, StatusCode::OK);
        let items = json.as_array().unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0]["version"], "0.6.0");
        assert_eq!(items[0]["latest"], true);
        assert_eq!(items[0]["preRelease"], false);
        assert_eq!(items[0]["description"], "new features");
        assert_eq!(items[1]["latest"], false);
        assert_eq!(items[1]["preRelease"], true);
        assert_eq!(items[0]["releaseDate"], "2023-12-15T00:00:00Z");
    }

    #[tokio::test]
    async fn update_check_reports_outcome() {
        let state = test_state();
        let key = seed_admin(&state);
        state.latest_update.set(Some(CheckOutcome {
            checked_at: time::macros::datetime!(2026-10-18 01:23:45 UTC),
            update: Some(UpdateInfo {
                version: "0.7.0".into(),
                url: "https://github.com/kmworks/kmrs/releases/tag/v0.7.0".into(),
                release_date: time::macros::datetime!(2026-10-15 12:34:56 UTC),
            }),
        }));
        let (status, _, body) = call(
            &state,
            router_with_base("http://127.0.0.1:1/releases"),
            get("/api/v1/update-check", &key),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "enabled": true,
                "checkedAt": "2026-10-18T01:23:45Z",
                "update": {
                    "version": "0.7.0",
                    "url": "https://github.com/kmworks/kmrs/releases/tag/v0.7.0",
                    "releaseDate": "2026-10-15T12:34:56Z",
                },
            })
        );
    }

    #[tokio::test]
    async fn update_check_before_first_run_is_all_null() {
        let state = test_state();
        let key = seed_admin(&state);
        let (status, _, body) = call(
            &state,
            router_with_base("http://127.0.0.1:1/releases"),
            get("/api/v1/update-check", &key),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"enabled": true, "checkedAt": null, "update": null})
        );
    }

    #[tokio::test]
    async fn update_check_requires_admin() {
        let state = test_state();
        insert_user(&state.db, "u@b.c", &[], &[], Default::default(), "k");
        let (status, _, _) = call(
            &state,
            router_with_base("http://127.0.0.1:1/releases"),
            get("/api/v1/update-check", "k"),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn fetch_failure_is_404() {
        let state = test_state();
        let key = seed_admin(&state);
        let (status, _, _) = call(
            &state,
            router_with_base("http://127.0.0.1:1/releases"),
            get("/api/v1/releases", &key),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn admin_required() {
        let state = test_state();
        insert_user(&state.db, "u@b.c", &[], &[], Default::default(), "k");
        let (status, _, _) = call(
            &state,
            router_with_base("http://127.0.0.1:1/releases"),
            get("/api/v1/releases", "k"),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }
}
