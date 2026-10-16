//! Aggregation queries for the kmrs-private reading-stats endpoint. Visibility (library
//! sharing + content restrictions) reuses the search-layer SQL fragments
//! (`content_restrictions_condition` / `library_ids_condition`), so the statistics count
//! exactly the books a search would return. kmrs-private, no Java equivalent.

use crate::error::Result;
use crate::pool::Database;
use crate::search_sql::{RequiredJoin, SqlWhere};
use komga_core::time_codec;
use rusqlite::types::Value;
use std::collections::HashSet;
use time::{Date, OffsetDateTime};

pub struct ReadingStatsDtoDao {
    db: Database,
}

/// One-row totals of the summary block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadingTotals {
    pub total_books: i64,
    pub books_started: i64,
    pub books_completed: i64,
    /// Completed books count their MEDIA page count, in-progress ones their current page.
    pub pages_read: i64,
    /// The completed-books part of `pages_read` (the `averagePagesPerBook` numerator).
    pub completed_pages_read: i64,
}

/// A visible completed book with its page count and completion day: the backfill source
/// for time-series days of books that have no READING_EVENT rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletedBook {
    pub book_id: String,
    pub page_count: i64,
    pub read_day: Date,
}

/// SERIES_METADATA joins on its primary key (1:1), so when the visibility fragment
/// does not reference it (no content restrictions) the join cannot change the result
/// and is skipped. `series_id_column` is the left side of the join condition.
fn series_metadata_join(visibility: &SqlWhere, series_id_column: &str) -> String {
    if visibility.joins.contains(&RequiredJoin::SeriesMetadata) {
        format!(" LEFT JOIN SERIES_METADATA ON ({series_id_column} = SERIES_METADATA.SERIES_ID)")
    } else {
        String::new()
    }
}

/// FROM skeleton over READ_PROGRESS ⨝ BOOK, with the metadata join the visibility
/// fragment may reference.
fn progress_from(visibility: &SqlWhere) -> String {
    format!(
        "FROM READ_PROGRESS \
         INNER JOIN BOOK ON (READ_PROGRESS.BOOK_ID = BOOK.ID){}",
        series_metadata_join(visibility, "BOOK.SERIES_ID")
    )
}

/// FROM skeleton over BOOK ⨝ MEDIA ⨝ READ_PROGRESS for [`ReadingStatsDtoDao::totals`];
/// the progress user id binds first (JOIN precedes WHERE).
fn totals_from(visibility: &SqlWhere) -> String {
    format!(
        "FROM BOOK \
         LEFT JOIN MEDIA ON (BOOK.ID = MEDIA.BOOK_ID) \
         LEFT JOIN READ_PROGRESS ON (BOOK.ID = READ_PROGRESS.BOOK_ID AND READ_PROGRESS.USER_ID = ?){}",
        series_metadata_join(visibility, "BOOK.SERIES_ID")
    )
}

fn where_clause(visibility: &SqlWhere) -> String {
    if visibility.sql.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", visibility.sql)
    }
}

fn and_where(visibility: &SqlWhere) -> String {
    if visibility.sql.is_empty() {
        String::new()
    } else {
        format!(" AND ({})", visibility.sql)
    }
}

fn user_params(user_id: &str, visibility: &SqlWhere) -> Vec<Value> {
    std::iter::once(Value::Text(user_id.to_string()))
        .chain(visibility.params.iter().cloned())
        .collect()
}

/// Series with at least one completed visible book: the counting base of the
/// top/distribution lists.
fn completed_series(visibility: &SqlWhere) -> String {
    format!(
        "SELECT DISTINCT BOOK.SERIES_ID {} \
         WHERE READ_PROGRESS.USER_ID = ? AND READ_PROGRESS.COMPLETED = 1{}",
        progress_from(visibility),
        and_where(visibility)
    )
}

