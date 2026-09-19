//! `AnnouncementController.kt`: announcements proxy (komga.org JSON feed) with a 1h cache and
//! per-user read markers.

use crate::auth::RequireAuth;
use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::State;
use axum::http::StatusCode;
use axum::{routing, Json, Router};
use komga_db::dao::user::UserDao;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub fn router() -> Router<AppState> {
    router_with_base("https://komga.org/blog/feed.json")
}

pub(crate) fn router_with_base(base_url: &'static str) -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/announcements",
            routing::get(get_announcements).put(mark_announcements_read),
        )
        .layer(axum::Extension(AnnouncementsClient::new(base_url)))
}

#[derive(Clone)]
struct AnnouncementsClient {
    base_url: &'static str,
    http: reqwest::Client,
    /// Caffeine `expireAfterAccess(1h)`: fetch at most once per hour
    cache: moka::sync::Cache<String, Option<JsonFeedDto>>,
}

impl AnnouncementsClient {
    fn new(base_url: &'static str) -> Self {
        Self {
            base_url,
            http: reqwest::Client::new(),
            cache: moka::sync::Cache::builder()
                .time_to_idle(std::time::Duration::from_secs(3600))
                .build(),
        }
    }

    async fn cached_feed(&self) -> Option<JsonFeedDto> {
        if let Some(feed) = self.cache.get("announcements") {
            return feed;
        }
        let feed = self.fetch().await.ok();
        self.cache.insert("announcements".to_string(), feed.clone());
        feed
    }

    async fn fetch(&self) -> Result<JsonFeedDto, reqwest::Error> {
        self.http
            .get(self.base_url)
            .send()
            .await?
            .error_for_status()?
            .json::<JsonFeedDto>()
            .await
    }
}

async fn get_announcements(
    State(state): State<AppState>,
    axum::Extension(client): axum::Extension<AnnouncementsClient>,
    auth: RequireAuth,
) -> Result<Json<JsonFeedDto>, ApiError> {
    auth.0.require_admin()?;
    let Some(feed) = client.cached_feed().await else {
        return Err(ApiError::not_found(""));
    };
    let read = UserDao::new(state.db.clone()).find_announcement_ids_read(&auth.0.user.id)?;
    let feed = feed.mark_read(&read);
    Ok(Json(feed))
}

async fn mark_announcements_read(
    State(state): State<AppState>,
    auth: RequireAuth,
    Json(body): Json<BTreeSet<String>>,
) -> Result<StatusCode, ApiError> {
    auth.0.require_admin()?;
    UserDao::new(state.db.clone()).save_announcement_ids_read(&auth.0.user.id, &body)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JsonFeedDto {
    pub version: String,
    pub title: String,
    #[serde(rename = "home_page_url", skip_serializing_if = "Option::is_none")]
    pub home_page_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub items: Vec<FeedItemDto>,
}

impl JsonFeedDto {
    fn mark_read(&self, read: &BTreeSet<String>) -> Self {
        let mut feed = self.clone();
        for item in &mut feed.items {
            item.komga_extension = Some(KomgaExtensionDto {
                read: read.contains(&item.id),
            });
        }
        feed
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeedItemDto {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(rename = "content_html", skip_serializing_if = "Option::is_none")]
    pub content_html: Option<String>,
    #[serde(
        rename = "date_modified",
        with = "komga_core::dto::progression::zoned_date_time_opt",
        skip_serializing_if = "Option::is_none"
    )]
    pub date_modified: Option<time::OffsetDateTime>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<FeedAuthorDto>,
    #[serde(default)]
    pub tags: BTreeSet<String>,
    #[serde(rename = "_komga", skip_serializing_if = "Option::is_none")]
    pub komga_extension: Option<KomgaExtensionDto>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeedAuthorDto {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KomgaExtensionDto {
    pub read: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::collections::tests::{call, get, insert_user, test_state};
    use axum::http::StatusCode;

    const FEED: &str = r#"{
        "version": "https://jsonfeed.org/version/1",
        "title": "Announcements",
        "home_page_url": "https://komga.org/blog",
        "description": "Latest Komga announcements",
        "items": [
            {"id": "a1", "url": "https://komga.org/blog/a1", "title": "A One", "summary": "s1",
             "content_html": "<p>one</p>", "date_modified": "2023-12-15T00:00:00Z",
             "author": {"name": "gotson", "url": "https://github.com/gotson"}, "tags": ["upgrade", "komga"]},
            {"id": "a2", "url": "https://komga.org/blog/a2", "title": "A Two", "summary": "s2",
             "content_html": "<p>two</p>", "date_modified": "2023-11-29T00:00:00Z",
             "author": {"name": "gotson"}, "tags": []}
        ]
    }"#;

    async fn serve_feed(body: &'static str) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                let (socket, _) = listener.accept().await.unwrap();
                let body = body.to_string();
                tokio::spawn(async move {
                    use tokio::io::AsyncWriteExt;
                    let mut socket = socket;
                    let response = format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                });
            }
        });
        format!("http://127.0.0.1:{port}/feed.json")
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
    async fn feed_with_read_markers() {
        let state = test_state();
        let key = seed_admin(&state);
        let url = serve_feed(FEED).await;
        let base: &'static str = Box::leak(url.into_boxed_str());

        // mark a1 as read by this user
        let user_id = state
            .db
            .ro()
            .query_row("SELECT ID FROM USER WHERE EMAIL = 'a@b.c'", [], |r| {
                r.get::<_, String>(0)
            })
            .unwrap();
        UserDao::new(state.db.clone())
            .save_announcement_ids_read(&user_id, &BTreeSet::from(["a1".to_string()]))
            .unwrap();

        let (status, json) = {
            let (s, _, b) = call(
                &state,
                router_with_base(base),
                get("/api/v1/announcements", &key),
            )
            .await;
            (s, serde_json::from_slice::<serde_json::Value>(&b).unwrap())
        };
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["title"], "Announcements");
        let items = json["items"].as_array().unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0]["id"], "a1");
        assert_eq!(items[0]["_komga"]["read"], true);
        assert_eq!(items[1]["_komga"]["read"], false);
        assert_eq!(items[0]["author"]["name"], "gotson");
        assert_eq!(items[0]["date_modified"], "2023-12-15T00:00:00Z");

        // PUT mark read
        let (status, _, _) = call(
            &state,
            router_with_base(base),
            axum::http::Request::put("/api/v1/announcements")
                .header("X-API-Key", "k")
                .header("Content-Type", "application/json")
                .body(axum::body::Body::from(r#"["a2"]"#))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let read = UserDao::new(state.db.clone())
            .find_announcement_ids_read(&user_id)
            .unwrap();
        assert!(read.contains("a1") && read.contains("a2"));
    }

    #[tokio::test]
    async fn fetch_failure_is_404() {
        let state = test_state();
        let key = seed_admin(&state);
        // nothing listens on this port
        let (status, _, _) = call(
            &state,
            router_with_base("http://127.0.0.1:1/feed.json"),
            get("/api/v1/announcements", &key),
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
            router_with_base("http://127.0.0.1:1/feed.json"),
            get("/api/v1/announcements", "k"),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }
}
