//! Equivalent of `ReferentialV1Controller` (`/api/v1`) and `ReferentialV2Controller` (`/api/v2`).

use crate::auth::RequireAuth;
use crate::dto::common::{Page, Pageable};
use crate::error::ApiError;
use crate::http::pagination::{QueryExt, QueryPageable};
use crate::state::AppState;
use axum::extract::State;
use axum::{routing, Json, Router};
use komga_core::dto::common::AuthorDto;
use komga_core::model::user::KomgaUser;
use komga_core::search::{FilterBy, FilterByEntity, FilterTags, SearchContext};
use komga_db::dto_dao::referential::ReferentialDao;
use komga_db::dto_dao::{DtoPage, PageRequest, SortOrder};
use serde::Serialize;
use std::collections::{BTreeSet, HashMap};

pub fn router() -> Router<AppState> {
    Router::new()
        // v1
        .route("/api/v1/authors", routing::get(get_authors_v1))
        .route("/api/v1/authors/names", routing::get(get_authors_names_v1))
        .route("/api/v1/authors/roles", routing::get(get_authors_roles_v1))
        .route("/api/v1/genres", routing::get(get_genres_v1))
        .route(
            "/api/v1/sharing-labels",
            routing::get(get_sharing_labels_v1),
        )
        .route("/api/v1/tags", routing::get(get_tags_v1))
        .route("/api/v1/tags/book", routing::get(get_book_tags_v1))
        .route("/api/v1/tags/series", routing::get(get_series_tags_v1))
        .route("/api/v1/languages", routing::get(get_languages_v1))
        .route("/api/v1/publishers", routing::get(get_publishers_v1))
        .route("/api/v1/age-ratings", routing::get(get_age_ratings_v1))
        .route(
            "/api/v1/series/release-dates",
            routing::get(get_series_release_dates_v1),
        )
        // v2
        .route("/api/v2/authors", routing::get(get_authors_v2))
        .route("/api/v2/authors/roles", routing::get(get_authors_roles_v2))
        .route("/api/v2/authors/names", routing::get(get_authors_names_v2))
        .route("/api/v2/genres", routing::get(get_genres_v2))
        .route(
            "/api/v2/sharing-labels",
            routing::get(get_sharing_labels_v2),
        )
        .route("/api/v2/languages", routing::get(get_languages_v2))
        .route("/api/v2/publishers", routing::get(get_publishers_v2))
        .route("/api/v2/tags", routing::get(get_tags_v2))
        .route(
            "/api/v2/series/release-years",
            routing::get(get_series_release_years_v2),
        )
        .route("/api/v2/age-ratings", routing::get(get_age_ratings_v2))
}

fn dao(state: &AppState) -> ReferentialDao {
    ReferentialDao::new(state.db.clone())
}

fn authorized(user: &KomgaUser) -> Option<BTreeSet<String>> {
    user.get_authorized_library_ids(None)
}

fn id_set(params: &HashMap<String, Vec<String>>, key: &str) -> BTreeSet<String> {
    params.all(key).iter().cloned().collect()
}

fn to_page_request(p: &Pageable) -> PageRequest {
    PageRequest {
        page: p.page,
        size: p.size,
        unpaged: p.unpaged,
        sort: p
            .sort
            .iter()
            .map(|s| SortOrder {
                property: s.property.clone(),
                descending: s.descending,
            })
            .collect(),
    }
}

/// Assembles the Spring `PageImpl` JSON. komga's `buildPage` swaps the request sort for the
/// DAO-provided one; the JSON only carries the sorted/unsorted flag trio.
fn to_page<T: Serialize>(dto_page: DtoPage<T>, pageable: &Pageable) -> Page<T> {
    Page::of_dto(dto_page, pageable)
}

// region v1

