//! Equivalent of `BookController` and `CommonBookController` (read side):
//! `/api/v1/books/**` list/detail/thumbnails/pages/file/progression/read-progress endpoints.

use crate::api::restriction;
use crate::auth::{MaybeAuth, RequireAuth};
use crate::dto::common::{Page, Pageable, SortOrder};
use crate::error::{ApiError, Violation};
use crate::http::headers::{check_not_modified, content_disposition, format_http_date};
use crate::http::pagination::{QueryExt, QueryPageable};
use crate::state::AppState;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{routing, Json, Router};
use komga_core::dto::book::{BookDto, PageDto};
use komga_core::dto::progression::{R2Locator, R2Progression, ReadProgressUpdateDto};
use komga_core::dto::readlist::ReadListDto;
use komga_core::dto::thumbnail::ThumbnailBookDto;
use komga_core::dto::url_to_file_path;
use komga_core::model::book::Book;
use komga_core::model::media::{Media, MediaStatus};
use komga_core::model::read_progress::ReadProgress;
use komga_core::model::thumbnail::ThumbnailBook;
use komga_core::model::user::{KomgaUser, UserRole};
use komga_core::search::{
    BookSearch, DateOp, Equality, MediaProfile, ReadStatus, SearchConditionBook, SearchContext,
};
use komga_core::time_codec;
use komga_db::dao::book::BookDao;
use komga_db::dao::media::MediaDao;
use komga_db::dao::read_progress::ReadProgressDao;
use komga_db::dao::thumbnail::ThumbnailBookDao;
use komga_db::dto_dao::book::BookDtoDao;
use komga_db::dto_dao::readlist::ReadListDtoDao;
use komga_db::dto_dao::{DtoPage, PageRequest};
use komga_media::container;
use komga_media::detect;
use komga_media::error::MediaError;
use komga_media::image::ImageType;
use std::collections::BTreeSet;
use std::io::Read;
use std::path::PathBuf;

const MEDIATYPE_PROGRESSION_JSON: &str = "application/vnd.readium.progression+json";

/// `CommonBookController.FONT_EXTENSIONS`
const FONT_EXTENSIONS: [&str; 6] = ["otf", "woff", "woff2", "eot", "ttf", "svg"];

const CSP_HEADER: &str = "script-src 'none'; object-src 'none';";

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/books", routing::get(get_all_books_deprecated))
        .route("/api/v1/books/list", routing::post(get_books))
        .route("/api/v1/books/latest", routing::get(get_books_latest))
        .route("/api/v1/books/ondeck", routing::get(get_books_on_deck))
        .route(
            "/api/v1/books/duplicates",
            routing::get(get_books_duplicates),
        )
        .route("/api/v1/books/{bookId}", routing::get(get_book_by_id))
        .route(
            "/api/v1/books/{bookId}/previous",
            routing::get(get_book_sibling_previous),
        )
        .route(
            "/api/v1/books/{bookId}/next",
            routing::get(get_book_sibling_next),
        )
        .route(
            "/api/v1/books/{bookId}/readlists",
            routing::get(get_readlists_by_book_id),
        )
        .route(
            "/api/v1/books/{bookId}/thumbnail",
            routing::get(get_book_thumbnail),
        )
        .route(
            "/api/v1/books/{bookId}/thumbnails",
            routing::get(get_book_thumbnails),
        )
        .route(
            "/api/v1/books/{bookId}/thumbnails/{thumbnailId}",
            routing::get(get_book_thumbnail_by_id),
        )
        .route("/api/v1/books/{bookId}/pages", routing::get(get_book_pages))
        .route(
            "/api/v1/books/{bookId}/pages/{pageNumber}",
            routing::get(get_book_page_by_number),
        )
        .route(
            "/api/v1/books/{bookId}/pages/{pageNumber}/thumbnail",
            routing::get(get_book_page_thumbnail_by_number),
        )
        .route(
            "/api/v1/books/{bookId}/pages/{pageNumber}/raw",
            routing::get(get_book_page_raw_by_number),
        )
        .route(
            "/api/v1/books/{bookId}/resource/{*resource}",
            routing::get(get_book_epub_resource),
        )
        .route(
            "/api/v1/books/{bookId}/file",
            routing::get(download_book_file),
        )
        .route(
            "/api/v1/books/{bookId}/file/{*rest}",
            routing::get(download_book_file_wildcard),
        )
        .route(
            "/api/v1/books/{bookId}/progression",
            routing::get(get_book_progression).put(update_book_progression),
        )
        .route(
            "/api/v1/books/{bookId}/read-progress",
            routing::patch(mark_book_read_progress).delete(delete_book_read_progress),
        )
}

fn book_dao(state: &AppState) -> BookDao {
    BookDao::new(state.db.clone())
}

fn media_dao(state: &AppState) -> MediaDao {
    MediaDao::new(state.db.clone())
}

fn book_dto_dao(state: &AppState) -> BookDtoDao {
    BookDtoDao::new(state.db.clone())
}

fn thumbnail_dao(state: &AppState) -> ThumbnailBookDao {
    ThumbnailBookDao::new(state.db.clone())
}

fn read_progress_dao(state: &AppState) -> ReadProgressDao {
    ReadProgressDao::new(state.db.clone())
}

/// Kotlin `mediaRepository.findById` throws when absent (data invariant) → 500
fn require_media(state: &AppState, book_id: &str) -> Result<Media, ApiError> {
    media_dao(state)
        .find_by_id(book_id)?
        .ok_or_else(|| ApiError::Internal(format!("no media for book {book_id}")))
}

fn book_path(book: &Book) -> PathBuf {
    PathBuf::from(url_to_file_path(&book.url))
}

fn to_page_request(p: &Pageable) -> PageRequest {
    PageRequest {
        page: p.page,
        size: p.size,
        unpaged: p.unpaged,
        sort: p
            .sort
            .iter()
            .map(|s| komga_db::dto_dao::SortOrder {
                property: s.property.clone(),
                descending: s.descending,
            })
            .collect(),
    }
}

/// The JSON `sort` reflects the ORDER BY actually applied (Spring `pageSort` semantics)
fn page_response<T: serde::Serialize>(dto: DtoPage<T>, pageable: &Pageable) -> Page<T> {
    let mut effective = pageable.clone();
    if !dto.sorted {
        effective.sort = vec![];
    }
    Page::of(dto.items, dto.total as u64, &effective)
}

fn restrict_books(books: Vec<BookDto>, user: &KomgaUser) -> Vec<BookDto> {
    let restricted = !user.is_admin();
    books
        .into_iter()
        .map(|b| b.restrict_url(restricted))
        .collect()
}

fn sort_or_relevance(requested: &[SortOrder], has_search: bool) -> Vec<SortOrder> {
    if !requested.is_empty() {
        return requested.to_vec();
    }
    if has_search {
        return vec![SortOrder {
            property: "relevance".to_string(),
            descending: false,
        }];
    }
    vec![]
}

fn invalid_param(name: &str, value: &str) -> ApiError {
    ApiError::bad_request(format!("Invalid value '{value}' for parameter '{name}'"))
}

fn parse_media_status(value: &str) -> Result<MediaStatus, ApiError> {
    MediaStatus::from_str(value).ok_or_else(|| invalid_param("media_status", value))
}

fn parse_read_status(value: &str) -> Result<ReadStatus, ApiError> {
    match value {
        "UNREAD" => Ok(ReadStatus::Unread),
        "READ" => Ok(ReadStatus::Read),
        "IN_PROGRESS" => Ok(ReadStatus::InProgress),
        _ => Err(invalid_param("read_status", value)),
    }
}

// region list endpoints

fn books_deprecated_condition(qp: &QueryPageable) -> Result<Option<SearchConditionBook>, ApiError> {
    let mut conditions = vec![];
    let any_of = |items: &[String],
                  f: &dyn Fn(&String) -> Result<SearchConditionBook, ApiError>| {
        items
            .iter()
            .map(f)
            .collect::<Result<Vec<_>, _>>()
            .map(|conditions| SearchConditionBook::AnyOf { conditions })
    };
    let library_ids = qp.params.all("library_id");
    if !library_ids.is_empty() {
        conditions.push(any_of(library_ids, &|id| {
            Ok(SearchConditionBook::LibraryId {
                operator: Equality::Is { value: id.clone() },
            })
        })?);
    }
    let media_status = qp.params.all("media_status");
    if !media_status.is_empty() {
        conditions.push(any_of(media_status, &|s| {
            Ok(SearchConditionBook::MediaStatus {
                operator: Equality::Is {
                    value: parse_media_status(s)?,
                },
            })
        })?);
    }
    let read_status = qp.params.all("read_status");
    if !read_status.is_empty() {
        conditions.push(any_of(read_status, &|s| {
            Ok(SearchConditionBook::ReadStatus {
                operator: Equality::Is {
                    value: parse_read_status(s)?,
                },
            })
        })?);
    }
    let tags = qp.params.all("tag");
    if !tags.is_empty() {
        conditions.push(any_of(tags, &|t| {
            Ok(SearchConditionBook::Tag {
                tag: komga_core::search::EqualityNullable::Is { value: t.clone() },
            })
        })?);
    }
    if let Some(released_after) = qp.params.first("released_after") {
        let date = time_codec::parse_date(released_after)
            .ok_or_else(|| invalid_param("released_after", released_after))?;
        let date_time = date.with_time(time::Time::MIDNIGHT).assume_utc();
        conditions.push(SearchConditionBook::ReleaseDate {
            operator: DateOp::After { date_time },
        });
    }
    Ok((!conditions.is_empty()).then_some(SearchConditionBook::AllOf { conditions }))
}

async fn get_all_books_deprecated(
    State(state): State<AppState>,
    auth: RequireAuth,
    qp: QueryPageable,
) -> Result<Json<Page<BookDto>>, ApiError> {
    let condition = books_deprecated_condition(&qp)?;
    let search_term = qp.params.first("search").map(str::to_string);
    let sort = sort_or_relevance(
        &qp.pageable.sort,
        search_term.as_deref().is_some_and(|s| !s.trim().is_empty()),
    );
    let mut pageable = qp.pageable.clone();
    pageable.sort = sort;
    let search = BookSearch {
        condition,
        full_text_search: search_term,
    };
    let ctx = SearchContext::of_user(&auth.0.user);
    let result = book_dto_dao(&state).find_all(&search, &ctx, &to_page_request(&pageable))?;
    let items = restrict_books(result.items, &auth.0.user);
    Ok(Json(page_response(
        DtoPage {
            items,
            total: result.total,
            sorted: result.sorted,
        },
        &pageable,
    )))
}

