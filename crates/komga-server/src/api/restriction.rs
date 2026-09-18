//! Content restriction checks, ported from `ContentRestrictionChecker.kt`.
//!
//! Single-entity checks run in Rust (as opposed to list queries, which are filtered in SQL):
//! library sharing violations and content restrictions produce 403; a missing entity produces 404.

use crate::error::ApiError;
use crate::state::AppState;
use komga_core::dto::book::BookDto;
use komga_core::dto::series::SeriesDto;
use komga_core::model::book::Book;
use komga_core::model::user::KomgaUser;
use komga_db::dao::book::BookDao;
use komga_db::dao::series::{SeriesDao, SeriesMetadataDao};
use komga_db::dao::thumbnail::{ThumbnailBookDao, ThumbnailSeriesDao};

fn forbidden() -> ApiError {
    ApiError::forbidden("")
}

fn not_found() -> ApiError {
    ApiError::not_found("")
}

fn check_metadata(state: &AppState, user: &KomgaUser, series_id: &str) -> Result<(), ApiError> {
    let metadata = SeriesMetadataDao::new(state.db.clone())
        .find_by_id(series_id)?
        .ok_or_else(|| ApiError::Internal(format!("no metadata for series {series_id}")))?;
    let labels: Vec<String> = metadata.sharing_labels.iter().cloned().collect();
    if !user.is_content_allowed(metadata.age_rating, &labels) {
        return Err(forbidden());
    }
    Ok(())
}

pub fn check_book(state: &AppState, user: &KomgaUser, book: &Book) -> Result<(), ApiError> {
    if !user.can_access_library(&book.library_id) {
        return Err(forbidden());
    }
    if user.restrictions.is_restricted() {
        check_metadata(state, user, &book.series_id)?;
    }
    Ok(())
}

pub fn check_book_dto(state: &AppState, user: &KomgaUser, book: &BookDto) -> Result<(), ApiError> {
    if !user.can_access_library(&book.library_id) {
        return Err(forbidden());
    }
    if user.restrictions.is_restricted() {
        check_metadata(state, user, &book.series_id)?;
    }
    Ok(())
}

pub fn check_book_by_id(state: &AppState, user: &KomgaUser, book_id: &str) -> Result<(), ApiError> {
    let dao = BookDao::new(state.db.clone());
    if !user.can_access_all_libraries() {
        match dao.get_library_id_or_null(book_id)? {
            Some(library_id) if user.can_access_library(&library_id) => {}
            Some(_) => return Err(forbidden()),
            None => return Err(not_found()),
        }
    }
    if user.restrictions.is_restricted() {
        match dao.get_series_id_or_null(book_id)? {
            Some(series_id) => check_metadata(state, user, &series_id)?,
            None => return Err(not_found()),
        }
    }
    Ok(())
}

pub fn check_book_thumbnail(
    state: &AppState,
    user: &KomgaUser,
    thumbnail_id: &str,
) -> Result<(), ApiError> {
    let dao = ThumbnailBookDao::new(state.db.clone());
    if !user.can_access_all_libraries() {
        match dao.get_library_id_or_null(thumbnail_id)? {
            Some(library_id) if user.can_access_library(&library_id) => {}
            Some(_) => return Err(forbidden()),
            None => return Err(not_found()),
        }
    }
    if user.restrictions.is_restricted() {
        match dao.get_series_id_or_null(thumbnail_id)? {
            Some(series_id) => check_metadata(state, user, &series_id)?,
            None => return Err(not_found()),
        }
    }
    Ok(())
}

pub fn check_series_dto(user: &KomgaUser, series: &SeriesDto) -> Result<(), ApiError> {
    if !user.can_access_library(&series.library_id) {
        return Err(forbidden());
    }
    let labels: Vec<String> = series.metadata.sharing_labels.iter().cloned().collect();
    if !user.is_content_allowed(series.metadata.age_rating, &labels) {
        return Err(forbidden());
    }
    Ok(())
}

pub fn check_series_by_id(
    state: &AppState,
    user: &KomgaUser,
    series_id: &str,
) -> Result<(), ApiError> {
    if !user.can_access_all_libraries() {
        match SeriesDao::new(state.db.clone()).get_library_id(series_id)? {
            Some(library_id) if user.can_access_library(&library_id) => {}
            Some(_) => return Err(forbidden()),
            None => return Err(not_found()),
        }
    }
    if user.restrictions.is_restricted() {
        check_metadata(state, user, series_id)?;
    }
    Ok(())
}

pub fn check_series_thumbnail(
    state: &AppState,
    user: &KomgaUser,
    thumbnail_id: &str,
) -> Result<(), ApiError> {
    let dao = ThumbnailSeriesDao::new(state.db.clone());
    if !user.can_access_all_libraries() {
        match dao.get_library_id_or_null(thumbnail_id)? {
            Some(library_id) if user.can_access_library(&library_id) => {}
            Some(_) => return Err(forbidden()),
            None => return Err(not_found()),
        }
    }
    if user.restrictions.is_restricted() {
        match dao.get_series_id_or_null(thumbnail_id)? {
            Some(series_id) => check_metadata(state, user, &series_id)?,
            None => return Err(not_found()),
        }
    }
    Ok(())
}
