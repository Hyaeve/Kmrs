//! Smart lists (kmrs-only enhancement): user-owned search filters persisted in
//! `kmrs.sqlite` and evaluated live through the same `/list` query path.
//! /api/v1/smart-lists/**.

use crate::api::collections::jpeg_response;
use crate::auth::RequireAuth;
use crate::dto::common::{Page, Pageable, SortOrder};
use crate::dto::smart_list::{SmartListCreationDto, SmartListDto, SmartListUpdateDto};
use crate::error::ApiError;
use crate::http::pagination::{QueryExt, QueryPageable};
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::{routing, Json, Router};
use komga_core::dto::book::BookDto;
use komga_core::dto::series::SeriesDto;
use komga_core::dto::thumbnail::ThumbnailSmartListDto;
use komga_core::model::smart_list::{SmartList, SmartListTarget, SmartListVisibility};
use komga_core::model::thumbnail::ThumbnailSmartList;
use komga_core::model::user::KomgaUser;
use komga_core::search::{BookSearch, SearchContext, SeriesSearch};
use komga_core::time_codec::now_utc;
use komga_db::dao::smart_list_thumbnail::SmartListThumbnailDao;
use komga_db::dao::user::UserDao;
use komga_db::dto_dao::book::BookDtoDao;
use komga_db::dto_dao::series::SeriesDtoDao;
use komga_db::dto_dao::{PageRequest, SortOrder as DbSortOrder};

use serde::Serialize;

/// minimal directory entry for choosing share targets
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ShareTargetDto {
    id: String,
    email: String,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/smart-lists",
            routing::get(get_smart_lists).post(create_smart_list),
        )
        .route(
            "/api/v1/smart-lists/share-targets",
            routing::get(get_share_targets),
        )
        .route(
            "/api/v1/smart-lists/{id}",
            routing::get(get_smart_list_by_id)
                .patch(update_smart_list_by_id)
                .delete(delete_smart_list_by_id),
        )
        .route(
            "/api/v1/smart-lists/{id}/thumbnail",
            routing::get(get_smart_list_thumbnail),
        )
        .route(
            "/api/v1/smart-lists/{id}/thumbnails",
            routing::get(get_smart_list_thumbnails).post(add_user_uploaded_smart_list_thumbnail),
        )
        .route(
            "/api/v1/smart-lists/{id}/thumbnails/{thumbnailId}",
            routing::get(get_smart_list_thumbnail_by_id)
                .delete(delete_user_uploaded_smart_list_thumbnail),
        )
        .route(
            "/api/v1/smart-lists/{id}/thumbnails/{thumbnailId}/selected",
            routing::put(mark_smart_list_thumbnail_selected),
        )
        .route(
            "/api/v1/smart-lists/{id}/books",
            routing::post(get_books_by_smart_list_id),
        )
        .route(
            "/api/v1/smart-lists/{id}/series",
            routing::post(get_series_by_smart_list_id),
        )
}

fn to_page_request(pageable: &Pageable) -> PageRequest {
    PageRequest {
        page: pageable.page,
        size: pageable.size,
        unpaged: pageable.unpaged,
        sort: pageable
            .sort
            .iter()
            .map(|s| DbSortOrder {
                property: s.property.clone(),
                descending: s.descending,
            })
            .collect(),
    }
}

fn to_dto(state: &AppState, smart_list: &SmartList) -> Result<SmartListDto, ApiError> {
    let shares = crate::service::smart_list::dao(state).find_share_targets(&smart_list.id)?;
    SmartListDto::of(smart_list, shares).map_err(|e| ApiError::Internal(e.to_string()))
}

/// The search document must deserialize against the list target; the parsed form is
/// stored so the JSON in `kmrs.sqlite` is always re-validatable.
fn canonical_search_json(
    target: SmartListTarget,
    search: &serde_json::Value,
) -> Result<String, ApiError> {
    let message = "search does not match the komga search format for target ";
    match target {
        SmartListTarget::Book => serde_json::from_value::<BookSearch>(search.clone())
            .map_err(|e| ApiError::bad_request(format!("{message}BOOK: {e}")))
            .and_then(|s| serde_json::to_string(&s).map_err(|e| ApiError::Internal(e.to_string()))),
        SmartListTarget::Series => serde_json::from_value::<SeriesSearch>(search.clone())
            .map_err(|e| ApiError::bad_request(format!("{message}SERIES: {e}")))
            .and_then(|s| serde_json::to_string(&s).map_err(|e| ApiError::Internal(e.to_string()))),
    }
}

/// Owner always; admin sees every list; PUBLIC lists every authenticated user;
/// SHARED lists only the users in the share scope. Anything else is a 404 so
/// list existence is not leaked.
fn find_visible_smart_list(
    state: &AppState,
    user: &KomgaUser,
    id: &str,
) -> Result<SmartList, ApiError> {
    crate::service::smart_list::find_visible(state, user, id)?
        .ok_or_else(|| ApiError::not_found(""))
}

/// minimal user directory for picking share targets. Only admins publish or share
/// lists, so only admins may list it — komga keeps /api/v2/users admin-only, and
/// exposing every user's email to all authenticated users is not acceptable
async fn get_share_targets(
    State(state): State<AppState>,
    auth: RequireAuth,
) -> Result<Json<Vec<ShareTargetDto>>, ApiError> {
    auth.0.require_admin()?;
    let users = UserDao::new(state.db.clone()).find_all()?;
    Ok(Json(
        users
            .into_iter()
            .map(|u| ShareTargetDto {
                id: u.id,
                email: u.email,
            })
            .collect(),
    ))
}