async fn get_books(
    State(state): State<AppState>,
    auth: RequireAuth,
    qp: QueryPageable,
    Json(search): Json<BookSearch>,
) -> Result<Json<Page<BookDto>>, ApiError> {
    let sort = sort_or_relevance(
        &qp.pageable.sort,
        search
            .full_text_search
            .as_deref()
            .is_some_and(|s| !s.trim().is_empty()),
    );
    let mut pageable = qp.pageable.clone();
    pageable.sort = sort;
    let ctx = SearchContext::of_user(&auth.0.user);
    let result = book_dto_dao(&state).find_all(&search, &ctx, &to_page_request(&pageable))?;
    let items = restrict_books(result.items, &auth.0.user);
    Ok(Json(page_response(
        DtoPage {
            items,
            total: result.total,
            sorted: result.sorted,
        },
        &pageable,
    )))
}

async fn get_books_latest(
    State(state): State<AppState>,
    auth: RequireAuth,
    qp: QueryPageable,
) -> Result<Json<Page<BookDto>>, ApiError> {
    let mut pageable = qp.pageable.clone();
    pageable.sort = vec![SortOrder {
        property: "lastModifiedDate".to_string(),
        descending: true,
    }];
    let ctx = SearchContext::of_user(&auth.0.user);
    let search = BookSearch {
        condition: None,
        full_text_search: None,
    };
    let result = book_dto_dao(&state).find_all(&search, &ctx, &to_page_request(&pageable))?;
    let items = restrict_books(result.items, &auth.0.user);
    Ok(Json(page_response(
        DtoPage {
            items,
            total: result.total,
            sorted: result.sorted,
        },
        &pageable,
    )))
}

async fn get_books_on_deck(
    State(state): State<AppState>,
    auth: RequireAuth,
    qp: QueryPageable,
) -> Result<Json<Page<BookDto>>, ApiError> {
    let library_set: Option<BTreeSet<String>> = qp
        .params
        .get("library_id")
        .map(|ids| ids.iter().cloned().collect());
    let authorized = auth.0.user.get_authorized_library_ids(library_set.as_ref());
    let result = book_dto_dao(&state).find_all_on_deck(
        &auth.0.user.id,
        authorized.as_ref(),
        &auth.0.user.restrictions,
        &to_page_request(&qp.pageable),
    )?;
    let items = restrict_books(result.items, &auth.0.user);
    Ok(Json(page_response(
        DtoPage {
            items,
            total: result.total,
            sorted: result.sorted,
        },
        &qp.pageable,
    )))
}

async fn get_books_duplicates(
    State(state): State<AppState>,
    auth: RequireAuth,
    qp: QueryPageable,
) -> Result<Json<Page<BookDto>>, ApiError> {
    auth.0.require_admin()?;
    let mut pageable = qp.pageable.clone();
    pageable.sort = if qp.pageable.sort.is_empty() {
        vec![SortOrder {
            property: "fileHash".to_string(),
            descending: false,
        }]
    } else {
        qp.pageable.sort.clone()
    };
    // Kotlin uses Pageable.unpaged() here (not UnpagedSorted): the sort is dropped when unpaged
    if pageable.unpaged {
        pageable.sort = vec![];
    }
    let result =
        book_dto_dao(&state).find_all_duplicates(&auth.0.user.id, &to_page_request(&pageable))?;
    let items = restrict_books(result.items, &auth.0.user);
    Ok(Json(page_response(
        DtoPage {
            items,
            total: result.total,
            sorted: result.sorted,
        },
        &pageable,
    )))
}

// endregion

// region detail endpoints

async fn get_book_by_id(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path(book_id): Path<String>,
) -> Result<Json<BookDto>, ApiError> {
    let Some(dto) = book_dto_dao(&state).find_by_id(&book_id, &auth.0.user.id)? else {
        return Err(ApiError::NotFoundEmpty);
    };
    restriction::check_book_dto(&state, &auth.0.user, &dto)?;
    Ok(Json(dto.restrict_url(!auth.0.user.is_admin())))
}

async fn get_book_sibling_previous(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path(book_id): Path<String>,
) -> Result<Json<BookDto>, ApiError> {
    get_book_sibling(state, auth, book_id, false).await
}

async fn get_book_sibling_next(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path(book_id): Path<String>,
) -> Result<Json<BookDto>, ApiError> {
    get_book_sibling(state, auth, book_id, true).await
}

async fn get_book_sibling(
    state: AppState,
    auth: RequireAuth,
    book_id: String,
    next: bool,
) -> Result<Json<BookDto>, ApiError> {
    restriction::check_book_by_id(&state, &auth.0.user, &book_id)?;
    let dao = book_dto_dao(&state);
    let dto = if next {
        dao.find_next_in_series(&book_id, &auth.0.user.id)?
    } else {
        dao.find_previous_in_series(&book_id, &auth.0.user.id)?
    };
    let Some(dto) = dto else {
        return Err(ApiError::not_found(""));
    };
    Ok(Json(dto.restrict_url(!auth.0.user.is_admin())))
}

async fn get_readlists_by_book_id(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path(book_id): Path<String>,
) -> Result<Json<Vec<ReadListDto>>, ApiError> {
    restriction::check_book_by_id(&state, &auth.0.user, &book_id)?;
    let readlists = ReadListDtoDao::new(state.db.clone()).find_all_containing_book_id(
        &book_id,
        auth.0.user.get_authorized_library_ids(None).as_ref(),
        &auth.0.user.restrictions,
    )?;
    Ok(Json(readlists.iter().map(ReadListDto::from).collect()))
}

// endregion

// region thumbnails

/// `ThumbnailBook.exists()`: blob present, or the sidecar file is on disk
fn thumbnail_exists(thumbnail: &ThumbnailBook) -> bool {
    thumbnail.thumbnail.is_some()
        || thumbnail
            .url
            .as_deref()
            .is_some_and(|url| std::path::Path::new(&url_to_file_path(url)).exists())
}

/// `BookLifecycle.thumbnailsHouseKeeping`: drop entries whose bytes are gone, then ensure
/// exactly one thumbnail is selected
fn thumbnails_house_keeping(state: &AppState, book_id: &str) -> Result<(), ApiError> {
    let dao = thumbnail_dao(state);
    let mut all = vec![];
    for thumbnail in dao.find_all_by_book_id(book_id)? {
        if thumbnail_exists(&thumbnail) {
            all.push(thumbnail);
        } else {
            tracing::warn!("Thumbnail doesn't exist, removing entry");
            dao.delete(&thumbnail.id)?;
        }
    }
    let selected: Vec<&ThumbnailBook> = all.iter().filter(|t| t.selected).collect();
    if selected.len() > 1 {
        dao.mark_selected(selected[0])?;
    } else if selected.is_empty() {
        if let Some(first) = all.first() {
            dao.mark_selected(first)?;
        }
    }
    Ok(())
}

/// `BookLifecycle.getThumbnail`
fn get_thumbnail(state: &AppState, book_id: &str) -> Result<Option<ThumbnailBook>, ApiError> {
    let dao = thumbnail_dao(state);
    let selected = dao.find_selected_by_book_id(book_id)?;
    let needs_housekeeping = match &selected {
        Some(t) => !thumbnail_exists(t),
        None => true,
    };
    if needs_housekeeping {
        thumbnails_house_keeping(state, book_id)?;
        return Ok(dao.find_selected_by_book_id(book_id)?);
    }
    Ok(selected)
}

/// `BookLifecycle.getBytesFromThumbnailBook`: blob first, then sidecar file
fn bytes_from_thumbnail(thumbnail: &ThumbnailBook) -> Result<Option<Vec<u8>>, ApiError> {
    if let Some(bytes) = &thumbnail.thumbnail {
        return Ok(Some(bytes.clone()));
    }
    if let Some(url) = &thumbnail.url {
        let path = url_to_file_path(url);
        return Ok(std::fs::read(path).ok());
    }
    Ok(None)
}

fn jpeg_response(bytes: Vec<u8>) -> Response {
    let mut response = Response::new(Body::from(bytes));
    response.headers_mut().insert(
        axum::http::header::CONTENT_TYPE,
        HeaderValue::from_static("image/jpeg"),
    );
    response
}

async fn get_book_thumbnail(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path(book_id): Path<String>,
) -> Result<Response, ApiError> {
    restriction::check_book_by_id(&state, &auth.0.user, &book_id)?;
    let Some(thumbnail) = get_thumbnail(&state, &book_id)? else {
        return Err(ApiError::not_found(""));
    };
    let Some(bytes) = bytes_from_thumbnail(&thumbnail)? else {
        return Err(ApiError::not_found(""));
    };
    Ok(jpeg_response(bytes))
}

async fn get_book_thumbnails(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path(book_id): Path<String>,
) -> Result<Json<Vec<ThumbnailBookDto>>, ApiError> {
    restriction::check_book_by_id(&state, &auth.0.user, &book_id)?;
    let thumbnails = thumbnail_dao(&state).find_all_by_book_id(&book_id)?;
    Ok(Json(
        thumbnails.iter().map(ThumbnailBookDto::from).collect(),
    ))
}