impl ReadingStatsDtoDao {
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    /// Totals over all visible books.
    pub fn totals(&self, user_id: &str, visibility: &SqlWhere) -> Result<ReadingTotals> {
        let conn = self.db.ro()?;
        let sql = format!(
            "SELECT COUNT(*), \
               COALESCE(SUM(CASE WHEN READ_PROGRESS.BOOK_ID IS NOT NULL THEN 1 ELSE 0 END), 0), \
               COALESCE(SUM(CASE WHEN READ_PROGRESS.COMPLETED = 1 THEN 1 ELSE 0 END), 0), \
               COALESCE(SUM(CASE WHEN READ_PROGRESS.COMPLETED = 1 THEN MEDIA.PAGE_COUNT ELSE READ_PROGRESS.PAGE END), 0), \
               COALESCE(SUM(CASE WHEN READ_PROGRESS.COMPLETED = 1 THEN MEDIA.PAGE_COUNT ELSE 0 END), 0) \
             {}{}",
            totals_from(visibility),
            where_clause(visibility)
        );
        let totals = conn.query_row(
            &sql,
            rusqlite::params_from_iter(user_params(user_id, visibility)),
            |row| {
                Ok(ReadingTotals {
                    total_books: row.get(0)?,
                    books_started: row.get(1)?,
                    books_completed: row.get(2)?,
                    pages_read: row.get(3)?,
                    completed_pages_read: row.get(4)?,
                })
            },
        )?;
        Ok(totals)
    }

