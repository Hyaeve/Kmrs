//! `TransientBooksController.kt`: transient book scan/analyze/page endpoints (admin only).

use crate::auth::RequireAuth;
use crate::error::ApiError;
use crate::http::headers::{check_not_modified, format_http_date};
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{routing, Json, Router};
use komga_core::dto::book::PageDto;
use komga_core::dto::{dto_datetime, url_to_file_path};
use komga_media::container::get_pdf_pages_dynamic;
use komga_media::detect;
use serde::{Deserialize, Serialize};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/transient-books",
            routing::post(scan_transient_books),
        )
        .route(
            "/api/v1/transient-books/{id}/analyze",
            routing::post(analyze_transient_book),
        )
        .route(
            "/api/v1/transient-books/{id}/pages/{pageNumber}",
            routing::get(get_page_by_transient_book_id),
        )
}

#[derive(Debug, Deserialize)]
pub struct ScanRequestDto {
    pub path: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransientBookDto {
    pub id: String,
    pub name: String,
    pub url: String,
    #[serde(with = "dto_datetime")]
    pub file_last_modified: time::OffsetDateTime,
    pub size_bytes: i64,
    pub size: String,
    pub status: String,
    pub media_type: String,
    pub pages: Vec<PageDto>,
    pub files: Vec<String>,
    pub comment: String,
    pub number: Option<f32>,
    pub series_id: Option<String>,
}

async fn scan_transient_books(
    State(state): State<AppState>,
    auth: RequireAuth,
    Json(body): Json<ScanRequestDto>,
) -> Result<Json<Vec<TransientBookDto>>, ApiError> {
    auth.0.require_admin()?;
    let path = std::path::PathBuf::from(&body.path);
    let books = crate::service::transient_book::scan_and_persist(&state, &path)
        .map_err(|e| ApiError::bad_request(e.code()))?;
    let mut dtos: Vec<TransientBookDto> = books.iter().map(to_dto).collect();
    dtos.sort_by(|a, b| a.url.cmp(&b.url));
    Ok(Json(dtos))
}

async fn analyze_transient_book(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path(id): Path<String>,
) -> Result<Json<TransientBookDto>, ApiError> {
    auth.0.require_admin()?;
    let Some(book) = crate::service::transient_book::find_by_id(&id) else {
        return Err(ApiError::not_found(""));
    };
    let updated = crate::service::transient_book::analyze_and_persist(&state, &book);
    Ok(Json(to_dto(&updated)))
}

async fn get_page_by_transient_book_id(
    State(_state): State<AppState>,
    auth: RequireAuth,
    Path((id, page_number)): Path<(String, i32)>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    auth.0.require_admin()?;
    let Some(book) = crate::service::transient_book::find_by_id(&id) else {
        return Err(ApiError::not_found(""));
    };
    let last_modified_millis = book.media.last_modified_date.unix_timestamp() * 1000
        + book.media.last_modified_date.nanosecond() as i64 / 1_000_000;
    if check_not_modified(last_modified_millis, &headers) {
        let mut response = StatusCode::NOT_MODIFIED.into_response();
        set_last_modified(&mut response, last_modified_millis);
        return Ok(response);
    }
    if page_number <= 0 {
        return Err(ApiError::bad_request("Page number does not exist"));
    }
    match crate::service::transient_book::get_book_page(&book, page_number as usize) {
        Ok(page_content) => {
            let mut response = (
                [(
                    axum::http::header::CONTENT_TYPE,
                    content_type_header(Some(&page_content.media_type)),
                )],
                page_content.bytes,
            )
                .into_response();
            set_last_modified(&mut response, last_modified_millis);
            Ok(response)
        }
        Err(komga_media::error::MediaError::PageOutOfBounds(_)) => {
            Err(ApiError::bad_request("Page number does not exist"))
        }
        Err(komga_media::error::MediaError::NotReady) => {
            Err(ApiError::not_found("Book analysis failed"))
        }
        Err(komga_media::error::MediaError::NoSuchFile(_)) => {
            Err(ApiError::not_found("File not found, it may have moved"))
        }
        Err(e) => Err(ApiError::Internal(e.to_string())),
    }
}

fn to_dto(book: &crate::service::transient_book::TransientBook) -> TransientBookDto {
    let media = &book.media;
    let pages = if komga_media::container::media_profile(media.media_type.as_deref())
        == Some(komga_core::search::MediaProfile::Pdf)
    {
        get_pdf_pages_dynamic(media).unwrap_or_default()
    } else {
        media.pages.clone()
    };
    TransientBookDto {
        id: book.book.id.clone(),
        name: book.book.name.clone(),
        url: url_to_file_path(&book.book.url),
        file_last_modified: book.book.file_last_modified,
        size_bytes: book.book.file_size,
        size: komga_core::dto::format_binary_bytes(book.book.file_size),
        status: media.status.as_str().to_string(),
        media_type: media.media_type.clone().unwrap_or_default(),
        pages: pages
            .iter()
            .enumerate()
            .map(|(index, page)| PageDto {
                number: (index + 1) as i32,
                file_name: page.file_name.clone(),
                media_type: page.media_type.clone(),
                width: page.width,
                height: page.height,
                size_bytes: page.file_size,
                size: page
                    .file_size
                    .map(komga_core::dto::format_binary_bytes)
                    .unwrap_or_default(),
            })
            .collect(),
        files: media.files.iter().map(|f| f.file_name.clone()).collect(),
        comment: media.comment.clone().unwrap_or_default(),
        number: book.metadata.number,
        series_id: book.metadata.series_id.clone(),
    }
}

fn set_last_modified(response: &mut Response, millis: i64) {
    response.headers_mut().insert(
        axum::http::header::LAST_MODIFIED,
        HeaderValue::from_str(&format_http_date(millis / 1000)).unwrap(),
    );
}

fn content_type_header(media_type: Option<&str>) -> HeaderValue {
    HeaderValue::from_str(&detect::media_type_or_default(media_type)).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::collections::tests::{call, get, insert_user, test_state};
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use komga_core::model::user::UserRole;

    fn seed_admin(state: &AppState) -> String {
        insert_user(
            &state.db,
            "a@b.c",
            &[UserRole::Admin],
            &[],
            Default::default(),
            "k",
        );
        "k".to_string()
    }

    fn seed_library(state: &AppState, root: &str) {
        state
            .db
            .rw()
            .execute(
                "INSERT INTO LIBRARY (ID, NAME, ROOT) VALUES ('l1', 'lib', ?)",
                [root],
            )
            .unwrap();
    }

    fn fixtures() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/resources")
    }