async fn get_book_thumbnail_by_id(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path((book_id, thumbnail_id)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    restriction::check_book_by_id(&state, &auth.0.user, &book_id)?;
    restriction::check_book_thumbnail(&state, &auth.0.user, &thumbnail_id)?;
    let Some(thumbnail) = thumbnail_dao(&state).find_by_id(&thumbnail_id)? else {
        return Err(ApiError::not_found(""));
    };
    let Some(bytes) = bytes_from_thumbnail(&thumbnail)? else {
        return Err(ApiError::not_found(""));
    };
    Ok(jpeg_response(bytes))
}

// endregion

// region pages

async fn get_book_pages(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path(book_id): Path<String>,
) -> Result<Json<Vec<PageDto>>, ApiError> {
    let Some(book) = book_dao(&state).find_by_id(&book_id)? else {
        return Err(ApiError::not_found(""));
    };
    restriction::check_book(&state, &auth.0.user, &book)?;
    let media = require_media(&state, &book.id)?;
    let pages = match media.status {
        MediaStatus::Unknown => return Err(ApiError::not_found("Book has not been analyzed yet")),
        MediaStatus::Outdated => {
            return Err(ApiError::not_found(
                "Book is outdated and must be re-analyzed",
            ))
        }
        MediaStatus::Error => return Err(ApiError::not_found("Book analysis failed")),
        MediaStatus::Unsupported => {
            return Err(ApiError::not_found("Book format is not supported"))
        }
        MediaStatus::Ready => {
            if container::media_profile(media.media_type.as_deref()) == Some(MediaProfile::Pdf) {
                container::get_pdf_pages_dynamic(&media).map_err(map_media_error)?
            } else {
                media.pages.clone()
            }
        }
    };
    Ok(Json(
        pages
            .iter()
            .enumerate()
            .map(|(index, page)| PageDto {
                number: index as i32 + 1,
                file_name: page.file_name.clone(),
                media_type: page.media_type.clone(),
                width: page.width,
                height: page.height,
                size_bytes: page.file_size,
                size: PageDto::size_of(page.file_size),
            })
            .collect(),
    ))
}

fn last_modified_millis(media: &Media) -> i64 {
    (media.last_modified_date.unix_timestamp_nanos() / 1_000_000) as i64
}

fn set_last_modified(response: &mut Response, media: &Media) {
    response.headers_mut().insert(
        axum::http::header::LAST_MODIFIED,
        HeaderValue::from_str(&format_http_date(last_modified_millis(media) / 1000)).unwrap(),
    );
}

fn not_modified_response(media: &Media) -> Response {
    let mut response = StatusCode::NOT_MODIFIED.into_response();
    set_last_modified(&mut response, media);
    response
}

/// `getMediaTypeOrDefault` for a Content-Type header
fn content_type_header(media_type: Option<&str>) -> HeaderValue {
    HeaderValue::from_str(&detect::media_type_or_default(media_type)).unwrap()
}

/// Maps media errors to the HTTP statuses of `getBookPageInternal` / `getBookPageRawInternal`
fn map_media_error(error: MediaError) -> ApiError {
    match error {
        MediaError::NotReady => ApiError::not_found("Book analysis failed"),
        MediaError::PageOutOfBounds(_) => ApiError::bad_request("Page number does not exist"),
        MediaError::Conversion(message) => ApiError::not_found(message),
        MediaError::NoSuchFile(path) => {
            tracing::warn!("File not found: {path}");
            ApiError::not_found("File not found, it may have moved")
        }
        // MediaUnsupportedException is uncaught by getBookPageInternal → 500
        MediaError::Unsupported { message, .. } => ApiError::Internal(message),
        MediaError::EntryNotFound(message) => ApiError::Internal(message),
        MediaError::Other(error) => ApiError::Internal(error.to_string()),
    }
}

/// `getBookPageRawInternal` maps MediaUnsupportedException to 400, unlike the image path
fn map_media_error_raw(error: MediaError) -> ApiError {
    match error {
        MediaError::Unsupported { message, .. } => ApiError::bad_request(message),
        other => map_media_error(other),
    }
}

struct PageRequestParams {
    convert: Option<String>,
    zero_based: bool,
    content_negotiation: bool,
}

fn page_params(qp: &QueryPageable) -> PageRequestParams {
    PageRequestParams {
        convert: qp.params.first("convert").map(str::to_string),
        zero_based: qp.params.first_bool("zero_based").unwrap_or(false),
        content_negotiation: qp.params.first_bool("contentNegotiation").unwrap_or(true),
    }
}

/// `getBookPageInternal` validates the convert format after content negotiation
fn parse_convert(convert: Option<&str>) -> Result<Option<ImageType>, ApiError> {
    match convert {
        None => Ok(None),
        Some("") => Ok(None),
        Some(c) if c.eq_ignore_ascii_case("jpeg") => Ok(Some(ImageType::Jpeg)),
        Some(c) if c.eq_ignore_ascii_case("png") => Ok(Some(ImageType::Png)),
        Some(c) => Err(ApiError::bad_request(format!(
            "Invalid conversion format: {c}"
        ))),
    }
}

struct Mime<'a> {
    type_: &'a str,
    subtype: &'a str,
    specificity_rank: u8,
    param_count: usize,
}

fn parse_accept(header: &str) -> Vec<Mime<'_>> {
    header
        .split(',')
        .filter_map(|part| {
            let part = part.trim();
            let (mime, params) = part.split_once(';').unwrap_or((part, ""));
            let (type_, subtype) = mime.trim().split_once('/')?;
            let type_ = type_.trim();
            let subtype = subtype.trim();
            if type_.is_empty() || subtype.is_empty() {
                return None;
            }
            let param_count = if params.is_empty() {
                0
            } else {
                params.split(';').count()
            };
            let specificity_rank = if type_ == "*" {
                2
            } else if subtype == "*" {
                1
            } else {
                0
            };
            Some(Mime {
                type_,
                subtype,
                specificity_rank,
                param_count,
            })
        })
        .collect()
}

/// Spring `MediaType.isCompatibleWith`
fn compatible(mime: &Mime<'_>, type_: &str, subtype: &str) -> bool {
    mime.type_ == "*"
        || (mime.type_ == type_
            && (mime.subtype == "*" || subtype == "*" || mime.subtype == subtype))
}

/// `MimeTypeUtils.sortBySpecificity` (stable): concrete < type/* < */*; different concrete types
/// order lexicographically by type; more parameters is more specific
fn sort_by_specificity(mimes: &mut [Mime<'_>]) {
    mimes.sort_by(|a, b| {
        a.specificity_rank
            .cmp(&b.specificity_rank)
            .then_with(|| a.type_.cmp(b.type_))
            .then_with(|| a.subtype.cmp(b.subtype))
            .then_with(|| b.param_count.cmp(&a.param_count))
    });
}

fn raw_page_response(
    book: &Book,
    media: &Media,
    page_number: i32,
    page_content: container::PageContent,
) -> Response {
    let extension = detect::media_type_to_extension(&page_content.media_type).unwrap_or("");
    let mut response = Response::new(Body::from(page_content.bytes));
    let headers = response.headers_mut();
    headers.insert(
        axum::http::header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&content_disposition(
            "inline",
            &format!("{}-{page_number}{extension}", book.name),
        ))
        .unwrap(),
    );
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        content_type_header(Some(&page_content.media_type)),
    );
    set_last_modified(&mut response, media);
    response
}

fn image_page_response(
    book: &Book,
    media: &Media,
    page_number: i32,
    page_content: container::PageContent,
) -> Response {
    // Kotlin quirk: the fallback is "jpeg" without a leading dot
    let extension = detect::media_type_to_extension(&page_content.media_type).unwrap_or("jpeg");
    let mut response = Response::new(Body::from(page_content.bytes));
    let headers = response.headers_mut();
    headers.insert(
        axum::http::header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&content_disposition(
            "inline",
            &format!("{}-{page_number}{extension}", book.name),
        ))
        .unwrap(),
    );
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        content_type_header(Some(&page_content.media_type)),
    );
    set_last_modified(&mut response, media);
    response
}

/// Page extraction parameters, bundled for `get_page_internal`
struct PageOptions<'a> {
    convert: Option<&'a str>,
    resize_to: Option<u32>,
    accept: Option<String>,
}

/// `CommonBookController.getBookPageInternal`
async fn get_page_internal(
    state: &AppState,
    user: &KomgaUser,
    headers: &HeaderMap,
    book_id: &str,
    page_number: i32,
    options: PageOptions<'_>,
) -> Result<Response, ApiError> {
    let PageOptions {
        convert,
        resize_to,
        accept,
    } = options;
    let Some(book) = book_dao(state).find_by_id(book_id)? else {
        return Err(ApiError::not_found(""));
    };
    let media = require_media(state, book_id)?;
    if check_not_modified(last_modified_millis(&media), headers) {
        return Ok(not_modified_response(&media));
    }
    restriction::check_book(state, user, &book)?;

    let is_pdf = container::media_profile(media.media_type.as_deref()) == Some(MediaProfile::Pdf);
    if is_pdf && resize_to.is_none() {
        if let Some(accept) = accept.as_deref() {
            let mut accepted = parse_accept(accept);
            if accepted.iter().any(|m| compatible(m, "application", "pdf")) {
                accepted
                    .retain(|m| compatible(m, "application", "pdf") || compatible(m, "image", "*"));
                sort_by_specificity(&mut accepted);
                if let Some(first) = accepted.first() {
                    if compatible(first, "application", "pdf") {
                        let page_content = extract_page_raw(&book, &media, page_number).await?;
                        return Ok(raw_page_response(&book, &media, page_number, page_content));
                    }
                }
            }
        }
    }

    let convert = parse_convert(convert)?;
    let page_content = extract_page(&book, &media, page_number, convert, resize_to).await?;
    Ok(image_page_response(
        &book,
        &media,
        page_number,
        page_content,
    ))
}

async fn extract_page(
    book: &Book,
    media: &Media,
    page_number: i32,
    convert: Option<ImageType>,
    resize_to: Option<u32>,
) -> Result<container::PageContent, ApiError> {
    let path = book_path(book);
    let name = book.name.clone();
    let media = media.clone();
    let number = usize::try_from(page_number).unwrap_or(usize::MAX);
    tokio::task::spawn_blocking(move || {
        container::get_book_page(&path, &name, &media, number, convert, resize_to)
    })
    .await
    .map_err(|e| ApiError::Internal(e.to_string()))?
    .map_err(map_media_error)
}

async fn extract_page_raw(
    book: &Book,
    media: &Media,
    page_number: i32,
) -> Result<container::PageContent, ApiError> {
    let path = book_path(book);
    let media = media.clone();
    let number = usize::try_from(page_number).unwrap_or(usize::MAX);
    tokio::task::spawn_blocking(move || container::get_page_content_raw(&path, &media, number))
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?
        .map_err(map_media_error_raw)
}

async fn get_book_page_by_number(
    State(state): State<AppState>,
    auth: RequireAuth,
    headers: HeaderMap,
    Path((book_id, page_number)): Path<(String, i32)>,
    qp: QueryPageable,
) -> Result<Response, ApiError> {
    auth.0.require_role(UserRole::PageStreaming)?;
    let params = page_params(&qp);
    let page_number = if params.zero_based {
        page_number + 1
    } else {
        page_number
    };
    let accept = if params.content_negotiation {
        headers
            .get(axum::http::header::ACCEPT)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
    } else {
        None
    };
    get_page_internal(
        &state,
        &auth.0.user,
        &headers,
        &book_id,
        page_number,
        PageOptions {
            convert: params.convert.as_deref(),
            resize_to: None,
            accept,
        },
    )
    .await
}

