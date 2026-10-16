//! Equivalent models for `Book.kt` / `BookMetadata.kt` / `Author.kt` / `WebLink.kt`.
//! `url` is stored as a `file:/…` string (consistent with the DB URL column).

pub use super::common::{Author, NotableEntity, WebLink};
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

    /// STRM books are pointers. Their remote content is resolved only when a reader opens them.
    pub fn is_strm(&self) -> bool {
        self.url
            .rsplit(['/', '\\'])
            .next()
            .is_some_and(|name| name.to_ascii_lowercase().ends_with(".strm"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time_codec;

    fn book(url: &str) -> Book {
        let now = time_codec::now_utc();
        Book {
            id: "b1".into(),
            name: "v01".into(),
            url: url.into(),
            file_last_modified: now,
            series_id: "s1".into(),
            library_id: "l1".into(),
            file_size: 0,
            number: 1,
            file_hash: String::new(),
            file_hash_koreader: String::new(),
            deleted_date: None,
            oneshot: false,
            created_date: now,
            last_modified_date: now,
        }
    }

    #[test]
    fn detects_strm_by_file_name_case_insensitively() {
        assert!(book("file:/library/series/v01.strm").is_strm());
        assert!(book("file:/library/series/v01.STRM").is_strm());
        assert!(!book("file:/library/series/v01.cbz").is_strm());
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
    pub notable_entities: Vec<NotableEntity>,
    pub tags: Vec<String>,
    pub isbn: String,
    pub links: Vec<WebLink>,
    pub title_lock: bool,
    pub summary_lock: bool,
    pub number_lock: bool,
    pub number_sort_lock: bool,
    pub release_date_lock: bool,
    pub authors_lock: bool,
    pub notable_entities_lock: bool,
    pub tags_lock: bool,
    pub isbn_lock: bool,
    pub links_lock: bool,
    pub created_date: OffsetDateTime,
    pub last_modified_date: OffsetDateTime,
}
