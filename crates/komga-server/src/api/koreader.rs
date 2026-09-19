//! `KoreaderSyncController.kt`: the KOReader sync API (`/koreader/**`).

use crate::auth::{MaybeAuth, RequireAuth};
use crate::dto::koreader::{DocumentProgressDto, UserAuthenticationDto};
use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{routing, Json, Router};
use komga_core::dto::progression::{R2Device, R2Location, R2Locator, R2Progression};
use komga_core::model::book::Book;
use komga_core::model::media::Media;
use komga_core::model::read_progress::ReadProgress;
use komga_core::search::MediaProfile;
use komga_core::time_codec;
use komga_db::dao::book::BookDao;
use komga_db::dao::media::MediaDao;
use komga_db::dao::read_progress::ReadProgressDao;
use komga_media::container;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/koreader/users/create", routing::post(register_user))
        .route("/koreader/users/auth", routing::get(authorize))
        .route(
            "/koreader/syncs/progress/{bookHash}",
            routing::get(get_progress),
        )
        .route("/koreader/syncs/progress", routing::put(update_progress))
}

/// `application/vnd.koreader.v1+json` when the client asks for it, `application/json` otherwise
fn content_type(headers: &HeaderMap) -> &'static str {
    headers
        .get(axum::http::header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .filter(|accept| accept.contains("application/vnd.koreader.v1+json"))
        .map(|_| "application/vnd.koreader.v1+json")
        .unwrap_or("application/json")
}

async fn register_user(auth: MaybeAuth) -> Result<Response, ApiError> {
    // KOReader hits this endpoint during setup, anonymously or not; komga always refuses
    let _ = auth;
    Err(ApiError::forbidden("User creation is disabled"))
}

async fn authorize(
    State(_state): State<AppState>,
    _auth: RequireAuth,
    headers: HeaderMap,
) -> Response {
    (
        [(axum::http::header::CONTENT_TYPE, content_type(&headers))],
        Json(UserAuthenticationDto::default()),
    )
        .into_response()
}

/// Looks up the single book matching a KOReader partial-MD5 hash
fn book_by_hash(state: &AppState, hash: &str) -> Result<Book, ApiError> {
    let books = BookDao::new(state.db.clone()).find_all_by_hash_koreader(hash)?;
    match books.len() {
        0 => Err(ApiError::not_found("Book not found")),
        1 => Ok(books.into_iter().next().expect("one book")),
        _ => Err(ApiError::conflict(
            "More than 1 book found with the same hash",
        )),
    }
}

/// `mediaRepository.findById(bookId)` — the Java side throws (500) when the row is missing
fn require_media(state: &AppState, book_id: &str) -> Result<Media, ApiError> {
    MediaDao::new(state.db.clone())
        .find_by_id(book_id)?
        .ok_or_else(|| ApiError::Internal(format!("no media for book {book_id}")))
}

/// `positions.groupBy { it.href }.keys`: distinct hrefs in first-appearance order
fn position_hrefs(positions: &[R2Locator]) -> Vec<&str> {
    let mut keys: Vec<&str> = vec![];
    for position in positions {
        if !keys.contains(&position.href.as_str()) {
            keys.push(&position.href);
        }
    }
    keys
}

async fn get_progress(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path(book_hash): Path<String>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let book = book_by_hash(&state, &book_hash)?;
    let media = require_media(&state, &book.id)?;

    let dao = ReadProgressDao::new(state.db.clone());
    let Some(read_progress) = dao.find_by_book_and_user(&book.id, &auth.0.user.id)? else {
        // Kotlin throws ResponseStatusException(OK, ...): a 200 carrying the Spring error body
        return Err(ApiError::Status {
            status: StatusCode::OK,
            message: "No progress found for this book".into(),
        });
    };

    let percentage = total_progression_of(&read_progress)
        .unwrap_or(read_progress.page as f32 / media.page_count as f32);

    let profile = container::media_profile(media.media_type.as_deref());
    let progress = match profile {
        Some(MediaProfile::Divina) | Some(MediaProfile::Pdf) => read_progress.page.to_string(),
        Some(MediaProfile::Epub) => {
            let extension = crate::api::books::decode_epub_extension(&media)?;
            let hrefs = position_hrefs(&extension.positions);
            let locator_href = read_progress
                .locator
                .as_ref()
                .and_then(|l| l.get("href"))
                .and_then(|v| v.as_str());
            // Kotlin's keys.indexOf returns -1 when absent; the +1 then yields 0
            let resource_index = hrefs
                .iter()
                .position(|&href| Some(href) == locator_href)
                .map(|i| i as i32)
                .unwrap_or(-1);
            format!("/body/DocFragment[{}].0", resource_index + 1)
        }
        None => return Err(ApiError::not_found("Book has no media profile")),
    };

    let dto = DocumentProgressDto {
        document: book_hash,
        percentage,
        progress,
        device: read_progress.device_name.clone(),
        device_id: read_progress.device_id.clone(),
    };
    Ok((
        [(axum::http::header::CONTENT_TYPE, content_type(&headers))],
        Json(dto),
    )
        .into_response())
}