async fn get_smart_lists(
    State(state): State<AppState>,
    auth: RequireAuth,
    qp: QueryPageable,
) -> Result<Json<Page<SmartListDto>>, ApiError> {
    let user = &auth.0.user;
    let dao = crate::service::smart_list::dao(&state);
    // names are unique per owner, so no global ordering contract: sort by name and
    // page in memory — the count of lists a user can create stays small
    let all = if user.is_admin() {
        // admins may browse by owner dimension; the parameter is ignored for others
        dao.find_all_admin(qp.params.first("owner"))?
    } else {
        dao.find_visible_for_user(&user.id)?
    };
    let total = all.len() as u64;
    let mut pageable = qp.pageable.clone();
    pageable.sort = vec![SortOrder {
        property: "name".into(),
        descending: false,
    }];
    let content: Vec<SmartListDto> = if pageable.unpaged {
        all.iter()
            .map(|s| to_dto(&state, s))
            .collect::<Result<_, _>>()?
    } else {
        all.into_iter()
            .skip(pageable.offset() as usize)
            .take(pageable.size as usize)
            .map(|s| to_dto(&state, &s))
            .collect::<Result<_, _>>()?
    };
    Ok(Json(Page::of(content, total, &pageable)))
}

async fn create_smart_list(
    State(state): State<AppState>,
    auth: RequireAuth,
    Json(body): Json<SmartListCreationDto>,
) -> Result<Json<SmartListDto>, ApiError> {
    let user = &auth.0.user;
    let violations = body.violations();
    if !violations.is_empty() {
        return Err(ApiError::Violations(violations));
    }
    // visibility and sharing are admin capabilities: a regular user's lists stay private
    if !user.is_admin() && (body.visibility.is_some() || body.shared_with_user_ids.is_some()) {
        return Err(ApiError::forbidden(
            "only admins may set visibility or share smart lists",
        ));
    }
    let search_json = canonical_search_json(body.target, &body.search)?;
    let shared_with = body.shared_with_user_ids.unwrap_or_default();
    validate_share_targets(&state, &shared_with)?;
    let smart_list = crate::service::smart_list::add_smart_list(
        &state,
        SmartList {
            id: String::new(),
            name: body.name,
            summary: body.summary,
            owner_user_id: user.id.clone(),
            target: body.target,
            visibility: body.visibility.unwrap_or_default(),
            search_json,
            created_date: now_utc(),
            last_modified_date: now_utc(),
        },
        &shared_with,
    )
    .map_err(|e| match e {
        crate::service::smart_list::SmartListError::DuplicateName => {
            ApiError::bad_request(crate::service::smart_list::DUPLICATE_NAME_MESSAGE)
        }
        crate::service::smart_list::SmartListError::Db(e) => ApiError::from(e),
    })?;
    Ok(Json(to_dto(&state, &smart_list)?))
}

async fn get_smart_list_by_id(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path(id): Path<String>,
) -> Result<Json<SmartListDto>, ApiError> {
    let smart_list = find_visible_smart_list(&state, &auth.0.user, &id)?;
    Ok(Json(to_dto(&state, &smart_list)?))
}

/// share targets must be existing users, otherwise a typo silently shares with nobody
fn validate_share_targets(state: &AppState, user_ids: &[String]) -> Result<(), ApiError> {
    let users = UserDao::new(state.db.clone());
    for user_id in user_ids {
        if users.find_by_id(user_id)?.is_none() {
            return Err(ApiError::bad_request(format!(
                "share target user does not exist: {user_id}"
            )));
        }
    }
    Ok(())
}