async fn get_book_page_thumbnail_by_number(
    State(state): State<AppState>,
    auth: RequireAuth,
    headers: HeaderMap,
    Path((book_id, page_number)): Path<(String, i32)>,
) -> Result<Response, ApiError> {
    get_page_internal(
        &state,
        &auth.0.user,
        &headers,
        &book_id,
        page_number,
        PageOptions {
            convert: None,
            resize_to: Some(300),
            accept: None,
        },
    )
    .await
}

async fn get_book_page_raw_by_number(
    State(state): State<AppState>,
    auth: RequireAuth,
    headers: HeaderMap,
    Path((book_id, page_number)): Path<(String, i32)>,
) -> Result<Response, ApiError> {
    auth.0.require_role(UserRole::PageStreaming)?;
    let Some(book) = book_dao(&state).find_by_id(&book_id)? else {
        return Err(ApiError::not_found(""));
    };
    let media = require_media(&state, &book_id)?;
    if check_not_modified(last_modified_millis(&media), &headers) {
        return Ok(not_modified_response(&media));
    }
    restriction::check_book(&state, &auth.0.user, &book)?;
    let page_content = extract_page_raw(&book, &media, page_number).await?;
    Ok(raw_page_response(&book, &media, page_number, page_content))
}

// endregion

// region epub resource