/// `readProgress.locator?.locations?.totalProgression`
fn total_progression_of(progress: &ReadProgress) -> Option<f32> {
    progress
        .locator
        .as_ref()
        .and_then(|l| l.get("locations"))
        .and_then(|l| l.get("totalProgression"))
        .and_then(|v| v.as_f64())
        .map(|v| v as f32)
}

/// Kotlin `DocFragment\[(\d+)]` with IGNORE_CASE
fn parse_doc_fragment_index(progress: &str) -> Option<i32> {
    let lower = progress.to_lowercase();
    let prefix = "docfragment[";
    let start = lower.find(prefix)? + prefix.len();
    let digits: String = lower[start..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    if digits.is_empty() {
        return None;
    }
    if lower[start + digits.len()..].starts_with(']') {
        digits.parse().ok()
    } else {
        None
    }
}

/// Kotlin `#_doc_fragment_(\d+)_` with IGNORE_CASE
fn parse_doc_fragment_index_v2(progress: &str) -> Option<i32> {
    let lower = progress.to_lowercase();
    let prefix = "#_doc_fragment_";
    let start = lower.find(prefix)? + prefix.len();
    let digits: String = lower[start..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    if digits.is_empty() {
        return None;
    }
    if lower[start + digits.len()..].starts_with('_') {
        digits.parse().ok()
    } else {
        None
    }
}

fn locator_of(position: i32, total_progression: f32) -> R2Locator {
    R2Locator {
        href: String::new(),
        type_: String::new(),
        title: None,
        locations: Some(R2Location {
            fragments: vec![],
            progression: None,
            position: Some(position),
            total_progression: Some(total_progression),
        }),
        text: None,
        kobo_span: None,
    }
}

async fn update_progress(
    State(state): State<AppState>,
    auth: RequireAuth,
    Json(koreader_progress): Json<DocumentProgressDto>,
) -> Result<StatusCode, ApiError> {
    let book = book_by_hash(&state, &koreader_progress.document)?;
    let media = require_media(&state, &book.id)?;

    let profile = container::media_profile(media.media_type.as_deref());
    let locator = match profile {
        Some(MediaProfile::Divina) | Some(MediaProfile::Pdf) => {
            // Kotlin's String.toInt() throws NumberFormatException → 500
            let position = koreader_progress.progress.parse::<i32>().map_err(|_| {
                ApiError::Internal(format!(
                    "For input string: \"{}\"",
                    koreader_progress.progress
                ))
            })?;
            locator_of(position, koreader_progress.percentage)
        }
        Some(MediaProfile::Epub) => {
            // KOReader indexes from 1 in the DocFragment form, from 0 in the anchor form
            let resource_index = parse_doc_fragment_index(&koreader_progress.progress)
                .map(|i| i - 1)
                .or_else(|| parse_doc_fragment_index_v2(&koreader_progress.progress))
                .ok_or_else(|| {
                    ApiError::bad_request(format!(
                        "Could not get Epub resource index from progress: {}",
                        koreader_progress.progress
                    ))
                })?;

            let extension = crate::api::books::decode_epub_extension(&media)?;
            let hrefs = position_hrefs(&extension.positions);
            // Kotlin's keys.elementAt throws IndexOutOfBoundsException → 500
            let href = hrefs.get(resource_index as usize).ok_or_else(|| {
                ApiError::Internal(format!("Index: {resource_index}, Size: {}", hrefs.len()))
            })?;

            R2Locator {
                href: (*href).to_string(),
                // the type is overwritten with the matched one when saved
                type_: "application/xhtml+xml".to_string(),
                title: None,
                locations: Some(R2Location {
                    fragments: vec![],
                    progression: Some(0.0),
                    position: None,
                    total_progression: Some(koreader_progress.percentage),
                }),
                text: None,
                kobo_span: None,
            }
        }
        None => return Err(ApiError::not_found("Book has no media profile")),
    };

    let progression = R2Progression {
        modified: time_codec::now_utc(),
        device: R2Device {
            id: koreader_progress.device_id.clone(),
            name: koreader_progress.device.clone(),
        },
        locator,
    };
    crate::api::books::mark_progression(&state, &auth.0.user, &book, &progression)?;
    Ok(StatusCode::OK)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::libraries::test_support::{insert_api_key, insert_user, TestApp};
    use axum::middleware;
    use komga_core::model::book::{Book, BookMetadata};
    use komga_core::model::media::{BookPage, Media, MediaFile, MediaFileSubType, MediaStatus};
    use komga_core::time_codec::now_utc;
    use komga_db::dao::book::{BookDao, BookMetadataDao};
    use komga_db::dao::sync_point::SyncPointDao;
    use komga_db::pool::Database;
    use rusqlite::params;

    fn test_app() -> TestApp {
        TestApp::new(
            Router::new()
                .merge(router())
                .merge(super::super::syncpoints::router())
                .layer(middleware::from_fn(
                    crate::http::error_path::error_path_middleware,
                ))
                .layer(middleware::from_fn(crate::http::etag::etag_middleware))
                .layer(middleware::from_fn(
                    crate::http::cache::cache_control_middleware,
                )),
        )
    }

    fn seed_library(db: &Database, id: &str) {
        db.rw()
            .execute(
                "INSERT INTO LIBRARY (ID, NAME, ROOT) VALUES (?, ?, ?)",
                params![id, "lib", "file:/data/lib/"],
            )
            .unwrap();
    }

    fn seed_series(db: &Database, id: &str, library_id: &str) {
        db.rw()
            .execute(
                "INSERT INTO SERIES (ID, NAME, URL, FILE_LAST_MODIFIED, LIBRARY_ID) VALUES (?, ?, ?, ?, ?)",
                params![id, "series", "file:/data/lib/series/", "2024-01-01 00:00:00.0", library_id],
            )
            .unwrap();
    }

    fn seed_book_with_hash(
        db: &Database,
        id: &str,
        series_id: &str,
        library_id: &str,
        hash_koreader: &str,
    ) {
        let now = now_utc();
        let book = Book {
            id: id.to_string(),
            name: format!("Book {id}"),
            url: format!("file:/data/lib/series/{id}.cbz"),
            file_last_modified: now,
            series_id: series_id.to_string(),
            library_id: library_id.to_string(),
            file_size: 1000,
            number: 1,
            file_hash: String::new(),
            file_hash_koreader: hash_koreader.to_string(),
            deleted_date: None,
            oneshot: false,
            created_date: now,
            last_modified_date: now,
        };
        BookDao::new(db.clone()).insert(&book).unwrap();
        BookMetadataDao::new(db.clone())
            .insert(&BookMetadata {
                book_id: id.to_string(),
                title: format!("Book {id}"),
                summary: String::new(),
                number: "1".into(),
                number_sort: 1.0,
                release_date: None,
                authors: vec![],
                tags: vec![],
                isbn: String::new(),
                links: vec![],
                title_lock: false,
                summary_lock: false,
                number_lock: false,
                number_sort_lock: false,
                release_date_lock: false,
                authors_lock: false,
                tags_lock: false,
                isbn_lock: false,
                links_lock: false,
                created_date: now,
                last_modified_date: now,
            })
            .unwrap();
    }

    fn zip_media(db: &Database, book_id: &str, page_count: i32) {
        let now = now_utc();
        let media = Media {
            book_id: book_id.to_string(),
            status: MediaStatus::Ready,
            media_type: Some("application/zip".to_string()),
            comment: None,
            page_count,
            pages: (0..page_count)
                .map(|i| BookPage {
                    file_name: format!("page-{i}.png"),
                    media_type: "image/png".into(),
                    width: Some(48),
                    height: Some(48),
                    file_hash: String::new(),
                    file_size: Some(100),
                })
                .collect(),
            files: vec![],
            extension_class: None,
            extension_value: None,
            epub_divina_compatible: false,
            epub_is_kepub: false,
            created_date: now,
            last_modified_date: now,
        };
        MediaDao::new(db.clone()).insert(&media).unwrap();
    }

    fn epub_media(db: &Database, book_id: &str) {
        let now = now_utc();
        let media = Media {
            book_id: book_id.to_string(),
            status: MediaStatus::Ready,
            media_type: Some("application/epub+zip".to_string()),
            comment: None,
            page_count: 10,
            pages: vec![],
            files: vec![
                MediaFile {
                    file_name: "text/ch1.xhtml".into(),
                    media_type: Some("application/xhtml+xml".to_string()),
                    sub_type: Some(MediaFileSubType::EpubPage),
                    file_size: None,
                },
                MediaFile {
                    file_name: "text/ch2.xhtml".into(),
                    media_type: Some("application/xhtml+xml".to_string()),
                    sub_type: Some(MediaFileSubType::EpubPage),
                    file_size: None,
                },
                MediaFile {
                    file_name: "text/ch3.xhtml".into(),
                    media_type: Some("application/xhtml+xml".to_string()),
                    sub_type: Some(MediaFileSubType::EpubPage),
                    file_size: None,
                },
            ],
            extension_class: Some("org.gotson.komga.domain.model.MediaExtensionEpub".to_string()),
            extension_value: Some(epub_extension_blob()),
            epub_divina_compatible: false,
            epub_is_kepub: false,
            created_date: now,
            last_modified_date: now,
        };
        MediaDao::new(db.clone()).insert(&media).unwrap();
    }

    fn epub_extension_blob() -> Vec<u8> {
        use std::io::Write;
        let position = |href: &str, progression: f32, position: i32, total: f32| {
            serde_json::json!({
                "href": href,
                "type": "application/xhtml+xml",
                "locations": {"progression": progression, "position": position, "totalProgression": total},
            })
        };
        let json = serde_json::json!({
            "toc": [],
            "landmarks": [],
            "pageList": [],
            "isFixedLayout": true,
            "positions": [
                position("text/ch1.xhtml", 0.0, 1, 0.25),
                position("text/ch2.xhtml", 0.5, 2, 0.75),
                position("text/ch3.xhtml", 0.75, 3, 1.0),
            ],
        });
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(json.to_string().as_bytes()).unwrap();
        encoder.finish().unwrap()
    }

    fn seed_progress(
        db: &Database,
        book_id: &str,
        user_id: &str,
        page: i32,
        locator: Option<serde_json::Value>,
    ) {
        ReadProgressDao::new(db.clone())
            .insert_or_update(&ReadProgress {
                book_id: book_id.to_string(),
                user_id: user_id.to_string(),
                page,
                completed: false,
                read_date: now_utc(),
                device_id: "dev-1".to_string(),
                device_name: "Kobo Clara".to_string(),
                locator,
                created_date: now_utc(),
                last_modified_date: now_utc(),
            })
            .unwrap();
    }

    fn seed_user_with_key(db: &Database, email: &str, plain_key: &str) -> String {
        let id = insert_user(db, email, false, true, &[]);
        insert_api_key(db, &id, plain_key);
        id
    }

    fn seed_sync_point(db: &Database, id: &str, user_id: &str, api_key_id: Option<&str>) {
        SyncPointDao::new(db.clone())
            .insert(&komga_core::model::sync_point::SyncPoint {
                id: id.to_string(),
                user_id: user_id.to_string(),
                api_key_id: api_key_id.map(str::to_string),
                created_date: now_utc(),
            })
            .unwrap();
    }

    #[tokio::test]
    async fn users_create_is_forbidden() {
        let app = test_app();
        let (status, body) = app
            .request_json("POST", "/koreader/users/create", "k1", None)
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["status"], 403);
        assert_eq!(
            body["message"],
            "403 FORBIDDEN \"User creation is disabled\""
        );
    }

    #[tokio::test]
    async fn users_auth() {
        let app = test_app();
        seed_user_with_key(&app.state.db, "user@x.c", "k1");
        let (status, body) = app.get_json("/koreader/users/auth", "k1").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, serde_json::json!({"authorized": "OK"}));
    }

    #[tokio::test]
    async fn progress_get_divina_page_and_fallback_percentage() {
        let app = test_app();
        let db = app.state.db.clone();
        seed_library(&db, "l1");
        seed_series(&db, "s1", "l1");
        seed_book_with_hash(&db, "b1", "s1", "l1", "hash-1");
        zip_media(&db, "b1", 10);
        let user_id = seed_user_with_key(&db, "user@x.c", "k1");
        seed_progress(&db, "b1", &user_id, 3, None);

        let (status, body) = app.get_json("/koreader/syncs/progress/hash-1", "k1").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["document"], "hash-1");
        assert_eq!(body["progress"], "3");
        assert_eq!(body["percentage"], 0.3);
        assert_eq!(body["device"], "Kobo Clara");
        assert_eq!(body["device_id"], "dev-1");
    }

    #[tokio::test]
    async fn progress_get_locator_total_progression_wins() {
        let app = test_app();
        let db = app.state.db.clone();
        seed_library(&db, "l1");
        seed_series(&db, "s1", "l1");
        seed_book_with_hash(&db, "b1", "s1", "l1", "hash-1");
        zip_media(&db, "b1", 10);
        let user_id = seed_user_with_key(&db, "user@x.c", "k1");
        seed_progress(
            &db,
            "b1",
            &user_id,
            3,
            Some(serde_json::json!({"href": "", "locations": {"totalProgression": 0.42}})),
        );

        let (status, body) = app.get_json("/koreader/syncs/progress/hash-1", "k1").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["percentage"], 0.42);
        assert_eq!(body["progress"], "3");
    }

    #[tokio::test]
    async fn progress_get_epub_docfragment() {
        let app = test_app();
        let db = app.state.db.clone();
        seed_library(&db, "l1");
        seed_series(&db, "s1", "l1");
        seed_book_with_hash(&db, "b1", "s1", "l1", "hash-1");
        epub_media(&db, "b1");
        let user_id = seed_user_with_key(&db, "user@x.c", "k1");
        seed_progress(
            &db,
            "b1",
            &user_id,
            4,
            Some(serde_json::json!({
                "href": "text/ch2.xhtml",
                "locations": {"totalProgression": 0.75}
            })),
        );

        let (status, body) = app.get_json("/koreader/syncs/progress/hash-1", "k1").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["progress"], "/body/DocFragment[2].0");
        assert_eq!(body["percentage"], 0.75);
    }

    #[tokio::test]
    async fn progress_get_not_found_and_conflict() {
        let app = test_app();
        let db = app.state.db.clone();
        seed_library(&db, "l1");
        seed_series(&db, "s1", "l1");
        seed_user_with_key(&db, "user@x.c", "k1");

        let (status, body) = app.get_json("/koreader/syncs/progress/nope", "k1").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["message"], "404 NOT_FOUND \"Book not found\"");

        seed_book_with_hash(&db, "b1", "s1", "l1", "dup");
        seed_book_with_hash(&db, "b2", "s1", "l1", "dup");
        let (status, body) = app.get_json("/koreader/syncs/progress/dup", "k1").await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(
            body["message"],
            "409 CONFLICT \"More than 1 book found with the same hash\""
        );
    }

    #[tokio::test]
    async fn progress_get_no_progress_returns_200_error_body() {
        let app = test_app();
        let db = app.state.db.clone();
        seed_library(&db, "l1");
        seed_series(&db, "s1", "l1");
        seed_book_with_hash(&db, "b1", "s1", "l1", "hash-1");
        zip_media(&db, "b1", 10);
        seed_user_with_key(&db, "user@x.c", "k1");

        let (status, body) = app.get_json("/koreader/syncs/progress/hash-1", "k1").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["status"], 200);
        assert_eq!(body["error"], "OK");
        assert_eq!(
            body["message"],
            "200 OK \"No progress found for this book\""
        );
        assert_eq!(body["path"], "/koreader/syncs/progress/hash-1");
    }

    #[tokio::test]
    async fn progress_put_divina() {
        let app = test_app();
        let db = app.state.db.clone();
        seed_library(&db, "l1");
        seed_series(&db, "s1", "l1");
        seed_book_with_hash(&db, "b1", "s1", "l1", "hash-1");
        zip_media(&db, "b1", 10);
        let user_id = seed_user_with_key(&db, "user@x.c", "k1");

        let (status, _) = app
            .request_json(
                "PUT",
                "/koreader/syncs/progress",
                "k1",
                Some(serde_json::json!({
                    "document": "hash-1",
                    "percentage": 0.5,
                    "progress": "5",
                    "device": "dev",
                    "device_id": "d1",
                })),
            )
            .await;
        assert_eq!(status, StatusCode::OK);

        let progress = ReadProgressDao::new(db.clone())
            .find_by_book_and_user("b1", &user_id)
            .unwrap()
            .expect("progress row");
        assert_eq!(progress.page, 5);
        assert!(!progress.completed);
        assert_eq!(progress.device_id, "d1");
        assert_eq!(progress.device_name, "dev");
        let locator = progress.locator.expect("locator");
        assert_eq!(locator["locations"]["position"], 5);
        assert_eq!(locator["locations"]["totalProgression"], 0.5);
    }

    #[tokio::test]
    async fn progress_put_epub_regex1() {
        let app = test_app();
        let db = app.state.db.clone();
        seed_library(&db, "l1");
        seed_series(&db, "s1", "l1");
        seed_book_with_hash(&db, "b1", "s1", "l1", "hash-1");
        epub_media(&db, "b1");
        let user_id = seed_user_with_key(&db, "user@x.c", "k1");

        let (status, _) = app
            .request_json(
                "PUT",
                "/koreader/syncs/progress",
                "k1",
                Some(serde_json::json!({
                    "document": "hash-1",
                    "percentage": 0.5,
                    "progress": "/body/DocFragment[2]/body/div/p[1]/text().0",
                    "device": "dev",
                    "device_id": "d1",
                })),
            )
            .await;
        assert_eq!(status, StatusCode::OK);

        let progress = ReadProgressDao::new(db.clone())
            .find_by_book_and_user("b1", &user_id)
            .unwrap()
            .expect("progress row");
        let locator = progress.locator.expect("locator");
        assert_eq!(locator["href"], "text/ch2.xhtml");
        // the matched position's totalProgression wins over the provided one
        assert_eq!(locator["locations"]["totalProgression"], 0.75);
        assert_eq!(progress.page, 8);
        assert!(!progress.completed);
    }

    #[tokio::test]
    async fn progress_put_epub_regex2() {
        let app = test_app();
        let db = app.state.db.clone();
        seed_library(&db, "l1");
        seed_series(&db, "s1", "l1");
        seed_book_with_hash(&db, "b1", "s1", "l1", "hash-1");
        epub_media(&db, "b1");
        let user_id = seed_user_with_key(&db, "user@x.c", "k1");

        let (status, _) = app
            .request_json(
                "PUT",
                "/koreader/syncs/progress",
                "k1",
                Some(serde_json::json!({
                    "document": "hash-1",
                    "percentage": 0.5,
                    "progress": "#_doc_fragment_1_ c37",
                    "device": "dev",
                    "device_id": "d1",
                })),
            )
            .await;
        assert_eq!(status, StatusCode::OK);

        let progress = ReadProgressDao::new(db.clone())
            .find_by_book_and_user("b1", &user_id)
            .unwrap()
            .expect("progress row");
        assert_eq!(progress.locator.expect("locator")["href"], "text/ch2.xhtml");
    }

    #[tokio::test]
    async fn progress_put_epub_invalid_progress_string() {
        let app = test_app();
        let db = app.state.db.clone();
        seed_library(&db, "l1");
        seed_series(&db, "s1", "l1");
        seed_book_with_hash(&db, "b1", "s1", "l1", "hash-1");
        epub_media(&db, "b1");
        seed_user_with_key(&db, "user@x.c", "k1");

        let (status, body) = app
            .request_json(
                "PUT",
                "/koreader/syncs/progress",
                "k1",
                Some(serde_json::json!({
                    "document": "hash-1",
                    "percentage": 0.5,
                    "progress": "garbage",
                    "device": "dev",
                    "device_id": "d1",
                })),
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(
            body["message"],
            "400 BAD_REQUEST \"Could not get Epub resource index from progress: garbage\""
        );
    }

    #[tokio::test]
    async fn syncpoints_delete_all_and_by_key() {
        let app = test_app();
        let db = app.state.db.clone();
        let user_id = seed_user_with_key(&db, "user@x.c", "k1");
        seed_sync_point(&db, "sp1", &user_id, None);
        seed_sync_point(&db, "sp2", &user_id, Some("ak-1"));
        seed_sync_point(&db, "sp3", &user_id, Some("ak-2"));

        let (status, _) = app
            .request_json("DELETE", "/api/v1/syncpoints/me", "k1", None)
            .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let dao = SyncPointDao::new(db.clone());
        assert_eq!(dao.find_by_user_id(&user_id).unwrap().len(), 0);

        seed_sync_point(&db, "sp4", &user_id, Some("ak-1"));
        seed_sync_point(&db, "sp5", &user_id, Some("ak-2"));
        let (status, _) = app
            .request_json("DELETE", "/api/v1/syncpoints/me?key_id=ak-1", "k1", None)
            .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let remaining = dao.find_by_user_id(&user_id).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].id, "sp5");
    }
}
