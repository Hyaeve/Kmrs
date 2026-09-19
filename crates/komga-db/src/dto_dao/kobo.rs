//! `KoboDtoDao.kt`: the book-metadata query behind the Kobo sync endpoints.

use crate::error::Result;
use crate::pool::Database;
use komga_core::time_codec;
use rusqlite::Row;
use std::collections::{BTreeMap, HashMap};
use time::{Date, OffsetDateTime};

/// One row of `KoboDtoDao.findBookMetadataByIds`, flattened for the DTO mapper in the server crate.
#[derive(Debug, Clone, PartialEq)]
pub struct KoboBookMetadataRow {
    pub book_id: String,
    pub title: String,
    pub number: String,
    pub number_sort: f32,
    pub isbn: String,
    pub summary: String,
    pub release_date: Option<Date>,
    pub created_date: OffsetDateTime,
    pub series_id: String,
    pub series_title: String,
    pub publisher: String,
    pub language: String,
    pub file_size: i64,
    pub oneshot: bool,
    pub epub_is_kepub: bool,
    /// `MediaExtensionEpub.isFixedLayout` decoded from the extension blob
    pub is_pre_paginated: bool,
    /// selected thumbnail id (coverImageId)
    pub cover_image_id: Option<String>,
    pub authors: Vec<String>,
}

pub struct KoboDtoDao {
    db: Database,
}

impl KoboDtoDao {
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    pub fn find_book_metadata_by_ids(
        &self,
        book_ids: &[String],
    ) -> Result<Vec<KoboBookMetadataRow>> {
        if book_ids.is_empty() {
            return Ok(vec![]);
        }
        let conn = self.db.ro();
        let authors = self.authors_by_book(&conn, book_ids)?;
        let mut rows = vec![];
        for chunk in book_ids.chunks(500) {
            let placeholders = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
            let mut stmt = conn.prepare(&format!(
                "SELECT BOOK_METADATA.BOOK_ID, BOOK_METADATA.TITLE, BOOK_METADATA.NUMBER, BOOK_METADATA.NUMBER_SORT, \
                 BOOK_METADATA.ISBN, BOOK_METADATA.SUMMARY, BOOK_METADATA.RELEASE_DATE, BOOK_METADATA.CREATED_DATE, \
                 SERIES_METADATA.SERIES_ID, SERIES_METADATA.TITLE, SERIES_METADATA.PUBLISHER, SERIES_METADATA.LANGUAGE, \
                 BOOK.FILE_SIZE, BOOK.ONESHOT, MEDIA.EPUB_IS_KEPUB, MEDIA.EXTENSION_VALUE_BLOB, THUMBNAIL_BOOK.ID \
                 FROM BOOK \
                 LEFT JOIN BOOK_METADATA ON BOOK.ID = BOOK_METADATA.BOOK_ID \
                 LEFT JOIN SERIES_METADATA ON BOOK.SERIES_ID = SERIES_METADATA.SERIES_ID \
                 LEFT JOIN MEDIA ON BOOK.ID = MEDIA.BOOK_ID \
                 LEFT JOIN THUMBNAIL_BOOK ON BOOK.ID = THUMBNAIL_BOOK.BOOK_ID AND THUMBNAIL_BOOK.SELECTED = 1 \
                 WHERE BOOK_METADATA.BOOK_ID IN ({placeholders})"
            ))?;
            let mapped = stmt.query_map(rusqlite::params_from_iter(chunk.iter()), |row| {
                Self::row_to_metadata(row, &authors)
            })?;
            for row in mapped {
                rows.push(row?);
            }
        }
        Ok(rows)
    }

