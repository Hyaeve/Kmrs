//! DAO for READ_PROGRESS + READ_PROGRESS_SERIES.
//! Aggregate maintenance follows `ReadProgressDao.aggregateSeriesProgress`: after a
//! progress change, the per-series aggregates are recomputed in full.

use super::{get_datetime, get_datetime_opt};
use crate::error::Result;
use crate::pool::Database;
use komga_core::model::read_progress::{ReadProgress, ReadProgressSeries};
use komga_core::time_codec;
use rusqlite::{params, Row};
use std::io::{Read, Write};

const COLUMNS: &str = "BOOK_ID, USER_ID, PAGE, COMPLETED, READ_DATE, DEVICE_ID, DEVICE_NAME, LOCATOR, CREATED_DATE, LAST_MODIFIED_DATE";

const SERIES_COLUMNS: &str =
    "SERIES_ID, USER_ID, READ_COUNT, IN_PROGRESS_COUNT, MOST_RECENT_READ_DATE, LAST_MODIFIED_DATE";

pub struct ReadProgressDao {
    db: Database,
}

impl ReadProgressDao {
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    fn row_to_progress(row: &Row<'_>) -> rusqlite::Result<ReadProgress> {
        let locator: Option<Vec<u8>> = row.get(7)?;
        Ok(ReadProgress {
            book_id: row.get(0)?,
            user_id: row.get(1)?,
            page: row.get(2)?,
            completed: row.get(3)?,
            read_date: get_datetime(row, 4)?,
            device_id: row.get(5)?,
            device_name: row.get(6)?,
            locator: locator
                .map(|b| {
                    gz_decode(&b).ok_or_else(|| super::invalid_column(7, "locator", "gzip+json"))
                })
                .transpose()?,
            created_date: get_datetime(row, 8)?,
            last_modified_date: get_datetime(row, 9)?,
        })
    }

    fn row_to_series(row: &Row<'_>) -> rusqlite::Result<ReadProgressSeries> {
        Ok(ReadProgressSeries {
            series_id: row.get(0)?,
            user_id: row.get(1)?,
            read_count: row.get(2)?,
            in_progress_count: row.get(3)?,
            most_recent_read_date: get_datetime_opt(row, 4)?,
            last_modified_date: get_datetime_opt(row, 5)?,
        })
    }

