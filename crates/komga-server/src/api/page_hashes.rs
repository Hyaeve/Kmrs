//! `PageHashController.kt`: duplicate page-hash management (admin only).

use crate::auth::RequireAuth;
use crate::dto::common::{Page, Pageable};
use crate::dto::page_hash::{
    PageHashCreationDto, PageHashKnownDto, PageHashMatchDto, PageHashUnknownDto,
};
use crate::error::ApiError;
use crate::http::pagination::{QueryExt, QueryPageable};
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{routing, Json, Router};
use komga_core::model::page_hash::{PageHashAction, PageHashKnown};
use komga_core::task::BookPageNumbered;
use komga_db::dao::page_hash::{PageHashDao, PageHashMatchRow};
use komga_db::dto_dao::{DtoPage, PageRequest};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/page-hashes", routing::get(get_known_page_hashes))
        .route(
            "/api/v1/page-hashes",
            routing::put(create_or_update_known_page_hash),
        )
        .route(
            "/api/v1/page-hashes/unknown",
            routing::get(get_unknown_page_hashes),
        )
        .route(
            "/api/v1/page-hashes/{pageHash}",
            routing::get(get_page_hash_matches),
        )
        .route(
            "/api/v1/page-hashes/{pageHash}/thumbnail",
            routing::get(get_known_page_hash_thumbnail),
        )
        .route(
            "/api/v1/page-hashes/unknown/{pageHash}/thumbnail",
            routing::get(get_unknown_page_hash_thumbnail),
        )
        .route(
            "/api/v1/page-hashes/{pageHash}/delete-all",
            routing::post(delete_duplicate_pages_by_page_hash),
        )
        .route(
            "/api/v1/page-hashes/{pageHash}/delete-match",
            routing::post(delete_single_match_by_page_hash),
        )
}

fn dao(state: &AppState) -> PageHashDao {
    PageHashDao::new(state.db.clone())
}

fn to_page_request(pageable: &Pageable) -> PageRequest {
    PageRequest {
        page: pageable.page,
        size: pageable.size,
        unpaged: pageable.unpaged,
        sort: pageable
            .sort
            .iter()
            .map(|s| komga_db::dto_dao::SortOrder {
                property: s.property.clone(),
                descending: s.descending,
            })
            .collect(),
    }
}

fn to_page<T: serde::Serialize>(dto: DtoPage<T>, pageable: &Pageable) -> Page<T> {
    let mut p = pageable.clone();
    if !dto.sorted {
        p.sort = Vec::new();
    }
    Page::of(dto.items, dto.total.max(0) as u64, &p)
}

async fn get_known_page_hashes(
    State(state): State<AppState>,
    auth: RequireAuth,
    query: QueryPageable,
) -> Result<Json<Page<PageHashKnownDto>>, ApiError> {
    auth.0.require_admin()?;
    let raw: Vec<PageHashAction> = query
        .params
        .all("action")
        .iter()
        .filter_map(|s| PageHashAction::from_str(s))
        .collect();
    let actions = if raw.is_empty() { None } else { Some(raw) };
    let page =
        dao(&state).find_all_known_paged(actions.as_deref(), &to_page_request(&query.pageable))?;
    Ok(Json(to_page(
        map_items(page, |p| PageHashKnownDto::from(&p)),
        &query.pageable,
    )))
}

async fn get_known_page_hash_thumbnail(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path(page_hash): Path<String>,
) -> Result<Response, ApiError> {
    auth.0.require_admin()?;
    let bytes = dao(&state)
        .get_known_thumbnail(&page_hash)?
        .ok_or_else(|| ApiError::not_found(""))?;
    Ok(([(axum::http::header::CONTENT_TYPE, "image/jpeg")], bytes).into_response())
}

async fn get_unknown_page_hashes(
    State(state): State<AppState>,
    auth: RequireAuth,
    query: QueryPageable,
) -> Result<Json<Page<PageHashUnknownDto>>, ApiError> {
    auth.0.require_admin()?;
    let page = dao(&state).find_all_unknown_paged(&to_page_request(&query.pageable))?;
    Ok(Json(to_page(
        map_items(page, |p| PageHashUnknownDto::from(&p)),
        &query.pageable,
    )))
}