    #[tokio::test]
    async fn scan_folder_and_err_1017() {
        let state = test_state();
        seed_admin(&state);
        let dir = tempfile::tempdir().unwrap();
        let lib_dir = dir.path().join("library");
        let scan_dir = lib_dir.join("scanme");
        std::fs::create_dir_all(&scan_dir).unwrap();
        std::fs::copy(
            fixtures().join("archives/zip.zip"),
            scan_dir.join("book.cbz"),
        )
        .unwrap();
        seed_library(&state, &format!("file:{}/", lib_dir.display()));

        // folder inside an existing library → ERR_1017
        let body = serde_json::json!({"path": lib_dir.to_string_lossy()});
        let (status, json) = {
            let (s, _, b) = call(
                &state,
                router(),
                Request::post("/api/v1/transient-books")
                    .header("X-API-Key", "k")
                    .header("Content-Type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await;
            (s, serde_json::from_slice::<serde_json::Value>(&b).unwrap())
        };
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(json["message"].as_str().unwrap().contains("ERR_1017"));

        // outside the library → scanned
        let outside = dir.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::copy(
            fixtures().join("archives/zip.zip"),
            outside.join("book.cbz"),
        )
        .unwrap();
        let body = serde_json::json!({"path": outside.to_string_lossy()});
        let (status, json) = {
            let (s, _, b) = call(
                &state,
                router(),
                Request::post("/api/v1/transient-books")
                    .header("X-API-Key", "k")
                    .header("Content-Type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await;
            (s, serde_json::from_slice::<serde_json::Value>(&b).unwrap())
        };
        assert_eq!(status, StatusCode::OK);
        let books = json.as_array().unwrap();
        assert_eq!(books.len(), 1);
        assert_eq!(books[0]["name"], "book");
        assert_eq!(books[0]["status"], "UNKNOWN");
    }

    #[tokio::test]
    async fn analyze_and_pages() {
        let state = test_state();
        let key = seed_admin(&state);
        let dir = tempfile::tempdir().unwrap();
        let scan_dir = dir.path().join("scanme");
        std::fs::create_dir_all(&scan_dir).unwrap();
        let book_path = scan_dir.join("book.cbz");
        std::fs::copy(fixtures().join("archives/zip.zip"), &book_path).unwrap();
        seed_library(&state, "file:/nonexistent-parent/");

        let books = crate::service::transient_book::scan_and_persist(&state, &scan_dir).unwrap();
        assert_eq!(books.len(), 1);
        let id = books[0].book.id.clone();

        // analyze
        let (status, json) = {
            let (s, _, b) = call(
                &state,
                router(),
                Request::post(format!("/api/v1/transient-books/{id}/analyze"))
                    .header("X-API-Key", "k")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await;
            (s, serde_json::from_slice::<serde_json::Value>(&b).unwrap())
        };
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["status"], "READY");
        assert_eq!(json["mediaType"], "application/zip");
        assert_eq!(json["pages"].as_array().unwrap().len(), 1);
        assert_eq!(json["pages"][0]["mediaType"], "image/png");

        // page 200
        let (status, headers, bytes) = call(
            &state,
            router(),
            get(&format!("/api/v1/transient-books/{id}/pages/1"), &key),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers["content-type"], "image/png");
        assert_eq!(&bytes[0..4], b"\x89PNG");

        // 304 with If-Modified-Since
        let last_modified = headers["last-modified"].to_str().unwrap();
        let (status, _, _) = call(
            &state,
            router(),
            Request::get(format!("/api/v1/transient-books/{id}/pages/1"))
                .header("X-API-Key", "k")
                .header("If-Modified-Since", last_modified)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_MODIFIED);

        // out of bounds → 400
        let (status, _, _) = call(
            &state,
            router(),
            get(&format!("/api/v1/transient-books/{id}/pages/99"), &key),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        // unknown id → 404
        let (status, _, _) = call(
            &state,
            router(),
            get("/api/v1/transient-books/nope/pages/1", &key),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _, _) = call(
            &state,
            router(),
            Request::post("/api/v1/transient-books/nope/analyze")
                .header("X-API-Key", "k")
                .body(Body::empty())
                .unwrap(),
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
            router(),
            Request::post("/api/v1/transient-books")
                .header("X-API-Key", "k")
                .header("Content-Type", "application/json")
                .body(Body::from(r#"{"path": "/tmp"}"#))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }
}