async fn get_authors_v1(
    State(state): State<AppState>,
    auth: RequireAuth,
    QueryPageable { params, .. }: QueryPageable,
) -> Result<Json<Vec<AuthorDto>>, ApiError> {
    let dao = dao(&state);
    let search = params.first("search").unwrap_or("");
    let authorized = authorized(&auth.0.user);
    let authors = if let Some(library_id) = params.first("library_id") {
        dao.find_all_authors_by_name_and_library(search, library_id, authorized.as_ref())?
    } else if let Some(collection_id) = params.first("collection_id") {
        dao.find_all_authors_by_name_and_collection(search, collection_id, authorized.as_ref())?
    } else if let Some(series_id) = params.first("series_id") {
        dao.find_all_authors_by_name_and_series(search, series_id, authorized.as_ref())?
    } else {
        dao.find_all_authors_by_name(search, authorized.as_ref())?
    };
    Ok(Json(authors.iter().map(AuthorDto::from).collect()))
}

async fn get_authors_names_v1(
    State(state): State<AppState>,
    auth: RequireAuth,
    QueryPageable { params, .. }: QueryPageable,
) -> Result<Json<Vec<String>>, ApiError> {
    let search = params.first("search").unwrap_or("");
    let names =
        dao(&state).find_all_authors_names_by_name(search, authorized(&auth.0.user).as_ref())?;
    Ok(Json(names))
}

async fn get_authors_roles_v1(
    State(state): State<AppState>,
    auth: RequireAuth,
) -> Result<Json<Vec<String>>, ApiError> {
    let roles = dao(&state).find_all_authors_roles(authorized(&auth.0.user).as_ref())?;
    Ok(Json(roles))
}

async fn get_genres_v1(
    State(state): State<AppState>,
    auth: RequireAuth,
    QueryPageable { params, .. }: QueryPageable,
) -> Result<Json<Vec<String>>, ApiError> {
    let dao = dao(&state);
    let authorized = authorized(&auth.0.user);
    let library_ids = id_set(&params, "library_id");
    let genres = if !library_ids.is_empty() {
        dao.find_all_genres_by_libraries(&library_ids, authorized.as_ref())?
    } else if let Some(collection_id) = params.first("collection_id") {
        dao.find_all_genres_by_collection(collection_id, authorized.as_ref())?
    } else {
        dao.find_all_genres(authorized.as_ref())?
    };
    Ok(Json(genres))
}

async fn get_sharing_labels_v1(
    State(state): State<AppState>,
    auth: RequireAuth,
    QueryPageable { params, .. }: QueryPageable,
) -> Result<Json<Vec<String>>, ApiError> {
    let dao = dao(&state);
    let authorized = authorized(&auth.0.user);
    let library_ids = id_set(&params, "library_id");
    let labels = if !library_ids.is_empty() {
        dao.find_all_sharing_labels_by_libraries(&library_ids, authorized.as_ref())?
    } else if let Some(collection_id) = params.first("collection_id") {
        dao.find_all_sharing_labels_by_collection(collection_id, authorized.as_ref())?
    } else {
        dao.find_all_sharing_labels(authorized.as_ref())?
    };
    Ok(Json(labels))
}

async fn get_tags_v1(
    State(state): State<AppState>,
    auth: RequireAuth,
    QueryPageable { params, .. }: QueryPageable,
) -> Result<Json<Vec<String>>, ApiError> {
    let dao = dao(&state);
    let authorized = authorized(&auth.0.user);
    let library_ids = id_set(&params, "library_id");
    let tags = if !library_ids.is_empty() {
        dao.find_all_series_and_book_tags_by_libraries(&library_ids, authorized.as_ref())?
    } else if let Some(collection_id) = params.first("collection_id") {
        dao.find_all_series_and_book_tags_by_collection(collection_id, authorized.as_ref())?
    } else {
        dao.find_all_series_and_book_tags(authorized.as_ref())?
    };
    Ok(Json(tags))
}

async fn get_book_tags_v1(
    State(state): State<AppState>,
    auth: RequireAuth,
    QueryPageable { params, .. }: QueryPageable,
) -> Result<Json<Vec<String>>, ApiError> {
    let dao = dao(&state);
    let user = &auth.0.user;
    let authorized = authorized(user);
    let library_ids = id_set(&params, "library_id");
    let tags = if let Some(series_id) = params.first("series_id") {
        dao.find_all_book_tags_by_series(series_id, authorized.as_ref())?
    } else if let Some(readlist_id) = params.first("readlist_id") {
        dao.find_all_book_tags_by_readlist(readlist_id, authorized.as_ref())?
    } else if !library_ids.is_empty() {
        dao.find_all_book_tags(user.get_authorized_library_ids(Some(&library_ids)).as_ref())?
    } else {
        dao.find_all_book_tags(authorized.as_ref())?
    };
    Ok(Json(tags))
}