async fn get_page_hash_matches(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path(page_hash): Path<String>,
    query: QueryPageable,
) -> Result<Json<Page<PageHashMatchDto>>, ApiError> {
    auth.0.require_admin()?;
    let page =
        dao(&state).find_matches_by_hash_paged(&page_hash, &to_page_request(&query.pageable))?;
    Ok(Json(to_page(
        map_items(page, |m| match_dto(&m)),
        &query.pageable,
    )))
}

async fn get_unknown_page_hash_thumbnail(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path(page_hash): Path<String>,
    query: QueryPageable,
) -> Result<Response, ApiError> {
    auth.0.require_admin()?;
    let resize = query.params.first_u32("resize");
    let page = crate::service::page_hash::get_page(&state, &page_hash, resize)?
        .ok_or_else(|| ApiError::not_found(""))?;
    Ok((
        [(
            axum::http::header::CONTENT_TYPE,
            content_type_header(Some(&page.media_type)),
        )],
        page.bytes,
    )
        .into_response())
}

async fn create_or_update_known_page_hash(
    State(state): State<AppState>,
    auth: RequireAuth,
    Json(body): Json<PageHashCreationDto>,
) -> Result<StatusCode, ApiError> {
    auth.0.require_admin()?;
    if body.hash.trim().is_empty() {
        return Err(ApiError::Violations(vec![crate::error::Violation {
            field_name: "hash".into(),
            message: "must not be blank".into(),
        }]));
    }
    crate::service::page_hash::create_or_update(
        &state,
        &PageHashKnown {
            hash: body.hash,
            size: PageHashKnown::normalize_size(body.size),
            action: body.action,
            delete_count: 0,
            match_count: 0,
            created_date: komga_core::time_codec::now_utc(),
            last_modified_date: komga_core::time_codec::now_utc(),
        },
    )
    .map_err(|e| ApiError::bad_request(e.to_string()))?;
    Ok(StatusCode::ACCEPTED)
}

async fn delete_duplicate_pages_by_page_hash(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path(page_hash): Path<String>,
) -> Result<StatusCode, ApiError> {
    auth.0.require_admin()?;
    let matches = dao(&state)
        .find_matches_by_hash_paged(
            &page_hash,
            &PageRequest {
                page: 0,
                size: 20,
                unpaged: true,
                sort: vec![],
            },
        )?
        .items;
    let mut grouped: std::collections::BTreeMap<String, Vec<BookPageNumbered>> =
        std::collections::BTreeMap::new();
    for m in matches {
        grouped
            .entry(m.book_id.clone())
            .or_default()
            .push(book_page_numbered(&m, &page_hash));
    }
    state
        .task_emitter
        .remove_duplicate_pages(&grouped, komga_core::task::DEFAULT_PRIORITY)?;
    Ok(StatusCode::ACCEPTED)
}

async fn delete_single_match_by_page_hash(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path(page_hash): Path<String>,
    Json(body): Json<PageHashMatchDto>,
) -> Result<StatusCode, ApiError> {
    auth.0.require_admin()?;
    let page = BookPageNumbered {
        file_name: body.file_name,
        media_type: body.media_type,
        file_hash: page_hash,
        file_size: Some(body.file_size),
        page_number: body.page_number,
        width: None,
        height: None,
    };
    state.task_emitter.remove_duplicate_pages(
        &std::collections::BTreeMap::from([(body.book_id, vec![page])]),
        komga_core::task::DEFAULT_PRIORITY,
    )?;
    Ok(StatusCode::ACCEPTED)
}

fn book_page_numbered(m: &PageHashMatchRow, hash: &str) -> BookPageNumbered {
    BookPageNumbered {
        file_name: m.file_name.clone(),
        media_type: m.media_type.clone(),
        file_hash: hash.to_string(),
        file_size: Some(m.file_size),
        page_number: m.page_number,
        width: None,
        height: None,
    }
}

fn match_dto(m: &PageHashMatchRow) -> PageHashMatchDto {
    PageHashMatchDto {
        book_id: m.book_id.clone(),
        url: komga_core::dto::url_to_file_path(&m.url),
        page_number: m.page_number,
        file_name: m.file_name.clone(),
        file_size: m.file_size,
        media_type: m.media_type.clone(),
    }
}

