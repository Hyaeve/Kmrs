//! Equivalent of `SeriesCollectionController` (read-only side): /api/v1/collections/**.

use crate::auth::RequireAuth;
use crate::dto::common::{Page, Pageable, SortOrder};
use crate::error::ApiError;
use crate::http::pagination::{QueryExt, QueryPageable};
use crate::state::AppState;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::header;
use axum::response::Response;
use axum::{routing, Json, Router};
use komga_core::dto::collection::CollectionDto;
use komga_core::dto::series::SeriesDto;
use komga_core::dto::thumbnail::ThumbnailSeriesCollectionDto;
use komga_core::model::collection::SeriesCollection;
use komga_core::model::library::SeriesCover;
use komga_core::model::series::SeriesStatus;
use komga_core::model::user::KomgaUser;
use komga_core::search::*;
use komga_db::dao::library::LibraryDao;
use komga_db::dao::series::SeriesDao;
use komga_db::dao::thumbnail::{
    ThumbnailBookDao, ThumbnailSeriesCollectionDao, ThumbnailSeriesDao,
};
use komga_db::dto_dao::collection::CollectionDtoDao;
use komga_db::dto_dao::series::SeriesDtoDao;
use komga_db::dto_dao::{DtoPage, PageRequest};
use komga_db::pool::Database;
use serde::Serialize;
use std::collections::BTreeSet;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/collections", routing::get(get_collections))
        .route(
            "/api/v1/collections/{id}",
            routing::get(get_collection_by_id),
        )
        .route(
            "/api/v1/collections/{id}/thumbnail",
            routing::get(get_collection_thumbnail),
        )
        .route(
            "/api/v1/collections/{id}/thumbnails",
            routing::get(get_collection_thumbnails),
        )
        .route(
            "/api/v1/collections/{id}/thumbnails/{thumbnailId}",
            routing::get(get_collection_thumbnail_by_id),
        )
        .route(
            "/api/v1/collections/{id}/series",
            routing::get(get_series_by_collection_id),
        )
}

async fn get_collections(
    State(state): State<AppState>,
    auth: RequireAuth,
    qp: QueryPageable,
) -> Result<Json<Page<CollectionDto>>, ApiError> {
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
    let result = CollectionDtoDao::new(state.db.clone()).find_all(
        user.get_authorized_library_ids(library_id_param(&qp).as_ref())
            .as_ref(),
        user.get_authorized_library_ids(None).as_ref(),
        search,
        &page_request,
        &user.restrictions,
    )?;
    Ok(Json(page_of(result, &qp.pageable, sort)))
}

async fn get_collection_by_id(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path(id): Path<String>,
) -> Result<Json<CollectionDto>, ApiError> {
    let collection = find_visible_collection(&state, &auth.0.user, &id)?;
    Ok(Json(CollectionDto::from(&collection)))
}

async fn get_collection_thumbnail(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let user = &auth.0.user;
    let collection = find_visible_collection(&state, user, &id)?;
    let bytes = collection_thumbnail_bytes(&state, user, &collection)?;
    Ok(jpeg_response(bytes, Some("private, max-age=3600")))
}

async fn get_collection_thumbnails(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path(id): Path<String>,
) -> Result<Json<Vec<ThumbnailSeriesCollectionDto>>, ApiError> {
    find_visible_collection(&state, &auth.0.user, &id)?;
    let thumbnails =
        ThumbnailSeriesCollectionDao::new(state.db.clone()).find_all_by_collection_id(&id)?;
    Ok(Json(
        thumbnails
            .iter()
            .map(ThumbnailSeriesCollectionDto::from)
            .collect(),
    ))
}