    /// Distinct READ_DATE days of the user's visible books, ascending (streak input).
    pub fn read_dates(&self, user_id: &str, visibility: &SqlWhere) -> Result<Vec<Date>> {
        let conn = self.db.ro()?;
        let sql = format!(
            "SELECT DISTINCT date(READ_PROGRESS.READ_DATE) {} \
             WHERE READ_PROGRESS.USER_ID = ?{} ORDER BY 1",
            progress_from(visibility),
            and_where(visibility)
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt
            .query_map(
                rusqlite::params_from_iter(user_params(user_id, visibility)),
                |row| {
                    let s: String = row.get(0)?;
                    time_codec::parse_date(&s)
                        .ok_or_else(|| crate::dao::invalid_column(row, 0, "date", &s))
                },
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn last_read_date(
        &self,
        user_id: &str,
        visibility: &SqlWhere,
    ) -> Result<Option<OffsetDateTime>> {
        let conn = self.db.ro()?;
        let sql = format!(
            "SELECT MAX(READ_PROGRESS.READ_DATE) {} \
             WHERE READ_PROGRESS.USER_ID = ?{}",
            progress_from(visibility),
            and_where(visibility)
        );
        let last = conn.query_row(
            &sql,
            rusqlite::params_from_iter(user_params(user_id, visibility)),
            |row| {
                let s: Option<String> = row.get(0)?;
                match s {
                    Some(s) => time_codec::parse_datetime_utc(&s)
                        .map(Some)
                        .ok_or_else(|| crate::dao::invalid_column(row, 0, "datetime", &s)),
                    None => Ok(None),
                }
            },
        )?;
        Ok(last)
    }

    /// Completed-book counts per READ_DATE day over the full history (time-series
    /// backfill; days stay UTC, the timezone offset only applies to the weekday/hour
    /// buckets).
    pub fn completions_by_day(
        &self,
        user_id: &str,
        visibility: &SqlWhere,
    ) -> Result<Vec<(Date, i64)>> {
        let conn = self.db.ro()?;
        let sql = format!(
            "SELECT date(READ_PROGRESS.READ_DATE), COUNT(*) {} \
             WHERE READ_PROGRESS.USER_ID = ? AND READ_PROGRESS.COMPLETED = 1{} \
             GROUP BY 1 ORDER BY 1",
            progress_from(visibility),
            and_where(visibility)
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt
            .query_map(
                rusqlite::params_from_iter(user_params(user_id, visibility)),
                |row| {
                    let s: String = row.get(0)?;
                    let date = time_codec::parse_date(&s)
                        .ok_or_else(|| crate::dao::invalid_column(row, 0, "date", &s))?;
                    Ok((date, row.get(1)?))
                },
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// (book_id, READ_DATE) of every visible book with a progress row, completed or not:
    /// the activity timestamps of books that have no READING_EVENT rows (the weekday /
    /// hour distributions merge both sources in Rust — the event log lives in the kmrs
    /// database and cannot join back).
    pub fn progress_activity(
        &self,
        user_id: &str,
        visibility: &SqlWhere,
    ) -> Result<Vec<(String, OffsetDateTime)>> {
        let conn = self.db.ro()?;
        let sql = format!(
            "SELECT READ_PROGRESS.BOOK_ID, READ_PROGRESS.READ_DATE {} \
             WHERE READ_PROGRESS.USER_ID = ?{}",
            progress_from(visibility),
            and_where(visibility)
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt
            .query_map(
                rusqlite::params_from_iter(user_params(user_id, visibility)),
                |row| {
                    let s: String = row.get(1)?;
                    let read_date = time_codec::parse_datetime_utc(&s)
                        .ok_or_else(|| crate::dao::invalid_column(row, 1, "datetime", &s))?;
                    Ok((row.get::<_, String>(0)?, read_date))
                },
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Ids of the series visible under the fragment: the READING_EVENT filter. The kmrs
    /// database cannot join back to the main one, and visibility is series-level
    /// (library sharing and content restrictions both key off the series), so the
    /// denormalized SERIES_ID on the event is enough.
    pub fn visible_series_ids(&self, visibility: &SqlWhere) -> Result<HashSet<String>> {
        let conn = self.db.ro()?;
        let sql = format!(
            "SELECT SERIES.ID FROM SERIES{}{}",
            series_metadata_join(visibility, "SERIES.ID"),
            where_clause(visibility)
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt
            .query_map(
                rusqlite::params_from_iter(visibility.params.iter().cloned()),
                |row| row.get::<_, String>(0),
            )?
            .collect::<std::result::Result<HashSet<_>, _>>()?;
        Ok(rows)
    }

    /// Visible completed books with their page count and completion day.
    pub fn completed_book_page_counts(
        &self,
        user_id: &str,
        visibility: &SqlWhere,
    ) -> Result<Vec<CompletedBook>> {
        let conn = self.db.ro()?;
        let sql = format!(
            "SELECT READ_PROGRESS.BOOK_ID, COALESCE(MEDIA.PAGE_COUNT, 0), date(READ_PROGRESS.READ_DATE) \
             FROM READ_PROGRESS \
             INNER JOIN BOOK ON (READ_PROGRESS.BOOK_ID = BOOK.ID) \
             LEFT JOIN MEDIA ON (BOOK.ID = MEDIA.BOOK_ID){} \
             WHERE READ_PROGRESS.USER_ID = ? AND READ_PROGRESS.COMPLETED = 1{}",
            series_metadata_join(visibility, "BOOK.SERIES_ID"),
            and_where(visibility)
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt
            .query_map(
                rusqlite::params_from_iter(user_params(user_id, visibility)),
                |row| {
                    let s: String = row.get(2)?;
                    Ok(CompletedBook {
                        book_id: row.get(0)?,
                        page_count: row.get(1)?,
                        read_day: time_codec::parse_date(&s)
                            .ok_or_else(|| crate::dao::invalid_column(row, 2, "date", &s))?,
                    })
                },
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Genre counts: each genre counts once per series with a completed book.
    pub fn genre_counts(&self, user_id: &str, visibility: &SqlWhere) -> Result<Vec<(String, i64)>> {
        let sql = format!(
            "SELECT SERIES_METADATA_GENRE.GENRE, COUNT(DISTINCT SERIES_METADATA_GENRE.SERIES_ID) \
             FROM SERIES_METADATA_GENRE \
             WHERE SERIES_METADATA_GENRE.SERIES_ID IN ({}) GROUP BY 1",
            completed_series(visibility)
        );
        self.name_counts(&sql, user_params(user_id, visibility))
    }

    /// Tag counts: series tags plus the tags of the completed books themselves, each tag
    /// counting once per series.
    pub fn tag_counts(&self, user_id: &str, visibility: &SqlWhere) -> Result<Vec<(String, i64)>> {
        let sql = format!(
            "SELECT name, COUNT(DISTINCT series_id) FROM ( \
               SELECT SERIES_METADATA_TAG.SERIES_ID AS series_id, SERIES_METADATA_TAG.TAG AS name \
               FROM SERIES_METADATA_TAG \
               WHERE SERIES_METADATA_TAG.SERIES_ID IN ({}) \
               UNION \
               SELECT BOOK.SERIES_ID, BOOK_METADATA_TAG.TAG \
               FROM BOOK_METADATA_TAG \
               INNER JOIN BOOK ON (BOOK_METADATA_TAG.BOOK_ID = BOOK.ID) \
               INNER JOIN READ_PROGRESS ON (BOOK_METADATA_TAG.BOOK_ID = READ_PROGRESS.BOOK_ID \
                   AND READ_PROGRESS.USER_ID = ? AND READ_PROGRESS.COMPLETED = 1){}{} \
             ) GROUP BY name",
            completed_series(visibility),
            series_metadata_join(visibility, "BOOK.SERIES_ID"),
            where_clause(visibility)
        );
        let mut params = user_params(user_id, visibility);
        params.extend(user_params(user_id, visibility));
        self.name_counts(&sql, params)
    }

    /// Author counts: series-aggregated authors plus the authors of the completed books
    /// (no role filter), each name counting once per series; blank names dropped.
    pub fn author_counts(
        &self,
        user_id: &str,
        visibility: &SqlWhere,
    ) -> Result<Vec<(String, i64)>> {
        let sql = format!(
            "SELECT name, COUNT(DISTINCT series_id) FROM ( \
               SELECT BOOK_METADATA_AGGREGATION_AUTHOR.SERIES_ID AS series_id, BOOK_METADATA_AGGREGATION_AUTHOR.NAME AS name \
               FROM BOOK_METADATA_AGGREGATION_AUTHOR \
               WHERE BOOK_METADATA_AGGREGATION_AUTHOR.SERIES_ID IN ({}) \
               UNION \
               SELECT BOOK.SERIES_ID, BOOK_METADATA_AUTHOR.NAME \
               FROM BOOK_METADATA_AUTHOR \
               INNER JOIN BOOK ON (BOOK_METADATA_AUTHOR.BOOK_ID = BOOK.ID) \
               INNER JOIN READ_PROGRESS ON (BOOK_METADATA_AUTHOR.BOOK_ID = READ_PROGRESS.BOOK_ID \
                   AND READ_PROGRESS.USER_ID = ? AND READ_PROGRESS.COMPLETED = 1){}{} \
             ) WHERE name <> '' GROUP BY name",
            completed_series(visibility),
            series_metadata_join(visibility, "BOOK.SERIES_ID"),
            where_clause(visibility)
        );
        let mut params = user_params(user_id, visibility);
        params.extend(user_params(user_id, visibility));
        self.name_counts(&sql, params)
    }

    /// Rows of (name, count) sorted by count desc, name asc (byte-wise): the contract of
    /// every top/distribution list.
    fn name_counts(&self, sql: &str, params: Vec<Value>) -> Result<Vec<(String, i64)>> {
        let conn = self.db.ro()?;
        let mut stmt = conn.prepare(sql)?;
        let mut rows = stmt
            .query_map(rusqlite::params_from_iter(params), |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        rows.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search_sql::{content_restrictions_condition, library_ids_condition};
    use crate::{Migrator, Placeholders};
    use komga_core::model::user::{AgeRestriction, AllowExclude, ContentRestrictions};
    use std::collections::BTreeSet;

    fn db() -> Database {
        let db = Database::open_in_memory(true).unwrap();
        let migrations = crate::main_migrations();
        Migrator::new(&migrations, Placeholders::default())
            .migrate(&db.rw().unwrap())
            .unwrap();
        db
    }

    fn exec(db: &Database, sql: &str, params: impl rusqlite::Params) {
        db.rw().unwrap().execute(sql, params).unwrap();
    }

    fn book_visibility(
        restrictions: &ContentRestrictions,
        ids: Option<BTreeSet<String>>,
    ) -> SqlWhere {
        content_restrictions_condition(restrictions)
            .and(library_ids_condition("BOOK", ids.as_ref()))
    }

    fn series_visibility(
        restrictions: &ContentRestrictions,
        ids: Option<BTreeSet<String>>,
    ) -> SqlWhere {
        content_restrictions_condition(restrictions)
            .and(library_ids_condition("SERIES", ids.as_ref()))
    }

    /// l1 with series s1 (books b1, b2), l2 with series s2 (book b3, age rating 18+).
    /// u1 completed b1 and b3, has b2 in progress, b4 unread (in s2).
    fn seed(db: &Database) {
        exec(
            db,
            "INSERT INTO USER (ID, EMAIL, PASSWORD) VALUES ('u1', 'u@x.y', 'x')",
            [],
        );
        for lib in ["l1", "l2"] {
            exec(
                db,
                "INSERT INTO LIBRARY (ID, NAME, ROOT) VALUES (?, ?, 'file:/data/')",
                [lib, lib],
            );
        }
        exec(db, "INSERT INTO SERIES (ID, NAME, URL, FILE_LAST_MODIFIED, LIBRARY_ID) VALUES ('s1', 's1', 'file:/data/s1/', '2024-01-01 00:00:00.0', 'l1')", []);
        exec(db, "INSERT INTO SERIES_METADATA (SERIES_ID, STATUS, TITLE, TITLE_SORT, AGE_RATING) VALUES ('s1', 'ONGOING', 's1', 's1', 10)", []);
        exec(db, "INSERT INTO SERIES (ID, NAME, URL, FILE_LAST_MODIFIED, LIBRARY_ID) VALUES ('s2', 's2', 'file:/data/s2/', '2024-01-01 00:00:00.0', 'l2')", []);
        exec(db, "INSERT INTO SERIES_METADATA (SERIES_ID, STATUS, TITLE, TITLE_SORT, AGE_RATING) VALUES ('s2', 'ONGOING', 's2', 's2', 21)", []);
        for (id, series, pages) in [
            ("b1", "s1", 100),
            ("b2", "s1", 80),
            ("b3", "s2", 50),
            ("b4", "s2", 60),
        ] {
            exec(
                db,
                "INSERT INTO BOOK (ID, NAME, URL, FILE_LAST_MODIFIED, SERIES_ID, LIBRARY_ID) \
                 VALUES (?, ?, 'file:/data/x.cbz', '2024-01-01 00:00:00.0', ?, ?)",
                rusqlite::params![id, id, series, if series == "s1" { "l1" } else { "l2" }],
            );
            exec(
                db,
                "INSERT INTO MEDIA (BOOK_ID, STATUS, PAGE_COUNT) VALUES (?, 'READY', ?)",
                rusqlite::params![id, pages],
            );
        }
        exec(db, "INSERT INTO READ_PROGRESS (BOOK_ID, USER_ID, PAGE, COMPLETED, READ_DATE) VALUES ('b1', 'u1', 100, 1, '2024-01-05 23:30:00.0')", []);
        exec(db, "INSERT INTO READ_PROGRESS (BOOK_ID, USER_ID, PAGE, COMPLETED, READ_DATE) VALUES ('b2', 'u1', 30, 0, '2024-01-06 10:00:00.0')", []);
        exec(db, "INSERT INTO READ_PROGRESS (BOOK_ID, USER_ID, PAGE, COMPLETED, READ_DATE) VALUES ('b3', 'u1', 50, 1, '2024-01-07 00:30:00.0')", []);
    }

    fn unrestricted() -> ContentRestrictions {
        ContentRestrictions::default()
    }

    #[test]
    fn totals_count_only_visible_books() {
        let db = db();
        seed(&db);
        let dao = ReadingStatsDtoDao::new(db.clone());

        let all = dao
            .totals("u1", &book_visibility(&unrestricted(), None))
            .unwrap();
        assert_eq!(all.total_books, 4);
        assert_eq!(all.books_started, 3);
        assert_eq!(all.books_completed, 2);
        // completed books count their page count, the in-progress one its current page
        assert_eq!(all.pages_read, 100 + 50 + 30);
        assert_eq!(all.completed_pages_read, 100 + 50);

        let l1_only = dao
            .totals(
                "u1",
                &book_visibility(&unrestricted(), Some(BTreeSet::from(["l1".to_string()]))),
            )
            .unwrap();
        assert_eq!(l1_only.total_books, 2);
        assert_eq!(l1_only.books_started, 2);
        assert_eq!(l1_only.books_completed, 1);
        assert_eq!(l1_only.pages_read, 100 + 30);
        assert_eq!(l1_only.completed_pages_read, 100);

        // an empty authorized set matches nothing, like search
        let none = dao
            .totals(
                "u1",
                &book_visibility(&unrestricted(), Some(BTreeSet::new())),
            )
            .unwrap();
        assert_eq!(none.total_books, 0);
        assert_eq!(none.pages_read, 0);
        assert_eq!(none.completed_pages_read, 0);

        // age-restricted user (exclude 18+): s2's books drop out of every number
        let restricted = ContentRestrictions::new(
            Some(AgeRestriction {
                age: 18,
                restriction: AllowExclude::Exclude,
            }),
            BTreeSet::new(),
            BTreeSet::new(),
        );
        let vis = book_visibility(&restricted, None);
        let totals = dao.totals("u1", &vis).unwrap();
        assert_eq!(totals.total_books, 2);
        assert_eq!(totals.books_completed, 1);
        assert_eq!(totals.pages_read, 100 + 30);
        assert_eq!(totals.completed_pages_read, 100);
        assert_eq!(
            dao.last_read_date("u1", &vis)
                .unwrap()
                .map(time_codec::format_datetime),
            Some("2024-01-06 10:00:00.0".to_string())
        );
    }

    #[test]
    fn completions_group_by_day_and_progress_activity_covers_in_progress_books() {
        let db = db();
        seed(&db);
        let dao = ReadingStatsDtoDao::new(db.clone());
        let vis = book_visibility(&unrestricted(), None);

        let day = |s: &str| time_codec::parse_date(s).unwrap();
        assert_eq!(
            dao.completions_by_day("u1", &vis).unwrap(),
            [(day("2024-01-05"), 1), (day("2024-01-07"), 1)]
        );
        assert_eq!(
            dao.read_dates("u1", &vis).unwrap(),
            [day("2024-01-05"), day("2024-01-06"), day("2024-01-07")]
        );

        // every progress row contributes its READ_DATE, completed or not
        let mut activity = dao.progress_activity("u1", &vis).unwrap();
        activity.sort();
        assert_eq!(
            activity,
            [
                (
                    "b1".to_string(),
                    time_codec::parse_datetime_utc("2024-01-05 23:30:00").unwrap()
                ),
                (
                    "b2".to_string(),
                    time_codec::parse_datetime_utc("2024-01-06 10:00:00").unwrap()
                ),
                (
                    "b3".to_string(),
                    time_codec::parse_datetime_utc("2024-01-07 00:30:00").unwrap()
                ),
            ]
        );
        assert_eq!(
            dao.progress_activity(
                "u1",
                &book_visibility(&unrestricted(), Some(BTreeSet::from(["l1".to_string()])))
            )
            .unwrap()
            .len(),
            2
        );
    }

    #[test]
    fn top_lists_count_each_name_once_per_series_and_respect_visibility() {
        let db = db();
        seed(&db);
        exec(&db, "INSERT INTO SERIES_METADATA_GENRE (SERIES_ID, GENRE) VALUES ('s1', 'action'), ('s2', 'drama')", []);
        exec(
            &db,
            "INSERT INTO SERIES_METADATA_TAG (SERIES_ID, TAG) VALUES ('s1', 'favorite')",
            [],
        );
        exec(&db, "INSERT INTO BOOK_METADATA_TAG (BOOK_ID, TAG) VALUES ('b1', 'favorite'), ('b1', 'classic'), ('b3', 'drama-tag')", []);
        exec(&db, "INSERT INTO BOOK_METADATA_AGGREGATION_AUTHOR (SERIES_ID, NAME, ROLE) VALUES ('s1', 'author-a', 'writer'), ('s2', 'author-b', 'writer')", []);
        exec(&db, "INSERT INTO BOOK_METADATA_AUTHOR (BOOK_ID, NAME, ROLE) VALUES ('b1', 'author-a', 'writer'), ('b1', '', 'writer'), ('b3', 'author-c', 'artist')", []);
        let dao = ReadingStatsDtoDao::new(db.clone());

        let vis = book_visibility(&unrestricted(), None);
        assert_eq!(
            dao.genre_counts("u1", &vis).unwrap(),
            [("action".to_string(), 1), ("drama".to_string(), 1)]
        );
        // 'favorite' comes from both the series tag and b1's book tag: one series, one count
        assert_eq!(
            dao.tag_counts("u1", &vis).unwrap(),
            [
                ("classic".to_string(), 1),
                ("drama-tag".to_string(), 1),
                ("favorite".to_string(), 1)
            ]
        );
        // author-a via aggregation and b1: one count; the blank name is dropped
        assert_eq!(
            dao.author_counts("u1", &vis).unwrap(),
            [
                ("author-a".to_string(), 1),
                ("author-b".to_string(), 1),
                ("author-c".to_string(), 1)
            ]
        );

        // with only l1 visible, s2 contributes nothing anywhere
        let l1 = BTreeSet::from(["l1".to_string()]);
        let vis = book_visibility(&unrestricted(), Some(l1.clone()));
        assert_eq!(
            dao.genre_counts("u1", &vis).unwrap(),
            [("action".to_string(), 1)]
        );
        assert_eq!(
            dao.tag_counts("u1", &vis).unwrap(),
            [("classic".to_string(), 1), ("favorite".to_string(), 1)]
        );
        assert_eq!(
            dao.author_counts("u1", &vis).unwrap(),
            [("author-a".to_string(), 1)]
        );
        assert_eq!(
            dao.visible_series_ids(&series_visibility(&unrestricted(), Some(l1)))
                .unwrap(),
            HashSet::from(["s1".to_string()])
        );

        // a user without completions has no top lists at all
        assert!(dao
            .genre_counts("nobody", &book_visibility(&unrestricted(), None))
            .unwrap()
            .is_empty());
    }

    #[test]
    fn completed_book_page_counts_feed_the_time_series_fallback() {
        let db = db();
        seed(&db);
        let dao = ReadingStatsDtoDao::new(db.clone());

        let day = |s: &str| time_codec::parse_date(s).unwrap();
        let books = dao
            .completed_book_page_counts("u1", &book_visibility(&unrestricted(), None))
            .unwrap();
        assert_eq!(
            books,
            [
                CompletedBook {
                    book_id: "b1".to_string(),
                    page_count: 100,
                    read_day: day("2024-01-05")
                },
                CompletedBook {
                    book_id: "b3".to_string(),
                    page_count: 50,
                    read_day: day("2024-01-07")
                },
            ]
        );
    }

    #[test]
    fn from_clauses_join_series_metadata_only_when_restricted() {
        // unrestricted visibility never references SERIES_METADATA: the join is skipped
        let open = book_visibility(&unrestricted(), None);
        for from in [
            totals_from(&open),
            progress_from(&open),
            series_metadata_join(&open, "SERIES.ID"),
        ] {
            assert!(!from.contains("SERIES_METADATA"), "{from}");
        }

        // content restrictions reference SERIES_METADATA: the join renders
        let restricted = ContentRestrictions::new(
            Some(AgeRestriction {
                age: 18,
                restriction: AllowExclude::Exclude,
            }),
            BTreeSet::new(),
            BTreeSet::new(),
        );
        let vis = book_visibility(&restricted, None);
        for from in [
            totals_from(&vis),
            progress_from(&vis),
            series_metadata_join(&vis, "SERIES.ID"),
        ] {
            assert!(from.contains("LEFT JOIN SERIES_METADATA ON"), "{from}");
        }
    }
}