    fn authors_by_book(
        &self,
        conn: &rusqlite::Connection,
        book_ids: &[String],
    ) -> Result<HashMap<String, Vec<String>>> {
        let mut map: HashMap<String, Vec<String>> = HashMap::new();
        for chunk in book_ids.chunks(500) {
            let placeholders = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
            let mut stmt = conn.prepare(&format!(
                "SELECT BOOK_ID, NAME FROM BOOK_METADATA_AUTHOR WHERE BOOK_ID IN ({placeholders}) AND NAME IS NOT NULL"
            ))?;
            let rows = stmt.query_map(rusqlite::params_from_iter(chunk.iter()), |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?;
            for row in rows {
                let (book_id, name) = row?;
                map.entry(book_id).or_default().push(name);
            }
        }
        Ok(map)
    }

    fn row_to_metadata(
        row: &Row<'_>,
        authors: &HashMap<String, Vec<String>>,
    ) -> rusqlite::Result<KoboBookMetadataRow> {
        let book_id: String = row.get(0)?;
        let created: String = row.get(7)?;
        let release: Option<String> = row.get(6)?;
        let blob: Option<Vec<u8>> = row.get(15)?;
        Ok(KoboBookMetadataRow {
            authors: authors.get(&book_id).cloned().unwrap_or_default(),
            book_id: book_id.clone(),
            title: row.get(1)?,
            number: row.get(2)?,
            number_sort: row.get(3)?,
            isbn: row.get(4)?,
            summary: row.get(5)?,
            release_date: release.and_then(|d| time_codec::parse_date(&d)),
            created_date: time_codec::parse_datetime_utc(&created)
                .ok_or_else(|| crate::dao::invalid_column(7, "datetime", &created))?,
            series_id: row.get(8)?,
            series_title: row.get(9)?,
            publisher: row.get(10)?,
            language: row.get(11)?,
            file_size: row.get(12)?,
            oneshot: row.get(13)?,
            epub_is_kepub: row.get(14)?,
            is_pre_paginated: is_fixed_layout(blob.as_deref()),
            cover_image_id: row.get(16)?,
        })
    }
}

/// `deserializeMediaExtension(..) as? MediaExtensionEpub -> isFixedLayout` (false when undecodable)
fn is_fixed_layout(blob: Option<&[u8]>) -> bool {
    use std::io::Read;
    let Some(blob) = blob else { return false };
    let mut json = vec![];
    if flate2::read::GzDecoder::new(blob)
        .read_to_end(&mut json)
        .is_err()
    {
        return false;
    }
    serde_json::from_slice::<serde_json::Value>(&json)
        .ok()
        .and_then(|v| v.get("isFixedLayout").and_then(|b| b.as_bool()))
        .unwrap_or(false)
}

/// Convenience grouping used by the sync endpoint (`associateBy { it.entitlementId }`).
pub fn by_entitlement_id(rows: Vec<KoboBookMetadataRow>) -> BTreeMap<String, KoboBookMetadataRow> {
    rows.into_iter().map(|r| (r.book_id.clone(), r)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migrate::Migrator;
    use crate::{main_migrations, Placeholders};
    use rusqlite::params;

    fn db() -> Database {
        let db = Database::open_in_memory(true).unwrap();
        let migrations = main_migrations();
        Migrator::new(&migrations, Placeholders::default())
            .migrate(&db.rw())
            .unwrap();
        db
    }

    fn seed(db: &Database) {
        let now = "2024-01-02 03:04:05.0";
        let rw = db.rw();
        rw.execute(
            "INSERT INTO LIBRARY (ID, NAME, ROOT) VALUES ('l1', 'lib', 'file:/data/')",
            [],
        )
        .unwrap();
        rw.execute(
            "INSERT INTO SERIES (ID, NAME, URL, FILE_LAST_MODIFIED, LIBRARY_ID, BOOK_COUNT, ONESHOT, CREATED_DATE, LAST_MODIFIED_DATE) \
             VALUES ('s1', 'Berserk', 'file:/data/berserk/', ?, 'l1', 1, 0, ?, ?)",
            params![now, now, now],
        )
        .unwrap();
        rw.execute(
            "INSERT INTO SERIES_METADATA (SERIES_ID, STATUS, TITLE, TITLE_SORT, SUMMARY, PUBLISHER, LANGUAGE, CREATED_DATE, LAST_MODIFIED_DATE) \
             VALUES ('s1', 'ONGOING', 'Berserk', 'Berserk', '', 'Hakusensha', 'ja', ?, ?)",
            params![now, now],
        )
        .unwrap();
        rw.execute(
            "INSERT INTO BOOK (ID, NAME, URL, FILE_LAST_MODIFIED, SERIES_ID, LIBRARY_ID, FILE_SIZE, NUMBER, FILE_HASH, FILE_HASH_KOREADER, ONESHOT, CREATED_DATE, LAST_MODIFIED_DATE) \
             VALUES ('b1', 'v01', 'file:/data/berserk/v01.cbz', ?, 's1', 'l1', 16227, 1, '', '', 0, ?, ?)",
            params![now, now, now],
        )
        .unwrap();
        rw.execute(
            "INSERT INTO BOOK_METADATA (BOOK_ID, TITLE, SUMMARY, NUMBER, NUMBER_SORT, ISBN, CREATED_DATE, LAST_MODIFIED_DATE) \
             VALUES ('b1', 'Berserk v01', 'Guts', '1', 1.0, '9781593070205', ?, ?)",
            params![now, now],
        )
        .unwrap();
        rw.execute(
            "INSERT INTO BOOK_METADATA_AUTHOR (BOOK_ID, NAME, ROLE) VALUES ('b1', 'Kentaro Miura', 'writer')",
            [],
        )
        .unwrap();
        rw.execute(
            "INSERT INTO MEDIA (BOOK_ID, STATUS, MEDIA_TYPE, PAGE_COUNT, EPUB_DIVINA_COMPATIBLE, EPUB_IS_KEPUB, CREATED_DATE, LAST_MODIFIED_DATE) \
             VALUES ('b1', 'READY', 'application/zip', 1, 0, 0, ?, ?)",
            params![now, now],
        )
        .unwrap();
        rw.execute(
            "INSERT INTO THUMBNAIL_BOOK (ID, BOOK_ID, THUMBNAIL, TYPE, SELECTED, MEDIA_TYPE, FILE_SIZE, WIDTH, HEIGHT, CREATED_DATE, LAST_MODIFIED_DATE) \
             VALUES ('t1', 'b1', X'00', 'GENERATED', 1, 'image/jpeg', 100, 48, 48, ?, ?)",
            params![now, now],
        )
        .unwrap();
    }

    #[test]
    fn metadata_query() {
        let db = db();
        seed(&db);
        let rows = KoboDtoDao::new(db)
            .find_book_metadata_by_ids(&["b1".to_string()])
            .unwrap();
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row.title, "Berserk v01");
        assert_eq!(row.series_title, "Berserk");
        assert_eq!(row.publisher, "Hakusensha");
        assert_eq!(row.language, "ja");
        assert_eq!(row.file_size, 16227);
        assert!(!row.oneshot);
        assert!(!row.is_pre_paginated);
        assert_eq!(row.cover_image_id.as_deref(), Some("t1"));
        assert_eq!(row.authors, vec!["Kentaro Miura".to_string()]);
        assert!(row.release_date.is_none());
        assert_eq!(by_entitlement_id(rows).len(), 1);
    }

    #[test]
    fn empty_and_missing() {
        let db = db();
        assert!(KoboDtoDao::new(db.clone())
            .find_book_metadata_by_ids(&[])
            .unwrap()
            .is_empty());
        assert!(KoboDtoDao::new(db)
            .find_book_metadata_by_ids(&["nope".to_string()])
            .unwrap()
            .is_empty());
    }

    #[test]
    fn fixed_layout_detection() {
        use flate2::write::GzEncoder;
        use std::io::Write;
        let mut enc = GzEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(br#"{"isFixedLayout":true,"positions":[]}"#)
            .unwrap();
        let blob = enc.finish().unwrap();
        assert!(is_fixed_layout(Some(&blob)));
        assert!(!is_fixed_layout(None));
        assert!(!is_fixed_layout(Some(b"not gzip")));
        let mut enc = GzEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(br#"{"isFixedLayout":false}"#).unwrap();
        assert!(!is_fixed_layout(Some(&enc.finish().unwrap())));
    }
}
