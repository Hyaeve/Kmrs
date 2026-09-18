//! Equivalent models for `Book.kt` / `BookMetadata.kt` / `Author.kt` / `WebLink.kt`.
//! `url` is stored as a `file:/…` string (consistent with the DB URL column).

pub use super::common::{Author, WebLink};
use time::{Date, OffsetDateTime};

#[derive(Debug, Clone, PartialEq)]
pub struct Book {
    pub id: String,
    pub name: String,
    pub url: String,
    pub file_last_modified: OffsetDateTime,
    pub series_id: String,
    pub library_id: String,
    pub file_size: i64,
    /// Legacy column; the actual number is `BookMetadata.number`
    pub number: i32,
    pub file_hash: String,
    pub file_hash_koreader: String,
    pub deleted_date: Option<OffsetDateTime>,
    pub oneshot: bool,
    pub created_date: OffsetDateTime,
    pub last_modified_date: OffsetDateTime,
}

impl Book {
    pub fn deleted(&self) -> bool {
        self.deleted_date.is_some()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct BookMetadata {
    pub book_id: String,
    pub title: String,
    pub summary: String,
    pub number: String,
    pub number_sort: f32,
    pub release_date: Option<Date>,
    pub authors: Vec<Author>,
    pub tags: Vec<String>,
    pub isbn: String,
    pub links: Vec<WebLink>,
    pub title_lock: bool,
    pub summary_lock: bool,
    pub number_lock: bool,
    pub number_sort_lock: bool,
    pub release_date_lock: bool,
    pub authors_lock: bool,
    pub tags_lock: bool,
    pub isbn_lock: bool,
    pub links_lock: bool,
    pub created_date: OffsetDateTime,
    pub last_modified_date: OffsetDateTime,
}