async fn get_book_epub_resource(
    State(state): State<AppState>,
    auth: MaybeAuth,
    headers: HeaderMap,
    Path((book_id, resource)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    let resource_name = resource.strip_prefix('/').unwrap_or(&resource).to_string();
    let extension = resource_name
        .rsplit_once('.')
        .map(|(_, e)| e.to_lowercase())
        .unwrap_or_default();
    let is_font = FONT_EXTENSIONS.contains(&extension.as_str());

    if !is_font && auth.0.is_none() {
        // ResponseStatusException(UNAUTHORIZED) with no reason: Spring error JSON, not the
        // filter-level 401 (this path is whitelisted in the security configuration)
        return Err(ApiError::Status {
            status: StatusCode::UNAUTHORIZED,
            message: String::new(),
        });
    }

    let Some(book) = book_dao(&state).find_by_id(&book_id)? else {
        return Err(ApiError::not_found(""));
    };
    let media = require_media(&state, &book.id)?;

    if check_not_modified(last_modified_millis(&media), &headers) {
        let mut response = not_modified_response(&media);
        response.headers_mut().insert(
            axum::http::header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static(CSP_HEADER),
        );
        return Ok(response);
    }

    if container::media_profile(media.media_type.as_deref()) != Some(MediaProfile::Epub) {
        return Err(ApiError::bad_request(format!(
            "Book media type '{}' not compatible with requested profile",
            media.media_type.as_deref().unwrap_or("null")
        )));
    }

    if !is_font {
        restriction::check_book(&state, &auth.0.as_ref().unwrap().user, &book)?;
    }

    let Some(res) = media.files.iter().find(|f| f.file_name == resource_name) else {
        return Err(ApiError::not_found(""));
    };

    let path = book_path(&book);
    let media_clone = media.clone();
    let name = resource_name.clone();
    let bytes = tokio::task::spawn_blocking(move || {
        container::get_file_content(&path, &media_clone, &name)
    })
    .await
    .map_err(|e| ApiError::Internal(e.to_string()))?
    .map_err(|e| match e {
        MediaError::EntryNotFound(_) => ApiError::not_found(""),
        other => map_media_error(other),
    })?;

    let basename = resource_name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(&resource_name)
        .to_string();
    let mut response = Response::new(Body::from(bytes));
    {
        let headers = response.headers_mut();
        headers.insert(
            axum::http::header::CONTENT_DISPOSITION,
            HeaderValue::from_str(&content_disposition("inline", &basename)).unwrap(),
        );
        headers.insert(
            axum::http::header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static(CSP_HEADER),
        );
        headers.insert(
            axum::http::header::CONTENT_TYPE,
            content_type_header(res.media_type.as_deref()),
        );
    }
    set_last_modified(&mut response, &media);
    Ok(response)
}

// endregion

// region file download

async fn download_book_file(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path(book_id): Path<String>,
) -> Result<Response, ApiError> {
    download_book_file_internal(state, auth, book_id).await
}

/// `/file/*` variant: the trailing path is ignored, like the Spring mapping
async fn download_book_file_wildcard(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path((book_id, _)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    download_book_file_internal(state, auth, book_id).await
}

async fn download_book_file_internal(
    state: AppState,
    auth: RequireAuth,
    book_id: String,
) -> Result<Response, ApiError> {
    auth.0.require_role(UserRole::FileDownload)?;
    let Some(book) = book_dao(&state).find_by_id(&book_id)? else {
        return Err(ApiError::not_found(""));
    };
    restriction::check_book(&state, &auth.0.user, &book)?;
    let media = require_media(&state, &book.id)?;
    let path = book_path(&book);
    let file = match tokio::fs::File::open(&path).await {
        Ok(file) => file,
        Err(_) => {
            tracing::warn!("File not found: {}", path.display());
            return Err(ApiError::not_found("File not found, it may have moved"));
        }
    };
    let length = file
        .metadata()
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?
        .len();
    let filename = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();

    let mut response = Response::new(Body::from_stream(tokio_util::io::ReaderStream::new(file)));
    {
        let headers = response.headers_mut();
        headers.insert(
            axum::http::header::CONTENT_DISPOSITION,
            HeaderValue::from_str(&content_disposition("attachment", &filename)).unwrap(),
        );
        headers.insert(
            axum::http::header::CONTENT_TYPE,
            content_type_header(media.media_type.as_deref()),
        );
        headers.insert(
            axum::http::header::CONTENT_LENGTH,
            HeaderValue::from_str(&length.to_string()).unwrap(),
        );
    }
    Ok(response)
}

// endregion

// region progression

async fn get_book_progression(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path(book_id): Path<String>,
) -> Result<Response, ApiError> {
    let Some(book) = book_dao(&state).find_by_id(&book_id)? else {
        return Err(ApiError::not_found(""));
    };
    restriction::check_book(&state, &auth.0.user, &book)?;
    let Some(progress) =
        read_progress_dao(&state).find_by_book_and_user(&book_id, &auth.0.user.id)?
    else {
        return Ok(StatusCode::NO_CONTENT.into_response());
    };
    let mut response = Json(R2Progression::from(&progress)).into_response();
    response.headers_mut().insert(
        axum::http::header::CONTENT_TYPE,
        HeaderValue::from_static(MEDIATYPE_PROGRESSION_JSON),
    );
    Ok(response)
}

/// `BookLifecycle.markProgression`
async fn update_book_progression(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path(book_id): Path<String>,
    Json(progression): Json<R2Progression>,
) -> Result<StatusCode, ApiError> {
    let Some(book) = book_dao(&state).find_by_id(&book_id)? else {
        return Err(ApiError::not_found(""));
    };
    restriction::check_book(&state, &auth.0.user, &book)?;
    mark_progression(&state, &auth.0.user, &book, &progression)?;
    Ok(StatusCode::NO_CONTENT)
}

/// `URLDecoder.decode(s, UTF_8)`: percent-escapes to bytes (UTF-8 lossy), `+` to space
fn url_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = |b: u8| (b as char).to_digit(16);
                match (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                    (Some(h), Some(l)) => {
                        out.push((h * 16 + l) as u8);
                        i += 3;
                    }
                    _ => {
                        out.push(bytes[i]);
                        i += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

struct EpubExtension {
    positions: Vec<R2Locator>,
    is_fixed_layout: bool,
}

/// `mediaRepository.findExtensionByIdOrNull(bookId) as? MediaExtensionEpub`
fn decode_epub_extension(media: &Media) -> Result<EpubExtension, ApiError> {
    let not_found = || ApiError::bad_request("Epub extension not found");
    let Some(blob) = &media.extension_value else {
        return Err(not_found());
    };
    let mut json = vec![];
    if flate2::read::GzDecoder::new(blob.as_slice())
        .read_to_end(&mut json)
        .is_err()
    {
        return Err(not_found());
    }
    let value: serde_json::Value = serde_json::from_slice(&json).map_err(|_| not_found())?;
    let positions = value
        .get("positions")
        .cloned()
        .and_then(|v| serde_json::from_value(v).ok())
        .ok_or_else(not_found)?;
    let is_fixed_layout = value
        .get("isFixedLayout")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    Ok(EpubExtension {
        positions,
        is_fixed_layout,
    })
}

fn mark_progression(
    state: &AppState,
    user: &KomgaUser,
    book: &Book,
    new: &R2Progression,
) -> Result<(), ApiError> {
    let dao = read_progress_dao(state);
    if let Some(saved) = dao.find_by_book_and_user(&book.id, &user.id)? {
        if new.modified <= saved.read_date {
            return Err(ApiError::conflict("Progression is older than existing"));
        }
    }

    let media = require_media(state, &book.id)?;
    let Some(profile) = container::media_profile(media.media_type.as_deref()) else {
        return Err(ApiError::bad_request("Media has no profile"));
    };
    // Java converts to the system-default zone before storing; komga runs in UTC
    let read_date = new.modified.to_offset(time::UtcOffset::UTC);

    let progress = match profile {
        MediaProfile::Divina | MediaProfile::Pdf => {
            let position = new.locator.locations.as_ref().and_then(|l| l.position);
            let in_range = position.is_some_and(|p| p >= 1 && p <= media.page_count);
            if !in_range {
                return Err(ApiError::bad_request(format!(
                    "Page argument ({}) must be within 1 and book page count ({})",
                    position
                        .map(|p| p.to_string())
                        .unwrap_or_else(|| "null".into()),
                    media.page_count
                )));
            }
            let position = position.unwrap();
            ReadProgress {
                book_id: book.id.clone(),
                user_id: user.id.clone(),
                page: position,
                completed: position == media.page_count,
                read_date,
                device_id: new.device.id.clone(),
                device_name: new.device.name.clone(),
                locator: Some(serde_json::to_value(&new.locator).unwrap()),
                created_date: time_codec::now_utc(),
                last_modified_date: time_codec::now_utc(),
            }
        }
        MediaProfile::Epub => {
            let href = {
                let raw = &new.locator.href;
                let stripped = match raw.find('#') {
                    Some(index) => &raw[..index],
                    None => raw.as_str(),
                };
                url_decode(stripped)
            };
            if !media.files.iter().any(|f| f.file_name == href) {
                return Err(ApiError::bad_request(format!(
                    "Resource does not exist in book: {href}"
                )));
            }
            let Some(new_progression) = new.locator.locations.as_ref().and_then(|l| l.progression)
            else {
                return Err(ApiError::bad_request("location.progression is required"));
            };

            let extension = decode_epub_extension(&media)?;
            let matching: Vec<&R2Locator> = extension
                .positions
                .iter()
                .filter(|p| p.href == href)
                .collect();
            let matched = if extension.is_fixed_layout && matching.len() == 1 {
                matching[0]
            } else {
                match matching.iter().find(|p| {
                    p.locations.as_ref().and_then(|l| l.progression) == Some(new_progression)
                }) {
                    Some(m) => m,
                    None => {
                        let before = matching
                            .iter()
                            .filter(|p| {
                                p.locations
                                    .as_ref()
                                    .and_then(|l| l.progression)
                                    .is_some_and(|prog| prog < new_progression)
                            })
                            .max_by_key(|p| {
                                p.locations.as_ref().and_then(|l| l.position).unwrap_or(0)
                            });
                        let after = matching
                            .iter()
                            .filter(|p| {
                                p.locations
                                    .as_ref()
                                    .and_then(|l| l.progression)
                                    .is_some_and(|prog| prog > new_progression)
                            })
                            .min_by_key(|p| {
                                p.locations.as_ref().and_then(|l| l.position).unwrap_or(0)
                            });
                        let (Some(before), Some(after)) = (before, after) else {
                            return Err(ApiError::bad_request("Invalid progression"));
                        };
                        let before_pos = before
                            .locations
                            .as_ref()
                            .and_then(|l| l.position)
                            .expect("position");
                        let after_pos = after
                            .locations
                            .as_ref()
                            .and_then(|l| l.position)
                            .expect("position");
                        if before_pos > after_pos {
                            return Err(ApiError::bad_request("Invalid progression"));
                        }
                        before
                    }
                }
            };

            let total_progression = matched.locations.as_ref().and_then(|l| l.total_progression);
            let page = total_progression
                .map(|tp| (media.page_count as f32 * tp + 0.5).floor() as i32)
                .unwrap_or(0);
            let completed = total_progression.is_some_and(|tp| tp >= 0.99);
            let locator = {
                let mut value = serde_json::to_value(&new.locator).unwrap();
                value["type"] = serde_json::Value::String(matched.type_.clone());
                if value.get("koboSpan").is_none_or(|v| v.is_null()) {
                    if let Some(kobo_span) = &matched.kobo_span {
                        value["koboSpan"] = serde_json::Value::String(kobo_span.clone());
                    }
                }
                if let Some(locations) = value.get_mut("locations") {
                    match total_progression {
                        Some(tp) => {
                            locations["totalProgression"] = serde_json::Value::from(tp);
                        }
                        None => {
                            if let Some(map) = locations.as_object_mut() {
                                map.remove("totalProgression");
                            }
                        }
                    }
                }
                value
            };
            ReadProgress {
                book_id: book.id.clone(),
                user_id: user.id.clone(),
                page,
                completed,
                read_date,
                device_id: new.device.id.clone(),
                device_name: new.device.name.clone(),
                locator: Some(locator),
                created_date: time_codec::now_utc(),
                last_modified_date: time_codec::now_utc(),
            }
        }
    };

    dao.insert_or_update(&progress)?;
    Ok(())
}

// endregion

// region read progress

/// `BookLifecycle.markReadProgress`
fn mark_read_progress(
    state: &AppState,
    user: &KomgaUser,
    book: &Book,
    page: i32,
) -> Result<(), ApiError> {
    let media = require_media(state, &book.id)?;
    if !(1..=media.page_count).contains(&page) {
        return Err(ApiError::bad_request(format!(
            "Page argument ({page}) must be within 1 and book page count ({})",
            media.page_count
        )));
    }

    let locator =
        if container::media_profile(media.media_type.as_deref()) == Some(MediaProfile::Epub) {
            if !media.epub_divina_compatible {
                return Err(ApiError::bad_request("epub book is not Divina compatible"));
            }
            let extension = decode_epub_extension(&media)?;
            let position = extension
                .positions
                .get((page - 1) as usize)
                .ok_or_else(|| ApiError::Internal(format!("no position for page {page}")))?;
            Some(serde_json::to_value(position).unwrap())
        } else {
            None
        };

    let progress = ReadProgress {
        book_id: book.id.clone(),
        user_id: user.id.clone(),
        page,
        completed: page == media.page_count,
        read_date: time_codec::now_utc(),
        device_id: String::new(),
        device_name: String::new(),
        locator,
        created_date: time_codec::now_utc(),
        last_modified_date: time_codec::now_utc(),
    };
    read_progress_dao(state).insert_or_update(&progress)?;
    Ok(())
}

/// `BookLifecycle.markReadProgressCompleted`
fn mark_read_progress_completed(
    state: &AppState,
    user: &KomgaUser,
    book_id: &str,
) -> Result<(), ApiError> {
    let media = require_media(state, book_id)?;
    let progress = ReadProgress {
        book_id: book_id.to_string(),
        user_id: user.id.clone(),
        page: media.page_count,
        completed: true,
        read_date: time_codec::now_utc(),
        device_id: String::new(),
        device_name: String::new(),
        locator: None,
        created_date: time_codec::now_utc(),
        last_modified_date: time_codec::now_utc(),
    };
    read_progress_dao(state).insert_or_update(&progress)?;
    Ok(())
}

async fn mark_book_read_progress(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path(book_id): Path<String>,
    Json(body): Json<ReadProgressUpdateDto>,
) -> Result<StatusCode, ApiError> {
    // the class-level constraint is a global error in Spring, which lands in `violations: []`
    if !body.is_valid() {
        return Err(ApiError::Violations(vec![]));
    }
    if let Some(page) = body.page {
        if page <= 0 {
            return Err(ApiError::Violations(vec![Violation {
                field_name: "page".into(),
                message: "must be greater than 0".into(),
            }]));
        }
    }
    let Some(book) = book_dao(&state).find_by_id(&book_id)? else {
        return Err(ApiError::not_found(""));
    };
    restriction::check_book(&state, &auth.0.user, &book)?;
    if body.completed == Some(true) {
        mark_read_progress_completed(&state, &auth.0.user, &book.id)?;
    } else {
        mark_read_progress(&state, &auth.0.user, &book, body.page.unwrap())?;
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_book_read_progress(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path(book_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let Some(book) = book_dao(&state).find_by_id(&book_id)? else {
        return Err(ApiError::not_found(""));
    };
    restriction::check_book(&state, &auth.0.user, &book)?;
    let dao = read_progress_dao(&state);
    if dao
        .find_by_book_and_user(&book.id, &auth.0.user.id)?
        .is_some()
    {
        dao.delete(&book.id, &auth.0.user.id)?;
    }
    Ok(StatusCode::NO_CONTENT)
}

// endregion

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::SessionStore;
    use crate::settings::SettingsProvider;
    use axum::middleware;
    use komga_core::model::book::BookMetadata;
    use komga_core::model::library::Library;
    use komga_core::model::media::BookPage;
    use komga_core::model::series::{Series, SeriesMetadata, SeriesStatus};
    use komga_core::model::thumbnail::{Dimension, ThumbnailType};
    use komga_core::model::user::{ContentRestrictions, KomgaUser};
    use komga_core::tsid::TsidFactory;
    use komga_db::dao::book::BookMetadataDao;
    use komga_db::dao::library::LibraryDao;
    use komga_db::dao::series::{SeriesDao, SeriesMetadataDao};
    use komga_db::dao::user::UserDao;
    use komga_db::pool::Database;
    use komga_db::{Migrator, Placeholders};
    use std::sync::Arc;
    use tower::ServiceExt;

    const FIXTURE_ZIP: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../komga/komga/src/test/resources/archives/zip.zip"
    );

    const PNG_1X1: &[u8] = &[
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6,
        0, 0, 0, 31, 21, 196, 137, 0, 0, 0, 10, 73, 68, 65, 84, 120, 156, 99, 0, 1, 0, 0, 5, 0, 1,
        13, 10, 45, 180, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
    ];

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
                crate::auth::auth_middleware,
            ))
            .with_state(state)
    }

    fn test_state_with_settings() -> AppState {
        let db = Database::open_in_memory(true).unwrap();
        let migrations = komga_db::main_migrations();
        Migrator::new(&migrations, Placeholders::default())
            .migrate(&db.rw())
            .unwrap();
        let tasks_db = Database::open_in_memory(false).unwrap();
        let config = crate::config::ServerConfig::from_env();
        AppState {
            config: Arc::new(config.clone()),
            settings: Arc::new(SettingsProvider::load(db.clone())),
            db,
            tasks_db,
            sessions: SessionStore::new(config.session_timeout),
            tsid: Arc::new(TsidFactory::new_random_node()),
            events: crate::events::event_bus(),
        }
    }

    fn seed_library(db: &Database, id: &str) {
        let now = time_codec::now_utc();
        let library = Library {
            id: id.to_string(),
            name: format!("Library {id}"),
            root: format!("file:/data/{id}/"),
            import_comicinfo_book: true,
            import_comicinfo_series: true,
            import_comicinfo_collection: true,
            import_comicinfo_readlist: true,
            import_comicinfo_series_append_volume: true,
            import_epub_book: true,
            import_epub_series: true,
            import_mylar_series: true,
            import_local_artwork: true,
            import_barcode_isbn: true,
            scan_force_modified_time: false,
            scan_on_startup: false,
            scan_interval: komga_core::model::library::ScanInterval::Daily,
            scan_cbx: true,
            scan_pdf: true,
            scan_epub: true,
            scan_directory_exclusions: vec![],
            repair_extensions: false,
            convert_to_cbz: false,
            empty_trash_after_scan: false,
            series_cover: komga_core::model::library::SeriesCover::First,
            hash_files: true,
            hash_pages: false,
            hash_koreader: false,
            analyze_dimensions: true,
            oneshots_directory: None,
            unavailable_date: None,
            created_date: now,
            last_modified_date: now,
        };
        LibraryDao::new(db.clone()).insert(&library).unwrap();
    }

    fn seed_series(db: &Database, id: &str, library_id: &str) {
        let now = time_codec::now_utc();
        let series = Series {
            id: id.to_string(),
            name: format!("Series {id}"),
            url: format!("file:/data/{id}/"),
            file_last_modified: now,
            library_id: library_id.to_string(),
            book_count: 0,
            deleted_date: None,
            oneshot: false,
            created_date: now,
            last_modified_date: now,
        };
        SeriesDao::new(db.clone()).insert(&series).unwrap();
        SeriesMetadataDao::new(db.clone())
            .insert(&SeriesMetadata {
                series_id: id.to_string(),
                status: SeriesStatus::Ongoing,
                title: format!("Series {id}"),
                title_sort: format!("Series {id}"),
                summary: String::new(),
                reading_direction: None,
                publisher: String::new(),
                age_rating: None,
                language: String::new(),
                genres: Default::default(),
                tags: Default::default(),
                total_book_count: None,
                sharing_labels: Default::default(),
                links: vec![],
                alternate_titles: vec![],
                status_lock: false,
                title_lock: false,
                title_sort_lock: false,
                summary_lock: false,
                reading_direction_lock: false,
                publisher_lock: false,
                age_rating_lock: false,
                language_lock: false,
                genres_lock: false,
                tags_lock: false,
                total_book_count_lock: false,
                sharing_labels_lock: false,
                links_lock: false,
                alternate_titles_lock: false,
                created_date: now,
                last_modified_date: now,
            })
            .unwrap();
        // the DTO queries expect the aggregation row to exist (komga invariant)
        komga_db::dao::series::BookMetadataAggregationDao::new(db.clone())
            .insert(&komga_core::model::series::BookMetadataAggregation {
                series_id: id.to_string(),
                authors: vec![],
                tags: Default::default(),
                release_date: None,
                summary: String::new(),
                summary_number: String::new(),
                created_date: now,
                last_modified_date: now,
            })
            .unwrap();
    }

    fn set_series_book_count(db: &Database, series_id: &str, count: i32) {
        db.rw()
            .execute(
                "UPDATE SERIES SET BOOK_COUNT = ? WHERE ID = ?",
                rusqlite::params![count, series_id],
            )
            .unwrap();
    }

    fn seed_book(db: &Database, id: &str, series_id: &str, library_id: &str, url: &str) {
        seed_book_with(db, id, series_id, library_id, url, 1.0);
    }

    fn seed_book_with(
        db: &Database,
        id: &str,
        series_id: &str,
        library_id: &str,
        url: &str,
        number_sort: f32,
    ) {
        let now = time_codec::now_utc();
        let book = Book {
            id: id.to_string(),
            name: format!("Book {id}"),
            url: url.to_string(),
            file_last_modified: now,
            series_id: series_id.to_string(),
            library_id: library_id.to_string(),
            file_size: 1000,
            number: 1,
            file_hash: String::new(),
            file_hash_koreader: String::new(),
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
                number_sort,
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
        // komga invariant: every book has a MEDIA row; queries LEFT JOIN it and map non-null
        upsert_media(db, id, MediaStatus::Ready, detect::APPLICATION_ZIP, vec![]);
    }

    fn upsert_media(
        db: &Database,
        book_id: &str,
        status: MediaStatus,
        media_type: &str,
        pages: Vec<BookPage>,
    ) {
        let now = time_codec::now_utc();
        let media = Media {
            book_id: book_id.to_string(),
            status,
            media_type: Some(media_type.to_string()),
            comment: None,
            page_count: pages.len() as i32,
            pages,
            files: vec![],
            extension_class: None,
            extension_value: None,
            epub_divina_compatible: false,
            epub_is_kepub: false,
            created_date: now,
            last_modified_date: now,
        };
        let dao = MediaDao::new(db.clone());
        if dao.find_by_id(book_id).unwrap().is_some() {
            dao.update(&media).unwrap();
        } else {
            dao.insert(&media).unwrap();
        }
    }

    fn seed_media(
        db: &Database,
        book_id: &str,
        status: MediaStatus,
        media_type: &str,
        pages: Vec<BookPage>,
    ) {
        upsert_media(db, book_id, status, media_type, pages)
    }

    fn zip_pages() -> Vec<BookPage> {
        vec![BookPage {
            file_name: "komga.png".into(),
            media_type: "image/png".into(),
            width: Some(48),
            height: Some(48),
            file_hash: String::new(),
            file_size: Some(3108),
        }]
    }

    fn seed_user(db: &Database, email: &str, password: &str, admin: bool) -> String {
        let mut roles = BTreeSet::new();
        if admin {
            roles.insert(UserRole::Admin);
        }
        let user = KomgaUser {
            id: String::new(),
            email: email.to_string(),
            password: bcrypt::hash(password, 4).unwrap(),
            roles,
            shared_libraries_ids: BTreeSet::new(),
            shared_all_libraries: true,
            restrictions: ContentRestrictions::default(),
            created_date: time_codec::now_utc(),
            last_modified_date: time_codec::now_utc(),
        };
        UserDao::new(db.clone()).insert(&user).unwrap()
    }

    fn basic(email: &str, password: &str) -> String {
        use base64::Engine;
        format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(format!("{email}:{password}"))
        )
    }

    async fn call(
        app: &Router,
        method: &str,
        uri: &str,
        auth: Option<&str>,
        body: Option<String>,
    ) -> (StatusCode, HeaderMap, Vec<u8>) {
        let mut builder = axum::http::Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json");
        if let Some(auth) = auth {
            builder = builder.header("authorization", auth);
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

    fn seed_readlist(db: &Database, id: &str, book_ids: &[&str]) {
        let conn = db.rw();
        conn.execute(
            "INSERT INTO READLIST (ID, NAME, SUMMARY, ORDERED, BOOK_COUNT) VALUES (?, ?, '', 1, ?)",
            rusqlite::params![id, format!("ReadList {id}"), book_ids.len() as i64],
        )
        .unwrap();
        for (index, book_id) in book_ids.iter().enumerate() {
            conn.execute(
                "INSERT INTO READLIST_BOOK (READLIST_ID, BOOK_ID, NUMBER) VALUES (?, ?, ?)",
                rusqlite::params![id, book_id, index as i64],
            )
            .unwrap();
        }
    }

    fn seed_thumbnail(db: &Database, id: &str, book_id: &str, selected: bool) {
        let now = time_codec::now_utc();
        thumbnail_dao_for(db)
            .insert(&ThumbnailBook {
                id: id.to_string(),
                book_id: book_id.to_string(),
                thumbnail: Some(PNG_1X1.to_vec()),
                url: None,
                selected,
                type_: ThumbnailType::Generated,
                media_type: "image/jpeg".into(),
                file_size: PNG_1X1.len() as i64,
                dimension: Dimension {
                    width: 1,
                    height: 1,
                },
                created_date: now,
                last_modified_date: now,
            })
            .unwrap();
    }

    fn thumbnail_dao_for(db: &Database) -> ThumbnailBookDao {
        ThumbnailBookDao::new(db.clone())
    }

    // region list/detail

    #[tokio::test]
    async fn books_list_pagination_and_restriction() {
        let state = test_state_with_settings();
        let db = state.db.clone();
        seed_library(&db, "l1");
        seed_library(&db, "l2");
        seed_series(&db, "s1", "l1");
        seed_series(&db, "s2", "l2");
        seed_book(&db, "b1", "s1", "l1", "file:/data/b1.cbz");
        seed_book(&db, "b2", "s1", "l1", "file:/data/b2.cbz");
        seed_book(&db, "b3", "s2", "l2", "file:/data/b3.cbz");
        seed_user(&db, "admin@example.org", "pw", true);
        seed_user(&db, "user@example.org", "pw", false);
        let app = test_router(state);

        // unpaged list: all 3 books
        let (status, _, body) = call(
            &app,
            "GET",
            "/api/v1/books?unpaged=true",
            Some(&basic("admin@example.org", "pw")),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let page = json(&body);
        assert_eq!(page["totalElements"], 3);
        assert_eq!(page["content"].as_array().unwrap().len(), 3);
        assert_eq!(page["pageable"]["paged"], true);

        // filter by library
        let (status, _, body) = call(
            &app,
            "GET",
            "/api/v1/books?library_id=l1",
            Some(&basic("admin@example.org", "pw")),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json(&body)["totalElements"], 2);

        // non-admin gets the url restricted to the file name
        let (status, _, body) = call(
            &app,
            "GET",
            "/api/v1/books/b1",
            Some(&basic("user@example.org", "pw")),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json(&body)["url"], "b1.cbz");

        // admin sees the full path
        let (status, _, body) = call(
            &app,
            "GET",
            "/api/v1/books/b1",
            Some(&basic("admin@example.org", "pw")),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json(&body)["url"], "/data/b1.cbz");

        // POST /list with a condition
        let (status, _, body) = call(
            &app,
            "POST",
            "/api/v1/books/list",
            Some(&basic("admin@example.org", "pw")),
            Some(r#"{"condition":{"seriesId":{"operator":"is","value":"s2"}}}"#.into()),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let page = json(&body);
        assert_eq!(page["totalElements"], 1);
        assert_eq!(page["content"][0]["id"], "b3");
    }

    #[tokio::test]
    async fn book_detail_not_found_empty_body() {
        let state = test_state_with_settings();
        seed_user(&state.db, "admin@example.org", "pw", true);
        let app = test_router(state);
        let (status, _, body) = call(
            &app,
            "GET",
            "/api/v1/books/nope",
            Some(&basic("admin@example.org", "pw")),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(body.is_empty());
    }

    #[tokio::test]
    async fn book_siblings() {
        let state = test_state_with_settings();
        let db = state.db.clone();
        seed_library(&db, "l1");
        seed_series(&db, "s1", "l1");
        seed_book_with(&db, "b1", "s1", "l1", "file:/data/b1.cbz", 1.0);
        seed_book_with(&db, "b2", "s1", "l1", "file:/data/b2.cbz", 2.0);
        seed_user(&db, "admin@example.org", "pw", true);
        let app = test_router(state);

        let (status, _, body) = call(
            &app,
            "GET",
            "/api/v1/books/b1/next",
            Some(&basic("admin@example.org", "pw")),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json(&body)["id"], "b2");

        let (status, _, _) = call(
            &app,
            "GET",
            "/api/v1/books/b1/previous",
            Some(&basic("admin@example.org", "pw")),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn books_latest_and_duplicates() {
        let state = test_state_with_settings();
        let db = state.db.clone();
        seed_library(&db, "l1");
        seed_series(&db, "s1", "l1");
        seed_book(&db, "b1", "s1", "l1", "file:/data/b1.cbz");
        seed_book(&db, "b2", "s1", "l1", "file:/data/b2.cbz");
        db.rw()
            .execute(
                "UPDATE BOOK SET LAST_MODIFIED_DATE = '2020-01-01 00:00:00.0' WHERE ID = 'b1'",
                [],
            )
            .unwrap();
        db.rw()
            .execute(
                "UPDATE BOOK SET LAST_MODIFIED_DATE = '2021-01-01 00:00:00.0' WHERE ID = 'b2'",
                [],
            )
            .unwrap();
        db.rw()
            .execute(
                "UPDATE BOOK SET FILE_HASH = 'dup', FILE_SIZE = 42 WHERE ID IN ('b1', 'b2')",
                [],
            )
            .unwrap();
        seed_user(&db, "admin@example.org", "pw", true);
        let app = test_router(state);

        let (status, _, body) = call(
            &app,
            "GET",
            "/api/v1/books/latest",
            Some(&basic("admin@example.org", "pw")),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let page = json(&body);
        assert_eq!(page["content"][0]["id"], "b2");

        let (status, _, body) = call(
            &app,
            "GET",
            "/api/v1/books/duplicates",
            Some(&basic("admin@example.org", "pw")),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json(&body)["totalElements"], 2);
    }

    #[tokio::test]
    async fn books_on_deck() {
        let state = test_state_with_settings();
        let db = state.db.clone();
        seed_library(&db, "l1");
        seed_series(&db, "s1", "l1");
        set_series_book_count(&db, "s1", 2);
        seed_book_with(&db, "b1", "s1", "l1", "file:/data/b1.cbz", 1.0);
        seed_book_with(&db, "b2", "s1", "l1", "file:/data/b2.cbz", 2.0);
        // series fully unread: not on deck
        seed_series(&db, "s2", "l1");
        set_series_book_count(&db, "s2", 1);
        seed_book_with(&db, "b3", "s2", "l1", "file:/data/b3.cbz", 1.0);
        let user_id = seed_user(&db, "admin@example.org", "pw", true);
        // s1: b1 read -> b2 is on deck
        read_progress_dao_for(&db)
            .insert_or_update(&ReadProgress {
                book_id: "b1".into(),
                user_id: user_id.clone(),
                page: 10,
                completed: true,
                read_date: time_codec::now_utc(),
                device_id: String::new(),
                device_name: String::new(),
                locator: None,
                created_date: time_codec::now_utc(),
                last_modified_date: time_codec::now_utc(),
            })
            .unwrap();
        let app = test_router(state);

        let (status, _, body) = call(
            &app,
            "GET",
            "/api/v1/books/ondeck",
            Some(&basic("admin@example.org", "pw")),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let page = json(&body);
        assert_eq!(page["totalElements"], 1);
        assert_eq!(page["content"][0]["id"], "b2");
    }

    fn read_progress_dao_for(db: &Database) -> ReadProgressDao {
        ReadProgressDao::new(db.clone())
    }

    #[tokio::test]
    async fn book_readlists() {
        let state = test_state_with_settings();
        let db = state.db.clone();
        seed_library(&db, "l1");
        seed_series(&db, "s1", "l1");
        seed_book(&db, "b1", "s1", "l1", "file:/data/b1.cbz");
        seed_readlist(&db, "r1", &["b1"]);
        seed_user(&db, "admin@example.org", "pw", true);
        let app = test_router(state);

        let (status, _, body) = call(
            &app,
            "GET",
            "/api/v1/books/b1/readlists",
            Some(&basic("admin@example.org", "pw")),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let list = json(&body);
        assert_eq!(list.as_array().unwrap().len(), 1);
        assert_eq!(list[0]["id"], "r1");
    }

    // endregion

    // region thumbnails

    #[tokio::test]
    async fn book_thumbnail_selected_and_missing() {
        let state = test_state_with_settings();
        let db = state.db.clone();
        seed_library(&db, "l1");
        seed_series(&db, "s1", "l1");
        seed_book(&db, "b1", "s1", "l1", "file:/data/b1.cbz");
        seed_book(&db, "b2", "s1", "l1", "file:/data/b2.cbz");
        seed_thumbnail(&db, "t1", "b1", true);
        seed_user(&db, "admin@example.org", "pw", true);
        let app = test_router(state);

        let (status, headers, body) = call(
            &app,
            "GET",
            "/api/v1/books/b1/thumbnail",
            Some(&basic("admin@example.org", "pw")),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers["content-type"], "image/jpeg");
        assert_eq!(body, PNG_1X1);

        // no thumbnail: 404
        let (status, _, _) = call(
            &app,
            "GET",
            "/api/v1/books/b2/thumbnail",
            Some(&basic("admin@example.org", "pw")),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        // list + by id
        let (status, _, body) = call(
            &app,
            "GET",
            "/api/v1/books/b1/thumbnails",
            Some(&basic("admin@example.org", "pw")),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let list = json(&body);
        assert_eq!(list.as_array().unwrap().len(), 1);
        assert_eq!(list[0]["id"], "t1");
        assert_eq!(list[0]["selected"], true);

        let (status, _, body) = call(
            &app,
            "GET",
            "/api/v1/books/b1/thumbnails/t1",
            Some(&basic("admin@example.org", "pw")),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, PNG_1X1);

        let (status, _, _) = call(
            &app,
            "GET",
            "/api/v1/books/b1/thumbnails/nope",
            Some(&basic("admin@example.org", "pw")),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    // endregion

    // region pages

    #[tokio::test]
    async fn book_pages_and_status_branches() {
        let state = test_state_with_settings();
        let db = state.db.clone();
        seed_library(&db, "l1");
        seed_series(&db, "s1", "l1");
        seed_book(&db, "b1", "s1", "l1", &format!("file:{FIXTURE_ZIP}"));
        seed_book(&db, "b2", "s1", "l1", "file:/data/b2.cbz");
        seed_media(
            &db,
            "b1",
            MediaStatus::Ready,
            detect::APPLICATION_ZIP,
            zip_pages(),
        );
        seed_media(
            &db,
            "b2",
            MediaStatus::Unknown,
            detect::APPLICATION_ZIP,
            vec![],
        );
        seed_user(&db, "admin@example.org", "pw", true);
        let app = test_router(state);

        let (status, _, body) = call(
            &app,
            "GET",
            "/api/v1/books/b1/pages",
            Some(&basic("admin@example.org", "pw")),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let pages = json(&body);
        assert_eq!(pages.as_array().unwrap().len(), 1);
        assert_eq!(pages[0]["number"], 1);
        assert_eq!(pages[0]["fileName"], "komga.png");
        assert_eq!(pages[0]["width"], 48);

        let (status, _, body) = call(
            &app,
            "GET",
            "/api/v1/books/b2/pages",
            Some(&basic("admin@example.org", "pw")),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(
            json(&body)["message"],
            "404 NOT_FOUND \"Book has not been analyzed yet\""
        );
    }

    #[tokio::test]
    async fn book_page_stream() {
        let state = test_state_with_settings();
        let db = state.db.clone();
        seed_library(&db, "l1");
        seed_series(&db, "s1", "l1");
        seed_book(&db, "b1", "s1", "l1", &format!("file:{FIXTURE_ZIP}"));
        seed_media(
            &db,
            "b1",
            MediaStatus::Ready,
            detect::APPLICATION_ZIP,
            zip_pages(),
        );
        seed_user(&db, "admin@example.org", "pw", true);
        let app = test_router(state);

        let (status, headers, body) = call(
            &app,
            "GET",
            "/api/v1/books/b1/pages/1",
            Some(&basic("admin@example.org", "pw")),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers["content-type"], "image/png");
        assert_eq!(&body[0..4], b"\x89PNG");
        let disposition = headers["content-disposition"].to_str().unwrap().to_string();
        assert!(
            disposition.starts_with("inline; filename*=UTF-8''"),
            "{disposition}"
        );
        assert!(headers.contains_key("last-modified"));
        let last_modified = headers["last-modified"].to_str().unwrap().to_string();

        // conditional request -> 304
        let builder = axum::http::Request::builder()
            .method("GET")
            .uri("/api/v1/books/b1/pages/1")
            .header("authorization", basic("admin@example.org", "pw"))
            .header("if-modified-since", &last_modified);
        let request = builder.body(Body::empty()).unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_MODIFIED);

        // zero_based: page 0 == page 1
        let (status, _, _) = call(
            &app,
            "GET",
            "/api/v1/books/b1/pages/0?zero_based=true",
            Some(&basic("admin@example.org", "pw")),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        // out of bounds
        let (status, _, body) = call(
            &app,
            "GET",
            "/api/v1/books/b1/pages/2",
            Some(&basic("admin@example.org", "pw")),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(
            json(&body)["message"],
            "400 BAD_REQUEST \"Page number does not exist\""
        );

        // invalid convert
        let (status, _, body) = call(
            &app,
            "GET",
            "/api/v1/books/b1/pages/1?convert=bmp",
            Some(&basic("admin@example.org", "pw")),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(
            json(&body)["message"],
            "400 BAD_REQUEST \"Invalid conversion format: bmp\""
        );

        // convert to jpeg
        let (status, headers, body) = call(
            &app,
            "GET",
            "/api/v1/books/b1/pages/1?convert=jpeg",
            Some(&basic("admin@example.org", "pw")),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers["content-type"], "image/jpeg");
        assert_eq!(&body[0..3], b"\xFF\xD8\xFF");
    }

    #[tokio::test]
    async fn book_page_requires_page_streaming_role() {
        let state = test_state_with_settings();
        let db = state.db.clone();
        seed_library(&db, "l1");
        seed_series(&db, "s1", "l1");
        seed_book(&db, "b1", "s1", "l1", &format!("file:{FIXTURE_ZIP}"));
        seed_media(
            &db,
            "b1",
            MediaStatus::Ready,
            detect::APPLICATION_ZIP,
            zip_pages(),
        );
        seed_user(&db, "user@example.org", "pw", false);
        let app = test_router(state);

        let (status, _, _) = call(
            &app,
            "GET",
            "/api/v1/books/b1/pages/1",
            Some(&basic("user@example.org", "pw")),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn book_page_thumbnail_resizes() {
        let state = test_state_with_settings();
        let db = state.db.clone();
        seed_library(&db, "l1");
        seed_series(&db, "s1", "l1");
        seed_book(&db, "b1", "s1", "l1", &format!("file:{FIXTURE_ZIP}"));
        seed_media(
            &db,
            "b1",
            MediaStatus::Ready,
            detect::APPLICATION_ZIP,
            zip_pages(),
        );
        seed_user(&db, "admin@example.org", "pw", true);
        let app = test_router(state);

        let (status, headers, body) = call(
            &app,
            "GET",
            "/api/v1/books/b1/pages/1/thumbnail",
            Some(&basic("admin@example.org", "pw")),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers["content-type"], "image/jpeg");
        assert_eq!(&body[0..3], b"\xFF\xD8\xFF");
    }

    #[tokio::test]
    async fn book_page_raw_rejects_non_pdf() {
        let state = test_state_with_settings();
        let db = state.db.clone();
        seed_library(&db, "l1");
        seed_series(&db, "s1", "l1");
        seed_book(&db, "b1", "s1", "l1", &format!("file:{FIXTURE_ZIP}"));
        seed_media(
            &db,
            "b1",
            MediaStatus::Ready,
            detect::APPLICATION_ZIP,
            zip_pages(),
        );
        seed_user(&db, "admin@example.org", "pw", true);
        let app = test_router(state);

        let (status, _, body) = call(
            &app,
            "GET",
            "/api/v1/books/b1/pages/1/raw",
            Some(&basic("admin@example.org", "pw")),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(
            json(&body)["message"],
            "400 BAD_REQUEST \"Extractor does not support raw extraction of pages\""
        );
    }

    // endregion

    // region file download

    #[tokio::test]
    async fn book_file_download() {
        let state = test_state_with_settings();
        let db = state.db.clone();
        seed_library(&db, "l1");
        seed_series(&db, "s1", "l1");
        seed_book(&db, "b1", "s1", "l1", &format!("file:{FIXTURE_ZIP}"));
        seed_media(
            &db,
            "b1",
            MediaStatus::Ready,
            detect::APPLICATION_ZIP,
            zip_pages(),
        );
        let mut roles = BTreeSet::new();
        roles.insert(UserRole::FileDownload);
        let user = KomgaUser {
            id: String::new(),
            email: "dl@example.org".into(),
            password: bcrypt::hash("pw", 4).unwrap(),
            roles,
            shared_libraries_ids: BTreeSet::new(),
            shared_all_libraries: true,
            restrictions: ContentRestrictions::default(),
            created_date: time_codec::now_utc(),
            last_modified_date: time_codec::now_utc(),
        };
        UserDao::new(db.clone()).insert(&user).unwrap();
        seed_user(&db, "user@example.org", "pw", false);
        let app = test_router(state);

        let size = std::fs::metadata(FIXTURE_ZIP).unwrap().len();
        let (status, headers, body) = call(
            &app,
            "GET",
            "/api/v1/books/b1/file",
            Some(&basic("dl@example.org", "pw")),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers["content-type"], "application/zip");
        assert_eq!(headers["content-length"], size.to_string());
        let disposition = headers["content-disposition"].to_str().unwrap().to_string();
        assert!(
            disposition.starts_with("attachment; filename*=UTF-8''zip.zip"),
            "{disposition}"
        );
        assert_eq!(body.len() as u64, size);

        // /file/* variant
        let (status, _, _) = call(
            &app,
            "GET",
            "/api/v1/books/b1/file/extra",
            Some(&basic("dl@example.org", "pw")),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        // no FILE_DOWNLOAD role -> 403
        let (status, _, _) = call(
            &app,
            "GET",
            "/api/v1/books/b1/file",
            Some(&basic("user@example.org", "pw")),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    // endregion

    // region progression & read progress

    #[tokio::test]
    async fn progression_get_and_put() {
        let state = test_state_with_settings();
        let db = state.db.clone();
        seed_library(&db, "l1");
        seed_series(&db, "s1", "l1");
        seed_book(&db, "b1", "s1", "l1", "file:/data/b1.cbz");
        let mut pages = zip_pages();
        pages.push(BookPage {
            file_name: "p2.png".into(),
            media_type: "image/png".into(),
            width: None,
            height: None,
            file_hash: String::new(),
            file_size: None,
        });
        pages.push(BookPage {
            file_name: "p3.png".into(),
            media_type: "image/png".into(),
            width: None,
            height: None,
            file_hash: String::new(),
            file_size: None,
        });
        seed_media(
            &db,
            "b1",
            MediaStatus::Ready,
            detect::APPLICATION_ZIP,
            pages,
        );
        seed_user(&db, "admin@example.org", "pw", true);
        let app = test_router(state);

        // no progress yet: 204
        let (status, _, _) = call(
            &app,
            "GET",
            "/api/v1/books/b1/progression",
            Some(&basic("admin@example.org", "pw")),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);

        // put progression
        let body = r#"{"modified":"2024-01-02T03:04:05Z","device":{"id":"dev1","name":"Tablet"},"locator":{"href":"b1.cbz","type":"application/zip","locations":{"position":2}}}"#;
        let (status, _, _) = call(
            &app,
            "PUT",
            "/api/v1/books/b1/progression",
            Some(&basic("admin@example.org", "pw")),
            Some(body.into()),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);

        let progress = read_progress_dao_for(&db)
            .find_by_book_and_user("b1", &user_id_of(&db, "admin@example.org"))
            .unwrap()
            .expect("progress missing");
        assert_eq!(progress.page, 2);
        assert!(!progress.completed);
        assert_eq!(progress.device_id, "dev1");

        // get progression
        let (status, headers, body) = call(
            &app,
            "GET",
            "/api/v1/books/b1/progression",
            Some(&basic("admin@example.org", "pw")),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            headers["content-type"],
            "application/vnd.readium.progression+json"
        );
        let progression = json(&body);
        assert_eq!(progression["device"]["id"], "dev1");
        assert_eq!(progression["modified"], "2024-01-02T03:04:05Z");

        // older progression -> 409
        let older = r#"{"modified":"2024-01-01T03:04:05Z","device":{"id":"dev1","name":"Tablet"},"locator":{"href":"b1.cbz","type":"application/zip","locations":{"position":3}}}"#;
        let (status, _, body) = call(
            &app,
            "PUT",
            "/api/v1/books/b1/progression",
            Some(&basic("admin@example.org", "pw")),
            Some(older.into()),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(
            json(&body)["message"],
            "409 CONFLICT \"Progression is older than existing\""
        );

        // out-of-range position -> 400
        let bad = r#"{"modified":"2024-02-02T03:04:05Z","device":{"id":"dev1","name":"Tablet"},"locator":{"href":"b1.cbz","type":"application/zip","locations":{"position":99}}}"#;
        let (status, _, body) = call(
            &app,
            "PUT",
            "/api/v1/books/b1/progression",
            Some(&basic("admin@example.org", "pw")),
            Some(bad.into()),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(
            json(&body)["message"],
            "400 BAD_REQUEST \"Page argument (99) must be within 1 and book page count (3)\""
        );
    }

    fn user_id_of(db: &Database, email: &str) -> String {
        UserDao::new(db.clone())
            .find_by_email_ignore_case(email)
            .unwrap()
            .unwrap()
            .id
    }

    #[tokio::test]
    async fn read_progress_patch_and_delete() {
        let state = test_state_with_settings();
        let db = state.db.clone();
        seed_library(&db, "l1");
        seed_series(&db, "s1", "l1");
        seed_book(&db, "b1", "s1", "l1", "file:/data/b1.cbz");
        let mut pages = zip_pages();
        pages.push(BookPage {
            file_name: "p2.png".into(),
            media_type: "image/png".into(),
            width: None,
            height: None,
            file_hash: String::new(),
            file_size: None,
        });
        seed_media(
            &db,
            "b1",
            MediaStatus::Ready,
            detect::APPLICATION_ZIP,
            pages,
        );
        seed_user(&db, "admin@example.org", "pw", true);
        let app = test_router(state);
        let user_id = user_id_of(&db, "admin@example.org");

        // in-progress mark
        let (status, _, _) = call(
            &app,
            "PATCH",
            "/api/v1/books/b1/read-progress",
            Some(&basic("admin@example.org", "pw")),
            Some(r#"{"page":1}"#.into()),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let progress = read_progress_dao_for(&db)
            .find_by_book_and_user("b1", &user_id)
            .unwrap()
            .expect("progress missing");
        assert_eq!(progress.page, 1);
        assert!(!progress.completed);

        // invalid page -> 400
        let (status, _, body) = call(
            &app,
            "PATCH",
            "/api/v1/books/b1/read-progress",
            Some(&basic("admin@example.org", "pw")),
            Some(r#"{"page":99}"#.into()),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(
            json(&body)["message"],
            "400 BAD_REQUEST \"Page argument (99) must be within 1 and book page count (2)\""
        );

        // non-positive page -> violations
        let (status, _, body) = call(
            &app,
            "PATCH",
            "/api/v1/books/b1/read-progress",
            Some(&basic("admin@example.org", "pw")),
            Some(r#"{"page":0}"#.into()),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json(&body)["violations"][0]["fieldName"], "page");

        // empty body object -> class-level violation, empty violations list
        let (status, _, body) = call(
            &app,
            "PATCH",
            "/api/v1/books/b1/read-progress",
            Some(&basic("admin@example.org", "pw")),
            Some(r#"{"completed":false}"#.into()),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json(&body)["violations"].as_array().unwrap().len(), 0);

        // completed mark
        let (status, _, _) = call(
            &app,
            "PATCH",
            "/api/v1/books/b1/read-progress",
            Some(&basic("admin@example.org", "pw")),
            Some(r#"{"completed":true}"#.into()),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let progress = read_progress_dao_for(&db)
            .find_by_book_and_user("b1", &user_id)
            .unwrap()
            .unwrap();
        assert!(progress.completed);
        assert_eq!(progress.page, 2);

        // delete
        let (status, _, _) = call(
            &app,
            "DELETE",
            "/api/v1/books/b1/read-progress",
            Some(&basic("admin@example.org", "pw")),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert!(read_progress_dao_for(&db)
            .find_by_book_and_user("b1", &user_id)
            .unwrap()
            .is_none());
    }

    // endregion
}