    pub fn find_all(&self) -> Result<Vec<ReadProgress>> {
        let conn = self.db.ro();
        let mut stmt = conn.prepare(&format!("SELECT {COLUMNS} FROM READ_PROGRESS"))?;
        let rows = stmt
            .query_map([], Self::row_to_progress)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn find_by_book_and_user(
        &self,
        book_id: &str,
        user_id: &str,
    ) -> Result<Option<ReadProgress>> {
        let conn = self.db.ro();
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLUMNS} FROM READ_PROGRESS WHERE BOOK_ID = ? AND USER_ID = ?"
        ))?;
        let row = stmt
            .query_map(params![book_id, user_id], Self::row_to_progress)?
            .collect::<std::result::Result<Vec<_>, _>>()?
            .into_iter()
            .next();
        Ok(row)
    }

    pub fn find_by_user(&self, user_id: &str) -> Result<Vec<ReadProgress>> {
        let conn = self.db.ro();
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLUMNS} FROM READ_PROGRESS WHERE USER_ID = ?"
        ))?;
        let rows = stmt
            .query_map([user_id], Self::row_to_progress)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn find_by_book(&self, book_id: &str) -> Result<Vec<ReadProgress>> {
        let conn = self.db.ro();
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLUMNS} FROM READ_PROGRESS WHERE BOOK_ID = ?"
        ))?;
        let rows = stmt
            .query_map([book_id], Self::row_to_progress)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn find_by_books_and_user(
        &self,
        book_ids: &[String],
        user_id: &str,
    ) -> Result<Vec<ReadProgress>> {
        if book_ids.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.db.ro();
        let placeholders = book_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLUMNS} FROM READ_PROGRESS WHERE BOOK_ID IN ({placeholders}) AND USER_ID = ?"
        ))?;
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = book_ids
            .iter()
            .map(|id| Box::new(id.clone()) as Box<dyn rusqlite::ToSql>)
            .collect();
        params.push(Box::new(user_id.to_string()));
        let rows = stmt
            .query_map(rusqlite::params_from_iter(params), Self::row_to_progress)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Aligned with Java `save`: upsert (on insert, CREATED/LAST_MODIFIED use DB
    /// defaults; on conflict, LAST_MODIFIED is set to the app-side UTC now), then
    /// recompute the aggregates for the series the book belongs to.
    pub fn insert_or_update(&self, progress: &ReadProgress) -> Result<()> {
        let conn = self.db.rw();
        conn.execute(
      "INSERT INTO READ_PROGRESS (BOOK_ID, USER_ID, PAGE, COMPLETED, READ_DATE, DEVICE_ID, DEVICE_NAME, LOCATOR) \
       VALUES (?,?,?,?,?,?,?,?) \
       ON CONFLICT(BOOK_ID, USER_ID) DO UPDATE SET \
       PAGE = excluded.PAGE, COMPLETED = excluded.COMPLETED, READ_DATE = excluded.READ_DATE, \
       LAST_MODIFIED_DATE = ?, DEVICE_ID = excluded.DEVICE_ID, DEVICE_NAME = excluded.DEVICE_NAME, LOCATOR = excluded.LOCATOR",
      params![
        progress.book_id,
        progress.user_id,
        progress.page,
        progress.completed,
        time_codec::format_datetime(progress.read_date),
        progress.device_id,
        progress.device_name,
        progress.locator.as_ref().map(gz_encode).transpose()?,
        time_codec::format_datetime(time_codec::now_utc()),
      ],
    )?;
        drop(conn);
        self.aggregate_series_progress(
            std::slice::from_ref(&progress.book_id),
            Some(&progress.user_id),
        )?;
        Ok(())
    }

    /// Aligned with Java `delete`: recompute the aggregates after deleting the row.
    pub fn delete(&self, book_id: &str, user_id: &str) -> Result<()> {
        let conn = self.db.rw();
        conn.execute(
            "DELETE FROM READ_PROGRESS WHERE BOOK_ID = ? AND USER_ID = ?",
            params![book_id, user_id],
        )?;
        drop(conn);
        self.aggregate_series_progress(std::slice::from_ref(&book_id.to_string()), Some(user_id))?;
        Ok(())
    }

    pub fn delete_by_book(&self, book_id: &str) -> Result<()> {
        let conn = self.db.rw();
        conn.execute("DELETE FROM READ_PROGRESS WHERE BOOK_ID = ?", [book_id])?;
        drop(conn);
        self.aggregate_series_progress(std::slice::from_ref(&book_id.to_string()), None)?;
        Ok(())
    }

    pub fn delete_by_user(&self, user_id: &str) -> Result<()> {
        let conn = self.db.rw();
        conn.execute("DELETE FROM READ_PROGRESS WHERE USER_ID = ?", [user_id])?;
        conn.execute(
            "DELETE FROM READ_PROGRESS_SERIES WHERE USER_ID = ?",
            [user_id],
        )?;
        Ok(())
    }

    /// Aligned with `aggregateSeriesProgress`: first delete the old aggregates of
    /// the affected series, then recompute them in full from BOOK ⨝ READ_PROGRESS.
    pub fn aggregate_series_progress(
        &self,
        book_ids: &[String],
        user_id: Option<&str>,
    ) -> Result<()> {
        if book_ids.is_empty() {
            return Ok(());
        }
        let conn = self.db.rw();
        let placeholders = book_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let series_query = format!("SELECT SERIES_ID FROM BOOK WHERE ID IN ({placeholders})");
        let make_params = || -> Vec<Box<dyn rusqlite::ToSql>> {
            book_ids
                .iter()
                .map(|id| Box::new(id.clone()) as Box<dyn rusqlite::ToSql>)
                .collect()
        };

        let mut delete_sql =
            format!("DELETE FROM READ_PROGRESS_SERIES WHERE SERIES_ID IN ({series_query})");
        let mut delete_params = make_params();
        if let Some(uid) = user_id {
            delete_sql.push_str(" AND USER_ID = ?");
            delete_params.push(Box::new(uid.to_string()));
        }
        conn.execute(&delete_sql, rusqlite::params_from_iter(delete_params))?;

        let mut insert_sql = format!(
      "INSERT INTO READ_PROGRESS_SERIES (SERIES_ID, USER_ID, READ_COUNT, IN_PROGRESS_COUNT, MOST_RECENT_READ_DATE, LAST_MODIFIED_DATE) \
       SELECT b.SERIES_ID, r.USER_ID, \
       SUM(CASE WHEN r.COMPLETED THEN 1 ELSE 0 END), \
       SUM(CASE WHEN r.COMPLETED THEN 0 ELSE 1 END), \
       MAX(r.READ_DATE), CURRENT_TIMESTAMP \
       FROM BOOK b INNER JOIN READ_PROGRESS r ON b.ID = r.BOOK_ID \
       WHERE b.SERIES_ID IN ({series_query})"
    );
        let mut insert_params = make_params();
        if let Some(uid) = user_id {
            insert_sql.push_str(" AND r.USER_ID = ?");
            insert_params.push(Box::new(uid.to_string()));
        }
        insert_sql.push_str(" GROUP BY b.SERIES_ID, r.USER_ID");
        conn.execute(&insert_sql, rusqlite::params_from_iter(insert_params))?;
        Ok(())
    }

    pub fn find_series(
        &self,
        series_id: &str,
        user_id: &str,
    ) -> Result<Option<ReadProgressSeries>> {
        let conn = self.db.ro();
        let mut stmt = conn.prepare(&format!(
            "SELECT {SERIES_COLUMNS} FROM READ_PROGRESS_SERIES WHERE SERIES_ID = ? AND USER_ID = ?"
        ))?;
        let row = stmt
            .query_map(params![series_id, user_id], Self::row_to_series)?
            .collect::<std::result::Result<Vec<_>, _>>()?
            .into_iter()
            .next();
        Ok(row)
    }

    pub fn find_series_by_user(&self, user_id: &str) -> Result<Vec<ReadProgressSeries>> {
        let conn = self.db.ro();
        let mut stmt = conn.prepare(&format!(
            "SELECT {SERIES_COLUMNS} FROM READ_PROGRESS_SERIES WHERE USER_ID = ?"
        ))?;
        let rows = stmt
            .query_map([user_id], Self::row_to_series)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn delete_series_by_series(&self, series_id: &str) -> Result<()> {
        let conn = self.db.rw();
        conn.execute(
            "DELETE FROM READ_PROGRESS_SERIES WHERE SERIES_ID = ?",
            [series_id],
        )?;
        Ok(())
    }
}

fn gz_encode(value: &serde_json::Value) -> Result<Vec<u8>> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    // serde_json::Value's map keys are always strings, so serialization cannot fail
    encoder
        .write_all(value.to_string().as_bytes())
        .and_then(|_| encoder.finish())
        .map_err(|e| crate::error::Error::Db(rusqlite::Error::ToSqlConversionFailure(e.into())))
}

