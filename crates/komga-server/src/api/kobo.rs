//! `KoboController.kt`: the Kobo sync endpoints under `/kobo/{authToken}/`.
//!
//! Authentication is the API key in the URL path (see `koboFilterChain` +
//! `UriRegexApiKeyAuthenticationConverter`): `sha512(token)` must match a USER_API_KEY row, and
//! the user needs the KOBO_SYNC role. Requests the local library cannot answer fall back to the
//! bundled noproxy responses, or to the Kobo store when the `koboProxy` setting is on
//! (`service/kobo_proxy.rs`). `convert_kepub` downloads are converted through the external
//! kepubify binary when configured (`service/kepub.rs`).

use crate::api::restriction;
use crate::auth::{sha512_hex, RequireAuth};
use crate::dto::kobo::*;
use crate::error::ApiError;
use crate::http::headers::content_disposition;
use crate::service::kobo_proxy::{KoboProxy, ProxiedResponse, ProxyError, IMAGE_HOST_URL};
use crate::state::AppState;
use axum::body::{Body, Bytes};
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::{routing, Json, Router};
use komga_core::dto::url_to_file_path;
use komga_core::model::read_progress::ReadProgress;
use komga_core::model::sync_point::SyncPoint;
use komga_core::model::user::{KomgaUser, UserRole};
use komga_core::time_codec;
use komga_db::dao::book::BookDao;
use komga_db::dao::media::MediaDao;
use komga_db::dao::sync_point::{SyncPage, SyncPointDao};
use komga_db::dao::thumbnail::ThumbnailBookDao;
use komga_db::dao::user::UserDao;
use komga_db::dto_dao::kobo::{by_entitlement_id, KoboDtoDao};
use serde::Serialize;
use std::collections::BTreeMap;

const X_KOBO_SYNCTOKEN: &str = "x-kobo-synctoken";
const X_KOBO_SYNC: &str = "x-kobo-sync";
const X_KOBO_APITOKEN: &str = "x-kobo-apitoken";
const X_KOBO_DEVICEID: &str = "x-kobo-deviceid";

// region store proxy

async fn proxy_current(
    state: &AppState,
    method: &Method,
    uri: &Uri,
    headers: &HeaderMap,
    body: Option<Bytes>,
    sync_token: Option<&KomgaSyncToken>,
) -> Result<ProxiedResponse, ProxyError> {
    state
        .kobo_proxy
        .proxy(method, uri.path(), uri.query(), headers, body, sync_token)
        .await
}

fn proxy_enabled(state: &AppState) -> bool {
    KoboProxy::is_enabled(&state.settings.get())
}

/// `ResponseEntity<JsonNode>`: upstream status + `x-kobo-*` headers + buffered JSON body
fn proxied_response(proxied: ProxiedResponse) -> Response {
    let mut response = (proxied.status, Json(proxied.body)).into_response();
    response.headers_mut().extend(proxied.headers);
    response
}

/// `ResponseStatusException(status, statusText)` for upstream errors
fn proxy_error(e: ProxyError) -> ApiError {
    match e {
        ProxyError::Upstream(status) => ApiError::Status {
            status,
            message: status.canonical_reason().unwrap_or_default().to_string(),
        },
        ProxyError::Internal(message) => ApiError::Internal(message),
    }
}

// endregion

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/kobo/{authToken}/ping", routing::get(ping))
        .route("/kobo/{authToken}/v1/initialization", routing::get(initialization))
        .route("/kobo/{authToken}/v1/auth/device", routing::post(auth_device))
        .route("/kobo/{authToken}/v1/library/sync", routing::get(sync_library))
        .route(
            "/kobo/{authToken}/v1/library/{bookId}/metadata",
            routing::get(get_book_metadata),
        )
        .route("/kobo/{authToken}/v1/library/{bookId}/state", routing::get(get_state))
        .route("/kobo/{authToken}/v1/library/{bookId}/state", routing::put(update_state))
        .route(
            "/kobo/{authToken}/v1/books/{bookId}/file/epub",
            routing::get(get_book_file),
        )
        .route(
            "/kobo/{authToken}/v1/books/{thumbnailId}/thumbnail/{width}/{height}/{isGreyScale}/image.jpg",
            routing::get(get_book_cover),
        )
        .route(
            "/kobo/{authToken}/v1/books/{thumbnailId}/thumbnail/{width}/{height}/{quality}/{isGreyScale}/image.jpg",
            routing::get(get_book_cover_quality),
        )
        .route("/kobo/{authToken}/v1/analytics/gettests", routing::get(analytics_get_tests).post(analytics_get_tests))
        .route("/kobo/{authToken}/{*path}", routing::get(catch_all).put(catch_all).post(catch_all).delete(catch_all).patch(catch_all))
}

// region kobo authentication

/// The API key from the URL path segment (`/kobo/{authToken}/...`).
pub struct KoboAuth {
    pub user: KomgaUser,
    pub api_key_id: Option<String>,
    pub api_key_comment: Option<String>,
}

impl axum::extract::FromRequestParts<AppState> for KoboAuth {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = parts
            .uri
            .path()
            .split('/')
            .nth(2)
            .filter(|s| !s.is_empty())
            .ok_or_else(ApiError::unauthorized)?;
        let hashed = sha512_hex(token);
        let dao = UserDao::new(state.db.clone());
        let Some((user, api_key)) = dao.find_by_api_key(&hashed)? else {
            let activity = crate::auth::ActivityDraft {
                user_id: None,
                email: None,
                api_key_id: None,
                // LoginListener.onFailure stores the masked key as apiKeyComment
                api_key_comment: Some(komga_media::hash::compute_hash_bytes(token.as_bytes())),
                success: false,
                error: Some("Bad credentials".into()),
                source: "ApiKey".into(),
            };
            state.record_activity(&Some(activity), parts).await;
            return Err(ApiError::unauthorized());
        };
        if !user.roles.contains(&UserRole::KoboSync) {
            return Err(ApiError::forbidden(""));
        }
        let activity = crate::auth::ActivityDraft {
            user_id: Some(api_key.user_id.clone()),
            email: Some(user.email.clone()),
            api_key_id: Some(api_key.id.clone()),
            api_key_comment: Some(api_key.comment.clone()),
            success: true,
            error: None,
            source: "ApiKey".into(),
        };
        state.record_activity(&Some(activity), parts).await;
        Ok(KoboAuth {
            user,
            api_key_id: Some(api_key.id),
            api_key_comment: Some(api_key.comment),
        })
    }
}

// endregion

// region sync token (`KomgaSyncTokenGenerator`)

const KOMGA_TOKEN_PREFIX: &str = "KOMGA.";

#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KomgaSyncToken {
    #[serde(default = "token_version")]
    pub version: i32,
    #[serde(default)]
    pub raw_kobo_sync_token: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ongoing_sync_point_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_successful_sync_point_id: Option<String>,
}

fn token_version() -> i32 {
    1
}

impl Default for KomgaSyncToken {
    fn default() -> Self {
        Self {
            version: 1,
            raw_kobo_sync_token: String::new(),
            ongoing_sync_point_id: None,
            last_successful_sync_point_id: None,
        }
    }
}

impl KomgaSyncToken {
    pub fn to_base64(&self) -> String {
        use base64::Engine;
        let json = serde_json::to_vec(self).expect("sync token serialization cannot fail");
        format!(
            "{}{}",
            KOMGA_TOKEN_PREFIX,
            base64::engine::general_purpose::STANDARD_NO_PAD.encode(json)
        )
    }

    pub fn from_base64(token: &str) -> KomgaSyncToken {
        use base64::Engine;
        if let Some(stripped) = token.strip_prefix(KOMGA_TOKEN_PREFIX) {
            if let Ok(json) = base64::engine::general_purpose::STANDARD_NO_PAD.decode(stripped) {
                if let Ok(token) = serde_json::from_slice::<KomgaSyncToken>(&json) {
                    return token;
                }
            }
        } else if !token.contains('.') {
            // possible Calibre Web token: base64 of {"data": {"raw_kobo_store_token": "..."}}
            if let Ok(json) = base64::engine::general_purpose::STANDARD_NO_PAD.decode(token) {
                if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&json) {
                    if let Some(raw) = value
                        .get("data")
                        .and_then(|d| d.get("raw_kobo_store_token"))
                        .and_then(|v| v.as_str())
                    {
                        return KomgaSyncToken {
                            raw_kobo_sync_token: raw.to_string(),
                            ..Default::default()
                        };
                    }
                }
            }
        } else if token.contains('.') {
            // official Kobo store token (`base64.base64`): kept as-is
            return KomgaSyncToken {
                raw_kobo_sync_token: token.to_string(),
                ..Default::default()
            };
        }
        KomgaSyncToken::default()
    }

    fn from_headers(headers: &HeaderMap) -> KomgaSyncToken {
        headers
            .get(X_KOBO_SYNCTOKEN)
            .and_then(|v| v.to_str().ok())
            .map(Self::from_base64)
            .unwrap_or_default()
    }
}

// endregion

// region small endpoints

async fn ping(_auth: KoboAuth) -> Json<&'static str> {
    Json("pong")
}

async fn initialization(
    State(state): State<AppState>,
    _auth: KoboAuth,
    Path(auth_token): Path<String>,
    uri: Uri,
    headers: HeaderMap,
) -> Response {
    let mut resources = if proxy_enabled(&state) {
        match proxy_current(&state, &Method::GET, &uri, &headers, None, None).await {
            Ok(proxied) => proxied
                .body
                .get("Resources")
                .filter(|v| !v.is_null())
                .cloned()
                .unwrap_or_else(native_kobo_resources),
            Err(ProxyError::Upstream(status)) if status == StatusCode::UNAUTHORIZED => {
                return proxy_error(ProxyError::Upstream(status)).into_response();
            }
            Err(e) => {
                tracing::warn!("Failed to get response from Kobo /v1/initialization, fallback to noproxy: {e:?}");
                native_kobo_resources()
            }
        }
    } else {
        native_kobo_resources()
    };
    let base = crate::http::base_url::base_url_for_kobo(&headers, &state.settings);
    let obj = resources.as_object_mut().expect("resources is an object");
    obj.insert(
        "image_host".to_string(),
        serde_json::Value::String(format!("{base}/")),
    );
    obj.insert(
        "image_url_template".to_string(),
        serde_json::Value::String(format!(
            "{base}/kobo/{auth_token}/v1/books/{{ImageId}}/thumbnail/{{Width}}/{{Height}}/false/image.jpg"
        )),
    );
    obj.insert(
        "image_url_quality_template".to_string(),
        serde_json::Value::String(format!(
            "{base}/kobo/{auth_token}/v1/books/{{ImageId}}/thumbnail/{{Width}}/{{Height}}/{{Quality}}/{{IsGreyscale}}/image.jpg"
        )),
    );
    let mut response = Json(ResourcesDto { resources }).into_response();
    response
        .headers_mut()
        .insert(X_KOBO_APITOKEN, HeaderValue::from_static("e30="));
    response
}