async fn get_series_tags_v1(
    State(state): State<AppState>,
    auth: RequireAuth,
    QueryPageable { params, .. }: QueryPageable,
) -> Result<Json<Vec<String>>, ApiError> {
    let dao = dao(&state);
    let authorized = authorized(&auth.0.user);
    let tags = if let Some(library_id) = params.first("library_id") {
        dao.find_all_series_tags_by_library(library_id, authorized.as_ref())?
    } else if let Some(collection_id) = params.first("collection_id") {
        dao.find_all_series_tags_by_collection(collection_id, authorized.as_ref())?
    } else {
        dao.find_all_series_tags(authorized.as_ref())?
    };
    Ok(Json(tags))
}

async fn get_languages_v1(
    State(state): State<AppState>,
    auth: RequireAuth,
    QueryPageable { params, .. }: QueryPageable,
) -> Result<Json<Vec<String>>, ApiError> {
    let dao = dao(&state);
    let authorized = authorized(&auth.0.user);
    let library_ids = id_set(&params, "library_id");
    let languages = if !library_ids.is_empty() {
        dao.find_all_languages_by_libraries(&library_ids, authorized.as_ref())?
    } else if let Some(collection_id) = params.first("collection_id") {
        dao.find_all_languages_by_collection(collection_id, authorized.as_ref())?
    } else {
        dao.find_all_languages(authorized.as_ref())?
    };
    Ok(Json(languages))
}

async fn get_publishers_v1(
    State(state): State<AppState>,
    auth: RequireAuth,
    QueryPageable { params, .. }: QueryPageable,
) -> Result<Json<Vec<String>>, ApiError> {
    let dao = dao(&state);
    let authorized = authorized(&auth.0.user);
    let library_ids = id_set(&params, "library_id");
    let publishers = if !library_ids.is_empty() {
        dao.find_all_publishers_by_libraries(&library_ids, authorized.as_ref())?
    } else if let Some(collection_id) = params.first("collection_id") {
        dao.find_all_publishers_by_collection(collection_id, authorized.as_ref())?
    } else {
        dao.find_all_publishers(authorized.as_ref())?
    };
    Ok(Json(publishers))
}

async fn get_age_ratings_v1(
    State(state): State<AppState>,
    auth: RequireAuth,
    QueryPageable { params, .. }: QueryPageable,
) -> Result<Json<Vec<String>>, ApiError> {
    let dao = dao(&state);
    let authorized = authorized(&auth.0.user);
    let library_ids = id_set(&params, "library_id");
    let ratings = if !library_ids.is_empty() {
        dao.find_all_age_ratings_by_libraries(&library_ids, authorized.as_ref())?
    } else if let Some(collection_id) = params.first("collection_id") {
        dao.find_all_age_ratings_by_collection(collection_id, authorized.as_ref())?
    } else {
        dao.find_all_age_ratings(authorized.as_ref())?
    };
    Ok(Json(
        ratings
            .iter()
            .map(|r| {
                r.map(|r| r.to_string())
                    .unwrap_or_else(|| "None".to_string())
            })
            .collect(),
    ))
}

async fn get_series_release_dates_v1(
    State(state): State<AppState>,
    auth: RequireAuth,
    QueryPageable { params, .. }: QueryPageable,
) -> Result<Json<Vec<String>>, ApiError> {
    let dao = dao(&state);
    let authorized = authorized(&auth.0.user);
    let library_ids = id_set(&params, "library_id");
    let dates = if !library_ids.is_empty() {
        dao.find_all_series_release_dates_by_libraries(&library_ids, authorized.as_ref())?
    } else if let Some(collection_id) = params.first("collection_id") {
        dao.find_all_series_release_dates_by_collection(collection_id, authorized.as_ref())?
    } else {
        dao.find_all_series_release_dates(authorized.as_ref())?
    };
    Ok(Json(dates.iter().map(|d| d.year().to_string()).collect()))
}

// endregion