async fn get_collection_thumbnail_by_id(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path((id, thumbnail_id)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    let collection = find_visible_collection(&state, &auth.0.user, &id)?;
    let thumbnail = ThumbnailSeriesCollectionDao::new(state.db.clone())
        .find_by_id(&thumbnail_id)?
        .ok_or_else(|| ApiError::not_found(""))?;
    if thumbnail.collection_id != collection.id {
        return Err(ApiError::bad_request(""));
    }
    Ok(jpeg_response(thumbnail.thumbnail, None))
}

async fn get_series_by_collection_id(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path(id): Path<String>,
    qp: QueryPageable,
) -> Result<Json<Page<SeriesDto>>, ApiError> {
    let user = &auth.0.user;
    let collection = find_visible_collection(&state, user, &id)?;
    let sort = vec![SortOrder {
        property: if collection.ordered {
            "collection.number".into()
        } else {
            "metadata.titleSort".into()
        },
        descending: false,
    }];
    let condition = collection_series_condition(&collection, &qp)?;
    let search = SeriesSearch {
        condition: Some(condition),
        full_text_search: None,
    };
    let page_request = to_page_request(&qp.pageable, sort.clone());
    let result = SeriesDtoDao::new(state.db.clone()).find_all(
        &search,
        None,
        &SearchContext::of_user(user),
        &page_request,
    )?;
    let items = result
        .items
        .into_iter()
        .map(|s| s.restrict_url(!user.is_admin()))
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

fn find_visible_collection(
    state: &AppState,
    user: &KomgaUser,
    id: &str,
) -> Result<SeriesCollection, ApiError> {
    CollectionDtoDao::new(state.db.clone())
        .find_by_id(
            id,
            user.get_authorized_library_ids(None).as_ref(),
            &user.restrictions,
        )?
        .ok_or_else(|| ApiError::not_found(""))
}

/// `SeriesCollectionLifecycle.getThumbnailBytes`: the selected thumbnail, or a 2x2 mosaic of the
/// first 4 member series' covers (the id list is cycled to fill the grid, as in komga)
fn collection_thumbnail_bytes(
    state: &AppState,
    user: &KomgaUser,
    collection: &SeriesCollection,
) -> Result<Vec<u8>, ApiError> {
    if let Some(selected) = ThumbnailSeriesCollectionDao::new(state.db.clone())
        .find_selected_by_collection_id(&collection.id)?
    {
        return Ok(selected.thumbnail);
    }
    let mut ids = Vec::new();
    while ids.len() < 4 && !collection.series_ids.is_empty() {
        ids.extend(collection.series_ids.iter().take(4).cloned());
    }
    ids.truncate(4);
    let images: Vec<Vec<u8>> = ids
        .iter()
        .filter_map(|id| series_thumbnail_bytes(state, id, &user.id))
        .collect();
    create_mosaic(&images, state.settings.get().thumbnail_size.max_edge())
}

/// `SeriesLifecycle.getThumbnailBytes`: selected thumbnail, else the cover book's thumbnail
/// chosen by the library's series-cover rule
fn series_thumbnail_bytes(state: &AppState, series_id: &str, user_id: &str) -> Option<Vec<u8>> {
    let db = &state.db;
    if let Some(bytes) = effective_series_thumbnail_bytes(db, series_id) {
        return Some(bytes);
    }
    let series = SeriesDao::new(db.clone())
        .find_by_id(series_id)
        .ok()
        .flatten()?;
    let library = LibraryDao::new(db.clone())
        .find_by_id(&series.library_id)
        .ok()
        .flatten()?;
    let book_id = match library.series_cover {
        SeriesCover::First => first_book_id(db, series_id),
        SeriesCover::FirstUnreadOrFirst => {
            first_unread_book_id(db, series_id, user_id).or_else(|| first_book_id(db, series_id))
        }
        SeriesCover::FirstUnreadOrLast => {
            first_unread_book_id(db, series_id, user_id).or_else(|| last_book_id(db, series_id))
        }
        SeriesCover::Last => last_book_id(db, series_id),
    }?;
    book_thumbnail_bytes(state, &book_id)
}

/// Read-side equivalent of `getSelectedThumbnail` + `thumbnailsHouseKeeping`: the first selected
/// thumbnail that exists, else the first existing one (housekeeping only re-selects, which this
/// order reproduces without writing)
fn effective_series_thumbnail_bytes(db: &Database, series_id: &str) -> Option<Vec<u8>> {
    let all = ThumbnailSeriesDao::new(db.clone())
        .find_all_by_series_id(series_id)
        .ok()?;
    let existing: Vec<_> = all
        .into_iter()
        .filter(|t| thumbnail_exists(&t.thumbnail, &t.url))
        .collect();
    let first = existing
        .iter()
        .find(|t| t.selected)
        .or_else(|| existing.first())?;
    thumbnail_bytes(first.thumbnail.clone(), first.url.clone())
}

/// `BookLifecycle.getThumbnailBytes` (no resize): the effective selected book thumbnail's bytes
pub(crate) fn book_thumbnail_bytes(state: &AppState, book_id: &str) -> Option<Vec<u8>> {
    let all = ThumbnailBookDao::new(state.db.clone())
        .find_all_by_book_id(book_id)
        .ok()?;
    let existing: Vec<_> = all
        .into_iter()
        .filter(|t| thumbnail_exists(&t.thumbnail, &t.url))
        .collect();
    let first = existing
        .iter()
        .find(|t| t.selected)
        .or_else(|| existing.first())?;
    thumbnail_bytes(first.thumbnail.clone(), first.url.clone())
}

fn thumbnail_bytes(blob: Option<Vec<u8>>, url: Option<String>) -> Option<Vec<u8>> {
    if let Some(blob) = blob {
        return Some(blob);
    }
    let url = url?;
    std::fs::read(komga_core::dto::url_to_file_path(&url)).ok()
}

/// `ThumbnailBook/ThumbnailSeries.exists()`: a URL thumbnail exists when the file does
fn thumbnail_exists(blob: &Option<Vec<u8>>, url: &Option<String>) -> bool {
    match url {
        Some(url) => std::path::Path::new(&komga_core::dto::url_to_file_path(url)).exists(),
        None => blob.is_some(),
    }
}

/// `BookRepository.findFirstIdInSeriesOrNull` (deleted books included, as in komga)
fn first_book_id(db: &Database, series_id: &str) -> Option<String> {
    book_id_by_number_sort(db, series_id, "ASC")
}

/// `BookRepository.findLastIdInSeriesOrNull`
fn last_book_id(db: &Database, series_id: &str) -> Option<String> {
    book_id_by_number_sort(db, series_id, "DESC")
}

fn book_id_by_number_sort(db: &Database, series_id: &str, dir: &str) -> Option<String> {
    db.ro()
        .prepare(&format!(
            "SELECT BOOK.ID FROM BOOK LEFT JOIN BOOK_METADATA ON (BOOK.ID = BOOK_METADATA.BOOK_ID) \
             WHERE BOOK.SERIES_ID = ? ORDER BY BOOK_METADATA.NUMBER_SORT {dir} LIMIT 1"
        ))
        .ok()?
        .query_map([series_id], |r| r.get(0))
        .ok()?
        .next()
        .transpose()
        .ok()
        .flatten()
}

/// `BookRepository.findFirstUnreadIdInSeriesOrNull`
fn first_unread_book_id(db: &Database, series_id: &str, user_id: &str) -> Option<String> {
    db.ro()
        .prepare(
            "SELECT BOOK.ID FROM BOOK \
             LEFT JOIN BOOK_METADATA ON (BOOK.ID = BOOK_METADATA.BOOK_ID) \
             LEFT JOIN READ_PROGRESS ON (BOOK.ID = READ_PROGRESS.BOOK_ID \
               AND (READ_PROGRESS.USER_ID = ? OR READ_PROGRESS.USER_ID IS NULL)) \
             WHERE BOOK.SERIES_ID = ? AND (READ_PROGRESS.COMPLETED IS NULL OR READ_PROGRESS.COMPLETED = 0) \
             ORDER BY BOOK_METADATA.NUMBER_SORT LIMIT 1",
        )
        .ok()?
        .query_map(rusqlite::params![user_id, series_id], |r| r.get(0))
        .ok()?
        .next()
        .transpose()
        .ok()
        .flatten()
}

/// `MosaicGenerator.createMosaic`: 2x2 grid with top-left anchored cells on a black background,
/// JPEG output; width = round(height * 0.7066666667)
pub(crate) fn create_mosaic(images: &[Vec<u8>], max_edge: u32) -> Result<Vec<u8>, ApiError> {
    let height = max_edge;
    let width = (height as f64 * 0.7066666667).round() as u32;
    let mut mosaic = image::RgbImage::new(width, height);
    let positions = [
        (0i64, 0i64),
        ((width / 2) as i64, 0),
        (0, (height / 2) as i64),
        ((width / 2) as i64, (height / 2) as i64),
    ];
    for (bytes, (x, y)) in images.iter().take(4).zip(positions) {
        let img = image::load_from_memory(bytes)
            .map_err(|e| ApiError::Internal(format!("could not decode mosaic image: {e}")))?;
        // `resize` keeps the aspect ratio and never upscales, like Thumbnailator's size()
        let thumb = img.resize(
            height / 2,
            height / 2,
            image::imageops::FilterType::Lanczos3,
        );
        image::imageops::overlay(&mut mosaic, &thumb.to_rgb8(), x, y);
    }
    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(mosaic)
        .write_to(&mut out, image::ImageFormat::Jpeg)
        .map_err(|e| ApiError::Internal(format!("could not encode mosaic: {e}")))?;
    Ok(out.into_inner())
}

/// Filter conditions of `getSeriesByCollectionId`: collection membership plus the query params
fn collection_series_condition(
    collection: &SeriesCollection,
    qp: &QueryPageable,
) -> Result<SearchConditionSeries, ApiError> {
    let mut conditions = vec![SearchConditionSeries::CollectionId {
        operator: Equality::Is {
            value: collection.id.clone(),
        },
    }];
    let library_ids = qp.params.all("library_id");
    if !library_ids.is_empty() {
        conditions.push(any_of_series(library_ids, |id| {
            SearchConditionSeries::LibraryId {
                operator: Equality::Is { value: id.clone() },
            }
        }));
    }
    if let Some(deleted) = qp.params.first_bool("deleted") {
        conditions.push(SearchConditionSeries::Deleted {
            deleted: bool_op(deleted),
        });
    }
    if let Some(complete) = qp.params.first_bool("complete") {
        conditions.push(SearchConditionSeries::Complete {
            complete: bool_op(complete),
        });
    }
    let statuses = qp.params.all("status");
    if !statuses.is_empty() {
        let parsed = statuses
            .iter()
            .map(|s| {
                SeriesStatus::from_str(s).ok_or_else(|| {
                    enum_conversion_error("org.gotson.komga.domain.model.SeriesMetadata$Status", s)
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        conditions.push(SearchConditionSeries::AnyOf {
            conditions: parsed
                .into_iter()
                .map(|value| SearchConditionSeries::SeriesStatus {
                    operator: Equality::Is { value },
                })
                .collect(),
        });
    }
    let publishers = qp.params.all("publisher");
    if !publishers.is_empty() {
        conditions.push(any_of_series(publishers, |p| {
            SearchConditionSeries::Publisher {
                publisher: Equality::Is { value: p.clone() },
            }
        }));
    }
    let languages = qp.params.all("language");
    if !languages.is_empty() {
        conditions.push(any_of_series(languages, |l| {
            SearchConditionSeries::Language {
                language: Equality::Is { value: l.clone() },
            }
        }));
    }
    let tags = qp.params.all("tag");
    if !tags.is_empty() {
        conditions.push(any_of_series(tags, |t| SearchConditionSeries::Tag {
            tag: EqualityNullable::Is { value: t.clone() },
        }));
    }
    let genres = qp.params.all("genre");
    if !genres.is_empty() {
        conditions.push(any_of_series(genres, |g| SearchConditionSeries::Genre {
            genre: EqualityNullable::Is { value: g.clone() },
        }));
    }
    let age_ratings = qp.params.all("age_rating");
    if !age_ratings.is_empty() {
        conditions.push(any_of_series(age_ratings, |a| {
            SearchConditionSeries::AgeRating {
                operator: match a.parse::<i32>() {
                    Ok(v) => NumericNullable::Is { value: v },
                    Err(_) => NumericNullable::IsNull,
                },
            }
        }));
    }
    let release_years: Vec<i32> = qp
        .params
        .all("release_year")
        .iter()
        .filter_map(|y| y.parse::<i32>().ok())
        .collect();
    if !release_years.is_empty() {
        conditions.push(any_of_series(&release_years, |y| {
            SearchConditionSeries::AllOf {
                conditions: vec![
                    SearchConditionSeries::ReleaseDate {
                        operator: DateOp::After {
                            date_time: year_boundary(y - 1, time::Month::December, 31),
                        },
                    },
                    SearchConditionSeries::ReleaseDate {
                        operator: DateOp::Before {
                            date_time: year_boundary(y + 1, time::Month::January, 1),
                        },
                    },
                ],
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
        conditions.push(SearchConditionSeries::AnyOf {
            conditions: parsed
                .into_iter()
                .map(|value| SearchConditionSeries::ReadStatus {
                    operator: Equality::Is { value },
                })
                .collect(),
        });
    }
    let authors = crate::http::headers::parse_authors(qp.params.all("author"));
    if !authors.is_empty() {
        conditions.push(any_of_series(&authors, |a| SearchConditionSeries::Author {
            author: Equality::Is {
                value: AuthorMatch {
                    name: Some(a.name.clone()),
                    role: Some(a.role.clone()),
                },
            },
        }));
    }
    Ok(SearchConditionSeries::AllOf { conditions })
}

pub(crate) fn any_of_series<T, F: Fn(&T) -> SearchConditionSeries>(
    values: &[T],
    f: F,
) -> SearchConditionSeries {
    SearchConditionSeries::AnyOf {
        conditions: values.iter().map(f).collect(),
    }
}

pub(crate) fn bool_op(value: bool) -> BooleanOp {
    if value {
        BooleanOp::IsTrue
    } else {
        BooleanOp::IsFalse
    }
}

/// `ZonedDateTime.of(..., 12:00 UTC)` used by the release_year filter
fn year_boundary(year: i32, month: time::Month, day: u8) -> time::OffsetDateTime {
    time::PrimitiveDateTime::new(
        time::Date::from_calendar_date(year, month, day).expect("valid calendar date"),
        time::Time::from_hms(12, 0, 0).expect("valid time"),
    )
    .assume_utc()
}

pub(crate) fn parse_read_status(s: &str) -> Option<ReadStatus> {
    match s {
        "UNREAD" => Some(ReadStatus::Unread),
        "READ" => Some(ReadStatus::Read),
        "IN_PROGRESS" => Some(ReadStatus::InProgress),
        _ => None,
    }
}

/// Spring's `MethodArgumentTypeMismatchException` for an invalid enum query value, shortened:
/// the middle ConversionFailedException wrapper text is dropped, status and message shape match
pub(crate) fn enum_conversion_error(fqn: &str, value: &str) -> ApiError {
    ApiError::bad_request(format!(
        "Failed to convert value of type 'java.lang.String' to required type '{fqn}'; nested exception is java.lang.IllegalArgumentException: No enum constant {fqn}.{value}"
    ))
}

/// `library_id` / `library_id[]` as a set; None when the parameter is absent
pub(crate) fn library_id_param(qp: &QueryPageable) -> Option<BTreeSet<String>> {
    qp.params
        .get("library_id")
        .map(|v| v.iter().cloned().collect())
}

pub(crate) fn to_page_request(pageable: &Pageable, sort: Vec<SortOrder>) -> PageRequest {
    PageRequest {
        page: pageable.page,
        size: pageable.size,
        unpaged: pageable.unpaged,
        sort: sort
            .into_iter()
            .map(|s| komga_db::dto_dao::SortOrder {
                property: s.property,
                descending: s.descending,
            })
            .collect(),
    }
}

/// `PageImpl` JSON: the sort echoes the effective sort, or is unsorted when the DAO dropped
/// every order (unknown properties)
pub(crate) fn page_of<T: Serialize>(
    page: DtoPage<T>,
    pageable: &Pageable,
    effective_sort: Vec<SortOrder>,
) -> Page<T> {
    let mut p = pageable.clone();
    p.sort = if page.sorted { effective_sort } else { vec![] };
    Page::of(page.items, page.total as u64, &p)
}

pub(crate) fn jpeg_response(bytes: Vec<u8>, cache_control: Option<&'static str>) -> Response {
    let mut builder = Response::builder().header(header::CONTENT_TYPE, "image/jpeg");
    if let Some(cc) = cache_control {
        builder = builder.header(header::CACHE_CONTROL, cc);
    }
    builder.body(Body::from(bytes)).expect("jpeg response")
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::auth::{sha512_hex, SessionStore};
    use crate::config::ServerConfig;
    use crate::settings::SettingsProvider;
    use axum::http::{HeaderMap, Request, StatusCode};
    use komga_core::model::user::{ApiKey, ContentRestrictions, KomgaUser, UserRole};
    use komga_core::time_codec::now_utc;
    use komga_db::dao::user::UserDao;
    use komga_db::{Migrator, Placeholders};
    use std::sync::Arc;
    use std::time::Duration;
    use tower::Service;

    pub(crate) fn test_state() -> AppState {
        let db = Database::open_in_memory(true).unwrap();
        let tasks_db = Database::open_in_memory(false).unwrap();
        let migrations = komga_db::main_migrations();
        Migrator::new(&migrations, Placeholders::default())
            .migrate(&db.rw())
            .unwrap();
        let tasks_migrations = komga_db::tasks_migrations();
        Migrator::new(&tasks_migrations, Placeholders::default())
            .migrate(&tasks_db.rw())
            .unwrap();
        AppState {
            config: Arc::new(ServerConfig::from_env()),
            settings: Arc::new(SettingsProvider::load(db.clone())),
            sessions: SessionStore::new(Duration::from_secs(3600)),
            tsid: Arc::new(komga_core::tsid::TsidFactory::new_random_node()),
            db,
            tasks_db,
        }
    }

    /// Inserts a user and an API key whose plaintext is `api_key`; returns the user id
    pub(crate) fn insert_user(
        db: &Database,
        email: &str,
        roles: &[UserRole],
        shared_libraries: &[&str],
        restrictions: ContentRestrictions,
        api_key: &str,
    ) -> String {
        let dao = UserDao::new(db.clone());
        let user = KomgaUser {
            id: String::new(),
            email: email.into(),
            password: "x".into(),
            roles: roles.iter().cloned().collect(),
            shared_libraries_ids: shared_libraries.iter().map(|s| s.to_string()).collect(),
            shared_all_libraries: shared_libraries.is_empty(),
            restrictions,
            created_date: now_utc(),
            last_modified_date: now_utc(),
        };
        let user_id = dao.insert(&user).unwrap();
        dao.insert_api_key(&ApiKey {
            id: String::new(),
            user_id: user_id.clone(),
            key: sha512_hex(api_key),
            comment: "test".into(),
            created_date: now_utc(),
            last_modified_date: now_utc(),
        })
        .unwrap();
        user_id
    }

    pub(crate) fn exec(db: &Database, sql: &str, params: impl rusqlite::Params) {
        db.rw().execute(sql, params).unwrap();
    }

    pub(crate) async fn call(
        state: &AppState,
        router: Router<AppState>,
        request: Request<Body>,
    ) -> (StatusCode, HeaderMap, Vec<u8>) {
        let mut app = Router::new()
            .merge(router)
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                crate::auth::auth_middleware,
            ))
            .with_state(state.clone());
        let response = app.call(request).await.unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec();
        (status, headers, bytes)
    }

    pub(crate) fn get(path: &str, api_key: &str) -> Request<Body> {
        Request::builder()
            .uri(path)
            .header("X-API-Key", api_key)
            .body(Body::empty())
            .unwrap()
    }

    fn seed_library(db: &Database, id: &str) {
        exec(
            db,
            "INSERT INTO LIBRARY (ID, NAME, ROOT) VALUES (?, ?, 'file:/l/')",
            rusqlite::params![id, id],
        );
    }

    fn seed_series(db: &Database, id: &str, library_id: &str, title_sort: &str) {
        exec(
            db,
            "INSERT INTO SERIES (ID, NAME, URL, FILE_LAST_MODIFIED, LIBRARY_ID) \
             VALUES (?, ?, 'file:/l/s/', '2020-01-01 00:00:00.0', ?)",
            rusqlite::params![id, id, library_id],
        );
        exec(
            db,
            "INSERT INTO SERIES_METADATA (SERIES_ID, STATUS, TITLE, TITLE_SORT, PUBLISHER) \
             VALUES (?, 'ONGOING', ?, ?, ?)",
            rusqlite::params![id, title_sort, title_sort, format!("pub-{id}")],
        );
        exec(
            db,
            "INSERT INTO BOOK_METADATA_AGGREGATION (SERIES_ID, SUMMARY, SUMMARY_NUMBER) \
             VALUES (?, '', '')",
            [id],
        );
    }

    fn seed_book(db: &Database, id: &str, series_id: &str, number_sort: f64, release_date: &str) {
        exec(
            db,
            "INSERT INTO BOOK (ID, NAME, URL, FILE_LAST_MODIFIED, SERIES_ID, LIBRARY_ID) \
             VALUES (?, ?, 'file:/l/s/b.cbz', '2020-01-01 00:00:00.0', ?, \
             (SELECT LIBRARY_ID FROM SERIES WHERE ID = ?))",
            rusqlite::params![id, id, series_id, series_id],
        );
        exec(
            db,
            "INSERT INTO BOOK_METADATA (BOOK_ID, TITLE, NUMBER, NUMBER_SORT, RELEASE_DATE) \
             VALUES (?, ?, '', ?, ?)",
            rusqlite::params![id, id, number_sort, release_date],
        );
        exec(
            db,
            "INSERT INTO MEDIA (BOOK_ID, STATUS, MEDIA_TYPE, PAGE_COUNT) \
             VALUES (?, 'READY', 'application/zip', 10)",
            [id],
        );
    }

    fn seed_collection(db: &Database, id: &str, name: &str, ordered: bool, series: &[&str]) {
        exec(
            db,
            "INSERT INTO COLLECTION (ID, NAME, ORDERED, SERIES_COUNT) VALUES (?, ?, ?, ?)",
            rusqlite::params![id, name, ordered, series.len() as i32],
        );
        for (i, s) in series.iter().enumerate() {
            exec(
                db,
                "INSERT INTO COLLECTION_SERIES (COLLECTION_ID, SERIES_ID, NUMBER) VALUES (?, ?, ?)",
                rusqlite::params![id, s, i as i32],
            );
        }
    }

    fn seed_thumbnail_collection(db: &Database, id: &str, collection_id: &str, selected: bool) {
        exec(
            db,
            "INSERT INTO THUMBNAIL_COLLECTION \
             (ID, COLLECTION_ID, THUMBNAIL, SELECTED, TYPE, MEDIA_TYPE, FILE_SIZE, WIDTH, HEIGHT) \
             VALUES (?, ?, ?, ?, 'USER_UPLOADED', 'image/jpeg', 3, 1, 1)",
            rusqlite::params![id, collection_id, tiny_jpeg(), selected],
        );
    }

    fn seed_thumbnail_book(db: &Database, id: &str, book_id: &str) {
        exec(
            db,
            "INSERT INTO THUMBNAIL_BOOK \
             (ID, BOOK_ID, THUMBNAIL, SELECTED, TYPE, MEDIA_TYPE, FILE_SIZE, WIDTH, HEIGHT) \
             VALUES (?, ?, ?, 1, 'GENERATED', 'image/jpeg', 3, 1, 1)",
            rusqlite::params![id, book_id, tiny_jpeg()],
        );
    }

    pub(crate) fn tiny_jpeg() -> Vec<u8> {
        let img = image::RgbImage::from_pixel(12, 8, image::Rgb([200, 30, 10]));
        let mut out = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut out, image::ImageFormat::Jpeg)
            .unwrap();
        out.into_inner()
    }

    pub(crate) const ADMIN_KEY: &str = "admin-key";
    pub(crate) const USER_KEY: &str = "user-key";

    /// Base fixture: l1/l2 libraries, s1-s3 series with books, c1 (ordered, s1+s2) / c2
    /// (unordered, s3) collections, an admin and a restricted user.
    pub(crate) fn seed_base(db: &Database) {
        seed_library(db, "l1");
        seed_library(db, "l2");
        seed_series(db, "s1", "l1", "Alpha");
        seed_series(db, "s2", "l1", "beta");
        seed_series(db, "s3", "l2", "Gamma");
        seed_book(db, "b1", "s1", 1.0, "2020-01-01");
        seed_book(db, "b2", "s1", 2.0, "2021-01-01");
        seed_book(db, "b3", "s2", 1.0, "2019-01-01");
        seed_book(db, "b4", "s3", 1.0, "2018-01-01");
        seed_collection(db, "c1", "Best", true, &["s1", "s2"]);
        seed_collection(db, "c2", "another", false, &["s3"]);
        insert_user(
            db,
            "admin@x.y",
            &[UserRole::Admin],
            &[],
            ContentRestrictions::default(),
            ADMIN_KEY,
        );
        insert_user(
            db,
            "user@x.y",
            &[],
            &["l1"],
            ContentRestrictions::default(),
            USER_KEY,
        );
    }

    #[tokio::test]
    async fn list_collections_pagination_and_name_sort() {
        let state = test_state();
        seed_base(&state.db);
        let (status, _, body) = call(
            &state,
            router(),
            get("/api/v1/collections?size=1", ADMIN_KEY),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let page: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(page["totalElements"], 2);
        assert_eq!(page["totalPages"], 2);
        assert_eq!(page["content"].as_array().unwrap().len(), 1);
        // unicode3 collation: "another" before "Best"
        assert_eq!(page["content"][0]["name"], "another");
        assert_eq!(page["sort"]["sorted"], true);

        let (status, _, body) = call(
            &state,
            router(),
            get("/api/v1/collections?page=1&size=1", ADMIN_KEY),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let page: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(page["content"][0]["name"], "Best");
    }

    #[tokio::test]
    async fn list_collections_search_returns_nothing_before_m6() {
        let state = test_state();
        seed_base(&state.db);
        let (status, _, body) = call(
            &state,
            router(),
            get("/api/v1/collections?search=best", ADMIN_KEY),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let page: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(page["totalElements"], 0);
    }

    #[tokio::test]
    async fn list_collections_library_filter() {
        let state = test_state();
        seed_base(&state.db);
        let (status, _, body) = call(
            &state,
            router(),
            get("/api/v1/collections?library_id=l2", ADMIN_KEY),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let page: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(page["totalElements"], 1);
        assert_eq!(page["content"][0]["id"], "c2");

        // user shared only on l1 does not see c2
        let (status, _, body) = call(&state, router(), get("/api/v1/collections", USER_KEY)).await;
        assert_eq!(status, StatusCode::OK);
        let page: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(page["totalElements"], 1);
        assert_eq!(page["content"][0]["id"], "c1");
    }

    #[tokio::test]
    async fn list_collections_restricted_filtered_flag() {
        let state = test_state();
        seed_base(&state.db);
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
        let (status, _, body) = call(
            &state,
            router(),
            get("/api/v1/collections", "restricted-key"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let page: serde_json::Value = serde_json::from_slice(&body).unwrap();
        // only c1 remains visible, with s2 filtered out of its members
        assert_eq!(page["totalElements"], 1);
        assert_eq!(page["content"][0]["id"], "c1");
        assert_eq!(page["content"][0]["filtered"], true);
        assert_eq!(
            page["content"][0]["seriesIds"].as_array().unwrap(),
            &vec![serde_json::json!("s1")]
        );
    }

    #[tokio::test]
    async fn get_collection_by_id_and_404() {
        let state = test_state();
        seed_base(&state.db);
        let (status, _, body) =
            call(&state, router(), get("/api/v1/collections/c1", ADMIN_KEY)).await;
        assert_eq!(status, StatusCode::OK);
        let dto: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(dto["name"], "Best");
        assert_eq!(dto["ordered"], true);
        assert_eq!(dto["seriesIds"].as_array().unwrap().len(), 2);

        let (status, _, _) =
            call(&state, router(), get("/api/v1/collections/nope", ADMIN_KEY)).await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        // c2 is on l2, invisible to the l1-only user
        let (status, _, _) = call(&state, router(), get("/api/v1/collections/c2", USER_KEY)).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn thumbnail_selected_with_1h_cache() {
        let state = test_state();
        seed_base(&state.db);
        seed_thumbnail_collection(&state.db, "tc1", "c1", true);
        let (status, headers, body) = call(
            &state,
            router(),
            get("/api/v1/collections/c1/thumbnail", ADMIN_KEY),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers[header::CONTENT_TYPE], "image/jpeg");
        assert_eq!(headers[header::CACHE_CONTROL], "private, max-age=3600");
        assert_eq!(body, tiny_jpeg());
    }

    #[tokio::test]
    async fn thumbnail_mosaic_when_no_selected() {
        let state = test_state();
        seed_base(&state.db);
        // c1 has no selected collection thumbnail; series s1's cover book b1 has one
        seed_thumbnail_book(&state.db, "tb1", "b1");
        let (status, headers, body) = call(
            &state,
            router(),
            get("/api/v1/collections/c1/thumbnail", ADMIN_KEY),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers[header::CONTENT_TYPE], "image/jpeg");
        assert_eq!(headers[header::CACHE_CONTROL], "private, max-age=3600");
        let img = image::load_from_memory(&body).unwrap();
        let settings = state.settings.get();
        assert_eq!(img.height(), settings.thumbnail_size.max_edge());
        assert_eq!(
            img.width(),
            (settings.thumbnail_size.max_edge() as f64 * 0.7066666667).round() as u32
        );
    }

    #[tokio::test]
    async fn thumbnails_list_and_by_id() {
        let state = test_state();
        seed_base(&state.db);
        seed_thumbnail_collection(&state.db, "tc1", "c1", true);
        let (status, _, body) = call(
            &state,
            router(),
            get("/api/v1/collections/c1/thumbnails", ADMIN_KEY),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let list: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(list.as_array().unwrap().len(), 1);
        assert_eq!(list[0]["id"], "tc1");
        assert_eq!(list[0]["collectionId"], "c1");
        assert_eq!(list[0]["selected"], true);

        let (status, headers, body) = call(
            &state,
            router(),
            get("/api/v1/collections/c1/thumbnails/tc1", ADMIN_KEY),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers[header::CONTENT_TYPE], "image/jpeg");
        assert_eq!(body, tiny_jpeg());

        // thumbnail exists but belongs to another collection
        let (status, _, _) = call(
            &state,
            router(),
            get("/api/v1/collections/c2/thumbnails/tc1", ADMIN_KEY),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        // thumbnail does not exist
        let (status, _, _) = call(
            &state,
            router(),
            get("/api/v1/collections/c1/thumbnails/nope", ADMIN_KEY),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn collection_series_ordered_sort_by_collection_number() {
        let state = test_state();
        seed_base(&state.db);
        let (status, _, body) = call(
            &state,
            router(),
            get("/api/v1/collections/c1/series", ADMIN_KEY),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let page: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let ids: Vec<&str> = page["content"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["s1", "s2"]);

        // c2 is unordered: falls back to metadata.titleSort
        let (status, _, body) = call(
            &state,
            router(),
            get("/api/v1/collections/c2/series", ADMIN_KEY),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let page: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(page["content"][0]["id"], "s3");
    }

    #[tokio::test]
    async fn collection_series_invalid_enum_param() {
        let state = test_state();
        seed_base(&state.db);
        let (status, _, body) = call(
            &state,
            router(),
            get("/api/v1/collections/c1/series?status=BOGUS", ADMIN_KEY),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let error: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(error["status"], 400);
        assert!(error["message"].as_str().unwrap().contains(
            "No enum constant org.gotson.komga.domain.model.SeriesMetadata$Status.BOGUS"
        ));
    }

    #[tokio::test]
    async fn collection_series_filters_and_restrict_url() {
        let state = test_state();
        seed_base(&state.db);
        // publisher filter (exact match on the seeded "pub-<id>" values)
        let (status, _, body) = call(
            &state,
            router(),
            get("/api/v1/collections/c1/series?publisher=pub-s2", ADMIN_KEY),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let page: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(page["totalElements"], 1);
        assert_eq!(page["content"][0]["id"], "s2");

        // non-admin sees a blanked url
        let (status, _, body) = call(
            &state,
            router(),
            get("/api/v1/collections/c1/series", USER_KEY),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let page: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(page["content"][0]["url"], "");
        // library_id filter: nothing on l2 inside c1
        let (status, _, body) = call(
            &state,
            router(),
            get("/api/v1/collections/c1/series?library_id=l2", ADMIN_KEY),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let page: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(page["totalElements"], 0);
    }
}