fn map_items<T, U>(page: DtoPage<T>, f: impl Fn(T) -> U) -> DtoPage<U> {
    DtoPage {
        items: page.items.into_iter().map(f).collect(),
        total: page.total,
        sorted: page.sorted,
    }
}

fn content_type_header(media_type: Option<&str>) -> axum::http::HeaderValue {
    axum::http::HeaderValue::from_str(&komga_media::detect::media_type_or_default(media_type))
        .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::collections::tests::{call, get, insert_user, test_state};
    use axum::body::Body;
    use axum::http::Request;
    use komga_core::model::user::UserRole;
    use komga_db::dao::tasks::TasksDao;

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

    fn seed_book_pages(db: &komga_db::pool::Database, book_id: &str, hash: &str, pages: i32) {
        let conn = db.rw();
        conn.execute(
            "INSERT INTO LIBRARY (ID, NAME, ROOT) SELECT 'l1', 'lib', 'file:/l/' WHERE NOT EXISTS (SELECT 1 FROM LIBRARY WHERE ID = 'l1')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO SERIES (ID, NAME, URL, FILE_LAST_MODIFIED, LIBRARY_ID) SELECT 's1', 's', 'file:/l/s/', '2020-01-01 00:00:00.0', 'l1' WHERE NOT EXISTS (SELECT 1 FROM SERIES WHERE ID = 's1')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO BOOK (ID, NAME, URL, FILE_LAST_MODIFIED, SERIES_ID, LIBRARY_ID) VALUES (?, 'b', 'file:/l/b.cbz', '2020-01-01 00:00:00.0', 's1', 'l1')",
            [book_id],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO MEDIA (BOOK_ID, STATUS, MEDIA_TYPE, PAGE_COUNT) VALUES (?, 'READY', 'application/zip', ?)",
            rusqlite::params![book_id, pages],
        )
        .unwrap();
        for i in 0..pages {
            conn.execute(
                "INSERT INTO MEDIA_PAGE (BOOK_ID, FILE_NAME, MEDIA_TYPE, NUMBER, FILE_HASH, FILE_SIZE) VALUES (?, ?, 'image/jpeg', ?, ?, 1024)",
                rusqlite::params![book_id, format!("p{i}.jpg"), i, hash],
            )
            .unwrap();
        }
    }

    #[tokio::test]
    async fn known_crud_and_filter() {
        let state = test_state();
        let key = seed_admin(&state);
        let dao = PageHashDao::new(state.db.clone());
        dao.insert(
            &PageHashKnown {
                hash: "h1".into(),
                size: Some(100),
                action: PageHashAction::DeleteAuto,
                delete_count: 0,
                match_count: 0,
                created_date: komga_core::time_codec::now_utc(),
                last_modified_date: komga_core::time_codec::now_utc(),
            },
            None,
        )
        .unwrap();
        dao.insert(
            &PageHashKnown {
                hash: "h2".into(),
                size: None,
                action: PageHashAction::Ignore,
                delete_count: 0,
                match_count: 0,
                created_date: komga_core::time_codec::now_utc(),
                last_modified_date: komga_core::time_codec::now_utc(),
            },
            Some(&[1, 2, 3]),
        )
        .unwrap();

        let (status, json) = {
            let (s, _, b) = call(&state, router(), get("/api/v1/page-hashes", &key)).await;
            (s, serde_json::from_slice::<serde_json::Value>(&b).unwrap())
        };
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["totalElements"], 2);
        assert_eq!(json["content"][0]["action"], "DELETE_AUTO");

        let (status, json) = {
            let (s, _, b) = call(
                &state,
                router(),
                get("/api/v1/page-hashes?action=IGNORE", &key),
            )
            .await;
            (s, serde_json::from_slice::<serde_json::Value>(&b).unwrap())
        };
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["totalElements"], 1);
        assert_eq!(json["content"][0]["hash"], "h2");

        // thumbnail of a known hash
        let (status, headers, bytes) = call(
            &state,
            router(),
            get("/api/v1/page-hashes/h2/thumbnail", &key),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers["content-type"], "image/jpeg");
        assert_eq!(bytes, vec![1, 2, 3]);
        let (status, _, _) = call(
            &state,
            router(),
            get("/api/v1/page-hashes/nope/thumbnail", &key),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn create_and_update_via_put() {
        let state = test_state();
        seed_admin(&state);
        let body = serde_json::json!({"hash": "h1", "size": 42, "action": "DELETE_MANUAL"});
        let (status, _) = {
            let (s, _, b) = call(
                &state,
                router(),
                Request::put("/api/v1/page-hashes")
                    .header("X-API-Key", "k")
                    .header("Content-Type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await;
            (s, b)
        };
        assert_eq!(status, StatusCode::ACCEPTED);
        let known = PageHashDao::new(state.db.clone())
            .find_known("h1")
            .unwrap()
            .unwrap();
        assert_eq!(known.action, PageHashAction::DeleteManual);
        assert_eq!(known.size, Some(42));

        // update the action
        let body = serde_json::json!({"hash": "h1", "action": "IGNORE"});
        let (status, _, _) = call(
            &state,
            router(),
            Request::put("/api/v1/page-hashes")
                .header("X-API-Key", "k")
                .header("Content-Type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED);
        let known = PageHashDao::new(state.db.clone())
            .find_known("h1")
            .unwrap()
            .unwrap();
        assert_eq!(known.action, PageHashAction::Ignore);

        // blank hash → violations
        let body = serde_json::json!({"hash": "  ", "action": "IGNORE"});
        let (status, json) = {
            let (s, _, b) = call(
                &state,
                router(),
                Request::put("/api/v1/page-hashes")
                    .header("X-API-Key", "k")
                    .header("Content-Type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await;
            (s, serde_json::from_slice::<serde_json::Value>(&b).unwrap())
        };
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(json["violations"].is_array());
    }

    #[tokio::test]
    async fn unknown_and_matches_and_delete() {
        let state = test_state();
        let key = seed_admin(&state);
        seed_book_pages(&state.db, "b1", "duphash", 2);
        seed_book_pages(&state.db, "b2", "duphash", 1);
        seed_book_pages(&state.db, "b3", "other", 1);

        // unknown list
        let (status, json) = {
            let (s, _, b) = call(&state, router(), get("/api/v1/page-hashes/unknown", &key)).await;
            (s, serde_json::from_slice::<serde_json::Value>(&b).unwrap())
        };
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["totalElements"], 1);
        assert_eq!(json["content"][0]["hash"], "duphash");
        assert_eq!(json["content"][0]["matchCount"], 3);

        // matches
        let (status, json) = {
            let (s, _, b) = call(&state, router(), get("/api/v1/page-hashes/duphash", &key)).await;
            (s, serde_json::from_slice::<serde_json::Value>(&b).unwrap())
        };
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["totalElements"], 3);
        assert_eq!(json["content"][0]["pageNumber"], 1);

        // delete-all → one RemoveHashedPages task per book
        let (status, _, _) = call(
            &state,
            router(),
            Request::post("/api/v1/page-hashes/duphash/delete-all")
                .header("X-API-Key", "k")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED);
        let tasks = TasksDao::new(state.tasks_db.clone()).find_all().unwrap();
        assert_eq!(tasks.len(), 2);
        assert!(tasks
            .iter()
            .all(|t| matches!(t, komga_core::task::Task::RemoveHashedPages(_))));

        // delete-match → single task with one page
        TasksDao::new(state.tasks_db.clone()).delete_all().unwrap();
        let body = serde_json::json!({
            "bookId": "b1", "url": "x", "pageNumber": 2, "fileName": "p1.jpg",
            "fileSize": 1024, "mediaType": "image/jpeg"
        });
        let (status, _, _) = call(
            &state,
            router(),
            Request::post("/api/v1/page-hashes/duphash/delete-match")
                .header("X-API-Key", "k")
                .header("Content-Type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED);
        let tasks = TasksDao::new(state.tasks_db.clone()).find_all().unwrap();
        assert_eq!(tasks.len(), 1);
        let komga_core::task::Task::RemoveHashedPages(t) = &tasks[0] else {
            panic!("expected RemoveHashedPages")
        };
        assert_eq!(t.book_id, "b1");
        assert_eq!(t.pages.len(), 1);
        assert_eq!(t.pages[0].page_number, 2);
    }

    #[tokio::test]
    async fn admin_required() {
        let state = test_state();
        insert_user(&state.db, "u@b.c", &[], &[], Default::default(), "k");
        let (status, _, _) = call(&state, router(), get("/api/v1/page-hashes", "k")).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }
}
