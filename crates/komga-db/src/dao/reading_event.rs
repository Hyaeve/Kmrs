//! The `READING_EVENT` table in the separate `kmrs.sqlite` database: one append-only row
//! per read-progress change. READ_PROGRESS only keeps the latest position per book, so
//! per-day page counts and activity streaks cannot be reconstructed from the main
//! database — they are computed from these rows instead.

use super::get_datetime;
use crate::error::Result;
use crate::pool::Database;
use komga_core::time_codec;
use rusqlite::Row;
use std::collections::{BTreeSet, HashSet};
use time::{Date, OffsetDateTime};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadingEvent {
    pub id: i64,
    pub user_id: String,
    pub book_id: String,
    pub series_id: String,
    pub page: i32,
    pub created_date: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewReadingEvent {
    pub user_id: String,
    pub book_id: String,
    pub series_id: String,
    pub page: i32,
    pub created_date: OffsetDateTime,
}

pub struct ReadingEventDao {
    db: Database,
}

const COLUMNS: &str = "ID, USER_ID, BOOK_ID, SERIES_ID, PAGE, CREATED_DATE";

fn row_to_event(row: &Row<'_>) -> rusqlite::Result<ReadingEvent> {
    Ok(ReadingEvent {
        id: row.get(0)?,
        user_id: row.get(1)?,
        book_id: row.get(2)?,
        series_id: row.get(3)?,
        page: row.get(4)?,
        created_date: get_datetime(row, 5)?,
    })
}

impl ReadingEventDao {
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    pub fn insert(&self, event: &NewReadingEvent) -> Result<()> {
        self.db.rw()?.execute(
            "INSERT INTO READING_EVENT (USER_ID, BOOK_ID, SERIES_ID, PAGE, CREATED_DATE) \
             VALUES (?,?,?,?,?)",
            rusqlite::params![
                event.user_id,
                event.book_id,
                event.series_id,
                event.page,
                time_codec::format_datetime(event.created_date),
            ],
        )?;
        Ok(())
    }

    /// Every event of one user, full history. The (book, time, id) order lets per-book
    /// page deltas replay in write order; `OffsetDateTime::date()` on the mapped rows
    /// truncates the UTC storage the same way `date()` would in SQL.
    pub fn find_all_by_user(&self, user_id: &str) -> Result<Vec<ReadingEvent>> {
        let conn = self.db.ro()?;
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLUMNS} FROM READING_EVENT WHERE USER_ID = ? ORDER BY BOOK_ID, CREATED_DATE, ID"
        ))?;
        let rows = stmt
            .query_map([user_id], row_to_event)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Every distinct event date of the user within the given (visible) series,
    /// ascending. The series filter stands in for the visibility join the kmrs database
    /// cannot do, and querying dates directly keeps the summary endpoint from loading
    /// the full event history.
    pub fn find_activity_dates(
        &self,
        user_id: &str,
        series_ids: &HashSet<String>,
    ) -> Result<Vec<Date>> {
        if series_ids.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.db.ro()?;
        let mut dates = BTreeSet::new();
        let ids: Vec<&String> = series_ids.iter().collect();
        // chunked to stay under SQLite's variable limit
        for chunk in ids.chunks(500) {
            let placeholders = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");
            let mut stmt = conn.prepare(&format!(
                "SELECT DISTINCT date(CREATED_DATE) FROM READING_EVENT \
                 WHERE USER_ID = ? AND SERIES_ID IN ({placeholders})"
            ))?;
            let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(user_id.to_string())];
            params.extend(
                chunk
                    .iter()
                    .map(|id| Box::new((*id).clone()) as Box<dyn rusqlite::ToSql>),
            );
            let rows = stmt
                .query_map(rusqlite::params_from_iter(params), |row| {
                    let s: String = row.get(0)?;
                    time_codec::parse_date(&s)
                        .ok_or_else(|| super::invalid_column(row, 0, "date", &s))
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            dates.extend(rows);
        }
        Ok(dates.into_iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Migrator;
    use crate::Placeholders;

    fn test_dao() -> ReadingEventDao {
        let db = Database::open_in_memory(false).unwrap();
        Migrator::new(&crate::kmrs_migrations(), Placeholders::default())
            .migrate(&db.rw().unwrap())
            .unwrap();
        ReadingEventDao::new(db)
    }

    fn at(s: &str) -> OffsetDateTime {
        time_codec::parse_datetime_utc(s).unwrap()
    }

    fn event(book_id: &str, page: i32, created_date: &str) -> NewReadingEvent {
        NewReadingEvent {
            user_id: "u1".into(),
            book_id: book_id.into(),
            series_id: "s1".into(),
            page,
            created_date: at(created_date),
        }
    }

    #[test]
    fn insert_and_find_all_orders_by_book_then_time() {
        let dao = test_dao();
        dao.insert(&event("b1", 20, "2026-09-30 09:00:00")).unwrap();
        dao.insert(&event("b2", 3, "2026-09-29 08:00:00")).unwrap();
        dao.insert(&event("b1", 10, "2026-09-29 10:00:00")).unwrap();
        dao.insert(&NewReadingEvent {
            user_id: "u2".into(),
            ..event("b1", 99, "2026-09-29 11:00:00")
        })
        .unwrap();

        let rows = dao.find_all_by_user("u1").unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(
            rows.iter()
                .map(|e| (e.book_id.as_str(), e.page, e.series_id.as_str()))
                .collect::<Vec<_>>(),
            [("b1", 10, "s1"), ("b1", 20, "s1"), ("b2", 3, "s1")]
        );
        assert!(rows.iter().all(|e| e.id > 0 && e.user_id == "u1"));

        assert_eq!(dao.find_all_by_user("u2").unwrap().len(), 1);
        assert!(dao.find_all_by_user("nobody").unwrap().is_empty());
    }

    #[test]
    fn activity_dates_are_distinct_sorted_and_scoped_to_the_given_series() {
        let dao = test_dao();
        dao.insert(&event("b1", 10, "2026-09-30 09:00:00")).unwrap();
        dao.insert(&event("b1", 20, "2026-09-30 18:00:00")).unwrap();
        dao.insert(&event("b2", 3, "2026-09-28 08:00:00")).unwrap();
        dao.insert(&NewReadingEvent {
            series_id: "s2".into(),
            ..event("b3", 5, "2026-09-29 08:00:00")
        })
        .unwrap();

        let day = |s: &str| time_codec::parse_date(s).unwrap();
        assert_eq!(
            dao.find_activity_dates("u1", &HashSet::from(["s1".to_string(), "s2".to_string()]))
                .unwrap(),
            [day("2026-09-28"), day("2026-09-29"), day("2026-09-30")]
        );
        assert_eq!(
            dao.find_activity_dates("u1", &HashSet::from(["s1".to_string()]))
                .unwrap(),
            [day("2026-09-28"), day("2026-09-30")]
        );
        assert!(dao
            .find_activity_dates("u1", &HashSet::new())
            .unwrap()
            .is_empty());
        assert!(dao
            .find_activity_dates("nobody", &HashSet::from(["s1".to_string()]))
            .unwrap()
            .is_empty());
    }
}