// region v2

/// Filter precedence: library_id > collection_id (the only two accepted by most v2 endpoints).
fn filter_by_lc(params: &HashMap<String, Vec<String>>) -> Option<FilterBy> {
    let library_ids = id_set(params, "library_id");
    if !library_ids.is_empty() {
        return Some(FilterBy {
            type_: FilterByEntity::Library,
            ids: library_ids,
        });
    }
    let collection_ids = id_set(params, "collection_id");
    if !collection_ids.is_empty() {
        return Some(FilterBy {
            type_: FilterByEntity::Collection,
            ids: collection_ids,
        });
    }
    None
}

/// Full precedence: library_id > collection_id > series_id > readlist_id.
fn filter_by_lcsr(params: &HashMap<String, Vec<String>>) -> Option<FilterBy> {
    filter_by_lc(params).or_else(|| {
        let series_ids = id_set(params, "series_id");
        if !series_ids.is_empty() {
            return Some(FilterBy {
                type_: FilterByEntity::Series,
                ids: series_ids,
            });
        }
        let readlist_ids = id_set(params, "readlist_id");
        if !readlist_ids.is_empty() {
            return Some(FilterBy {
                type_: FilterByEntity::ReadList,
                ids: readlist_ids,
            });
        }
        None
    })
}

fn search_ctx(user: &KomgaUser) -> SearchContext {
    SearchContext::of_user(user)
}

async fn get_authors_v2(
    State(state): State<AppState>,
    auth: RequireAuth,
    QueryPageable { params, pageable }: QueryPageable,
) -> Result<Json<Page<AuthorDto>>, ApiError> {
    let result = dao(&state).find_authors(
        &search_ctx(&auth.0.user),
        params.first("search"),
        params.first("role"),
        filter_by_lcsr(&params).as_ref(),
        &to_page_request(&pageable),
    )?;
    Ok(Json(to_page(result, &pageable)))
}

async fn get_authors_roles_v2(
    State(state): State<AppState>,
    auth: RequireAuth,
    QueryPageable { params, pageable }: QueryPageable,
) -> Result<Json<Page<String>>, ApiError> {
    let result = dao(&state).find_authors_roles(
        &search_ctx(&auth.0.user),
        filter_by_lcsr(&params).as_ref(),
        &to_page_request(&pageable),
    )?;
    Ok(Json(to_page(result, &pageable)))
}

async fn get_authors_names_v2(
    State(state): State<AppState>,
    auth: RequireAuth,
    QueryPageable { params, pageable }: QueryPageable,
) -> Result<Json<Page<String>>, ApiError> {
    let result = dao(&state).find_authors_names(
        &search_ctx(&auth.0.user),
        params.first("search"),
        params.first("role"),
        filter_by_lcsr(&params).as_ref(),
        &to_page_request(&pageable),
    )?;
    Ok(Json(to_page(result, &pageable)))
}

async fn get_genres_v2(
    State(state): State<AppState>,
    auth: RequireAuth,
    QueryPageable { params, pageable }: QueryPageable,
) -> Result<Json<Page<String>>, ApiError> {
    let result = dao(&state).find_genres(
        &search_ctx(&auth.0.user),
        params.first("search"),
        filter_by_lc(&params).as_ref(),
        &to_page_request(&pageable),
    )?;
    Ok(Json(to_page(result, &pageable)))
}

async fn get_sharing_labels_v2(
    State(state): State<AppState>,
    auth: RequireAuth,
    QueryPageable { params, pageable }: QueryPageable,
) -> Result<Json<Page<String>>, ApiError> {
    let result = dao(&state).find_sharing_labels(
        &search_ctx(&auth.0.user),
        params.first("search"),
        filter_by_lc(&params).as_ref(),
        &to_page_request(&pageable),
    )?;
    Ok(Json(to_page(result, &pageable)))
}

async fn get_languages_v2(
    State(state): State<AppState>,
    auth: RequireAuth,
    QueryPageable { params, pageable }: QueryPageable,
) -> Result<Json<Page<String>>, ApiError> {
    let result = dao(&state).find_languages(
        &search_ctx(&auth.0.user),
        params.first("search"),
        filter_by_lc(&params).as_ref(),
        &to_page_request(&pageable),
    )?;
    Ok(Json(to_page(result, &pageable)))
}

