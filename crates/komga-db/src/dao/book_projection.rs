//! `BookProjectionDao.kt`

use super::get_datetime;
use crate::error::Result;
use crate::pool::Database;
use komga_core::model::book_projection::BookProjection;
use komga_core::time_codec;
use rusqlite::{params, Row};

const COLUMNS: &str = "BOOK_ID, PROFILE, FILE_SIZE, CREATED_DATE, LAST_MODIFIED_DATE";

pub struct BookProjectionDao {
    db: Database,
}

impl BookProjectionDao {
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    fn row_to_projection(row: &Row<'_>) -> rusqlite::Result<BookProjection> {
        Ok(BookProjection {
            book_id: row.get(0)?,
            profile: row.get(1)?,
            file_size: row.get(2)?,
            created_date: get_datetime(row, 3)?,
            last_modified_date: get_datetime(row, 4)?,
        })
    }

    /// Upsert: a re-conversion (e.g. after the source file changed) refreshes both FILE_SIZE
    /// and LAST_MODIFIED_DATE. komga 1.27.1 keeps the first stored size here (its upsert sets
    /// the column to itself), which serves a stale kepub size to Kobo after a file change.
    pub fn save(&self, projection: &BookProjection) -> Result<()> {
        self.db.rw().execute(
            "INSERT INTO BOOK_PROJECTION (BOOK_ID, PROFILE, FILE_SIZE) VALUES (?,?,?) \
             ON CONFLICT(BOOK_ID, PROFILE) DO UPDATE SET FILE_SIZE = excluded.FILE_SIZE, LAST_MODIFIED_DATE = ?",
            params![
                projection.book_id,
                projection.profile,
                projection.file_size,
                time_codec::format_datetime(time_codec::now_utc()),
            ],
        )?;
        Ok(())
    }

    pub fn find_by_book_ids(&self, book_ids: &[String]) -> Result<Vec<BookProjection>> {
        if book_ids.is_empty() {
            return Ok(vec![]);
        }
        let conn = self.db.ro();
        let mut out = vec![];
        for chunk in book_ids.chunks(500) {
            let placeholders = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
            let mut stmt = conn.prepare(&format!(
                "SELECT {COLUMNS} FROM BOOK_PROJECTION WHERE BOOK_ID IN ({placeholders})"
            ))?;
            let rows = stmt.query_map(rusqlite::params_from_iter(chunk.iter()), |row| {
                Self::row_to_projection(row)
            })?;
            for row in rows {
                out.push(row?);
            }
        }
        Ok(out)
    }

    pub fn delete(&self, book_id: &str) -> Result<()> {
        self.db
            .rw()
            .execute("DELETE FROM BOOK_PROJECTION WHERE BOOK_ID = ?", [book_id])?;
        Ok(())
    }

    pub fn delete_by_book_ids(&self, book_ids: &[String]) -> Result<()> {
        let conn = self.db.rw();
        for chunk in book_ids.chunks(500) {
            let placeholders = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
            conn.execute(
                &format!("DELETE FROM BOOK_PROJECTION WHERE BOOK_ID IN ({placeholders})"),
                rusqlite::params_from_iter(chunk.iter()),
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migrate::Migrator;
    use crate::{main_migrations, Placeholders};

    fn dao() -> BookProjectionDao {
        let db = Database::open_in_memory(true).unwrap();
        let migrations = main_migrations();
        Migrator::new(&migrations, Placeholders::default())
            .migrate(&db.rw())
            .unwrap();
        let rw = db.rw();
        rw.execute(
            "INSERT INTO LIBRARY (ID, NAME, ROOT) VALUES ('l1', 'lib', 'file:/l/')",
            [],
        )
        .unwrap();
        rw.execute(
            "INSERT INTO SERIES (ID, NAME, URL, FILE_LAST_MODIFIED, LIBRARY_ID) \
             VALUES ('s1', 's', 'file:/l/s/', '2020-01-01 00:00:00.0', 'l1')",
            [],
        )
        .unwrap();
        drop(rw);
        BookProjectionDao::new(db)
    }

    fn seed_book(db: &Database, id: &str) {
        db.rw()
            .execute(
                "INSERT INTO BOOK (ID, NAME, URL, FILE_LAST_MODIFIED, SERIES_ID, LIBRARY_ID) \
                 VALUES (?, ?, 'file:/l/s/b.epub', '2020-01-01 00:00:00.0', 's1', 'l1')",
                rusqlite::params![id, id],
            )
            .unwrap();
    }

    fn projection(book_id: &str, profile: &str, file_size: i64) -> BookProjection {
        BookProjection {
            book_id: book_id.into(),
            profile: profile.into(),
            file_size,
            created_date: time_codec::now_utc(),
            last_modified_date: time_codec::now_utc(),
        }
    }

    #[test]
    fn save_find_delete() {
        let dao = dao();
        seed_book(&dao.db, "b1");
        seed_book(&dao.db, "b2");
        dao.save(&projection("b1", "kepub_default", 100)).unwrap();
        dao.save(&projection("b1", "other", 200)).unwrap();
        dao.save(&projection("b2", "kepub_default", 300)).unwrap();

        let found = dao.find_by_book_ids(&["b1".to_string()]).unwrap();
        assert_eq!(found.len(), 2);

        // a conflict refreshes the stored size
        dao.save(&projection("b1", "kepub_default", 999)).unwrap();
        let found = dao.find_by_book_ids(&["b1".to_string()]).unwrap();
        assert_eq!(found.len(), 2);
        let kepub = found.iter().find(|p| p.profile == "kepub_default").unwrap();
        assert_eq!(kepub.file_size, 999);

        dao.delete("b1").unwrap();
        assert!(dao
            .find_by_book_ids(&["b1".to_string()])
            .unwrap()
            .is_empty());
        assert_eq!(
            dao.find_by_book_ids(&["b1".to_string(), "b2".to_string()])
                .unwrap()
                .len(),
            1
        );
        dao.delete_by_book_ids(&["b2".to_string()]).unwrap();
        assert!(dao
            .find_by_book_ids(&["b2".to_string()])
            .unwrap()
            .is_empty());
    }

    #[test]
    fn save_requires_persisted_book() {
        let dao = dao();
        assert!(dao.save(&projection("ghost", "kepub_default", 1)).is_err());
    }
}