fn native_kobo_resources() -> serde_json::Value {
    serde_json::from_str(include_str!("kobo_resources.json"))
        .expect("bundled resources JSON is valid")
}

async fn auth_device(
    State(state): State<AppState>,
    _auth: KoboAuth,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if proxy_enabled(&state) {
        match proxy_current(
            &state,
            &Method::POST,
            &uri,
            &headers,
            Some(body.clone()),
            None,
        )
        .await
        {
            Ok(proxied) => return proxied_response(proxied),
            Err(e) => {
                tracing::warn!(
                    "Failed to get response from Kobo /v1/auth/device, fallback to noproxy: {e:?}"
                );
            }
        }
    }
    let parsed: Option<serde_json::Value> = serde_json::from_slice(&body).ok();
    Json(AuthDto {
        access_token: random_alphanumeric(24),
        refresh_token: random_alphanumeric(24),
        token_type: "Bearer".to_string(),
        tracking_id: uuid::Uuid::new_v4().to_string(),
        user_key: parsed
            .as_ref()
            .and_then(|b| b.get("UserKey"))
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
    })
    .into_response()
}

/// `RandomStringUtils.secure().nextAlphanumeric(n)`
fn random_alphanumeric(n: usize) -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    (0..n)
        .map(|_| {
            let h = RandomState::new().build_hasher().finish();
            (h % 62) as u8
        })
        .map(|v| match v {
            0..=9 => (b'0' + v) as char,
            10..=35 => (b'a' + v - 10) as char,
            _ => (b'A' + v - 36) as char,
        })
        .collect()
}

async fn analytics_get_tests(
    State(_state): State<AppState>,
    _auth: KoboAuth,
    headers: HeaderMap,
) -> Json<TestsDto> {
    Json(TestsDto {
        result: "Success".to_string(),
        test_key: headers
            .get("X-Kobo-userkey")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string(),
        tests: Default::default(),
    })
}

// endregion

// region sync

async fn sync_library(
    State(state): State<AppState>,
    auth: KoboAuth,
    Path(auth_token): Path<String>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let user = &auth.user;
    let sync_token_received = KomgaSyncToken::from_headers(&headers);

    let to_sync_point = get_sync_point_verified(
        &state,
        sync_token_received.ongoing_sync_point_id.as_deref(),
        &user.id,
    )?
    .map(Ok)
    .unwrap_or_else(|| {
        crate::service::sync_point::create_sync_point(
            &state,
            user,
            auth.api_key_id.as_deref(),
            None,
        )
    })?;

    let from_sync_point = get_sync_point_verified(
        &state,
        sync_token_received.last_successful_sync_point_id.as_deref(),
        &user.id,
    )?;

    let limit = state.config.kobo_sync_item_limit;
    let download_base = download_url_base(&state, &headers, &auth_token);
    let kepub_available = state
        .kepub
        .is_available(&state.settings.get(), &state.config);

    let mut should_continue_sync: bool;
    let results: Vec<SyncResultDto>;

    if let Some(from) = &from_sync_point {
        let mut remaining = limit;

        let books_added = crate::service::sync_point::take_books_added(
            &state,
            &from.id,
            &to_sync_point.id,
            0,
            remaining,
        )?;
        remaining = remaining.saturating_sub(books_added.number_of_elements() as u32);
        let mut cont = books_added.has_next();

        let books_changed = if books_added.is_last() && remaining > 0 {
            let r = crate::service::sync_point::take_books_changed(
                &state,
                &from.id,
                &to_sync_point.id,
                0,
                remaining,
            )?;
            remaining = remaining.saturating_sub(r.number_of_elements() as u32);
            cont = cont || r.has_next();
            r
        } else {
            SyncPage::of(vec![], 0, 0, remaining.max(1))
        };

        let books_removed = if books_changed.is_last() && remaining > 0 {
            let r = crate::service::sync_point::take_books_removed(
                &state,
                &from.id,
                &to_sync_point.id,
                0,
                remaining,
            )?;
            remaining = remaining.saturating_sub(r.number_of_elements() as u32);
            cont = cont || r.has_next();
            r
        } else {
            SyncPage::of(vec![], 0, 0, remaining.max(1))
        };

        let progress_changed = if books_removed.is_last() && remaining > 0 {
            let r = crate::service::sync_point::take_books_read_progress_changed(
                &state,
                &from.id,
                &to_sync_point.id,
                0,
                remaining,
            )?;
            remaining = remaining.saturating_sub(r.number_of_elements() as u32);
            cont = cont || r.has_next();
            r
        } else {
            SyncPage::of(vec![], 0, 0, remaining.max(1))
        };

        let read_lists_added = if progress_changed.is_last() && remaining > 0 {
            let r = crate::service::sync_point::take_read_lists_added(
                &state,
                &from.id,
                &to_sync_point.id,
                0,
                remaining,
            )?;
            remaining = remaining.saturating_sub(r.number_of_elements() as u32);
            cont = cont || r.has_next();
            r
        } else {
            SyncPage::of(vec![], 0, 0, remaining.max(1))
        };

        let read_lists_changed = if read_lists_added.is_last() && remaining > 0 {
            let r = crate::service::sync_point::take_read_lists_changed(
                &state,
                &from.id,
                &to_sync_point.id,
                0,
                remaining,
            )?;
            remaining = remaining.saturating_sub(r.number_of_elements() as u32);
            cont = cont || r.has_next();
            r
        } else {
            SyncPage::of(vec![], 0, 0, remaining.max(1))
        };

        let read_lists_removed = if read_lists_changed.is_last() && remaining > 0 {
            let r = crate::service::sync_point::take_read_lists_removed(
                &state,
                &from.id,
                &to_sync_point.id,
                0,
                remaining,
            )?;
            cont = cont || r.has_next();
            r
        } else {
            SyncPage::of(vec![], 0, 0, remaining.max(1))
        };

        should_continue_sync = cont;

        let metadata_ids: Vec<String> = books_added
            .content
            .iter()
            .chain(books_changed.content.iter())
            .map(|b| b.book_id.clone())
            .collect();
        let metadata = by_entitlement_id(
            KoboDtoDao::new(state.db.clone()).find_book_metadata_by_ids(&metadata_ids)?,
        )
        .into_iter()
        .map(|(id, row)| {
            (
                id,
                with_download_urls(
                    KoboBookMetadataDto::from(&row),
                    &download_base,
                    kepub_available,
                ),
            )
        })
        .collect::<BTreeMap<_, _>>();

        let progress_ids: Vec<String> = books_added
            .content
            .iter()
            .chain(books_changed.content.iter())
            .chain(progress_changed.content.iter())
            .map(|b| b.book_id.clone())
            .collect();
        let progress = read_progress_by_book(&state, &user.id, &progress_ids)?;

        let rl_ids: Vec<String> = read_lists_added
            .content
            .iter()
            .chain(read_lists_changed.content.iter())
            .map(|r| r.readlist_id.clone())
            .collect();
        let rl_books = readlist_books_grouped(&state, &to_sync_point.id, &rl_ids)?;

        let mut out: Vec<SyncResultDto> = vec![];
        let now = time_codec::now_utc();
        for book in books_added
            .content
            .iter()
            .chain(books_changed.content.iter())
        {
            out.push(SyncResultDto::NewEntitlement {
                new_entitlement: BookEntitlementContainerDto {
                    book_entitlement: book_entitlement_of(book, false, now),
                    book_metadata: metadata[&book.book_id].clone(),
                    reading_state: Some(
                        progress
                            .get(&book.book_id)
                            .map(reading_state_of)
                            .unwrap_or_else(|| {
                                empty_reading_state(&book.book_id, book.book_created_date)
                            }),
                    ),
                },
            });
        }
        for book in &books_changed.content {
            out.push(SyncResultDto::ChangedProductMetadata {
                changed_product_metadata: metadata[&book.book_id].clone(),
            });
        }
        for book in &books_removed.content {
            out.push(SyncResultDto::ChangedEntitlement {
                changed_entitlement: BookEntitlementContainerDto {
                    book_entitlement: book_entitlement_of(book, true, now),
                    book_metadata: metadata_for_removed_book(&book.book_id),
                    reading_state: None,
                },
            });
        }
        // changed books double as ChangedReadingState: Kobo ignores ReadingState inside ChangedEntitlement
        for book in books_changed
            .content
            .iter()
            .chain(progress_changed.content.iter())
        {
            if let Some(p) = progress.get(&book.book_id) {
                out.push(SyncResultDto::ChangedReadingState {
                    changed_reading_state: WrappedReadingStateDto {
                        reading_state: reading_state_of(p),
                    },
                });
            }
        }
        for rl in &read_lists_added.content {
            out.push(SyncResultDto::NewTag {
                new_tag: wrapped_tag_of(rl, rl_books.get(&rl.readlist_id).cloned()),
            });
        }
        for rl in &read_lists_changed.content {
            out.push(SyncResultDto::ChangedTag {
                changed_tag: wrapped_tag_of(rl, rl_books.get(&rl.readlist_id).cloned()),
            });
        }
        for rl in &read_lists_removed.content {
            out.push(SyncResultDto::DeletedTag {
                deleted_tag: wrapped_tag_of(rl, None),
            });
        }
        results = out;
    } else {
        // initial sync: everything
        let mut remaining = limit;
        let books =
            crate::service::sync_point::take_books(&state, &to_sync_point.id, 0, remaining)?;
        remaining = remaining.saturating_sub(books.number_of_elements() as u32);
        let mut cont = books.has_next();

        let read_lists = if books.is_last() && remaining > 0 {
            let r = crate::service::sync_point::take_read_lists(
                &state,
                &to_sync_point.id,
                0,
                remaining,
            )?;
            cont = cont || r.has_next();
            r
        } else {
            SyncPage::of(vec![], 0, 0, remaining.max(1))
        };
        should_continue_sync = cont;

        let book_ids: Vec<String> = books.content.iter().map(|b| b.book_id.clone()).collect();
        let metadata = by_entitlement_id(
            KoboDtoDao::new(state.db.clone()).find_book_metadata_by_ids(&book_ids)?,
        )
        .into_iter()
        .map(|(id, row)| {
            (
                id,
                with_download_urls(
                    KoboBookMetadataDto::from(&row),
                    &download_base,
                    kepub_available,
                ),
            )
        })
        .collect::<BTreeMap<_, _>>();
        let progress = read_progress_by_book(&state, &user.id, &book_ids)?;

        let rl_ids: Vec<String> = read_lists
            .content
            .iter()
            .map(|r| r.readlist_id.clone())
            .collect();
        let rl_books = readlist_books_grouped(&state, &to_sync_point.id, &rl_ids)?;

        let mut out: Vec<SyncResultDto> = vec![];
        let now = time_codec::now_utc();
        for book in &books.content {
            out.push(SyncResultDto::NewEntitlement {
                new_entitlement: BookEntitlementContainerDto {
                    book_entitlement: book_entitlement_of(book, false, now),
                    book_metadata: metadata[&book.book_id].clone(),
                    reading_state: Some(
                        progress
                            .get(&book.book_id)
                            .map(reading_state_of)
                            .unwrap_or_else(|| {
                                empty_reading_state(&book.book_id, book.book_created_date)
                            }),
                    ),
                },
            });
        }
        for rl in &read_lists.content {
            out.push(SyncResultDto::NewTag {
                new_tag: wrapped_tag_of(rl, rl_books.get(&rl.readlist_id).cloned()),
            });
        }
        results = out;
    }

    // merge the Kobo store sync response, only once all Komga updates have been processed
    let mut store_results: Vec<serde_json::Value> = vec![];
    let mut sync_token_merged = sync_token_received.clone();
    if !should_continue_sync && proxy_enabled(&state) {
        match proxy_current(
            &state,
            &Method::GET,
            &uri,
            &headers,
            None,
            Some(&sync_token_received),
        )
        .await
        {
            Ok(proxied) => {
                store_results = proxied.body.as_array().cloned().unwrap_or_default();
                sync_token_merged = proxied
                    .headers
                    .get(X_KOBO_SYNCTOKEN)
                    .and_then(|v| v.to_str().ok())
                    .map(KomgaSyncToken::from_base64)
                    .unwrap_or_else(|| sync_token_received.clone());
                should_continue_sync = proxied
                    .headers
                    .get(X_KOBO_SYNC)
                    .and_then(|v| v.to_str().ok())
                    .is_some_and(|v| v.eq_ignore_ascii_case("continue"));
            }
            Err(e) => {
                tracing::error!("Kobo sync endpoint failure: {e:?}");
                should_continue_sync = false;
            }
        }
    }

    let sync_token_updated = if should_continue_sync {
        KomgaSyncToken {
            ongoing_sync_point_id: Some(to_sync_point.id.clone()),
            ..sync_token_merged
        }
    } else {
        if let Some(from) = &from_sync_point {
            SyncPointDao::new(state.db.clone()).delete_one(&from.id)?;
        }
        KomgaSyncToken {
            ongoing_sync_point_id: None,
            last_successful_sync_point_id: Some(to_sync_point.id.clone()),
            ..sync_token_merged
        }
    };

    let merged_results: Vec<serde_json::Value> = results
        .iter()
        .map(|r| serde_json::to_value(r).expect("sync result serialization cannot fail"))
        .chain(store_results)
        .collect();
    let mut response = Json(merged_results).into_response();
    let headers_mut = response.headers_mut();
    if should_continue_sync {
        headers_mut.insert(X_KOBO_SYNC, HeaderValue::from_static("continue"));
    }
    headers_mut.insert(
        X_KOBO_SYNCTOKEN,
        HeaderValue::from_str(&sync_token_updated.to_base64()).unwrap(),
    );
    Ok(response)
}