async fn get_publishers_v2(
    State(state): State<AppState>,
    auth: RequireAuth,
    QueryPageable { params, pageable }: QueryPageable,
) -> Result<Json<Page<String>>, ApiError> {
    let result = dao(&state).find_publishers(
        &search_ctx(&auth.0.user),
        params.first("search"),
        filter_by_lc(&params).as_ref(),
        &to_page_request(&pageable),
    )?;
    Ok(Json(to_page(result, &pageable)))
}

fn parse_include(params: &HashMap<String, Vec<String>>) -> Result<FilterTags, ApiError> {
    match params.first("include") {
        None => Ok(FilterTags::Both),
        Some("SERIES") => Ok(FilterTags::Series),
        Some("BOOK") => Ok(FilterTags::Book),
        Some("BOTH") => Ok(FilterTags::Both),
        // Spring rejects an unknown enum binding with 400; the exact message differs (see report)
        Some(other) => Err(ApiError::bad_request(format!(
            "Invalid value '{other}' for parameter 'include'"
        ))),
    }
}

async fn get_tags_v2(
    State(state): State<AppState>,
    auth: RequireAuth,
    QueryPageable { params, pageable }: QueryPageable,
) -> Result<Json<Page<String>>, ApiError> {
    let include = parse_include(&params)?;
    let result = dao(&state).find_tags(
        &search_ctx(&auth.0.user),
        params.first("search"),
        filter_by_lcsr(&params).as_ref(),
        include,
        &to_page_request(&pageable),
    )?;
    Ok(Json(to_page(result, &pageable)))
}

async fn get_series_release_years_v2(
    State(state): State<AppState>,
    auth: RequireAuth,
    QueryPageable { params, pageable }: QueryPageable,
) -> Result<Json<Page<String>>, ApiError> {
    let result = dao(&state).find_series_release_years(
        &search_ctx(&auth.0.user),
        filter_by_lc(&params).as_ref(),
        &to_page_request(&pageable),
    )?;
    Ok(Json(to_page(result, &pageable)))
}

async fn get_age_ratings_v2(
    State(state): State<AppState>,
    auth: RequireAuth,
    QueryPageable { params, pageable }: QueryPageable,
) -> Result<Json<Page<i32>>, ApiError> {
    let result = dao(&state).find_age_ratings(
        &search_ctx(&auth.0.user),
        filter_by_lc(&params).as_ref(),
        &to_page_request(&pageable),
    )?;
    Ok(Json(to_page(result, &pageable)))
}

