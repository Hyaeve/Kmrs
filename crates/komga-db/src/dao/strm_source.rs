//! Resolved `.strm` pointers in the separate `kmrs.sqlite` database.

use crate::pool::Database;
use crate::Result;
use komga_core::time_codec;
use rusqlite::Row;
use time::OffsetDateTime;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StrmSource {
    pub book_id: String,
    pub source_url: String,
    pub strm_last_modified: OffsetDateTime,
    pub cache_path: String,
    pub file_name: String,
    pub media_type: Option<String>,
    pub file_size: i64,
    pub created_date: OffsetDateTime,
    pub last_modified_date: OffsetDateTime,
}

pub struct StrmSourceDao {
    db: Database,
}

const COLUMNS: &str = "BOOK_ID, SOURCE_URL, STRM_LAST_MODIFIED, CACHE_PATH, FILE_NAME, \
                       MEDIA_TYPE, FILE_SIZE, CREATED_DATE, LAST_MODIFIED_DATE";

fn row_to_source(row: &Row<'_>) -> rusqlite::Result<StrmSource> {
    Ok(StrmSource {
        book_id: row.get(0)?,
        source_url: row.get(1)?,
        strm_last_modified: super::get_datetime(row, 2)?,
        cache_path: row.get(3)?,
        file_name: row.get(4)?,
        media_type: row.get(5)?,
        file_size: row.get(6)?,
        created_date: super::get_datetime(row, 7)?,
        last_modified_date: super::get_datetime(row, 8)?,
    })
}

impl StrmSourceDao {
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    pub fn get(&self, book_id: &str) -> Result<Option<StrmSource>> {
        let conn = self.db.ro()?;
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLUMNS} FROM STRM_SOURCE WHERE BOOK_ID = ?"
        ))?;
        let mut rows = stmt.query_map([book_id], row_to_source)?;
        Ok(rows.next().transpose()?)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn upsert(
        &self,
        book_id: &str,
        source_url: &str,
        strm_last_modified: OffsetDateTime,
        cache_path: &str,
        file_name: &str,
        media_type: Option<&str>,
        file_size: i64,
    ) -> Result<()> {
        let now = time_codec::format_datetime(time_codec::now_utc());
        self.db.rw()?.execute(
            "INSERT INTO STRM_SOURCE \
             (BOOK_ID, SOURCE_URL, STRM_LAST_MODIFIED, CACHE_PATH, FILE_NAME, MEDIA_TYPE, FILE_SIZE, CREATED_DATE, LAST_MODIFIED_DATE) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT (BOOK_ID) DO UPDATE SET \
                 SOURCE_URL = excluded.SOURCE_URL, \
                 STRM_LAST_MODIFIED = excluded.STRM_LAST_MODIFIED, \
                 CACHE_PATH = excluded.CACHE_PATH, \
                 FILE_NAME = excluded.FILE_NAME, \
                 MEDIA_TYPE = excluded.MEDIA_TYPE, \
                 FILE_SIZE = excluded.FILE_SIZE, \
                 LAST_MODIFIED_DATE = excluded.LAST_MODIFIED_DATE",
            rusqlite::params![
                book_id,
                source_url,
                time_codec::format_datetime(strm_last_modified),
                cache_path,
                file_name,
                media_type,
                file_size,
                now,
                now,
            ],
        )?;
        Ok(())
    }

    pub fn delete(&self, book_id: &str) -> Result<Option<StrmSource>> {
        let existing = self.get(book_id)?;
        self.db
            .rw()?
            .execute("DELETE FROM STRM_SOURCE WHERE BOOK_ID = ?", [book_id])?;
        Ok(existing)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Migrator, Placeholders};

    fn test_dao() -> StrmSourceDao {
        let db = Database::open_in_memory(false).unwrap();
        Migrator::new(&crate::kmrs_migrations(), Placeholders::default())
            .migrate(&db.rw().unwrap())
            .unwrap();
        StrmSourceDao::new(db)
    }

    #[test]
    fn round_trips_and_deletes_resolution() {
        let dao = test_dao();
        assert!(dao.get("b1").unwrap().is_none());
        let modified = time_codec::now_utc();
        dao.upsert(
            "b1",
            "https://example.test/book.cbz",
            modified,
            "/cache/b1.cbz",
            "book.cbz",
            Some("application/zip"),
            42,
        )
        .unwrap();

        let row = dao.get("b1").unwrap().unwrap();
        assert_eq!(row.book_id, "b1");
        assert_eq!(row.source_url, "https://example.test/book.cbz");
        assert_eq!(row.strm_last_modified, modified);
        assert_eq!(row.cache_path, "/cache/b1.cbz");
        assert_eq!(row.file_name, "book.cbz");
        assert_eq!(row.media_type.as_deref(), Some("application/zip"));
        assert_eq!(row.file_size, 42);

        let deleted = dao.delete("b1").unwrap().unwrap();
        assert_eq!(deleted.book_id, "b1");
        assert!(dao.get("b1").unwrap().is_none());
    }
}