fn get_sync_point_verified(
    state: &AppState,
    sync_point_id: Option<&str>,
    user_id: &str,
) -> Result<Option<SyncPoint>, ApiError> {
    let Some(id) = sync_point_id else {
        return Ok(None);
    };
    let sync_point = SyncPointDao::new(state.db.clone()).find_by_id(id)?;
    Ok(sync_point.filter(|sp| sp.user_id == user_id))
}

fn read_progress_by_book(
    state: &AppState,
    user_id: &str,
    book_ids: &[String],
) -> Result<BTreeMap<String, ReadProgress>, ApiError> {
    if book_ids.is_empty() {
        return Ok(BTreeMap::new());
    }
    let rows = komga_db::dao::read_progress::ReadProgressDao::new(state.db.clone())
        .find_by_books_and_user(book_ids, user_id)?;
    Ok(rows.into_iter().map(|p| (p.book_id.clone(), p)).collect())
}

fn readlist_books_grouped(
    state: &AppState,
    sync_point_id: &str,
    readlist_ids: &[String],
) -> Result<BTreeMap<String, Vec<TagItemDto>>, ApiError> {
    let rows = SyncPointDao::new(state.db.clone())
        .find_book_ids_by_readlist_ids(sync_point_id, readlist_ids)?;
    let mut map: BTreeMap<String, Vec<TagItemDto>> = BTreeMap::new();
    for row in rows {
        map.entry(row.readlist_id).or_default().push(TagItemDto {
            revision_id: row.book_id,
            type_: "ProductRevisionTagItem".to_string(),
        });
    }
    Ok(map)
}

fn download_url_base(state: &AppState, headers: &HeaderMap, auth_token: &str) -> String {
    let base = crate::http::base_url::base_url_for_kobo(headers, &state.settings);
    format!("{base}/kobo/{auth_token}/v1/books")
}

/// `KoboBookMetadataDto.withDownloadUrls`
fn with_download_urls(
    mut metadata: KoboBookMetadataDto,
    download_base: &str,
    kepub_available: bool,
) -> KoboBookMetadataDto {
    let (format, convert) = if metadata.is_pre_paginated {
        // fixed-layout books already paginate chapter-per-page, no Kepub conversion needed
        (FormatDto::Epub3fl, false)
    } else if metadata.is_kepub || kepub_available {
        // provide Kepub if available, or convert if possible
        (FormatDto::Kepub, !metadata.is_kepub)
    } else {
        (FormatDto::Epub3, false)
    };
    metadata.download_urls = vec![DownloadUrlDto {
        drm_type: "None".to_string(),
        format,
        size: metadata.file_size,
        platform: "Generic".to_string(),
        url: format!(
            "{download_base}/{}/file/epub?convert_kepub={convert}",
            metadata.entitlement_id
        ),
    }];
    metadata
}

fn metadata_for_removed_book(book_id: &str) -> KoboBookMetadataDto {
    KoboBookMetadataDto {
        categories: vec![DUMMY_ID.to_string()],
        contributor_roles: vec![],
        contributors: vec![],
        cover_image_id: Some(book_id.to_string()),
        cross_revision_id: book_id.to_string(),
        current_display_price: AmountDto {
            currency_code: Some("USD".to_string()),
            total_amount: 0,
        },
        current_love_display_price: AmountDto {
            currency_code: None,
            total_amount: 0,
        },
        description: None,
        download_urls: vec![],
        entitlement_id: book_id.to_string(),
        external_ids: vec![],
        genre: DUMMY_ID.to_string(),
        is_eligible_for_kobo_love: false,
        is_internet_archive: false,
        is_pre_order: false,
        is_social_enabled: true,
        isbn: None,
        language: "en".to_string(),
        phonetic_pronunciations: Default::default(),
        publication_date: None,
        publisher: None,
        revision_id: book_id.to_string(),
        series: None,
        slug: None,
        sub_title: None,
        title: book_id.to_string(),
        work_id: book_id.to_string(),
        is_kepub: false,
        is_pre_paginated: false,
        file_size: 0,
    }
}

// endregion

// region metadata / state

async fn get_book_metadata(
    State(state): State<AppState>,
    auth: KoboAuth,
    Path((auth_token, book_id)): Path<(String, String)>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    if BookDao::new(state.db.clone())
        .find_by_id(&book_id)?
        .is_none()
    {
        if proxy_enabled(&state) {
            let proxied = proxy_current(&state, &Method::GET, &uri, &headers, None, None)
                .await
                .map_err(proxy_error)?;
            return Ok(proxied_response(proxied));
        }
        return Err(ApiError::not_found(""));
    }
    restriction::check_book_by_id(&state, &auth.user, &book_id)?;
    let rows = KoboDtoDao::new(state.db.clone())
        .find_book_metadata_by_ids(std::slice::from_ref(&book_id))?;
    let download_base = download_url_base(&state, &headers, &auth_token);
    let kepub_available = state
        .kepub
        .is_available(&state.settings.get(), &state.config);
    let dtos: Vec<KoboBookMetadataDto> = rows
        .iter()
        .map(|row| {
            with_download_urls(
                KoboBookMetadataDto::from(row),
                &download_base,
                kepub_available,
            )
        })
        .collect();
    Ok(Json(dtos).into_response())
}