// endregion

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::libraries::test_support::{
        insert_api_key, insert_library, insert_user, TestApp,
    };
    use axum::http::StatusCode;
    use komga_core::model::book::{Book, BookMetadata};
    use komga_core::model::collection::SeriesCollection;
    use komga_core::model::common::Author;
    use komga_core::model::readlist::ReadList;
    use komga_core::model::series::{
        BookMetadataAggregation, ReadingDirection, Series, SeriesMetadata, SeriesStatus,
    };
    use komga_core::time_codec::now_utc;
    use komga_db::dao::book::{BookDao, BookMetadataDao};
    use komga_db::dao::collection::CollectionDao;
    use komga_db::dao::readlist::ReadListDao;
    use komga_db::dao::series::{BookMetadataAggregationDao, SeriesDao, SeriesMetadataDao};
    use komga_db::pool::Database;
    use time::Date;

    fn app() -> TestApp {
        TestApp::new(router())
    }

    fn series(id: &str, library_id: &str, name: &str) -> Series {
        Series {
            id: id.into(),
            name: name.into(),
            url: format!("file:/data/{name}/"),
            file_last_modified: now_utc(),
            library_id: library_id.into(),
            book_count: 1,
            deleted_date: None,
            oneshot: false,
            created_date: now_utc(),
            last_modified_date: now_utc(),
        }
    }

    fn metadata(series_id: &str, title_sort: &str, age_rating: Option<i32>) -> SeriesMetadata {
        SeriesMetadata {
            series_id: series_id.into(),
            status: SeriesStatus::Ongoing,
            title: title_sort.into(),
            title_sort: title_sort.into(),
            summary: String::new(),
            reading_direction: Some(ReadingDirection::LeftToRight),
            publisher: format!("pub-{series_id}"),
            age_rating,
            language: format!("lang-{series_id}"),
            genres: [format!("genre-{series_id}")].into_iter().collect(),
            tags: [format!("stag-{series_id}")].into_iter().collect(),
            total_book_count: Some(1),
            sharing_labels: [format!("label-{series_id}")].into_iter().collect(),
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
            created_date: now_utc(),
            last_modified_date: now_utc(),
        }
    }

    fn aggregation(series_id: &str, release_date: Option<Date>) -> BookMetadataAggregation {
        BookMetadataAggregation {
            series_id: series_id.into(),
            authors: vec![
                Author::new("Miura", "writer"),
                Author::new("Gaga", "penciller"),
            ],
            tags: [format!("atag-{series_id}")].into_iter().collect(),
            release_date,
            summary: String::new(),
            summary_number: String::new(),
            created_date: now_utc(),
            last_modified_date: now_utc(),
        }
    }

    fn book(id: &str, series_id: &str, library_id: &str) -> Book {
        Book {
            id: id.into(),
            name: format!("{series_id} v01"),
            url: format!("file:/data/{series_id}/{id}.cbz"),
            file_last_modified: now_utc(),
            series_id: series_id.into(),
            library_id: library_id.into(),
            file_size: 100,
            number: 1,
            file_hash: String::new(),
            file_hash_koreader: String::new(),
            deleted_date: None,
            oneshot: false,
            created_date: now_utc(),
            last_modified_date: now_utc(),
        }
    }

    fn book_metadata(book_id: &str, authors: Vec<Author>) -> BookMetadata {
        BookMetadata {
            book_id: book_id.into(),
            title: book_id.into(),
            summary: String::new(),
            number: "1".into(),
            number_sort: 1.0,
            release_date: None,
            authors,
            tags: vec![format!("btag-{book_id}")],
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
            created_date: now_utc(),
            last_modified_date: now_utc(),
        }
    }

    /// Two libraries, one series each; s2 has no age rating and no release date.
    fn seed(db: &Database) {
        insert_library(db, "l1", "L1");
        insert_library(db, "l2", "L2");
        let series_dao = SeriesDao::new(db.clone());
        let metadata_dao = SeriesMetadataDao::new(db.clone());
        let aggregation_dao = BookMetadataAggregationDao::new(db.clone());
        let book_dao = BookDao::new(db.clone());
        let book_metadata_dao = BookMetadataDao::new(db.clone());

        series_dao.insert(&series("s1", "l1", "S1")).unwrap();
        metadata_dao
            .insert(&metadata("s1", "alpha", Some(12)))
            .unwrap();
        aggregation_dao
            .insert(&aggregation(
                "s1",
                Date::from_calendar_date(1990, time::Month::January, 15).ok(),
            ))
            .unwrap();
        book_dao.insert(&book("b1", "s1", "l1")).unwrap();
        book_metadata_dao
            .insert(&book_metadata("b1", vec![Author::new("Miura", "writer")]))
            .unwrap();

        series_dao.insert(&series("s2", "l2", "S2")).unwrap();
        metadata_dao.insert(&metadata("s2", "beta", None)).unwrap();
        aggregation_dao.insert(&aggregation("s2", None)).unwrap();
        book_dao.insert(&book("b2", "s2", "l2")).unwrap();
        book_metadata_dao
            .insert(&book_metadata("b2", vec![Author::new("Gaga", "penciller")]))
            .unwrap();

        CollectionDao::new(db.clone())
            .insert(&SeriesCollection {
                id: "c1".into(),
                name: "coll".into(),
                ordered: false,
                series_ids: vec!["s1".into()],
                filtered: false,
                created_date: now_utc(),
                last_modified_date: now_utc(),
            })
            .unwrap();
        ReadListDao::new(db.clone())
            .insert(&ReadList {
                id: "r1".into(),
                name: "rl".into(),
                summary: String::new(),
                ordered: true,
                book_ids: [(0, "b1".to_string())].into_iter().collect(),
                filtered: false,
                created_date: now_utc(),
                last_modified_date: now_utc(),
            })
            .unwrap();
    }

    fn admin_key(app: &TestApp) -> String {
        let admin = insert_user(&app.state.db, "admin@x.c", true, true, &[]);
        insert_api_key(&app.state.db, &admin, "k-admin");
        "k-admin".to_string()
    }

    #[tokio::test]
    async fn v1_authors_branches() {
        let app = app();
        seed(&app.state.db);
        let key = admin_key(&app);

        let (status, body) = app.get_json("/api/v1/authors", &key).await;
        assert_eq!(status, StatusCode::OK);
        // Miura appears in both book and aggregation tables; results are distinct pairs
        let names: Vec<&str> = body
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"Miura"), "authors: {names:?}");
        assert!(names.contains(&"Gaga"), "authors: {names:?}");

        let (status, body) = app.get_json("/api/v1/authors?library_id=l2", &key).await;
        assert_eq!(status, StatusCode::OK);
        // library branch reads the aggregation table of that library's series
        let names: Vec<&str> = body
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"Miura"), "authors: {names:?}");

        let (status, body) = app.get_json("/api/v1/authors?collection_id=c1", &key).await;
        assert_eq!(status, StatusCode::OK);
        let names: Vec<&str> = body
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"Gaga"), "authors: {names:?}");

        let (status, body) = app
            .get_json("/api/v1/authors?series_id=s1&search=gag", &key)
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.as_array().unwrap().len(), 1);
        assert_eq!(body[0]["name"], "Gaga");
        assert_eq!(body[0]["role"], "penciller");
    }

    #[tokio::test]
    async fn v1_names_roles_and_tags() {
        let app = app();
        seed(&app.state.db);
        let key = admin_key(&app);

        let (status, body) = app.get_json("/api/v1/authors/names?search=miu", &key).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.as_array().unwrap(), &["Miura"]);

        let (status, body) = app.get_json("/api/v1/authors/roles", &key).await;
        assert_eq!(status, StatusCode::OK);
        let roles: Vec<&str> = body
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r.as_str().unwrap())
            .collect();
        assert!(
            roles.contains(&"writer") && roles.contains(&"penciller"),
            "{roles:?}"
        );

        let (status, body) = app.get_json("/api/v1/genres?library_id=l1", &key).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.as_array().unwrap(), &["genre-s1"]);

        let (status, body) = app.get_json("/api/v1/genres?collection_id=c1", &key).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.as_array().unwrap(), &["genre-s1"]);

        let (status, body) = app.get_json("/api/v1/tags", &key).await;
        assert_eq!(status, StatusCode::OK);
        let tags: Vec<&str> = body
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t.as_str().unwrap())
            .collect();
        // the SERIES_AND_BOOK_TAG view unions series tags and book tags (not aggregation tags)
        for expected in ["stag-s1", "stag-s2", "btag-b1", "btag-b2"] {
            assert!(tags.contains(&expected), "missing {expected} in {tags:?}");
        }
        assert_eq!(tags.len(), 4);

        let (status, body) = app.get_json("/api/v1/tags/book?readlist_id=r1", &key).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.as_array().unwrap(), &["btag-b1"]);

        let (status, body) = app
            .get_json("/api/v1/tags/series?library_id=l2", &key)
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.as_array().unwrap(), &["stag-s2"]);

        let (status, body) = app.get_json("/api/v1/languages", &key).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.as_array().unwrap().len(), 2);

        let (status, body) = app.get_json("/api/v1/publishers?library_id=l1", &key).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.as_array().unwrap(), &["pub-s1"]);

        let (status, body) = app.get_json("/api/v1/sharing-labels", &key).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn v1_age_ratings_and_release_dates() {
        let app = app();
        seed(&app.state.db);
        let key = admin_key(&app);

        let (status, body) = app.get_json("/api/v1/age-ratings", &key).await;
        assert_eq!(status, StatusCode::OK);
        let ratings: Vec<&str> = body
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r.as_str().unwrap())
            .collect();
        // NULL maps to "None" and sorts first (SQLite ASC nulls-first)
        assert_eq!(ratings, ["None", "12"]);

        let (status, body) = app
            .get_json("/api/v1/age-ratings?library_id=l2", &key)
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.as_array().unwrap(), &["None"]);

        let (status, body) = app.get_json("/api/v1/series/release-dates", &key).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.as_array().unwrap(), &["1990"]);
    }

    #[tokio::test]
    async fn v2_authors_paged_and_filtered() {
        let app = app();
        seed(&app.state.db);
        let key = admin_key(&app);

        let (status, body) = app.get_json("/api/v2/authors?size=1&page=0", &key).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["totalElements"], 2);
        assert_eq!(body["totalPages"], 2);
        assert_eq!(body["content"].as_array().unwrap().len(), 1);
        assert_eq!(body["pageable"]["pageSize"], 1);

        let (status, body) = app.get_json("/api/v2/authors?role=penciller", &key).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["totalElements"], 1);
        assert_eq!(body["content"][0]["role"], "penciller");

        let (status, body) = app.get_json("/api/v2/authors?search=gag", &key).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["totalElements"], 1);

        let (status, body) = app.get_json("/api/v2/authors?readlist_id=r1", &key).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["totalElements"], 1);
        assert_eq!(body["content"][0]["name"], "Miura");

        let (status, body) = app.get_json("/api/v2/authors?series_id=s2", &key).await;
        assert_eq!(status, StatusCode::OK);
        // series filter scopes the author to books of that series
        assert_eq!(body["totalElements"], 1);
        assert_eq!(body["content"][0]["name"], "Gaga");

        let (status, body) = app.get_json("/api/v2/authors?unpaged=true", &key).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["pageable"]["paged"], true);
        assert_eq!(body["content"].as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn v2_tags_include_modes() {
        let app = app();
        seed(&app.state.db);
        let key = admin_key(&app);

        let (status, body) = app.get_json("/api/v2/tags?include=SERIES", &key).await;
        assert_eq!(status, StatusCode::OK);
        let tags: Vec<&str> = body["content"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t.as_str().unwrap())
            .collect();
        assert!(
            tags.contains(&"stag-s1") && !tags.contains(&"btag-b1"),
            "{tags:?}"
        );

        let (status, body) = app.get_json("/api/v2/tags?include=BOOK", &key).await;
        assert_eq!(status, StatusCode::OK);
        let tags: Vec<&str> = body["content"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t.as_str().unwrap())
            .collect();
        assert!(
            tags.contains(&"btag-b1") && !tags.contains(&"stag-s1"),
            "{tags:?}"
        );

        // BOTH reads the SERIES_AND_BOOK_TAG view (aggregation tags + series tags)
        let (status, body) = app
            .get_json("/api/v2/tags?include=BOTH&search=atag", &key)
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["totalElements"], 2);

        let (status, _) = app.get_json("/api/v2/tags?include=invalid", &key).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn v2_release_years_and_age_ratings() {
        let app = app();
        seed(&app.state.db);
        let key = admin_key(&app);

        let (status, body) = app.get_json("/api/v2/series/release-years", &key).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["content"].as_array().unwrap(), &["1990"]);
        // the raw count includes the NULL row, but Spring's PageImpl shrinks the total
        // to offset + content size on a non-full page (verified against Java komga)
        assert_eq!(body["totalElements"], 1);
        // v2 referential pages carry the DAO-provided sort, so they render as sorted
        assert_eq!(body["sort"]["sorted"], true);
        assert_eq!(body["pageable"]["sort"]["sorted"], true);

        let (status, body) = app.get_json("/api/v2/age-ratings", &key).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["totalElements"], 1);
        assert_eq!(body["content"].as_array().unwrap(), &[12]);
    }

    #[tokio::test]
    async fn restricted_user_sees_only_authorized() {
        let app = app();
        seed(&app.state.db);
        let user = insert_user(&app.state.db, "user@x.c", false, false, &["l2"]);
        insert_api_key(&app.state.db, &user, "k-user");

        let (status, body) = app.get_json("/api/v1/genres", "k-user").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.as_array().unwrap(), &["genre-s2"]);

        let (status, body) = app.get_json("/api/v2/publishers", "k-user").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["content"].as_array().unwrap(), &["pub-s2"]);
    }
}