async fn update_smart_list_by_id(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path(id): Path<String>,
    Json(body): Json<SmartListUpdateDto>,
) -> Result<StatusCode, ApiError> {
    let violations = body.violations();
    if !violations.is_empty() {
        return Err(ApiError::Violations(violations));
    }
    let existing = find_visible_smart_list(&state, &auth.0.user, &id)?;
    require_owner_or_admin(&auth.0.user, &existing)?;
    // visibility and sharing are admin capabilities: non-admin lists stay private
    if !auth.0.user.is_admin() && (body.visibility.is_some() || body.shared_with_user_ids.is_some())
    {
        return Err(ApiError::forbidden(
            "only admins may set visibility or share smart lists",
        ));
    }
    // target and search change together or not at all: switching only the target
    // re-validates the stored document against the new target, which fails across
    // BOOK/SERIES (condition shapes differ), so a target switch must send search too
    let target = body.target.unwrap_or(existing.target);
    let search_json = match &body.search {
        Some(search) => canonical_search_json(target, search)?,
        None => {
            // the stored document was validated when written; re-validate after a target switch
            let stored: serde_json::Value = serde_json::from_str(&existing.search_json)
                .map_err(|e| ApiError::Internal(e.to_string()))?;
            canonical_search_json(target, &stored)?
        }
    };
    let updated = SmartList {
        name: body.name.unwrap_or(existing.name.clone()),
        summary: body.summary.unwrap_or(existing.summary.clone()),
        visibility: body.visibility.unwrap_or(existing.visibility),
        target,
        search_json,
        ..existing
    };
    let shares = match body.shared_with_user_ids {
        Some(ids) => {
            validate_share_targets(&state, &ids)?;
            Some(ids)
        }
        // absent means keep the current scope
        None => None,
    };
    // a SHARED list with an empty scope is invisible to everyone but owner and admin;
    // create rejects it, so PATCH must not be able to produce it either
    if updated.visibility == SmartListVisibility::Shared {
        let scope_empty = match &shares {
            Some(ids) => ids.is_empty(),
            None => crate::service::smart_list::dao(&state)
                .find_share_targets(&id)?
                .is_empty(),
        };
        if scope_empty {
            return Err(ApiError::bad_request(
                "sharedWithUserIds must not be empty when visibility is SHARED",
            ));
        }
    }
    crate::service::smart_list::update_smart_list(&state, &updated, shares.as_deref()).map_err(
        |e| match e {
            crate::service::smart_list::SmartListError::DuplicateName => {
                ApiError::bad_request(crate::service::smart_list::DUPLICATE_NAME_MESSAGE)
            }
            crate::service::smart_list::SmartListError::Db(e) => ApiError::from(e),
        },
    )?;
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_smart_list_by_id(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let smart_list = find_visible_smart_list(&state, &auth.0.user, &id)?;
    require_owner_or_admin(&auth.0.user, &smart_list)?;
    crate::service::smart_list::delete_smart_list(&state, &smart_list)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn get_smart_list_thumbnail(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let smart_list = find_visible_smart_list(&state, &auth.0.user, &id)?;
    let bytes = crate::service::smart_list::get_thumbnail_bytes(&state, &smart_list)?;
    // matched content is dynamic, so cache far shorter than the static read-list covers
    Ok(jpeg_response(bytes, Some("max-age=60, private")))
}

/// uploaded covers are owner-managed, unlike the admin-managed read lists
fn require_owner(user: &KomgaUser, smart_list: &SmartList) -> Result<(), ApiError> {
    if user.id != smart_list.owner_user_id {
        return Err(ApiError::forbidden(""));
    }
    Ok(())
}

/// visibility grants read access only; modifying or deleting a list is
/// owner-or-admin, like komga's readlist writes
fn require_owner_or_admin(user: &KomgaUser, smart_list: &SmartList) -> Result<(), ApiError> {
    if user.id != smart_list.owner_user_id && !user.is_admin() {
        return Err(ApiError::forbidden(""));
    }
    Ok(())
}

async fn get_smart_list_thumbnails(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path(id): Path<String>,
) -> Result<Json<Vec<ThumbnailSmartListDto>>, ApiError> {
    find_visible_smart_list(&state, &auth.0.user, &id)?;
    let thumbnails =
        SmartListThumbnailDao::new(state.kmrs_db.clone()).find_all_by_smart_list_id(&id)?;
    Ok(Json(
        thumbnails.iter().map(ThumbnailSmartListDto::from).collect(),
    ))
}

async fn get_smart_list_thumbnail_by_id(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path((id, thumbnail_id)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    let smart_list = find_visible_smart_list(&state, &auth.0.user, &id)?;
    let thumbnail = SmartListThumbnailDao::new(state.kmrs_db.clone())
        .find_by_id(&thumbnail_id)?
        .ok_or_else(|| ApiError::not_found(""))?;
    if thumbnail.smart_list_id != smart_list.id {
        return Err(ApiError::bad_request(""));
    }
    Ok(jpeg_response(thumbnail.thumbnail, None))
}

async fn add_user_uploaded_smart_list_thumbnail(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path(id): Path<String>,
    mut multipart: axum::extract::Multipart,
) -> Result<Json<ThumbnailSmartListDto>, ApiError> {
    let smart_list = find_visible_smart_list(&state, &auth.0.user, &id)?;
    require_owner(&auth.0.user, &smart_list)?;
    let (bytes, selected) = crate::api::readlists::parse_thumbnail_upload(&mut multipart).await?;
    let media_type = komga_media::detect::detect_media_type(&bytes);
    if !komga_media::detect::is_image(&media_type) {
        return Err(ApiError::unsupported_media_type(""));
    }
    let dimension = komga_media::image::get_dimension(&bytes)
        .map(|(w, h)| komga_core::model::thumbnail::Dimension {
            width: w as i32,
            height: h as i32,
        })
        .unwrap_or_else(crate::service::collection::zero_dimension);
    let thumbnail = crate::service::smart_list::add_thumbnail(
        &state,
        &smart_list,
        ThumbnailSmartList {
            id: String::new(),
            smart_list_id: smart_list.id.clone(),
            thumbnail: bytes.clone(),
            fingerprint: String::new(),
            selected,
            type_: komga_core::model::thumbnail::ThumbnailType::UserUploaded,
            media_type,
            file_size: bytes.len() as i64,
            dimension,
            created_date: now_utc(),
            last_modified_date: now_utc(),
        },
    )?;
    Ok(Json(ThumbnailSmartListDto::from(&thumbnail)))
}

async fn mark_smart_list_thumbnail_selected(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path((id, thumbnail_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let smart_list = find_visible_smart_list(&state, &auth.0.user, &id)?;
    require_owner(&auth.0.user, &smart_list)?;
    if let Some(poster) =
        SmartListThumbnailDao::new(state.kmrs_db.clone()).find_by_id(&thumbnail_id)?
    {
        if poster.smart_list_id != smart_list.id {
            return Err(ApiError::bad_request(""));
        }
        crate::service::smart_list::mark_selected_thumbnail(&state, &smart_list, &poster)?;
    }
    // a missing thumbnail is silently accepted, as in komga
    Ok(StatusCode::ACCEPTED)
}

async fn delete_user_uploaded_smart_list_thumbnail(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path((id, thumbnail_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let smart_list = find_visible_smart_list(&state, &auth.0.user, &id)?;
    require_owner(&auth.0.user, &smart_list)?;
    if let Some(poster) =
        SmartListThumbnailDao::new(state.kmrs_db.clone()).find_by_id(&thumbnail_id)?
    {
        if poster.smart_list_id != smart_list.id {
            return Err(ApiError::bad_request(""));
        }
        crate::service::smart_list::delete_thumbnail(&state, &smart_list, &poster)?;
    }
    Ok(StatusCode::ACCEPTED)
}

async fn get_books_by_smart_list_id(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path(id): Path<String>,
    qp: QueryPageable,
    overlay: Option<Json<BookSearch>>,
) -> Result<Json<Page<BookDto>>, ApiError> {
    let user = &auth.0.user;
    let smart_list = find_visible_smart_list(&state, user, &id)?;
    if smart_list.target != SmartListTarget::Book {
        return Err(ApiError::bad_request("smart list target is not BOOK"));
    }
    let stored: BookSearch = serde_json::from_str(&smart_list.search_json)
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    // page-side filters narrow the stored filter; an overlay search term replaces it
    let search = crate::service::smart_list::merge_book_search(&stored, overlay.as_deref());
    let mut pageable = qp.pageable.clone();
    if pageable.sort.is_empty()
        && search
            .full_text_search
            .as_deref()
            .is_some_and(|q| !q.trim().is_empty())
    {
        pageable.sort = vec![SortOrder {
            property: "relevance".into(),
            descending: false,
        }];
    }
    let result = BookDtoDao::new(state.db.clone())
        .with_searcher(Some(crate::search_index::searcher(&state)))
        .find_all(
            &search,
            &SearchContext::of_user(user),
            &to_page_request(&pageable),
        )?;
    let restricted = !user.is_admin();
    let items = result
        .items
        .into_iter()
        .map(|b| b.restrict_url(restricted))
        .collect();
    Ok(Json(Page::of_dto(
        komga_db::dto_dao::DtoPage {
            items,
            total: result.total,
            sorted: result.sorted,
        },
        &pageable,
    )))
}

async fn get_series_by_smart_list_id(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path(id): Path<String>,
    qp: QueryPageable,
    overlay: Option<Json<SeriesSearch>>,
) -> Result<Json<Page<SeriesDto>>, ApiError> {
    let user = &auth.0.user;
    let smart_list = find_visible_smart_list(&state, user, &id)?;
    if smart_list.target != SmartListTarget::Series {
        return Err(ApiError::bad_request("smart list target is not SERIES"));
    }
    let stored: SeriesSearch = serde_json::from_str(&smart_list.search_json)
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    let search = crate::service::smart_list::merge_series_search(&stored, overlay.as_deref());
    let mut pageable = qp.pageable.clone();
    if pageable.sort.is_empty()
        && search
            .full_text_search
            .as_deref()
            .is_some_and(|q| !q.trim().is_empty())
    {
        pageable.sort = vec![SortOrder {
            property: "relevance".into(),
            descending: false,
        }];
    }
    let result = SeriesDtoDao::new(state.db.clone())
        .with_searcher(Some(crate::search_index::searcher(&state)))
        .find_all(
            &search,
            None,
            &SearchContext::of_user(user),
            &to_page_request(&pageable),
        )?;
    let restricted = !user.is_admin();
    let items = result
        .items
        .into_iter()
        .map(|s| s.restrict_url(restricted))
        .collect();
    Ok(Json(Page::of_dto(
        komga_db::dto_dao::DtoPage {
            items,
            total: result.total,
            sorted: result.sorted,
        },
        &pageable,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::collections::tests as shared;
    use axum::body::Body;
    use axum::http::Request;
    use komga_core::model::user::UserRole;

    async fn call(state: &AppState, request: Request<Body>) -> (StatusCode, Vec<u8>) {
        let (status, _, body) = shared::call(state, router(), request).await;
        (status, body)
    }

    fn post_json(path: &str, api_key: &str, body: serde_json::Value) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri(path)
            .header("X-API-Key", api_key)
            .header("Content-Type", "application/json")
            .body(Body::from(serde_json::to_string(&body).unwrap()))
            .unwrap()
    }

    async fn create_list(
        state: &AppState,
        api_key: &str,
        name: &str,
        target: &str,
        search: serde_json::Value,
    ) -> String {
        let (status, body) = call(
            state,
            post_json(
                "/api/v1/smart-lists",
                api_key,
                serde_json::json!({
                    "name": name,
                    "target": target,
                    "search": search,
                }),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
        serde_json::from_slice::<serde_json::Value>(&body).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string()
    }

    fn seed_scenario(state: &AppState) {
        let db = &state.db;
        for lib in ["lib1", "lib2"] {
            shared::exec(
                db,
                "INSERT INTO LIBRARY (ID, NAME, ROOT) VALUES (?, ?, 'file:/l/')",
                rusqlite::params![lib, lib],
            );
        }
        // s1 in lib1: manga tag; s2 in lib1: no tag; s3 in lib2: manga tag
        for (id, lib, title) in [
            ("s1", "lib1", "Alpha"),
            ("s2", "lib1", "Beta"),
            ("s3", "lib2", "Gamma"),
        ] {
            shared::exec(
                db,
                "INSERT INTO SERIES (ID, NAME, URL, FILE_LAST_MODIFIED, LIBRARY_ID) \
                 VALUES (?, ?, 'file:/l/s/', '2020-01-01 00:00:00.0', ?)",
                rusqlite::params![id, title, lib],
            );
            shared::exec(
                db,
                "INSERT INTO SERIES_METADATA (SERIES_ID, STATUS, TITLE, TITLE_SORT, PUBLISHER) \
                 VALUES (?, 'ONGOING', ?, ?, 'pub')",
                rusqlite::params![id, title, title],
            );
            shared::exec(
                db,
                "INSERT INTO BOOK_METADATA_AGGREGATION (SERIES_ID, SUMMARY, SUMMARY_NUMBER) \
                 VALUES (?, '', '')",
                [id],
            );
        }
        for (id, series, number_sort) in [
            ("b1", "s1", 1.0),
            ("b2", "s1", 2.0),
            ("b3", "s2", 1.0),
            ("b4", "s3", 1.0),
        ] {
            shared::exec(
                db,
                "INSERT INTO BOOK (ID, NAME, URL, FILE_LAST_MODIFIED, SERIES_ID, LIBRARY_ID) \
                 VALUES (?, ?, 'file:/l/s/b.cbz', '2020-01-01 00:00:00.0', ?, \
                 (SELECT LIBRARY_ID FROM SERIES WHERE ID = ?))",
                rusqlite::params![id, id, series, series],
            );
            shared::exec(
                db,
                "INSERT INTO BOOK_METADATA (BOOK_ID, TITLE, NUMBER, NUMBER_SORT, RELEASE_DATE) \
                 VALUES (?, ?, '', ?, '2020-01-01')",
                rusqlite::params![id, id, number_sort],
            );
            shared::exec(
                db,
                "INSERT INTO MEDIA (BOOK_ID, STATUS, MEDIA_TYPE, PAGE_COUNT) \
                 VALUES (?, 'READY', 'application/zip', 10)",
                [id],
            );
        }
        for book in ["b1", "b4"] {
            shared::exec(
                db,
                "INSERT INTO BOOK_METADATA_TAG (TAG, BOOK_ID) VALUES ('manga', ?)",
                [book],
            );
        }
        // b1 fully read by u1: read-status conditions must reflect the requesting user
        let u1 = user_id(state, "u1@example.org");
        shared::exec(
            db,
            "INSERT INTO READ_PROGRESS (BOOK_ID, USER_ID, PAGE, COMPLETED, READ_DATE) \
             VALUES ('b1', ?, 10, 1, '2026-10-01 00:00:00.0')",
            rusqlite::params![u1],
        );
    }

    fn user_id(state: &AppState, email: &str) -> String {
        komga_db::dao::user::UserDao::new(state.db.clone())
            .find_by_email_ignore_case(email)
            .unwrap()
            .unwrap()
            .id
    }

    fn setup() -> (AppState, String) {
        let state = shared::test_state();
        shared::insert_user(
            &state.db,
            "admin@example.org",
            &[UserRole::Admin],
            &[],
            Default::default(),
            "adminkey",
        );
        shared::insert_user(
            &state.db,
            "u1@example.org",
            &[],
            &[],
            Default::default(),
            "u1key",
        );
        shared::insert_user(
            &state.db,
            "u2@example.org",
            &[],
            &[],
            Default::default(),
            "u2key",
        );
        seed_scenario(&state);
        let u1 = user_id(&state, "u1@example.org");
        (state, u1)
    }

    #[tokio::test]
    async fn create_get_update_delete_roundtrip() {
        let (state, u1) = setup();
        let list_id = create_list(
            &state,
            "u1key",
            "Unread manga",
            "BOOK",
            serde_json::json!({"condition": {"tag": {"operator": "is", "value": "manga"}}}),
        )
        .await;

        // owner reads it back; the stored search comes back parsed
        let (status, body) = call(
            &state,
            shared::get(&format!("/api/v1/smart-lists/{list_id}"), "u1key"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let json = serde_json::from_slice::<serde_json::Value>(&body).unwrap();
        assert_eq!(json["name"], "Unread manga");
        assert_eq!(json["target"], "BOOK");
        assert_eq!(json["ownerId"], u1);
        assert_eq!(
            json["search"]["condition"]["tag"],
            serde_json::json!({"operator": "is", "value": "manga"})
        );

        // another user cannot see it (404, not 403: no existence leak); admin can
        let (status, _) = call(
            &state,
            shared::get(&format!("/api/v1/smart-lists/{list_id}"), "u2key"),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _) = call(
            &state,
            shared::get(&format!("/api/v1/smart-lists/{list_id}"), "adminkey"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        // a user sees only their own lists, admin sees every list
        let (status, body) = call(&state, shared::get("/api/v1/smart-lists", "u1key")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&body).unwrap()["totalElements"],
            1
        );
        let (status, body) = call(&state, shared::get("/api/v1/smart-lists", "adminkey")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&body).unwrap()["totalElements"],
            1
        );

        // duplicate name for the same owner is rejected; another owner may reuse it
        let (status, body) = call(
            &state,
            post_json(
                "/api/v1/smart-lists",
                "u1key",
                serde_json::json!({
                    "name": "UNREAD MANGA",
                    "target": "BOOK",
                    "search": {},
                }),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(String::from_utf8_lossy(&body).contains("already exists"));
        create_list(
            &state,
            "u2key",
            "Unread manga",
            "BOOK",
            serde_json::json!({}),
        )
        .await;

        // PATCH summary
        let (status, _) = call(
            &state,
            Request::builder()
                .method("PATCH")
                .uri(format!("/api/v1/smart-lists/{list_id}"))
                .header("X-API-Key", "u1key")
                .header("Content-Type", "application/json")
                .body(Body::from(
                    serde_json::to_string(&serde_json::json!({"summary": "tag filter"})).unwrap(),
                ))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);

        // DELETE, then gone for owner and admin
        let (status, _) = call(
            &state,
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/v1/smart-lists/{list_id}"))
                .header("X-API-Key", "u1key")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (status, _) = call(
            &state,
            shared::get(&format!("/api/v1/smart-lists/{list_id}"), "adminkey"),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn invalid_search_document_is_rejected() {
        let (state, _) = setup();
        let (status, body) = call(
            &state,
            post_json(
                "/api/v1/smart-lists",
                "u1key",
                serde_json::json!({
                    "name": "broken",
                    "target": "BOOK",
                    "search": {"condition": {"noSuchCondition": {}}},
                }),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(String::from_utf8_lossy(&body).contains("search does not match"));
    }

    #[tokio::test]
    async fn books_endpoint_evaluates_the_stored_filter() {
        let (state, _) = setup();
        // tag Is manga →b1 (s1) and b4 (s3), across both libraries
        let by_tag = create_list(
            &state,
            "u1key",
            "manga",
            "BOOK",
            serde_json::json!({"condition": {"tag": {"operator": "is", "value": "manga"}}}),
        )
        .await;
        let (status, body) = call(
            &state,
            post_json(
                &format!("/api/v1/smart-lists/{by_tag}/books"),
                "u1key",
                serde_json::json!({}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
        let json = serde_json::from_slice::<serde_json::Value>(&body).unwrap();
        let mut ids: Vec<String> = json["content"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| b["id"].as_str().unwrap().to_string())
            .collect();
        ids.sort();
        assert_eq!(ids, ["b1", "b4"]);
        assert_eq!(json["totalElements"], 2);

        // same result as posting the condition to /api/v1/books/list directly
        let (status, _, direct_body) = shared::call(
            &state,
            crate::api::books::router(),
            post_json(
                "/api/v1/books/list",
                "u1key",
                serde_json::json!({"condition": {"tag": {"operator": "is", "value": "manga"}}}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let direct: Vec<String> = serde_json::from_slice::<serde_json::Value>(&direct_body)
            .unwrap()["content"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| b["id"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(ids, direct);
    }

    #[tokio::test]
    async fn read_status_conditions_are_evaluated_per_requesting_user() {
        let (state, _) = setup();
        // u1 has completed b1; u2 has read nothing
        let list_id = create_list(
            &state,
            "u1key",
            "read",
            "BOOK",
            serde_json::json!({"condition": {"readStatus": {"operator": "is", "value": "READ"}}}),
        )
        .await;

        let (status, body) = call(
            &state,
            post_json(
                &format!("/api/v1/smart-lists/{list_id}/books"),
                "u1key",
                serde_json::json!({}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
        let json = serde_json::from_slice::<serde_json::Value>(&body).unwrap();
        assert_eq!(json["totalElements"], 1);
        assert_eq!(json["content"][0]["id"], "b1");

        // evaluation uses the requesting user's read state, not the owner's:
        // admin can read the list but has completed nothing
        let (status, body) = call(
            &state,
            post_json(
                &format!("/api/v1/smart-lists/{list_id}/books"),
                "adminkey",
                serde_json::json!({}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
        let json = serde_json::from_slice::<serde_json::Value>(&body).unwrap();
        assert_eq!(json["totalElements"], 0);

        // a non-owner non-admin cannot even evaluate it
        let (status, _) = call(
            &state,
            post_json(
                &format!("/api/v1/smart-lists/{list_id}/books"),
                "u2key",
                serde_json::json!({}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn multi_library_anyof_and_wrong_target() {
        let (state, _) = setup();
        let list_id = create_list(
            &state,
            "u1key",
            "cross-lib",
            "SERIES",
            serde_json::json!({"condition": {"anyOf": [
                {"libraryId": {"operator": "is", "value": "lib1"}},
                {"libraryId": {"operator": "is", "value": "lib2"}},
            ]}}),
        )
        .await;
        let (status, body) = call(
            &state,
            post_json(
                &format!("/api/v1/smart-lists/{list_id}/series"),
                "u1key",
                serde_json::json!({}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&body).unwrap()["totalElements"],
            3
        );

        let lib2_only = create_list(
            &state,
            "u1key",
            "lib2",
            "SERIES",
            serde_json::json!({"condition": {"libraryId": {"operator": "is", "value": "lib2"}}}),
        )
        .await;
        let (_, body) = call(
            &state,
            post_json(
                &format!("/api/v1/smart-lists/{lib2_only}/series"),
                "u1key",
                serde_json::json!({}),
            ),
        )
        .await;
        let json = serde_json::from_slice::<serde_json::Value>(&body).unwrap();
        assert_eq!(json["totalElements"], 1);
        assert_eq!(json["content"][0]["id"], "s3");

        // a SERIES list has no books endpoint
        let (status, _) = call(
            &state,
            post_json(
                &format!("/api/v1/smart-lists/{list_id}/books"),
                "u1key",
                serde_json::json!({}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn overlay_conditions_narrow_the_stored_filter() {
        let (state, _) = setup();
        // stored: manga tag → b1, b4; overlay: library lib2 → only b4 remains
        let list_id = create_list(
            &state,
            "u1key",
            "manga",
            "BOOK",
            serde_json::json!({"condition": {"tag": {"operator": "is", "value": "manga"}}}),
        )
        .await;
        let (status, body) = call(
            &state,
            post_json(
                &format!("/api/v1/smart-lists/{list_id}/books"),
                "u1key",
                serde_json::json!({"condition": {"libraryId": {"operator": "is", "value": "lib2"}}}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
        let json = serde_json::from_slice::<serde_json::Value>(&body).unwrap();
        assert_eq!(json["totalElements"], 1);
        assert_eq!(json["content"][0]["id"], "b4");

        // an empty overlay body behaves like no overlay at all
        let (status, body) = call(
            &state,
            post_json(
                &format!("/api/v1/smart-lists/{list_id}/books"),
                "u1key",
                serde_json::json!({}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&body).unwrap()["totalElements"],
            2
        );
    }

    #[tokio::test]
    async fn thumbnail_is_a_generated_mosaic_of_matched_books() {
        let (state, _) = setup();
        // covers for the manga-tagged books (b1, b4)
        for (id, book) in [("t1", "b1"), ("t4", "b4")] {
            shared::exec(
                &state.db,
                "INSERT INTO THUMBNAIL_BOOK \
                 (ID, BOOK_ID, THUMBNAIL, SELECTED, TYPE, MEDIA_TYPE, FILE_SIZE, WIDTH, HEIGHT) \
                 VALUES (?, ?, ?, 1, 'GENERATED', 'image/jpeg', 3, 1, 1)",
                rusqlite::params![id, book, shared::tiny_jpeg()],
            );
        }
        let list_id = create_list(
            &state,
            "u1key",
            "manga",
            "BOOK",
            serde_json::json!({"condition": {"tag": {"operator": "is", "value": "manga"}}}),
        )
        .await;

        let (status, headers, body) = shared::call(
            &state,
            router(),
            shared::get(&format!("/api/v1/smart-lists/{list_id}/thumbnail"), "u1key"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers.get("content-type").unwrap(), "image/jpeg");
        assert!(!body.is_empty());

        // regenerates when the matched set changes: drop the tag from b1 → only b4 remains
        shared::exec(
            &state.db,
            "DELETE FROM BOOK_METADATA_TAG WHERE BOOK_ID = 'b1'",
            [],
        );
        let (_, _, body2) = shared::call(
            &state,
            router(),
            shared::get(&format!("/api/v1/smart-lists/{list_id}/thumbnail"), "u1key"),
        )
        .await;
        assert!(!body2.is_empty());

        // SERIES lists get a mosaic of the matched series covers too
        for (id, series) in [("ts1", "s1"), ("ts3", "s3")] {
            shared::exec(
                &state.db,
                "INSERT INTO THUMBNAIL_SERIES \
                 (ID, SERIES_ID, THUMBNAIL, SELECTED, TYPE, MEDIA_TYPE, FILE_SIZE, WIDTH, HEIGHT) \
                 VALUES (?, ?, ?, 1, 'GENERATED', 'image/jpeg', 3, 1, 1)",
                rusqlite::params![id, series, shared::tiny_jpeg()],
            );
        }
        let series_list = create_list(
            &state,
            "u1key",
            "all-series",
            "SERIES",
            serde_json::json!({"condition": {"anyOf": [
                {"libraryId": {"operator": "is", "value": "lib1"}},
                {"libraryId": {"operator": "is", "value": "lib2"}},
            ]}}),
        )
        .await;
        let (status, headers, body3) = shared::call(
            &state,
            router(),
            shared::get(
                &format!("/api/v1/smart-lists/{series_list}/thumbnail"),
                "u1key",
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers.get("content-type").unwrap(), "image/jpeg");
        assert!(!body3.is_empty());
    }

    #[tokio::test]
    async fn uploaded_covers_win_over_the_generated_mosaic() {
        let (state, _) = setup();
        shared::exec(
            &state.db,
            "INSERT INTO THUMBNAIL_BOOK \
             (ID, BOOK_ID, THUMBNAIL, SELECTED, TYPE, MEDIA_TYPE, FILE_SIZE, WIDTH, HEIGHT) \
             VALUES ('t1', 'b1', ?, 1, 'GENERATED', 'image/jpeg', 3, 1, 1)",
            rusqlite::params![shared::tiny_jpeg()],
        );
        let list_id = create_list(
            &state,
            "u1key",
            "covers",
            "BOOK",
            serde_json::json!({"condition": {"tag": {"operator": "is", "value": "manga"}}}),
        )
        .await;

        // generated mosaic first
        let (_, _, mosaic) = shared::call(
            &state,
            router(),
            shared::get(&format!("/api/v1/smart-lists/{list_id}/thumbnail"), "u1key"),
        )
        .await;
        assert!(!mosaic.is_empty());

        let upload = |key: &str, bytes: Vec<u8>| {
            let boundary = "test-boundary";
            let mut body = Vec::new();
            body.extend_from_slice(
                format!(
                    "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"p.jpg\"\r\nContent-Type: image/jpeg\r\n\r\n"
                )
                .as_bytes(),
            );
            body.extend_from_slice(&bytes);
            body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/smart-lists/{list_id}/thumbnails"))
                .header("X-API-Key", key)
                .header(
                    "Content-Type",
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(body))
                .unwrap()
        };

        // a non-owner cannot even see the list (404, no existence leak)
        let (status, _, _) =
            shared::call(&state, router(), upload("u2key", shared::tiny_jpeg())).await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        // owner uploads two covers; the second upload takes selection
        let jpeg1 = shared::tiny_jpeg();
        let (status, _, body) =
            shared::call(&state, router(), upload("u1key", jpeg1.clone())).await;
        assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
        let dto = serde_json::from_slice::<serde_json::Value>(&body).unwrap();
        let first_id = dto["id"].as_str().unwrap().to_string();
        assert_eq!(dto["type"], "USER_UPLOADED");
        assert!(dto["selected"].as_bool().unwrap());

        let (status, _, body) =
            shared::call(&state, router(), upload("u1key", shared::tiny_jpeg())).await;
        assert_eq!(status, StatusCode::OK);
        let second_id = serde_json::from_slice::<serde_json::Value>(&body).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();

        // listing shows both with exactly one selected
        let (status, _, body) = shared::call(
            &state,
            router(),
            shared::get(
                &format!("/api/v1/smart-lists/{list_id}/thumbnails"),
                "u1key",
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let listed = serde_json::from_slice::<serde_json::Value>(&body).unwrap();
        // the generated mosaic row is listed too, like an upload: one generated + two uploads
        assert_eq!(listed.as_array().unwrap().len(), 3);
        assert_eq!(
            listed
                .as_array()
                .unwrap()
                .iter()
                .filter(|t| t["type"] == "USER_UPLOADED")
                .count(),
            2
        );
        assert_eq!(
            listed
                .as_array()
                .unwrap()
                .iter()
                .filter(|t| t["selected"].as_bool().unwrap())
                .count(),
            1
        );

        // the main thumbnail endpoint serves the selected upload, not the mosaic
        let (status, _, body) = shared::call(
            &state,
            router(),
            shared::get(&format!("/api/v1/smart-lists/{list_id}/thumbnail"), "u1key"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, shared::tiny_jpeg());
        assert_ne!(body, mosaic);

        // selecting the first cover flips selection; per-thumbnail bytes endpoint serves it
        let (status, _, _) = shared::call(
            &state,
            router(),
            Request::builder()
                .method("PUT")
                .uri(format!(
                    "/api/v1/smart-lists/{list_id}/thumbnails/{first_id}/selected"
                ))
                .header("X-API-Key", "u1key")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED);
        let (status, _, body) = shared::call(
            &state,
            router(),
            shared::get(
                &format!("/api/v1/smart-lists/{list_id}/thumbnails/{first_id}"),
                "u1key",
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, jpeg1);

        // deleting both uploads falls back to the generated mosaic
        for id in [first_id, second_id] {
            let (status, _, _) = shared::call(
                &state,
                router(),
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/smart-lists/{list_id}/thumbnails/{id}"))
                    .header("X-API-Key", "u1key")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await;
            assert_eq!(status, StatusCode::ACCEPTED);
        }
        let (status, _, body) = shared::call(
            &state,
            router(),
            shared::get(&format!("/api/v1/smart-lists/{list_id}/thumbnail"), "u1key"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, mosaic);
    }

    #[tokio::test]
    async fn visibility_scopes_who_sees_a_list() {
        let (state, u1) = setup();
        let u2_id = user_id(&state, "u2@example.org");

        // a regular user's lists are always private
        let (status, body) = call(
            &state,
            post_json(
                "/api/v1/smart-lists",
                "u1key",
                serde_json::json!({
                    "name": "priv",
                    "target": "BOOK",
                    "search": {},
                }),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
        let private_id = serde_json::from_slice::<serde_json::Value>(&body).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();

        // publishing and sharing are admin capabilities: u1 cannot set visibility
        // or share targets, on create or later — even an explicit PRIVATE, so the
        // rule is uniform with PATCH (absent fields only)
        for (name, extra) in [
            ("pub", serde_json::json!({"visibility": "PUBLIC"})),
            (
                "shrd",
                serde_json::json!({"visibility": "SHARED", "sharedWithUserIds": [u2_id.clone()]}),
            ),
            (
                "sneaky",
                serde_json::json!({"sharedWithUserIds": [u2_id.clone()]}),
            ),
            ("explicit", serde_json::json!({"visibility": "PRIVATE"})),
        ] {
            let mut body = serde_json::json!({"name": name, "target": "BOOK", "search": {}});
            body.as_object_mut().unwrap().extend(
                extra
                    .as_object()
                    .unwrap()
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone())),
            );
            let (status, body) =
                call(&state, post_json("/api/v1/smart-lists", "u1key", body)).await;
            assert_eq!(status, StatusCode::FORBIDDEN);
            assert!(
                String::from_utf8_lossy(&body).contains("only admins"),
                "{}",
                String::from_utf8_lossy(&body)
            );
        }

        // the admin publishes one list and shares another with u2
        let (status, body) = call(
            &state,
            post_json(
                "/api/v1/smart-lists",
                "adminkey",
                serde_json::json!({
                    "name": "pub",
                    "target": "BOOK",
                    "visibility": "PUBLIC",
                    "search": {},
                }),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
        let public_id = serde_json::from_slice::<serde_json::Value>(&body).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();
        let (status, body) = call(
            &state,
            post_json(
                "/api/v1/smart-lists",
                "adminkey",
                serde_json::json!({
                    "name": "shrd",
                    "target": "BOOK",
                    "visibility": "SHARED",
                    "sharedWithUserIds": [u2_id],
                    "search": {},
                }),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
        let shared_id = serde_json::from_slice::<serde_json::Value>(&body).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();

        // SHARED without targets is rejected
        let (status, body) = call(
            &state,
            post_json(
                "/api/v1/smart-lists",
                "adminkey",
                serde_json::json!({
                    "name": "broken-share",
                    "target": "BOOK",
                    "visibility": "SHARED",
                    "search": {},
                }),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(String::from_utf8_lossy(&body).contains("sharedWithUserIds"));

        // an unknown share target is rejected
        let (status, body) = call(
            &state,
            post_json(
                "/api/v1/smart-lists",
                "adminkey",
                serde_json::json!({
                    "name": "ghost-share",
                    "target": "BOOK",
                    "visibility": "SHARED",
                    "sharedWithUserIds": ["no-such-user"],
                    "search": {},
                }),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(String::from_utf8_lossy(&body).contains("no-such-user"));

        // u1 cannot widen the scope of their own list either
        let (status, body) = call(
            &state,
            Request::builder()
                .method("PATCH")
                .uri(format!("/api/v1/smart-lists/{private_id}"))
                .header("X-API-Key", "u1key")
                .header("Content-Type", "application/json")
                .body(Body::from(
                    serde_json::to_string(&serde_json::json!({
                        "visibility": "SHARED",
                        "sharedWithUserIds": [u2_id],
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert!(String::from_utf8_lossy(&body).contains("only admins"));

        // u2 sees the public and the shared-with-them lists only
        let (status, body) = call(&state, shared::get("/api/v1/smart-lists", "u2key")).await;
        assert_eq!(status, StatusCode::OK);
        let json = serde_json::from_slice::<serde_json::Value>(&body).unwrap();
        let names: Vec<&str> = json["content"]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| l["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["pub", "shrd"]);

        // u2 can read and evaluate the shared list, but not the private one
        let (status, _) = call(
            &state,
            shared::get(&format!("/api/v1/smart-lists/{shared_id}"), "u2key"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (status, _) = call(
            &state,
            post_json(
                &format!("/api/v1/smart-lists/{shared_id}/books"),
                "u2key",
                serde_json::json!({}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (status, _) = call(
            &state,
            shared::get(&format!("/api/v1/smart-lists/{private_id}"), "u2key"),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        // visibility grants read only: a sharee cannot modify or delete the list
        let patch = |key: &str, id: &str, body: serde_json::Value| {
            Request::builder()
                .method("PATCH")
                .uri(format!("/api/v1/smart-lists/{id}"))
                .header("X-API-Key", key)
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_string(&body).unwrap()))
                .unwrap()
        };
        let (status, _) = call(
            &state,
            patch(
                "u2key",
                &shared_id,
                serde_json::json!({"summary": "hijack"}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _) = call(
            &state,
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/v1/smart-lists/{shared_id}"))
                .header("X-API-Key", "u2key")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        // the owner's list is untouched
        let (status, body) = call(
            &state,
            shared::get(&format!("/api/v1/smart-lists/{shared_id}"), "adminkey"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_ne!(
            serde_json::from_slice::<serde_json::Value>(&body).unwrap()["summary"],
            "hijack"
        );

        // same for PUBLIC lists: any authenticated user reads, only owner/admin writes
        let (status, _) = call(
            &state,
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/v1/smart-lists/{public_id}"))
                .header("X-API-Key", "u2key")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);

        // admin may write other users' lists
        let (status, _) = call(
            &state,
            patch(
                "adminkey",
                &private_id,
                serde_json::json!({"summary": "admin edit"}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);

        // switching a list to SHARED without a scope is rejected, like create
        let (status, body) = call(
            &state,
            patch(
                "adminkey",
                &private_id,
                serde_json::json!({"visibility": "SHARED"}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(String::from_utf8_lossy(&body).contains("sharedWithUserIds"));

        // admin sees everything and can narrow the browse to one owner
        let (status, body) = call(&state, shared::get("/api/v1/smart-lists", "adminkey")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&body).unwrap()["totalElements"],
            3
        );
        let (status, body) = call(
            &state,
            shared::get(&format!("/api/v1/smart-lists?owner={u1}"), "adminkey"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&body).unwrap()["totalElements"],
            1
        );

        // the user directory is admin-only, like komga's /api/v2/users
        let (status, _) = call(
            &state,
            shared::get("/api/v1/smart-lists/share-targets", "u2key"),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, body) = call(
            &state,
            shared::get("/api/v1/smart-lists/share-targets", "adminkey"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let targets = serde_json::from_slice::<serde_json::Value>(&body).unwrap();
        assert_eq!(targets.as_array().unwrap().len(), 3);
        assert!(!public_id.is_empty());
    }
}