async fn get_state(
    State(state): State<AppState>,
    auth: KoboAuth,
    Path((_auth_token, book_id)): Path<(String, String)>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let Some(book) = BookDao::new(state.db.clone()).find_by_id(&book_id)? else {
        if proxy_enabled(&state) {
            let proxied = proxy_current(&state, &Method::GET, &uri, &headers, None, None)
                .await
                .map_err(proxy_error)?;
            return Ok(proxied_response(proxied));
        }
        return Err(ApiError::not_found(""));
    };
    restriction::check_book(&state, &auth.user, &book)?;
    let progress = komga_db::dao::read_progress::ReadProgressDao::new(state.db.clone())
        .find_by_book_and_user(&book_id, &auth.user.id)?;
    let dto = progress
        .as_ref()
        .map(reading_state_of)
        .unwrap_or_else(|| empty_reading_state(&book_id, book.created_date));
    Ok(Json(vec![dto]).into_response())
}

async fn update_state(
    State(state): State<AppState>,
    auth: KoboAuth,
    Path((_auth_token, book_id)): Path<(String, String)>,
    uri: Uri,
    headers: HeaderMap,
    raw_body: Bytes,
) -> Result<Response, ApiError> {
    let body: ReadingStateStateUpdateDto =
        serde_json::from_slice(&raw_body).map_err(|_| ApiError::bad_request(""))?;
    let Some(book) = BookDao::new(state.db.clone()).find_by_id(&book_id)? else {
        if proxy_enabled(&state) {
            let proxied = proxy_current(&state, &Method::PUT, &uri, &headers, Some(raw_body), None)
                .await
                .map_err(proxy_error)?;
            return Ok(proxied_response(proxied));
        }
        return Err(ApiError::not_found(""));
    };
    restriction::check_book(&state, &auth.user, &book)?;

    let Some(update) = body.reading_states.into_iter().next() else {
        return Err(ApiError::bad_request(""));
    };
    if update.current_bookmark.location.is_none()
        || update
            .current_bookmark
            .content_source_progress_percent
            .is_none()
    {
        return Err(ApiError::bad_request(""));
    }
    let location = update.current_bookmark.location.unwrap();
    let progression_percent = update
        .current_bookmark
        .content_source_progress_percent
        .unwrap();

    let locator = if update.status_info.status == StatusDto::Finished {
        // Kobo reports the first resource for finished books: always trust the last position instead
        let media = komga_db::dao::media::MediaDao::new(state.db.clone())
            .find_by_id(&book_id)?
            .ok_or_else(|| ApiError::Internal(format!("no media for book {book_id}")))?;
        let extension = crate::api::books::decode_epub_extension(&media)
            .map_err(|_| ApiError::Internal("Epub extension not found".to_string()))?;
        let Some(last) = extension.positions.into_iter().last() else {
            return Err(ApiError::Internal("Epub extension not found".to_string()));
        };
        last
    } else {
        komga_core::dto::progression::R2Locator {
            href: location.source.clone(),
            type_: "application/xhtml+xml".to_string(),
            title: None,
            kobo_span: if location
                .type_
                .as_deref()
                .is_some_and(|t| t.eq_ignore_ascii_case("kobospan"))
            {
                location.value.clone()
            } else {
                None
            },
            locations: Some(komga_core::dto::progression::R2Location {
                fragments: vec![],
                progression: Some(progression_percent / 100.0),
                position: None,
                total_progression: update.current_bookmark.progress_percent.map(|p| p / 100.0),
            }),
            text: None,
        }
    };

    let progression = komga_core::dto::progression::R2Progression {
        modified: update.last_modified,
        device: komga_core::dto::progression::R2Device {
            id: auth
                .api_key_id
                .clone()
                .unwrap_or_else(|| "unknown".to_string()),
            name: auth
                .api_key_comment
                .clone()
                .unwrap_or_else(|| "unknown".to_string()),
        },
        locator,
    };

    match crate::api::books::mark_progression(&state, &auth.user, &book, &progression) {
        Ok(()) => Ok(Json(update_success(&book_id)).into_response()),
        Err(e) => {
            tracing::error!("Could not update progression for book {book_id}: {e:?}");
            Ok(Json(update_failure(&book_id)).into_response())
        }
    }
}

// endregion

// region files