fn gz_decode(bytes: &[u8]) -> Option<serde_json::Value> {
    let mut json = Vec::new();
    flate2::read::GzDecoder::new(bytes)
        .read_to_end(&mut json)
        .ok()?;
    serde_json::from_slice(&json).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migrate::Migrator;
    use crate::{main_migrations, Placeholders};
    use komga_core::time_codec::now_utc;

    fn dao() -> ReadProgressDao {
        let db = Database::open_in_memory(true).unwrap();
        let migrations = main_migrations();
        Migrator::new(&migrations, Placeholders::default())
            .migrate(&db.rw())
            .unwrap();
        ReadProgressDao::new(db)
    }

    fn seed_book(db: &Database, book_id: &str, series_id: &str) {
        let conn = db.rw();
        conn.execute(
            "INSERT OR IGNORE INTO LIBRARY (ID, NAME, ROOT) VALUES ('lib1', 'L', 'file:/l/')",
            [],
        )
        .unwrap();
        conn.execute(
      "INSERT OR IGNORE INTO SERIES (ID, NAME, URL, FILE_LAST_MODIFIED, LIBRARY_ID) VALUES (?, 'S', 'file:/l/s/', '2020-01-01 00:00:00.0', 'lib1')",
      [series_id],
    ).unwrap();
        conn.execute(
      "INSERT INTO BOOK (ID, NAME, URL, FILE_LAST_MODIFIED, SERIES_ID, LIBRARY_ID) VALUES (?, 'B', 'file:/l/s/b.cbz', '2020-01-01 00:00:00.0', ?, 'lib1')",
      params![book_id, series_id],
    ).unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO USER (ID, EMAIL, PASSWORD) VALUES ('u1', 'a@b.c', 'x')",
            [],
        )
        .unwrap();
    }

    fn sample(book_id: &str) -> ReadProgress {
        ReadProgress {
            book_id: book_id.into(),
            user_id: "u1".into(),
            page: 12,
            completed: false,
            read_date: now_utc(),
            device_id: "dev-1".into(),
            device_name: "Tablet".into(),
            locator: Some(serde_json::json!({
              "href": "OEBPS/ch1.xhtml",
              "type": "application/xhtml+xml",
              "locations": {"progression": 0.5}
            })),
            created_date: now_utc(),
            last_modified_date: now_utc(),
        }
    }

    #[test]
    fn upsert_and_locator_roundtrip() {
        let dao = dao();
        seed_book(&dao.db, "b1", "s1");

        dao.insert_or_update(&sample("b1")).unwrap();
        let found = dao
            .find_by_book_and_user("b1", "u1")
            .unwrap()
            .expect("not found");
        assert_eq!(found.page, 12);
        assert!(!found.completed);
        assert_eq!(found.device_id, "dev-1");
        assert_eq!(found.device_name, "Tablet");
        assert_eq!(
            found.locator.as_ref().unwrap()["href"],
            serde_json::json!("OEBPS/ch1.xhtml")
        );

        // update: page and completion state change, aggregate table stays in sync
        let mut updated = sample("b1");
        updated.page = 200;
        updated.completed = true;
        updated.locator = None;
        dao.insert_or_update(&updated).unwrap();
        let found = dao.find_by_book_and_user("b1", "u1").unwrap().unwrap();
        assert_eq!(found.page, 200);
        assert!(found.completed);
        assert!(found.locator.is_none());

        let series = dao
            .find_series("s1", "u1")
            .unwrap()
            .expect("aggregate missing");
        assert_eq!(series.read_count, 1);
        assert_eq!(series.in_progress_count, 0);
        assert!(series.most_recent_read_date.is_some());
        assert!(series.last_modified_date.is_some());

        // after deletion the aggregate is gone
        dao.delete("b1", "u1").unwrap();
        assert!(dao.find_by_book_and_user("b1", "u1").unwrap().is_none());
        assert!(dao.find_series("s1", "u1").unwrap().is_none());
    }

    #[test]
    fn aggregate_counts_mixed_states() {
        let dao = dao();
        seed_book(&dao.db, "b1", "s1");
        seed_book(&dao.db, "b2", "s1");

        let mut p1 = sample("b1");
        p1.completed = true;
        dao.insert_or_update(&p1).unwrap();
        dao.insert_or_update(&sample("b2")).unwrap();

        let series = dao.find_series("s1", "u1").unwrap().unwrap();
        assert_eq!(series.read_count, 1);
        assert_eq!(series.in_progress_count, 1);

        assert_eq!(dao.find_by_user("u1").unwrap().len(), 2);
        assert_eq!(dao.find_by_book("b1").unwrap().len(), 1);
        assert_eq!(
            dao.find_by_books_and_user(&["b1".to_string(), "b2".to_string()], "u1")
                .unwrap()
                .len(),
            2
        );

        dao.delete_by_user("u1").unwrap();
        assert_eq!(dao.find_by_user("u1").unwrap().len(), 0);
        assert_eq!(dao.find_series_by_user("u1").unwrap().len(), 0);
    }
}
