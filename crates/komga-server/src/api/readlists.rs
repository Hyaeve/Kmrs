//! Equivalent of `ReadListController` (read-only side + read progress + zip download):
//! /api/v1/readlists/**.

use crate::api::collections::{
    book_thumbnail_bytes, bool_op, create_mosaic, enum_conversion_error, jpeg_response,
    library_id_param, page_of, parse_read_status, to_page_request,
};
use crate::auth::RequireAuth;
use crate::dto::common::{Page, SortOrder};
use crate::error::{ApiError, Violation};
use crate::http::headers::content_disposition;
use crate::http::pagination::{QueryExt, QueryPageable};
use crate::state::AppState;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::Response;
use axum::{routing, Json, Router};
use komga_core::dto::book::BookDto;
use komga_core::dto::readlist::ReadListDto;
use komga_core::dto::tachiyomi::{TachiyomiReadProgressDto, TachiyomiReadProgressUpdateDto};
use komga_core::dto::thumbnail::ThumbnailReadListDto;
use komga_core::model::media::MediaStatus;
use komga_core::model::read_progress::ReadProgress;
use komga_core::model::readlist::ReadList;
use komga_core::model::user::{KomgaUser, UserRole};
use komga_core::search::*;
use komga_core::time_codec::now_utc;
use komga_db::dao::book::BookDao;
use komga_db::dao::media::MediaDao;
use komga_db::dao::read_progress::ReadProgressDao;
use komga_db::dao::thumbnail::ThumbnailReadListDao;
use komga_db::dto_dao::book::BookDtoDao;
use komga_db::dto_dao::read_progress::ReadProgressDtoDao;
use komga_db::dto_dao::readlist::ReadListDtoDao;
use komga_db::dto_dao::{DtoPage, PageRequest};
use std::io::Write;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/readlists", routing::get(get_readlists))
        .route("/api/v1/readlists/{id}", routing::get(get_readlist_by_id))
        .route(
            "/api/v1/readlists/{id}/thumbnail",
            routing::get(get_readlist_thumbnail),
        )
        .route(
            "/api/v1/readlists/{id}/thumbnails",
            routing::get(get_readlist_thumbnails),
        )
        .route(
            "/api/v1/readlists/{id}/thumbnails/{thumbnailId}",
            routing::get(get_readlist_thumbnail_by_id),
        )
        .route(
            "/api/v1/readlists/{id}/books",
            routing::get(get_books_by_readlist_id),
        )
        .route(
            "/api/v1/readlists/{id}/books/{bookId}/previous",
            routing::get(get_book_sibling_previous_in_readlist),
        )
        .route(
            "/api/v1/readlists/{id}/books/{bookId}/next",
            routing::get(get_book_sibling_next_in_readlist),
        )
        .route(
            "/api/v1/readlists/{id}/read-progress/tachiyomi",
            routing::get(get_mihon_read_progress).put(update_mihon_read_progress),
        )
        .route(
            "/api/v1/readlists/{id}/file",
            routing::get(download_readlist_as_zip),
        )
}

async fn get_readlists(
    State(state): State<AppState>,
    auth: RequireAuth,
    qp: QueryPageable,
) -> Result<Json<Page<ReadListDto>>, ApiError> {
    let user = &auth.0.user;
    let search = qp.params.first("search").filter(|s| !s.trim().is_empty());
    let sort = if !qp.pageable.sort.is_empty() {
        qp.pageable.sort.clone()
    } else if search.is_some() {
        vec![SortOrder {
            property: "relevance".into(),
            descending: false,
        }]
    } else {
        vec![SortOrder {
            property: "name".into(),
            descending: false,
        }]
    };
    let page_request = to_page_request(&qp.pageable, sort.clone());
    let result = ReadListDtoDao::new(state.db.clone()).find_all(
        user.get_authorized_library_ids(library_id_param(&qp).as_ref())
            .as_ref(),
        user.get_authorized_library_ids(None).as_ref(),
        search,
        &page_request,
        &user.restrictions,
    )?;
    Ok(Json(page_of(result, &qp.pageable, sort)))
}