async fn get_book_file(
    State(state): State<AppState>,
    auth: KoboAuth,
    Path((_auth_token, book_id)): Path<(String, String)>,
    axum::extract::RawQuery(raw_query): axum::extract::RawQuery,
) -> Result<Response, ApiError> {
    let query = crate::http::pagination::parse_query_multi(raw_query.as_deref().unwrap_or(""));
    let convert_kepub = crate::http::pagination::QueryExt::first(&query, "convert_kepub")
        .map(|v| v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    if !convert_kepub {
        return crate::api::books::download_book_file_internal(
            state,
            RequireAuth(crate::auth::Auth {
                user: auth.user,
                source: crate::auth::AuthSource::ApiKey,
                api_key_id: auth.api_key_id,
            }),
            book_id,
        )
        .await;
    }

    if !auth.user.roles.contains(&UserRole::FileDownload) && !auth.user.is_admin() {
        return Err(ApiError::forbidden(""));
    }
    let Some(book) = BookDao::new(state.db.clone()).find_by_id(&book_id)? else {
        return Err(ApiError::not_found(""));
    };
    restriction::check_book(&state, &auth.user, &book)?;
    let media = MediaDao::new(state.db.clone())
        .find_by_id(&book.id)?
        .ok_or_else(|| ApiError::Internal(format!("no media for book {book_id}")))?;
    let book_path = std::path::PathBuf::from(url_to_file_path(&book.url));
    if media.media_type.as_deref() != Some(komga_media::detect::APPLICATION_EPUB) {
        return Err(ApiError::Internal(format!(
            "Cannot convert, not an EPUB: {}",
            book_path.display()
        )));
    }
    if media.epub_is_kepub {
        return Err(ApiError::Internal(format!(
            "Cannot convert, EPUB is already a KEPUB: {}",
            book_path.display()
        )));
    }
    if !book_path.exists() {
        return Err(ApiError::Internal(format!(
            "Source file does not exist: {}",
            book_path.display()
        )));
    }

    let conversion_failed = || ApiError::Status {
        status: StatusCode::SERVICE_UNAVAILABLE,
        message: "Kepub conversion failed".to_string(),
    };
    let kepubify = state
        .kepub
        .kepubify_path(&state.settings.get(), &state.config)
        .ok_or_else(conversion_failed)?;
    // `Book.computeCacheKey`: "$id-$fileLastModified"
    let cache_key = format!("{}-{}", book.id, book.file_last_modified);
    let kepub_path = state
        .kepub
        .cached_or_convert(&cache_key, |tmp_dir| {
            komga_media::kepubify::convert(&kepubify, &book_path, Some(tmp_dir))
        })
        .ok_or_else(conversion_failed)?;

    let file = match tokio::fs::File::open(&kepub_path).await {
        Ok(file) => file,
        Err(_) => {
            tracing::warn!("File not found: {}", kepub_path.display());
            return Err(ApiError::not_found("File not found, it may have moved"));
        }
    };
    let length = file
        .metadata()
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?
        .len();
    let stem = book_path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let filename = format!("{stem}.kepub.epub");

    let mut response = Response::new(Body::from_stream(tokio_util::io::ReaderStream::new(file)));
    {
        let headers = response.headers_mut();
        headers.insert(
            axum::http::header::CONTENT_DISPOSITION,
            HeaderValue::from_str(&content_disposition("attachment", &filename)).unwrap(),
        );
        headers.insert(
            axum::http::header::CONTENT_TYPE,
            HeaderValue::from_static("application/epub+zip"),
        );
        headers.insert(
            axum::http::header::CONTENT_LENGTH,
            HeaderValue::from_str(&length.to_string()).unwrap(),
        );
    }
    Ok(response)
}

async fn get_book_cover(
    State(state): State<AppState>,
    auth: KoboAuth,
    Path((_auth_token, thumbnail_id, width, height, _grey)): Path<(
        String,
        String,
        String,
        String,
        String,
    )>,
) -> Result<Response, ApiError> {
    book_cover(&state, &auth, &thumbnail_id, &width, &height).await
}

async fn get_book_cover_quality(
    State(state): State<AppState>,
    auth: KoboAuth,
    Path((_auth_token, thumbnail_id, width, height, _q, _grey)): Path<(
        String,
        String,
        String,
        String,
        String,
        String,
    )>,
) -> Result<Response, ApiError> {
    book_cover(&state, &auth, &thumbnail_id, &width, &height).await
}

async fn book_cover(
    state: &AppState,
    auth: &KoboAuth,
    thumbnail_id: &str,
    width: &str,
    height: &str,
) -> Result<Response, ApiError> {
    if !ThumbnailBookDao::new(state.db.clone()).exists_by_id(thumbnail_id)? && proxy_enabled(state)
    {
        let location = IMAGE_HOST_URL
            .replace("{ImageId}", thumbnail_id)
            .replace("{Width}", width)
            .replace("{Height}", height);
        return Ok((
            StatusCode::TEMPORARY_REDIRECT,
            [(axum::http::header::LOCATION, location)],
        )
            .into_response());
    }
    restriction::check_book_thumbnail(state, &auth.user, thumbnail_id)?;
    let Some(poster) =
        crate::service::book::get_thumbnail_bytes_by_thumbnail_id(state, thumbnail_id)?
    else {
        return Err(ApiError::not_found(""));
    };
    let bytes = if poster.media_type != komga_media::detect::IMAGE_JPEG {
        komga_media::image::convert(&poster.bytes, komga_media::image::ImageType::Jpeg)
            .map_err(|e| ApiError::Internal(e.to_string()))?
    } else {
        poster.bytes
    };
    Ok((
        [(
            axum::http::header::CONTENT_TYPE,
            komga_media::detect::IMAGE_JPEG,
        )],
        bytes,
    )
        .into_response())
}

// endregion

// region catch-all

async fn catch_all(
    State(state): State<AppState>,
    _auth: KoboAuth,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    if proxy_enabled(&state) {
        let proxied = proxy_current(
            &state,
            &method,
            &uri,
            &headers,
            if body.is_empty() { None } else { Some(body) },
            None,
        )
        .await
        .map_err(proxy_error)?;
        return Ok(proxied_response(proxied));
    }
    Ok(Json(serde_json::json!({})).into_response())
}

// endregion

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth;
    use crate::config::ServerConfig;
    use crate::settings::SettingsProvider;
    use crate::state::test_search_index;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use axum::middleware;
    use komga_core::model::user::{ApiKey, ContentRestrictions};
    use komga_core::time_codec::now_utc;
    use komga_db::dao::user::UserDao;
    use komga_db::pool::Database;
    use komga_db::{Migrator, Placeholders};
    use rusqlite::params;
    use std::collections::BTreeSet;
    use std::sync::Arc;
    use tower::ServiceExt;

    fn test_state() -> AppState {
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
        let config = ServerConfig::from_env();
        AppState {
            config: Arc::new(config.clone()),
            settings: Arc::new(SettingsProvider::load(db.clone())),
            task_emitter: Arc::new(crate::service::TaskEmitter::new(
                db.clone(),
                tasks_db.clone(),
                std::sync::Arc::new(tokio::sync::Notify::new()),
            )),
            db,
            tasks_db,
            sessions: auth::SessionStore::new(config.session_timeout),
            tsid: Arc::new(komga_core::tsid::TsidFactory::new_random_node()),
            events: crate::events::event_bus(),
            search_index: test_search_index(),
            kepub: crate::service::kepub::KepubConverter::new(tempfile::tempdir().unwrap().keep()),
            kobo_proxy: crate::service::kobo_proxy::KoboProxy::new(),
            shutdown_tx: tokio::sync::watch::channel(false).0,
        }
    }

    fn test_router(state: AppState) -> Router {
        Router::new()
            .merge(router())
            .layer(middleware::from_fn(
                crate::http::error_path::error_path_middleware,
            ))
            .layer(middleware::from_fn(crate::http::etag::etag_middleware))
            .layer(middleware::from_fn(
                crate::http::cache::cache_control_middleware,
            ))
            .layer(middleware::from_fn_with_state(
                state.clone(),
                auth::auth_middleware,
            ))
            .with_state(state)
    }

    async fn call(
        app: &Router,
        method: &str,
        uri: &str,
        body: Option<String>,
    ) -> (StatusCode, HeaderMap, Vec<u8>) {
        call_with_headers(app, method, uri, body, &[]).await
    }

    async fn call_with_headers(
        app: &Router,
        method: &str,
        uri: &str,
        body: Option<String>,
        extra_headers: &[(&str, &str)],
    ) -> (StatusCode, HeaderMap, Vec<u8>) {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json");
        for (name, value) in extra_headers {
            builder = builder.header(*name, *value);
        }
        let request = builder.body(Body::from(body.unwrap_or_default())).unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec();
        (status, headers, bytes)
    }

    fn json(bytes: &[u8]) -> serde_json::Value {
        serde_json::from_slice(bytes).unwrap()
    }

    fn seed_user(state: &AppState, email: &str, roles: &[UserRole], key: &str) -> String {
        let dao = UserDao::new(state.db.clone());
        let user_id = dao
            .insert(&KomgaUser {
                id: String::new(),
                email: email.to_string(),
                password: bcrypt::hash("pass", 10).unwrap(),
                roles: roles.iter().cloned().collect(),
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
            key: sha512_hex(key),
            comment: "kobo test".to_string(),
            created_date: now_utc(),
            last_modified_date: now_utc(),
        })
        .unwrap();
        user_id
    }

    fn seed_library(db: &Database, id: &str) {
        db.rw()
            .execute(
                "INSERT INTO LIBRARY (ID, NAME, ROOT) VALUES (?, ?, 'file:/data/')",
                params![id, "Manga"],
            )
            .unwrap();
    }

    fn seed_series(db: &Database, library_id: &str, id: &str) {
        let now = "2024-01-02 03:04:05.0";
        let rw = db.rw();
        rw.execute(
            "INSERT INTO SERIES (ID, NAME, URL, FILE_LAST_MODIFIED, LIBRARY_ID, BOOK_COUNT, ONESHOT, CREATED_DATE, LAST_MODIFIED_DATE) \
             VALUES (?, ?, 'file:/data/berserk/', ?, ?, 1, 0, ?, ?)",
            params![id, "Berserk", now, library_id, now, now],
        )
        .unwrap();
        rw.execute(
            "INSERT INTO SERIES_METADATA (SERIES_ID, STATUS, TITLE, TITLE_SORT, SUMMARY, PUBLISHER, LANGUAGE, CREATED_DATE, LAST_MODIFIED_DATE) \
             VALUES (?, 'ONGOING', 'Berserk', 'Berserk', '', 'Hakusensha', 'ja', ?, ?)",
            params![id, now, now],
        )
        .unwrap();
    }

    fn seed_book(db: &Database, series_id: &str, library_id: &str, id: &str, name: &str) {
        let now = "2024-01-02 03:04:05.0";
        let rw = db.rw();
        rw.execute(
            "INSERT INTO BOOK (ID, NAME, URL, FILE_LAST_MODIFIED, SERIES_ID, LIBRARY_ID, FILE_SIZE, NUMBER, FILE_HASH, FILE_HASH_KOREADER, ONESHOT, CREATED_DATE, LAST_MODIFIED_DATE) \
             VALUES (?, ?, 'file:/data/berserk/v01.epub', ?, ?, ?, 16227, 1, '', '', 0, ?, ?)",
            params![id, name, now, series_id, library_id, now, now],
        )
        .unwrap();
        rw.execute(
            "INSERT INTO BOOK_METADATA (BOOK_ID, TITLE, SUMMARY, NUMBER, NUMBER_SORT, ISBN, CREATED_DATE, LAST_MODIFIED_DATE) \
             VALUES (?, 'Berserk v01', 'Guts', '1', 1.0, '9781593070205', ?, ?)",
            params![id, now, now],
        )
        .unwrap();
        rw.execute(
            "INSERT INTO MEDIA (BOOK_ID, STATUS, MEDIA_TYPE, PAGE_COUNT, EPUB_DIVINA_COMPATIBLE, EPUB_IS_KEPUB, CREATED_DATE, LAST_MODIFIED_DATE) \
             VALUES (?, 'READY', 'application/epub+zip', 2, 1, 0, ?, ?)",
            params![id, now, now],
        )
        .unwrap();
        for (page, sub) in [
            ("page_1.xhtml", "EPUB_PAGE"),
            ("page_2.xhtml", "EPUB_PAGE"),
            ("style.css", "EPUB_ASSET"),
        ] {
            rw.execute(
                "INSERT INTO MEDIA_FILE (BOOK_ID, FILE_NAME, MEDIA_TYPE, SUB_TYPE) VALUES (?, ?, 'application/xhtml+xml', ?)",
                params![id, page, sub],
            )
            .unwrap();
        }
        rw.execute(
            "INSERT INTO THUMBNAIL_BOOK (ID, BOOK_ID, THUMBNAIL, TYPE, SELECTED, MEDIA_TYPE, FILE_SIZE, WIDTH, HEIGHT, CREATED_DATE, LAST_MODIFIED_DATE) \
             VALUES (?, ?, X'00', 'GENERATED', 1, 'image/jpeg', 100, 48, 48, ?, ?)",
            params![format!("thumb-{id}"), id, now, now],
        )
        .unwrap();
        rw.execute(
            "UPDATE SERIES SET BOOK_COUNT = (SELECT COUNT(*) FROM BOOK WHERE SERIES_ID = ?) WHERE ID = ?",
            params![series_id, series_id],
        )
        .unwrap();
    }

    fn seed_epub_extension(db: &Database, book_id: &str, is_fixed_layout: bool) {
        let positions = serde_json::json!([
            {"href":"page_1.xhtml","type":"application/xhtml+xml","locations":{"progression":0.1,"position":1,"totalProgression":0.5},"koboSpan":"kobo.1.1"},
            {"href":"page_1.xhtml","type":"application/xhtml+xml","locations":{"progression":0.5,"position":2,"totalProgression":0.75},"koboSpan":"kobo.1.2"},
            {"href":"page_2.xhtml","type":"application/xhtml+xml","locations":{"progression":0.9,"position":3,"totalProgression":0.99},"koboSpan":"kobo.1.3"},
        ]);
        let ext = serde_json::json!({
            "toc": [], "landmarks": [], "pageList": [],
            "isFixedLayout": is_fixed_layout,
            "positions": positions,
        });
        use std::io::Write;
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(ext.to_string().as_bytes()).unwrap();
        let blob = enc.finish().unwrap();
        db.rw()
            .execute(
                "UPDATE MEDIA SET EXTENSION_CLASS = 'org.gotson.komga.domain.model.MediaExtensionEpub', EXTENSION_VALUE_BLOB = ? WHERE BOOK_ID = ?",
                rusqlite::params![blob, book_id],
            )
            .unwrap();
    }

    fn seed_progress(db: &Database, book_id: &str, user_id: &str, page: i32, completed: bool) {
        seed_progress_at(db, book_id, user_id, page, completed, "2024-06-01 10:00:00")
    }

    fn seed_progress_at(
        db: &Database,
        book_id: &str,
        user_id: &str,
        page: i32,
        completed: bool,
        read_date: &str,
    ) {
        // insert_or_update recomputes the READ_PROGRESS_SERIES aggregate the On Deck query needs
        komga_db::dao::read_progress::ReadProgressDao::new(db.clone())
            .insert_or_update(&ReadProgress {
                book_id: book_id.to_string(),
                user_id: user_id.to_string(),
                page,
                completed,
                read_date: komga_core::time_codec::parse_datetime_utc(read_date).unwrap(),
                device_id: "dev".to_string(),
                device_name: "device".to_string(),
                locator: None,
                created_date: now_utc(),
                last_modified_date: now_utc(),
            })
            .unwrap();
    }

    fn seed_readlist(db: &Database, id: &str, book_id: &str) {
        let now = "2024-01-02 03:04:05.0";
        let rw = db.rw();
        rw.execute(
            "INSERT INTO READLIST (ID, NAME, SUMMARY, ORDERED, BOOK_COUNT, CREATED_DATE, LAST_MODIFIED_DATE) VALUES (?, 'Favorites', '', 1, 1, ?, ?)",
            params![id, now, now],
        )
        .unwrap();
        rw.execute(
            "INSERT INTO READLIST_BOOK (READLIST_ID, BOOK_ID, NUMBER) VALUES (?, ?, 0)",
            params![id, book_id],
        )
        .unwrap();
    }

    fn seed_base(state: &AppState, email: &str) -> String {
        let user_id = seed_user(
            state,
            email,
            &[UserRole::Admin, UserRole::KoboSync, UserRole::FileDownload],
            "kobokey",
        );
        let db = &state.db;
        seed_library(db, "lib1");
        seed_series(db, "lib1", "s1");
        user_id
    }

    fn bearer_token(headers: &HeaderMap) -> KomgaSyncToken {
        KomgaSyncToken::from_base64(
            headers
                .get(X_KOBO_SYNCTOKEN)
                .and_then(|v| v.to_str().ok())
                .unwrap(),
        )
    }

    #[test]
    fn sync_token_roundtrip() {
        let token = KomgaSyncToken {
            ongoing_sync_point_id: Some("sp1".to_string()),
            last_successful_sync_point_id: Some("sp0".to_string()),
            ..Default::default()
        };
        let b64 = token.to_base64();
        assert!(b64.starts_with(KOMGA_TOKEN_PREFIX));
        assert_eq!(KomgaSyncToken::from_base64(&b64), token);

        // kobo store token (base64.base64) is kept raw
        let store = KomgaSyncToken::from_base64("abc.def");
        assert_eq!(store.raw_kobo_sync_token, "abc.def");
        assert!(store.ongoing_sync_point_id.is_none());

        // garbage falls back to a default token
        assert_eq!(
            KomgaSyncToken::from_base64("KOMGA.not-json"),
            KomgaSyncToken::default()
        );
    }

    #[tokio::test]
    async fn full_sync_returns_entitlements_and_token() {
        let state = test_state();
        seed_base(&state, "a@b.c");
        seed_book(&state.db, "s1", "lib1", "b1", "v01");
        seed_book(&state.db, "s1", "lib1", "b2", "v02");
        let app = test_router(state);

        let (status, headers, bytes) =
            call(&app, "GET", "/kobo/kobokey/v1/library/sync", None).await;
        assert_eq!(status, StatusCode::OK);
        let body = json(&bytes);
        let results = body.as_array().unwrap();
        assert_eq!(results.len(), 2);
        let first = &results[0]["NewEntitlement"];
        assert!(first["BookEntitlement"]["Id"].is_string());
        assert_eq!(first["BookMetadata"]["Genre"], DUMMY_ID);
        assert_eq!(first["BookMetadata"]["Language"], "ja");
        assert_eq!(first["BookMetadata"]["DownloadUrls"][0]["Format"], "EPUB3");
        assert!(first["BookMetadata"]["DownloadUrls"][0]["Url"]
            .as_str()
            .unwrap()
            .contains("/file/epub?convert_kepub=false"));
        assert_eq!(first["ReadingState"]["StatusInfo"]["Status"], "ReadyToRead");
        // finished: all done in one batch, no continue header, token carries lastSuccessful
        assert!(headers.get(X_KOBO_SYNC).is_none());
        let token = bearer_token(&headers);
        assert!(token.ongoing_sync_point_id.is_none());
        assert!(token.last_successful_sync_point_id.is_some());
    }

    #[tokio::test]
    async fn incremental_sync_reports_changes() {
        let state = test_state();
        let user_id = seed_base(&state, "a@b.c");
        seed_book(&state.db, "s1", "lib1", "b1", "v01");
        seed_book(&state.db, "s1", "lib1", "b3", "v03");
        let app = test_router(state.clone());

        // initial sync to get a baseline sync point
        let (status, headers, _) = call(&app, "GET", "/kobo/kobokey/v1/library/sync", None).await;
        assert_eq!(status, StatusCode::OK);
        let token = bearer_token(&headers);
        let last_id = token.last_successful_sync_point_id.clone().unwrap();

        // changes: finish b1 (progress + On Deck rotation), soft-delete b3, rewrite b2's file
        seed_progress(&state.db, "b1", &user_id, 2, true);
        state
            .db
            .rw()
            .execute(
                "UPDATE BOOK SET DELETED_DATE = '2024-07-01 00:00:00.0' WHERE ID = 'b3'",
                [],
            )
            .unwrap();
        state
            .db
            .rw()
            .execute(
                "UPDATE BOOK SET FILE_HASH = 'newhash', FILE_LAST_MODIFIED = '2024-07-01 00:00:00.0' WHERE ID = 'b1'",
                [],
            )
            .unwrap();
        seed_book(&state.db, "s1", "lib1", "b2", "v02");

        let (status, headers, bytes) = call_with_headers(
            &app,
            "GET",
            "/kobo/kobokey/v1/library/sync",
            None,
            &[("x-kobo-synctoken", &token.to_base64())],
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let body = json(&bytes);
        let results = body.as_array().unwrap();
        let kinds: Vec<&str> = results
            .iter()
            .map(|r| r.as_object().unwrap().keys().next().unwrap().as_str())
            .collect();
        assert!(kinds.contains(&"NewEntitlement"), "b2 changed: {kinds:?}");
        assert!(
            kinds.contains(&"ChangedProductMetadata"),
            "b2 metadata: {kinds:?}"
        );
        assert!(
            kinds.contains(&"ChangedEntitlement"),
            "b3 removed: {kinds:?}"
        );
        assert!(
            kinds.contains(&"ChangedReadingState"),
            "b1 finished: {kinds:?}"
        );
        assert!(
            kinds.contains(&"NewTag"),
            "On Deck readlist added: {kinds:?}"
        );

        let new_token = bearer_token(&headers);
        assert_eq!(
            new_token
                .last_successful_sync_point_id
                .as_deref()
                .map(|s| s.len()),
            Some(13),
            "new sync point issued"
        );
        assert_ne!(
            new_token.last_successful_sync_point_id.as_deref(),
            Some(last_id.as_str()),
            "a new sync point replaces the old one"
        );
        // old sync point was cleaned up
        let old_count: i64 = state
            .db
            .ro()
            .query_row(
                "SELECT COUNT(*) FROM SYNC_POINT WHERE ID = ?",
                [last_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(old_count, 0);
    }

    #[tokio::test]
    async fn readlist_tag_changes() {
        let state = test_state();
        let user_id = seed_base(&state, "a@b.c");
        // series with one read and one unread book -> On Deck list exists
        seed_book(&state.db, "s1", "lib1", "b1", "v01");
        seed_book(&state.db, "s1", "lib1", "b2", "v02");
        seed_progress(&state.db, "b1", &user_id, 2, true);
        let app = test_router(state.clone());

        let (status, headers, _) = call(&app, "GET", "/kobo/kobokey/v1/library/sync", None).await;
        assert_eq!(status, StatusCode::OK);
        let token = bearer_token(&headers);

        // On Deck entry from the first sync
        let rl_count: i64 = state
            .db
            .ro()
            .query_row("SELECT COUNT(*) FROM SYNC_POINT_READLIST", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rl_count, 1, "On Deck readlist exists in sync point");

        // touch the read progress of the on-deck book to change the readlist lastModified
        seed_progress_at(&state.db, "b2", &user_id, 1, false, "2024-07-01 10:00:00");
        let (status, _, bytes) = call_with_headers(
            &app,
            "GET",
            "/kobo/kobokey/v1/library/sync",
            None,
            &[("x-kobo-synctoken", &token.to_base64())],
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let body = json(&bytes);
        let results = body.as_array().unwrap();
        let kinds: Vec<&str> = results
            .iter()
            .map(|r| r.as_object().unwrap().keys().next().unwrap().as_str())
            .collect();
        assert!(
            kinds.contains(&"DeletedTag"),
            "b2 in progress removes the On Deck list: {kinds:?}"
        );
        let deleted = results
            .iter()
            .find(|r| r.get("DeletedTag").is_some())
            .unwrap();
        assert_eq!(deleted["DeletedTag"]["Tag"]["Name"], "On Deck");
        // the On Deck row is gone from the new sync point
        let to_count: i64 = state
            .db
            .ro()
            .query_row("SELECT COUNT(*) FROM SYNC_POINT_READLIST", [], |r| r.get(0))
            .unwrap();
        assert_eq!(to_count, 0);
    }

    #[tokio::test]
    async fn state_get_and_update() {
        let state = test_state();
        let user_id = seed_base(&state, "a@b.c");
        seed_book(&state.db, "s1", "lib1", "b1", "v01");
        seed_epub_extension(&state.db, "b1", false);
        let app = test_router(state.clone());

        // GET with no progress -> ReadyToRead
        let (status, _, bytes) = call(&app, "GET", "/kobo/kobokey/v1/library/b1/state", None).await;
        assert_eq!(status, StatusCode::OK);
        let body = json(&bytes);
        assert_eq!(body[0]["StatusInfo"]["Status"], "ReadyToRead");

        // PUT a normal progression
        let update = serde_json::json!({
            "ReadingStates": [{
                "EntitlementId": "b1",
                "LastModified": "2026-09-19T10:00:00Z",
                "CurrentBookmark": {
                    "LastModified": "2026-09-19T10:00:00Z",
                    "ContentSourceProgressPercent": 42.0,
                    "ProgressPercent": 21.0,
                    "Location": {"Source": "page_1.xhtml", "Type": "KoboSpan", "Value": "kobo.1.1"}
                },
                "Statistics": {"LastModified": "2026-09-19T10:00:00Z"},
                "StatusInfo": {"LastModified": "2026-09-19T10:00:00Z", "Status": "Reading"}
            }]
        });
        let (status, _, bytes) = call(
            &app,
            "PUT",
            "/kobo/kobokey/v1/library/b1/state",
            Some(update.to_string()),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let body = json(&bytes);
        assert_eq!(body["RequestResult"], "Success");
        let saved = komga_db::dao::read_progress::ReadProgressDao::new(state.db.clone())
            .find_by_book_and_user("b1", &user_id)
            .unwrap()
            .expect("progress saved");
        assert_eq!(saved.page, 1);
        assert!(!saved.completed);

        // FINISHED takes the last extension position
        seed_epub_extension(&state.db, "b1", false);
        let update = serde_json::json!({
            "ReadingStates": [{
                "EntitlementId": "b1",
                "LastModified": "2026-09-19T11:00:00Z",
                "CurrentBookmark": {
                    "LastModified": "2026-09-19T11:00:00Z",
                    "ContentSourceProgressPercent": 100.0,
                    "ProgressPercent": 100.0,
                    "Location": {"Source": "page_1.xhtml", "Type": "KoboSpan", "Value": "kobo.1.1"}
                },
                "Statistics": {"LastModified": "2026-09-19T11:00:00Z"},
                "StatusInfo": {"LastModified": "2026-09-19T11:00:00Z", "Status": "Finished"}
            }]
        });
        let (status, _, bytes) = call(
            &app,
            "PUT",
            "/kobo/kobokey/v1/library/b1/state",
            Some(update.to_string()),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let body = json(&bytes);
        assert_eq!(body["RequestResult"], "Success");
        {
            let rows: Vec<(String, String, i32, i64, String)> = state
                .db
                .ro()
                .prepare("SELECT BOOK_ID, USER_ID, PAGE, COMPLETED, READ_DATE FROM READ_PROGRESS")
                .unwrap()
                .query_map([], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
                })
                .unwrap()
                .collect::<std::result::Result<_, _>>()
                .unwrap();
            println!("read_progress rows: {rows:?} (user_id={user_id})");
        }
        let saved = komga_db::dao::read_progress::ReadProgressDao::new(state.db.clone())
            .find_by_book_and_user("b1", &user_id)
            .unwrap()
            .expect("progress saved");
        assert!(saved.completed);
        let locator = saved.locator.unwrap();
        assert_eq!(locator["href"], "page_2.xhtml");
        assert_eq!(locator["koboSpan"], "kobo.1.3");
    }

    #[tokio::test]
    async fn metadata_404_and_catchall_and_auth_device() {
        let state = test_state();
        seed_base(&state, "a@b.c");
        let app = test_router(state);

        let (status, _, _) =
            call(&app, "GET", "/kobo/kobokey/v1/library/nope/metadata", None).await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        let (status, _, bytes) =
            call(&app, "GET", "/kobo/kobokey/v1/some/unknown/path", None).await;
        println!(
            "catchall status={status} body={:?}",
            String::from_utf8_lossy(&bytes)
        );
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json(&bytes), serde_json::json!({}));

        let (status, _, bytes) = call(
            &app,
            "POST",
            "/kobo/kobokey/v1/auth/device",
            Some(r#"{"UserKey":"uk-123"}"#.to_string()),
        )
        .await;
        println!(
            "auth_device status={status} body={:?}",
            String::from_utf8_lossy(&bytes)
        );
        assert_eq!(status, StatusCode::OK);
        let body = json(&bytes);
        assert_eq!(body["UserKey"], "uk-123");
        assert_eq!(body["TokenType"], "Bearer");
        assert_eq!(body["AccessToken"].as_str().unwrap().len(), 24);

        // ping
        let (status, _, bytes) = call(&app, "GET", "/kobo/kobokey/ping", None).await;
        println!(
            "ping status={status} body={:?}",
            String::from_utf8_lossy(&bytes)
        );
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json(&bytes), "pong");

        // initialization carries the image templates and the apitoken header
        let (status, headers, bytes) =
            call(&app, "GET", "/kobo/kobokey/v1/initialization", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers.get(X_KOBO_APITOKEN).unwrap(), "e30=");
        let body = json(&bytes);
        assert!(body["Resources"]["image_url_template"]
            .as_str()
            .unwrap()
            .contains(
                "/kobo/kobokey/v1/books/{ImageId}/thumbnail/{Width}/{Height}/false/image.jpg"
            ));
        assert!(body["Resources"]["account_page"].is_string());
    }

    #[tokio::test]
    async fn invalid_api_key_is_rejected() {
        let state = test_state();
        seed_base(&state, "a@b.c");
        let app = test_router(state);

        let (status, _, _) = call(&app, "GET", "/kobo/wrongkey/v1/library/sync", None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn file_epub_kepub_unavailable_and_thumbnail() {
        let state = test_state();
        seed_base(&state, "a@b.c");
        seed_book(&state.db, "s1", "lib1", "b1", "v01");
        let app = test_router(state.clone());

        // the source file does not exist: the require check fails before conversion (500)
        let (status, _, _) = call(
            &app,
            "GET",
            "/kobo/kobokey/v1/books/b1/file/epub?convert_kepub=true",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);

        // the source file exists but kepubify is not configured: 503
        let dir = tempfile::tempdir().unwrap();
        let epub = dir.path().join("v01.epub");
        std::fs::write(&epub, b"epub").unwrap();
        state
            .db
            .rw()
            .execute(
                "UPDATE BOOK SET URL = ? WHERE ID = 'b1'",
                [format!("file:{}", epub.display())],
            )
            .unwrap();
        let (status, _, bytes) = call(
            &app,
            "GET",
            "/kobo/kobokey/v1/books/b1/file/epub?convert_kepub=true",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert!(String::from_utf8_lossy(&bytes).contains("Kepub conversion failed"));

        // thumbnail by id: a JPEG thumbnail passes through
        let (status, headers, bytes) = call(
            &app,
            "GET",
            "/kobo/kobokey/v1/books/thumb-b1/thumbnail/48/48/false/image.jpg",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers.get("content-type").unwrap(), "image/jpeg");
        assert!(!bytes.is_empty());
    }

    // region kepubify integration

    fn executable_script(path: &std::path::Path, content: &str) {
        std::fs::write(path, content).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    fn save_setting(state: &AppState, key: &str, value: &str) {
        komga_db::dao::settings::SettingsDao::new(state.db.clone())
            .save_setting(key, value)
            .unwrap();
        state.settings.reload();
    }

    fn save_setting_bool(state: &AppState, key: &str, value: bool) {
        komga_db::dao::settings::SettingsDao::new(state.db.clone())
            .save_setting_bool(key, value)
            .unwrap();
        state.settings.reload();
    }

    #[tokio::test]
    async fn kepubify_available_advertises_kepub_and_downloads_converted() {
        let state = test_state();
        seed_base(&state, "a@b.c");
        seed_book(&state.db, "s1", "lib1", "b1", "v01");
        let dir = tempfile::tempdir().unwrap();
        let epub = dir.path().join("v01.epub");
        std::fs::write(&epub, b"EPUB-BYTES").unwrap();
        state
            .db
            .rw()
            .execute(
                "UPDATE BOOK SET URL = ? WHERE ID = 'b1'",
                [format!("file:{}", epub.display())],
            )
            .unwrap();
        // fake kepubify: copies input to output, counting invocations
        let counter = dir.path().join("count");
        let script = dir.path().join("kepubify");
        executable_script(
            &script,
            &format!(
                "#!/bin/sh\necho x >> \"{}\"\ncp \"$1\" \"$3\"\n",
                counter.display()
            ),
        );
        save_setting(&state, "KEPUBIFY_PATH", script.to_str().unwrap());
        let app = test_router(state.clone());

        // download URLs advertise KEPUB with convert_kepub=true
        let (status, _, bytes) = call(&app, "GET", "/kobo/kobokey/v1/library/sync", None).await;
        assert_eq!(status, StatusCode::OK);
        let body = json(&bytes);
        let url = &body[0]["NewEntitlement"]["BookMetadata"]["DownloadUrls"][0];
        assert_eq!(url["Format"], "KEPUB");
        assert!(url["Url"]
            .as_str()
            .unwrap()
            .contains("/file/epub?convert_kepub=true"));

        // the converted download: converted bytes, kepub file name, epub content type
        let (status, headers, bytes) = call(
            &app,
            "GET",
            "/kobo/kobokey/v1/books/b1/file/epub?convert_kepub=true",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers.get("content-type").unwrap(), "application/epub+zip");
        let disposition = headers
            .get("content-disposition")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(disposition.contains("v01.kepub.epub"), "{disposition}");
        assert_eq!(bytes, b"EPUB-BYTES");
        let runs = std::fs::read_to_string(&counter).unwrap().lines().count();
        assert_eq!(runs, 1);

        // the second download is served from the cache: no new conversion
        let (status, _, bytes) = call(
            &app,
            "GET",
            "/kobo/kobokey/v1/books/b1/file/epub?convert_kepub=true",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(bytes, b"EPUB-BYTES");
        let runs = std::fs::read_to_string(&counter).unwrap().lines().count();
        assert_eq!(runs, 1);
    }

    // endregion

    // region store proxy

    #[derive(Clone, Debug)]
    struct Recorded {
        method: String,
        uri: String,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    }

    type RecordedRequests = std::sync::Arc<std::sync::Mutex<Vec<Recorded>>>;

    /// A mock Kobo store: records every request and answers with `respond`.
    async fn spawn_upstream(
        respond: impl Fn(&Recorded) -> Response + Send + Sync + 'static,
    ) -> (String, RecordedRequests) {
        let recorded: RecordedRequests = std::sync::Arc::new(std::sync::Mutex::new(vec![]));
        let rec = recorded.clone();
        let respond = std::sync::Arc::new(respond);
        let app = Router::new().fallback(axum::routing::any(move |req: axum::extract::Request| {
            let rec = rec.clone();
            let respond = respond.clone();
            async move {
                let (parts, body) = req.into_parts();
                let bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();
                let r = Recorded {
                    method: parts.method.to_string(),
                    uri: parts.uri.to_string(),
                    headers: parts
                        .headers
                        .iter()
                        .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or_default().to_string()))
                        .collect(),
                    body: bytes.to_vec(),
                };
                rec.lock().unwrap().push(r.clone());
                respond(&r)
            }
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (format!("http://{addr}"), recorded)
    }

    fn proxy_state(state: AppState, upstream: &str) -> AppState {
        let mut state = state;
        state.kobo_proxy = crate::service::kobo_proxy::KoboProxy::with_upstream(upstream);
        save_setting_bool(&state, "KOBO_PROXY", true);
        state
    }

    fn json_response(status: u16, body: &str) -> Response {
        Response::builder()
            .status(status)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    #[tokio::test]
    async fn proxy_catch_all_forwards_filtered_request() {
        let state = test_state();
        seed_base(&state, "a@b.c");
        let (upstream, recorded) = spawn_upstream(|_| {
            Response::builder()
                .header("x-kobo-echo", "yes")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"ok":true}"#))
                .unwrap()
        })
        .await;
        let app = test_router(proxy_state(state, &upstream));

        let (status, headers, bytes) = call_with_headers(
            &app,
            "GET",
            "/kobo/kobokey/v1/some/unknown/path?x=1&y=2",
            None,
            &[
                ("authorization", "Bearer abc"),
                ("x-kobo-deviceid", "device-1"),
                ("x-kobo-synctoken", "should-be-dropped"),
                ("cookie", "session=secret"),
            ],
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers.get("x-kobo-echo").unwrap(), "yes");
        assert_eq!(json(&bytes), serde_json::json!({"ok": true}));

        let requests = recorded.lock().unwrap();
        assert_eq!(requests.len(), 1);
        let r = &requests[0];
        assert_eq!(r.uri, "/v1/some/unknown/path?x=1&y=2");
        let has = |name: &str| r.headers.iter().any(|(k, _)| k == name);
        assert!(has("authorization"));
        assert!(has("x-kobo-deviceid"));
        assert!(
            !has("x-kobo-synctoken"),
            "sync token dropped without includeSyncToken"
        );
        assert!(!has("cookie"));
    }

    #[tokio::test]
    async fn proxy_state_metadata_and_cover_for_unknown_book() {
        let state = test_state();
        seed_base(&state, "a@b.c");
        let (upstream, recorded) = spawn_upstream(|r| {
            let path = r.uri.split('?').next().unwrap_or("");
            let body = match (r.method.as_str(), path) {
                ("GET", "/v1/library/unknown/state") => r#"[{"StatusInfo":{"Status":"Reading"}}]"#,
                ("PUT", "/v1/library/unknown/state") => r#"{"RequestResult":"Success"}"#,
                ("GET", "/v1/library/unknown/metadata") => r#"[{"Title":"Store Book"}]"#,
                _ => r#"{}"#,
            };
            json_response(200, body)
        })
        .await;
        let app = test_router(proxy_state(state, &upstream));

        let (status, _, bytes) =
            call(&app, "GET", "/kobo/kobokey/v1/library/unknown/state", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json(&bytes)[0]["StatusInfo"]["Status"], "Reading");

        let update = serde_json::json!({
            "ReadingStates": [{
                "EntitlementId": "unknown",
                "LastModified": "2026-09-19T10:00:00Z",
                "CurrentBookmark": {
                    "LastModified": "2026-09-19T10:00:00Z",
                    "ContentSourceProgressPercent": 42.0,
                    "Location": {"Source": "page_1.xhtml", "Type": "KoboSpan", "Value": "kobo.1.1"}
                },
                "Statistics": {"LastModified": "2026-09-19T10:00:00Z"},
                "StatusInfo": {"LastModified": "2026-09-19T10:00:00Z", "Status": "Reading"}
            }]
        });
        let (status, _, bytes) = call(
            &app,
            "PUT",
            "/kobo/kobokey/v1/library/unknown/state",
            Some(update.to_string()),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json(&bytes)["RequestResult"], "Success");

        let (status, _, bytes) = call(
            &app,
            "GET",
            "/kobo/kobokey/v1/library/unknown/metadata",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json(&bytes)[0]["Title"], "Store Book");

        // the state update was forwarded with its raw body
        {
            let requests = recorded.lock().unwrap();
            let put = requests
                .iter()
                .find(|r| r.method == "PUT")
                .expect("proxied PUT");
            assert_eq!(put.uri, "/v1/library/unknown/state");
            assert_eq!(put.body, update.to_string().as_bytes());
        }

        // unknown thumbnails redirect to the Kobo CDN (quality variant uses the same URL)
        for uri in [
            "/kobo/kobokey/v1/books/unknown-thumb/thumbnail/48/60/false/image.jpg",
            "/kobo/kobokey/v1/books/unknown-thumb/thumbnail/48/60/80/false/image.jpg",
        ] {
            let (status, headers, _) = call(&app, "GET", uri, None).await;
            assert_eq!(status, StatusCode::TEMPORARY_REDIRECT, "{uri}");
            assert_eq!(
                headers.get("location").unwrap(),
                "https://cdn.kobo.com/book-images/unknown-thumb/48/60/false/image.jpg"
            );
        }
    }

    #[tokio::test]
    async fn proxy_initialization_and_auth_device() {
        let state = test_state();
        seed_base(&state, "a@b.c");
        let (upstream, _) = spawn_upstream(|r| {
            let path = r.uri.split('?').next().unwrap_or("");
            let body = match path {
                "/v1/initialization" => {
                    r#"{"Resources":{"account_page":"https://store.example/account","image_host":"https://cdn.example/"}}"#
                }
                "/v1/auth/device" => {
                    r#"{"AccessToken":"store-access","RefreshToken":"store-refresh","TokenType":"Bearer","TrackingId":"t-1","UserKey":"uk-9"}"#
                }
                _ => r#"{}"#,
            };
            json_response(200, body)
        })
        .await;
        let app = test_router(proxy_state(state, &upstream));

        // Resources come from the store; the image templates are rewritten to the local base
        let (status, headers, bytes) =
            call(&app, "GET", "/kobo/kobokey/v1/initialization", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers.get(X_KOBO_APITOKEN).unwrap(), "e30=");
        let body = json(&bytes);
        assert_eq!(
            body["Resources"]["account_page"],
            "https://store.example/account"
        );
        assert!(body["Resources"]["image_url_template"]
            .as_str()
            .unwrap()
            .contains("/kobo/kobokey/v1/books/{ImageId}/thumbnail/"));

        // the store's auth response is returned as-is
        let (status, _, bytes) = call(
            &app,
            "POST",
            "/kobo/kobokey/v1/auth/device",
            Some(r#"{"UserKey":"uk-123"}"#.to_string()),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json(&bytes)["AccessToken"], "store-access");
    }

    #[tokio::test]
    async fn proxy_initialization_unauthorized_passthrough() {
        let state = test_state();
        seed_base(&state, "a@b.c");
        let (upstream, _) =
            spawn_upstream(|_| json_response(401, r#"{"error":"Unauthorized"}"#)).await;
        let app = test_router(proxy_state(state, &upstream));

        let (status, _, _) = call(&app, "GET", "/kobo/kobokey/v1/initialization", None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn proxy_sync_merges_store_results() {
        let state = test_state();
        seed_base(&state, "a@b.c");
        seed_book(&state.db, "s1", "lib1", "b1", "v01");
        let app = test_router(state.clone());
        // first sync without the proxy to establish a sync point
        let (status, headers, _) = call(&app, "GET", "/kobo/kobokey/v1/library/sync", None).await;
        assert_eq!(status, StatusCode::OK);
        let sp1 = bearer_token(&headers)
            .last_successful_sync_point_id
            .clone()
            .unwrap();

        let (upstream, recorded) = spawn_upstream(|_| {
            Response::builder()
                .header("x-kobo-synctoken", "raw-store-token-2")
                .header("x-kobo-sync", "continue")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"[{"ChangedReadingState":{"ReadingState":{"EntitlementId":"store-book"}}}]"#,
                ))
                .unwrap()
        })
        .await;
        let app = test_router(proxy_state(state, &upstream));

        let crafted = KomgaSyncToken {
            raw_kobo_sync_token: "raw-store-token".to_string(),
            last_successful_sync_point_id: Some(sp1.clone()),
            ..Default::default()
        };
        let (status, headers, bytes) = call_with_headers(
            &app,
            "GET",
            "/kobo/kobokey/v1/library/sync",
            None,
            &[("x-kobo-synctoken", &crafted.to_base64())],
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        // komga has no changes; the store event is appended
        let results = json(&bytes);
        let results = results.as_array().unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(
            results[0]["ChangedReadingState"]["ReadingState"]["EntitlementId"],
            "store-book"
        );
        // the store asked to continue: the new token carries the ongoing sync point and the
        // store's raw token
        assert_eq!(headers.get(X_KOBO_SYNC).unwrap(), "continue");
        let new_token = bearer_token(&headers);
        assert_eq!(new_token.raw_kobo_sync_token, "raw-store-token-2");
        assert!(new_token.ongoing_sync_point_id.is_some());
        assert_ne!(
            new_token.ongoing_sync_point_id.as_deref(),
            Some(sp1.as_str())
        );

        // the store received the raw sync token, not the Komga-wrapped one
        let requests = recorded.lock().unwrap();
        let sync_req = requests
            .iter()
            .find(|r| r.uri.starts_with("/v1/library/sync"))
            .expect("store sync request");
        let token_header = sync_req
            .headers
            .iter()
            .find(|(k, _)| k == "x-kobo-synctoken")
            .expect("sync token header");
        assert_eq!(token_header.1, "raw-store-token");
    }

    // endregion

    #[tokio::test]
    async fn kobo_port_overrides_download_url_port() {
        let state = test_state();
        seed_base(&state, "a@b.c");
        seed_book(&state.db, "s1", "lib1", "b1", "v01");
        save_setting(&state, "KOBO_PORT", "8443");
        let app = test_router(state);

        // without forwarded headers the koboPort setting rewrites the port
        let (status, _, bytes) = call(&app, "GET", "/kobo/kobokey/v1/library/sync", None).await;
        assert_eq!(status, StatusCode::OK);
        let url = json(&bytes)[0]["NewEntitlement"]["BookMetadata"]["DownloadUrls"][0]["Url"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(url.starts_with("http://localhost:8443/"), "{url}");

        // forwarded headers take over: the setting is ignored
        let (status, _, bytes) = call_with_headers(
            &app,
            "GET",
            "/kobo/kobokey/v1/library/sync",
            None,
            &[
                ("x-forwarded-host", "library.example"),
                ("x-forwarded-proto", "https"),
            ],
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let url = json(&bytes)[0]["NewEntitlement"]["BookMetadata"]["DownloadUrls"][0]["Url"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(url.starts_with("https://library.example/"), "{url}");
    }
}