async fn get_readlist_by_id(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path(id): Path<String>,
) -> Result<Json<ReadListDto>, ApiError> {
    let readlist = find_visible_readlist(&state, &auth.0.user, &id)?;
    Ok(Json(ReadListDto::from(&readlist)))
}

async fn get_readlist_thumbnail(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let readlist = find_visible_readlist(&state, &auth.0.user, &id)?;
    let bytes = readlist_thumbnail_bytes(&state, &readlist)?;
    Ok(jpeg_response(bytes, Some("private, max-age=3600")))
}

async fn get_readlist_thumbnails(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path(id): Path<String>,
) -> Result<Json<Vec<ThumbnailReadListDto>>, ApiError> {
    find_visible_readlist(&state, &auth.0.user, &id)?;
    let thumbnails = ThumbnailReadListDao::new(state.db.clone()).find_all_by_read_list_id(&id)?;
    Ok(Json(
        thumbnails.iter().map(ThumbnailReadListDto::from).collect(),
    ))
}

async fn get_readlist_thumbnail_by_id(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path((id, thumbnail_id)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    let readlist = find_visible_readlist(&state, &auth.0.user, &id)?;
    let thumbnail = ThumbnailReadListDao::new(state.db.clone())
        .find_by_id(&thumbnail_id)?
        .ok_or_else(|| ApiError::not_found(""))?;
    if thumbnail.read_list_id != readlist.id {
        return Err(ApiError::bad_request(""));
    }
    Ok(jpeg_response(thumbnail.thumbnail, None))
}

async fn get_books_by_readlist_id(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path(id): Path<String>,
    qp: QueryPageable,
) -> Result<Json<Page<BookDto>>, ApiError> {
    let user = &auth.0.user;
    let readlist = find_visible_readlist(&state, user, &id)?;
    let sort = vec![SortOrder {
        property: if readlist.ordered {
            "readList.number".into()
        } else {
            "metadata.releaseDate".into()
        },
        descending: false,
    }];
    let condition = readlist_books_condition(&readlist, &qp)?;
    let search = BookSearch {
        condition: Some(condition),
        full_text_search: None,
    };
    let page_request = to_page_request(&qp.pageable, sort.clone());
    let result = BookDtoDao::new(state.db.clone()).find_all(
        &search,
        &SearchContext::of_user(user),
        &page_request,
    )?;
    let items = result
        .items
        .into_iter()
        .map(|b| b.restrict_url(!user.is_admin()))
        .collect();
    Ok(Json(page_of(
        DtoPage {
            items,
            total: result.total,
            sorted: result.sorted,
        },
        &qp.pageable,
        sort,
    )))
}

async fn get_book_sibling_previous_in_readlist(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path((id, book_id)): Path<(String, String)>,
) -> Result<Json<BookDto>, ApiError> {
    get_book_sibling_in_readlist(&state, &auth, &id, &book_id, false).await
}

async fn get_book_sibling_next_in_readlist(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path((id, book_id)): Path<(String, String)>,
) -> Result<Json<BookDto>, ApiError> {
    get_book_sibling_in_readlist(&state, &auth, &id, &book_id, true).await
}

async fn get_book_sibling_in_readlist(
    state: &AppState,
    auth: &RequireAuth,
    id: &str,
    book_id: &str,
    next: bool,
) -> Result<Json<BookDto>, ApiError> {
    let user = &auth.0.user;
    let readlist = find_visible_readlist(state, user, id)?;
    let dao = BookDtoDao::new(state.db.clone());
    let authorized = user.get_authorized_library_ids(None);
    let book = if next {
        dao.find_next_in_readlist(
            &readlist,
            book_id,
            &user.id,
            authorized.as_ref(),
            &user.restrictions,
        )?
    } else {
        dao.find_previous_in_readlist(
            &readlist,
            book_id,
            &user.id,
            authorized.as_ref(),
            &user.restrictions,
        )?
    }
    .ok_or_else(|| ApiError::not_found(""))?;
    Ok(Json(book.restrict_url(!user.is_admin())))
}

async fn get_mihon_read_progress(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path(id): Path<String>,
) -> Result<Json<TachiyomiReadProgressDto>, ApiError> {
    let user = &auth.0.user;
    let readlist = find_visible_readlist(&state, user, &id)?;
    let progress = ReadProgressDtoDao::new(state.db.clone())
        .find_progress_by_readlist(&readlist.id, &user.id)?;
    Ok(Json(progress))
}

async fn update_mihon_read_progress(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path(id): Path<String>,
    Json(body): Json<TachiyomiReadProgressUpdateDto>,
) -> Result<StatusCode, ApiError> {
    if body.last_book_read < 0 {
        return Err(ApiError::Violations(vec![Violation {
            field_name: "lastBookRead".into(),
            message: "must be greater than or equal to 0".into(),
        }]));
    }
    let user = &auth.0.user;
    let readlist = find_visible_readlist(&state, user, &id)?;
    let search = BookSearch {
        condition: Some(SearchConditionBook::ReadListId {
            operator: Equality::Is {
                value: readlist.id.clone(),
            },
        }),
        full_text_search: None,
    };
    let page_request = PageRequest {
        page: 0,
        size: 20,
        unpaged: true,
        sort: vec![komga_db::dto_dao::SortOrder {
            property: "readList.number".into(),
            descending: false,
        }],
    };
    let books = BookDtoDao::new(state.db.clone()).find_all(
        &search,
        &SearchContext::of_user(user),
        &page_request,
    )?;
    // Kotlin's filterIndexed { index < lastBookRead }
    for book in books.items.iter().take(body.last_book_read as usize) {
        if book.read_progress.as_ref().map(|p| p.completed) != Some(true) {
            mark_read_progress_completed(&state, &book.id, user)?;
        }
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn download_readlist_as_zip(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    auth.0.require_role(UserRole::FileDownload)?;
    let readlist = find_visible_readlist(&state, &auth.0.user, &id)?;

    let mut entries: Vec<(String, std::path::PathBuf)> = vec![];
    for (index, book_id) in &readlist.book_ids {
        let Some(book) = BookDao::new(state.db.clone()).find_by_id(book_id)? else {
            continue;
        };
        let path = std::path::PathBuf::from(komga_core::dto::url_to_file_path(&book.url));
        if !path.exists() {
            tracing::warn!(
                "Book file not found, skipping archive entry: {}",
                path.display()
            );
            continue;
        }
        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        entries.push((format!("{} - {}", index + 1, file_name), path));
    }

    // the whole archive is built in memory: pages are Stored (no compression work), and a zip
    // buffer cannot be rewound mid-stream without much more complexity
    let mut cursor = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut cursor);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        for (name, path) in &entries {
            zip.start_file(name, options)
                .map_err(|e| ApiError::Internal(e.to_string()))?;
            let bytes = std::fs::read(path).map_err(|e| {
                ApiError::Internal(format!("could not read {}: {e}", path.display()))
            })?;
            zip.write_all(&bytes)
                .map_err(|e| ApiError::Internal(e.to_string()))?;
        }
        zip.finish()
            .map_err(|e| ApiError::Internal(e.to_string()))?;
    }

    Ok(Response::builder()
        .header(header::CONTENT_TYPE, "application/zip")
        .header(
            header::CONTENT_DISPOSITION,
            content_disposition("attachment", &format!("{}.zip", readlist.name)),
        )
        .body(Body::from(cursor.into_inner()))
        .expect("zip response"))
}

fn find_visible_readlist(
    state: &AppState,
    user: &KomgaUser,
    id: &str,
) -> Result<ReadList, ApiError> {
    ReadListDtoDao::new(state.db.clone())
        .find_by_id(
            id,
            user.get_authorized_library_ids(None).as_ref(),
            &user.restrictions,
        )?
        .ok_or_else(|| ApiError::not_found(""))
}

/// `ReadListLifecycle.getThumbnailBytes`: the selected thumbnail, or a 2x2 mosaic of the first
/// 4 member books' thumbnails (the id list is cycled to fill the grid, as in komga)
fn readlist_thumbnail_bytes(state: &AppState, readlist: &ReadList) -> Result<Vec<u8>, ApiError> {
    if let Some(selected) =
        ThumbnailReadListDao::new(state.db.clone()).find_selected_by_read_list_id(&readlist.id)?
    {
        return Ok(selected.thumbnail);
    }
    let mut ids = Vec::new();
    let book_ids: Vec<&String> = readlist.book_ids.values().collect();
    while ids.len() < 4 && !book_ids.is_empty() {
        ids.extend(book_ids.iter().take(4).map(|id| id.to_string()));
    }
    ids.truncate(4);
    let images: Vec<Vec<u8>> = ids
        .iter()
        .filter_map(|id| book_thumbnail_bytes(state, id))
        .collect();
    create_mosaic(&images, state.settings.get().thumbnail_size.max_edge())
}

/// Filter conditions of `getBooksByReadListId`: read list membership plus the query params
fn readlist_books_condition(
    readlist: &ReadList,
    qp: &QueryPageable,
) -> Result<SearchConditionBook, ApiError> {
    let mut conditions = vec![SearchConditionBook::ReadListId {
        operator: Equality::Is {
            value: readlist.id.clone(),
        },
    }];
    let library_ids = qp.params.all("library_id");
    if !library_ids.is_empty() {
        conditions.push(any_of_book(library_ids, |id| {
            SearchConditionBook::LibraryId {
                operator: Equality::Is { value: id.clone() },
            }
        }));
    }
    let read_statuses = qp.params.all("read_status");
    if !read_statuses.is_empty() {
        let parsed = read_statuses
            .iter()
            .map(|s| {
                parse_read_status(s).ok_or_else(|| {
                    enum_conversion_error("org.gotson.komga.domain.model.ReadStatus", s)
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        conditions.push(SearchConditionBook::AnyOf {
            conditions: parsed
                .into_iter()
                .map(|value| SearchConditionBook::ReadStatus {
                    operator: Equality::Is { value },
                })
                .collect(),
        });
    }
    let tags = qp.params.all("tag");
    if !tags.is_empty() {
        conditions.push(any_of_book(tags, |t| SearchConditionBook::Tag {
            tag: EqualityNullable::Is { value: t.clone() },
        }));
    }
    let media_statuses = qp.params.all("media_status");
    if !media_statuses.is_empty() {
        let parsed = media_statuses
            .iter()
            .map(|s| {
                MediaStatus::from_str(s).ok_or_else(|| {
                    enum_conversion_error("org.gotson.komga.domain.model.Media$Status", s)
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        conditions.push(SearchConditionBook::AnyOf {
            conditions: parsed
                .into_iter()
                .map(|value| SearchConditionBook::MediaStatus {
                    operator: Equality::Is { value },
                })
                .collect(),
        });
    }
    if let Some(deleted) = qp.params.first_bool("deleted") {
        conditions.push(SearchConditionBook::Deleted {
            deleted: bool_op(deleted),
        });
    }
    let authors = crate::http::headers::parse_authors(qp.params.all("author"));
    if !authors.is_empty() {
        conditions.push(any_of_book(&authors, |a| SearchConditionBook::Author {
            author: Equality::Is {
                value: AuthorMatch {
                    name: Some(a.name.clone()),
                    role: Some(a.role.clone()),
                },
            },
        }));
    }
    Ok(SearchConditionBook::AllOf { conditions })
}

fn any_of_book<T, F: Fn(&T) -> SearchConditionBook>(values: &[T], f: F) -> SearchConditionBook {
    SearchConditionBook::AnyOf {
        conditions: values.iter().map(f).collect(),
    }
}

/// `BookLifecycle.markReadProgressCompleted`: progress at the last page, completed; the DAO
/// recomputes the per-series aggregate
fn mark_read_progress_completed(
    state: &AppState,
    book_id: &str,
    user: &KomgaUser,
) -> Result<(), ApiError> {
    let media = MediaDao::new(state.db.clone())
        .find_by_id(book_id)?
        .ok_or_else(|| ApiError::Internal(format!("no media for book {book_id}")))?;
    let progress = ReadProgress {
        book_id: book_id.into(),
        user_id: user.id.clone(),
        page: media.page_count,
        completed: true,
        read_date: now_utc(),
        device_id: String::new(),
        device_name: String::new(),
        locator: None,
        created_date: now_utc(),
        last_modified_date: now_utc(),
    };
    ReadProgressDao::new(state.db.clone()).insert_or_update(&progress)?;
    let _ = state
        .events
        .send(crate::events::DomainEvent::ReadProgressChanged(progress));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::collections::tests::{
        call, exec, get, insert_user, seed_base, test_state, tiny_jpeg, ADMIN_KEY, USER_KEY,
    };
    use axum::http::Request;
    use komga_core::model::user::ContentRestrictions;

    fn seed_readlist(
        db: &komga_db::pool::Database,
        id: &str,
        name: &str,
        ordered: bool,
        books: &[&str],
    ) {
        exec(
            db,
            "INSERT INTO READLIST (ID, NAME, SUMMARY, ORDERED, BOOK_COUNT) VALUES (?, ?, '', ?, ?)",
            rusqlite::params![id, name, ordered, books.len() as i32],
        );
        for (i, b) in books.iter().enumerate() {
            exec(
                db,
                "INSERT INTO READLIST_BOOK (READLIST_ID, BOOK_ID, NUMBER) VALUES (?, ?, ?)",
                rusqlite::params![id, b, i as i32],
            );
        }
    }

    fn seed_thumbnail_readlist(
        db: &komga_db::pool::Database,
        id: &str,
        readlist_id: &str,
        selected: bool,
    ) {
        exec(
            db,
            "INSERT INTO THUMBNAIL_READLIST \
             (ID, READLIST_ID, THUMBNAIL, SELECTED, TYPE, MEDIA_TYPE, FILE_SIZE, WIDTH, HEIGHT) \
             VALUES (?, ?, ?, ?, 'USER_UPLOADED', 'image/jpeg', 3, 1, 1)",
            rusqlite::params![id, readlist_id, tiny_jpeg(), selected],
        );
    }

    fn seed_thumbnail_book(db: &komga_db::pool::Database, id: &str, book_id: &str) {
        exec(
            db,
            "INSERT INTO THUMBNAIL_BOOK \
             (ID, BOOK_ID, THUMBNAIL, SELECTED, TYPE, MEDIA_TYPE, FILE_SIZE, WIDTH, HEIGHT) \
             VALUES (?, ?, ?, 1, 'GENERATED', 'image/jpeg', 3, 1, 1)",
            rusqlite::params![id, book_id, tiny_jpeg()],
        );
    }

    fn seed_read_progress(
        db: &komga_db::pool::Database,
        book_id: &str,
        user_id: &str,
        completed: bool,
    ) {
        // via the DAO so the per-series aggregate is recomputed like on the write path
        let read_date = komga_core::time_codec::parse_datetime_utc("2024-01-01 00:00:00").unwrap();
        ReadProgressDao::new(db.clone())
            .insert_or_update(&ReadProgress {
                book_id: book_id.into(),
                user_id: user_id.into(),
                page: if completed { 10 } else { 5 },
                completed,
                read_date,
                device_id: String::new(),
                device_name: String::new(),
                locator: None,
                created_date: read_date,
                last_modified_date: read_date,
            })
            .unwrap();
    }

    fn seed_base_with_readlists(db: &komga_db::pool::Database) {
        seed_base(db);
        // r1 ordered: b1, b3, b2 (deliberately not series order); r2 unordered: b4
        seed_readlist(db, "r1", "Marvel", true, &["b1", "b3", "b2"]);
        seed_readlist(db, "r2", "z-last", false, &["b4"]);
    }

    fn admin_id(db: &komga_db::pool::Database) -> String {
        db.ro()
            .query_row("SELECT ID FROM USER WHERE EMAIL = 'admin@x.y'", [], |r| {
                r.get(0)
            })
            .unwrap()
    }

    #[tokio::test]
    async fn list_readlists_pagination_library_filter_and_404() {
        let state = test_state();
        seed_base_with_readlists(&state.db);
        let (status, _, body) = call(&state, router(), get("/api/v1/readlists", ADMIN_KEY)).await;
        assert_eq!(status, StatusCode::OK);
        let page: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(page["totalElements"], 2);
        // unicode3 collation: "Marvel" before "z-last"
        assert_eq!(page["content"][0]["name"], "Marvel");

        // library filter: r2's book is on l2 only
        let (status, _, body) = call(
            &state,
            router(),
            get("/api/v1/readlists?library_id=l2", ADMIN_KEY),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let page: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(page["totalElements"], 1);
        assert_eq!(page["content"][0]["id"], "r2");

        // user shared only on l1 does not see r2
        let (status, _, body) = call(&state, router(), get("/api/v1/readlists", USER_KEY)).await;
        assert_eq!(status, StatusCode::OK);
        let page: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(page["totalElements"], 1);
        assert_eq!(page["content"][0]["id"], "r1");

        let (status, _, _) = call(&state, router(), get("/api/v1/readlists/nope", ADMIN_KEY)).await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        let (status, _, body) =
            call(&state, router(), get("/api/v1/readlists/r1", ADMIN_KEY)).await;
        assert_eq!(status, StatusCode::OK);
        let dto: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(dto["name"], "Marvel");
        assert_eq!(dto["ordered"], true);
        assert_eq!(dto["bookIds"].as_array().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn list_readlists_restricted_filtered_flag() {
        let state = test_state();
        seed_base_with_readlists(&state.db);
        exec(
            &state.db,
            "INSERT INTO SERIES_METADATA_SHARING (SERIES_ID, LABEL) VALUES ('s1', 'kids')",
            [],
        );
        insert_user(
            &state.db,
            "restricted@x.y",
            &[],
            &[],
            ContentRestrictions::new(
                None,
                ["kids".to_string()].into_iter().collect(),
                std::collections::BTreeSet::new(),
            ),
            "restricted-key",
        );
        let (status, _, body) =
            call(&state, router(), get("/api/v1/readlists", "restricted-key")).await;
        assert_eq!(status, StatusCode::OK);
        let page: serde_json::Value = serde_json::from_slice(&body).unwrap();
        // only r1 remains visible; b3 (series s2, no label) is filtered out
        assert_eq!(page["totalElements"], 1);
        assert_eq!(page["content"][0]["id"], "r1");
        assert_eq!(page["content"][0]["filtered"], true);
        let book_ids: Vec<&str> = page["content"][0]["bookIds"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| b.as_str().unwrap())
            .collect();
        assert_eq!(book_ids, ["b1", "b2"]);
    }

    #[tokio::test]
    async fn thumbnail_selected_mosaic_and_by_id() {
        let state = test_state();
        seed_base_with_readlists(&state.db);
        seed_thumbnail_readlist(&state.db, "tr1", "r1", true);
        let (status, headers, body) = call(
            &state,
            router(),
            get("/api/v1/readlists/r1/thumbnail", ADMIN_KEY),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers[header::CONTENT_TYPE], "image/jpeg");
        assert_eq!(headers[header::CACHE_CONTROL], "private, max-age=3600");
        assert_eq!(body, tiny_jpeg());

        // mosaic when no selected thumbnail (b1 has a book thumbnail)
        seed_thumbnail_book(&state.db, "tb1", "b1");
        let (status, headers, _) = call(
            &state,
            router(),
            get("/api/v1/readlists/r2/thumbnail", ADMIN_KEY),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers[header::CACHE_CONTROL], "private, max-age=3600");

        let (status, _, body) = call(
            &state,
            router(),
            get("/api/v1/readlists/r1/thumbnails", ADMIN_KEY),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let list: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(list.as_array().unwrap().len(), 1);
        assert_eq!(list[0]["id"], "tr1");
        assert_eq!(list[0]["readListId"], "r1");

        let (status, _, _) = call(
            &state,
            router(),
            get("/api/v1/readlists/r2/thumbnails/tr1", ADMIN_KEY),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let (status, _, _) = call(
            &state,
            router(),
            get("/api/v1/readlists/r1/thumbnails/nope", ADMIN_KEY),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn readlist_books_ordered_and_unordered_sort() {
        let state = test_state();
        seed_base_with_readlists(&state.db);
        let (status, _, body) = call(
            &state,
            router(),
            get("/api/v1/readlists/r1/books", ADMIN_KEY),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let page: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let ids: Vec<&str> = page["content"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| b["id"].as_str().unwrap())
            .collect();
        // readList.number order: b1, b3, b2
        assert_eq!(ids, ["b1", "b3", "b2"]);
        assert_eq!(page["totalElements"], 3);

        // unordered r2 falls back to metadata.releaseDate
        let (status, _, body) = call(
            &state,
            router(),
            get("/api/v1/readlists/r2/books", ADMIN_KEY),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let page: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(page["content"][0]["id"], "b4");
        assert_eq!(page["content"][0]["seriesTitle"], "Gamma");
    }

    #[tokio::test]
    async fn readlist_books_invalid_enum_param() {
        let state = test_state();
        seed_base_with_readlists(&state.db);
        let (status, _, body) = call(
            &state,
            router(),
            get("/api/v1/readlists/r1/books?read_status=BOGUS", ADMIN_KEY),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let error: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(error["status"], 400);
        assert!(error["message"]
            .as_str()
            .unwrap()
            .contains("No enum constant org.gotson.komga.domain.model.ReadStatus.BOGUS"));
    }

    #[tokio::test]
    async fn readlist_sibling_previous_next() {
        let state = test_state();
        seed_base_with_readlists(&state.db);
        let (status, _, body) = call(
            &state,
            router(),
            get("/api/v1/readlists/r1/books/b3/previous", ADMIN_KEY),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let book: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(book["id"], "b1");

        let (status, _, body) = call(
            &state,
            router(),
            get("/api/v1/readlists/r1/books/b3/next", ADMIN_KEY),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let book: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(book["id"], "b2");

        // boundaries: b1 has no previous, b2 has no next
        let (status, _, _) = call(
            &state,
            router(),
            get("/api/v1/readlists/r1/books/b1/previous", ADMIN_KEY),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _, _) = call(
            &state,
            router(),
            get("/api/v1/readlists/r1/books/b2/next", ADMIN_KEY),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn read_progress_tachiyomi_get_and_put() {
        let state = test_state();
        seed_base_with_readlists(&state.db);
        let uid = admin_id(&state.db);
        // b1 completed; b3 in progress
        seed_read_progress(&state.db, "b1", &uid, true);
        seed_read_progress(&state.db, "b3", &uid, false);

        let (status, _, body) = call(
            &state,
            router(),
            get("/api/v1/readlists/r1/read-progress/tachiyomi", ADMIN_KEY),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let dto: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(dto["booksCount"], 3);
        assert_eq!(dto["booksReadCount"], 1);
        assert_eq!(dto["booksInProgressCount"], 1);
        assert_eq!(dto["booksUnreadCount"], 1);
        // leading completed run stops at b3 (in progress): last read index 1
        assert_eq!(dto["lastReadContinuousIndex"], 1);

        // PUT lastBookRead=2: b1 (already read, untouched) and b3 (marked completed)
        let request = Request::builder()
            .method("PUT")
            .uri("/api/v1/readlists/r1/read-progress/tachiyomi")
            .header("X-API-Key", ADMIN_KEY)
            .header("Content-Type", "application/json")
            .body(Body::from(r#"{"lastBookRead":2}"#))
            .unwrap();
        let (status, _, _) = call(&state, router(), request).await;
        assert_eq!(status, StatusCode::NO_CONTENT);

        let progress = ReadProgressDao::new(state.db.clone())
            .find_by_book_and_user("b3", &uid)
            .unwrap()
            .expect("b3 progress");
        assert!(progress.completed);
        assert_eq!(progress.page, 10); // media page_count
                                       // b1's original row is still there, b2 untouched
        assert!(ReadProgressDao::new(state.db.clone())
            .find_by_book_and_user("b1", &uid)
            .unwrap()
            .is_some());
        assert!(ReadProgressDao::new(state.db.clone())
            .find_by_book_and_user("b2", &uid)
            .unwrap()
            .is_none());

        // the series aggregate now counts b1 and b3 read for s1/s2 respectively
        let s1 = ReadProgressDao::new(state.db.clone())
            .find_series("s1", &uid)
            .unwrap()
            .expect("s1 aggregate");
        assert_eq!(s1.read_count, 1);
        let s2 = ReadProgressDao::new(state.db.clone())
            .find_series("s2", &uid)
            .unwrap()
            .expect("s2 aggregate");
        assert_eq!(s2.read_count, 1);
    }

    #[tokio::test]
    async fn read_progress_tachiyomi_put_validation() {
        let state = test_state();
        seed_base_with_readlists(&state.db);
        let request = Request::builder()
            .method("PUT")
            .uri("/api/v1/readlists/r1/read-progress/tachiyomi")
            .header("X-API-Key", ADMIN_KEY)
            .header("Content-Type", "application/json")
            .body(Body::from(r#"{"lastBookRead":-1}"#))
            .unwrap();
        let (status, _, body) = call(&state, router(), request).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let violations: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(violations["violations"][0]["fieldName"], "lastBookRead");
        assert_eq!(
            violations["violations"][0]["message"],
            "must be greater than or equal to 0"
        );
    }

    #[tokio::test]
    async fn readlist_file_zip() {
        let state = test_state();
        seed_base_with_readlists(&state.db);

        // real book files on disk for the zip entries
        let dir = tempfile::tempdir().unwrap();
        let f1 = dir.path().join("Book One.cbz");
        let f2 = dir.path().join("Book Two.cbz");
        std::fs::write(&f1, b"content-one").unwrap();
        std::fs::write(&f2, b"content-two").unwrap();
        for (id, path) in [("b10", &f1), ("b11", &f2)] {
            exec(
                &state.db,
                "INSERT INTO BOOK (ID, NAME, URL, FILE_LAST_MODIFIED, SERIES_ID, LIBRARY_ID) \
                 VALUES (?, ?, ?, '2020-01-01 00:00:00.0', 's1', 'l1')",
                rusqlite::params![id, id, format!("file:{}", path.display())],
            );
            exec(
                &state.db,
                "INSERT INTO BOOK_METADATA (BOOK_ID, TITLE, NUMBER, NUMBER_SORT) VALUES (?, ?, '', 1)",
                rusqlite::params![id, id],
            );
        }
        seed_readlist(&state.db, "r10", "Zip Me", true, &["b10", "b11"]);

        // no FILE_DOWNLOAD role -> 403
        let (status, _, _) = call(
            &state,
            router(),
            get("/api/v1/readlists/r10/file", USER_KEY),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);

        let (status, headers, body) = call(
            &state,
            router(),
            get("/api/v1/readlists/r10/file", ADMIN_KEY),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers[header::CONTENT_TYPE], "application/zip");
        assert_eq!(
            headers[header::CONTENT_DISPOSITION],
            "attachment; filename*=UTF-8''Zip%20Me.zip"
        );

        let cursor = std::io::Cursor::new(&body);
        let mut archive = zip::ZipArchive::new(cursor).unwrap();
        assert_eq!(archive.len(), 2);
        let names: Vec<String> = (0..archive.len())
            .map(|i| archive.by_index(i).unwrap().name().to_string())
            .collect();
        assert_eq!(names, ["1 - Book One.cbz", "2 - Book Two.cbz"]);
        let mut entry = archive.by_name("1 - Book One.cbz").unwrap();
        let mut content = Vec::new();
        std::io::Read::read_to_end(&mut entry, &mut content).unwrap();
        assert_eq!(content, b"content-one");
    }
}
