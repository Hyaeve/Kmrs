//! DAOs for SERIES, SERIES_METADATA (with 5 child tables), and
//! BOOK_METADATA_AGGREGATION (with 2 child tables).

use super::{get_date, get_datetime, get_datetime_opt};
use crate::error::Result;
use crate::pool::Database;
use komga_core::model::series::{
    AlternateTitle, Author, BookMetadataAggregation, ReadingDirection, Series, SeriesMetadata,
    SeriesStatus, WebLink,
};
use komga_core::time_codec;
use komga_core::tsid::TsidFactory;
use rusqlite::{params, Row};
use std::collections::{BTreeSet, HashMap};

const SERIES_COLUMNS: &str =
  "ID, NAME, URL, FILE_LAST_MODIFIED, LIBRARY_ID, BOOK_COUNT, DELETED_DATE, ONESHOT, CREATED_DATE, LAST_MODIFIED_DATE";

const METADATA_COLUMNS: &str =
    "SERIES_ID, STATUS, STATUS_LOCK, TITLE, TITLE_LOCK, TITLE_SORT, TITLE_SORT_LOCK, \
 SUMMARY, SUMMARY_LOCK, READING_DIRECTION, READING_DIRECTION_LOCK, PUBLISHER, PUBLISHER_LOCK, \
 AGE_RATING, AGE_RATING_LOCK, LANGUAGE, LANGUAGE_LOCK, GENRES_LOCK, TAGS_LOCK, \
 TOTAL_BOOK_COUNT, TOTAL_BOOK_COUNT_LOCK, SHARING_LABELS_LOCK, LINKS_LOCK, ALTERNATE_TITLES_LOCK, \
 CREATED_DATE, LAST_MODIFIED_DATE";

const AGGREGATION_COLUMNS: &str =
    "SERIES_ID, RELEASE_DATE, SUMMARY, SUMMARY_NUMBER, CREATED_DATE, LAST_MODIFIED_DATE";

pub struct SeriesDao {
    db: Database,
    tsid: TsidFactory,
}

impl SeriesDao {
    pub fn new(db: Database) -> Self {
        Self {
            db,
            tsid: TsidFactory::new_random_node(),
        }
    }

    fn row_to_series(row: &Row<'_>) -> rusqlite::Result<Series> {
        Ok(Series {
            id: row.get(0)?,
            name: row.get(1)?,
            url: row.get(2)?,
            file_last_modified: get_datetime(row, 3)?,
            library_id: row.get(4)?,
            book_count: row.get(5)?,
            deleted_date: get_datetime_opt(row, 6)?,
            oneshot: row.get(7)?,
            created_date: get_datetime(row, 8)?,
            last_modified_date: get_datetime(row, 9)?,
        })
    }

    pub fn find_all(&self) -> Result<Vec<Series>> {
        let conn = self.db.ro();
        let mut stmt = conn.prepare(&format!("SELECT {SERIES_COLUMNS} FROM SERIES"))?;
        let series = stmt
            .query_map([], Self::row_to_series)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(series)
    }

    pub fn find_by_id(&self, id: &str) -> Result<Option<Series>> {
        let conn = self.db.ro();
        let mut stmt =
            conn.prepare(&format!("SELECT {SERIES_COLUMNS} FROM SERIES WHERE ID = ?"))?;
        let series = stmt
            .query_map([id], Self::row_to_series)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(series.into_iter().next())
    }

    pub fn find_by_library_id(&self, library_id: &str) -> Result<Vec<Series>> {
        let conn = self.db.ro();
        let mut stmt = conn.prepare(&format!(
            "SELECT {SERIES_COLUMNS} FROM SERIES WHERE LIBRARY_ID = ?"
        ))?;
        let series = stmt
            .query_map([library_id], Self::row_to_series)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(series)
    }

    pub fn find_all_ids_by_library_id(&self, library_id: &str) -> Result<Vec<String>> {
        let conn = self.db.ro();
        let mut stmt = conn.prepare("SELECT ID FROM SERIES WHERE LIBRARY_ID = ?")?;
        let ids = stmt
            .query_map([library_id], |r| r.get(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(ids)
    }

    pub fn find_not_deleted_by_library_id_and_url_or_null(
        &self,
        library_id: &str,
        url: &str,
    ) -> Result<Option<Series>> {
        let conn = self.db.ro();
        let mut stmt = conn.prepare(&format!(
      "SELECT {SERIES_COLUMNS} FROM SERIES WHERE LIBRARY_ID = ? AND URL = ? AND DELETED_DATE IS NULL \
       ORDER BY LAST_MODIFIED_DATE DESC"
    ))?;
        let series = stmt
            .query_map(params![library_id, url], Self::row_to_series)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(series.into_iter().next())
    }

    pub fn get_library_id(&self, series_id: &str) -> Result<Option<String>> {
        let conn = self.db.ro();
        let mut stmt = conn.prepare("SELECT LIBRARY_ID FROM SERIES WHERE ID = ?")?;
        let ids = stmt
            .query_map([series_id], |r| r.get(0))?
            .collect::<std::result::Result<Vec<String>, _>>()?;
        Ok(ids.into_iter().next())
    }

    /// Audit columns and BOOK_COUNT fall back to DB defaults, matching the jOOQ insert behavior.
    pub fn insert(&self, series: &Series) -> Result<String> {
        let conn = self.db.rw();
        let id = if series.id.is_empty() {
            self.tsid.create_string()
        } else {
            series.id.clone()
        };
        conn.execute(
      "INSERT INTO SERIES (ID, NAME, URL, FILE_LAST_MODIFIED, LIBRARY_ID, DELETED_DATE, ONESHOT) \
       VALUES (?, ?, ?, ?, ?, ?, ?)",
      params![
        id,
        series.name,
        series.url,
        time_codec::format_datetime(series.file_last_modified),
        series.library_id,
        series.deleted_date.map(time_codec::format_datetime),
        series.oneshot,
      ],
    )?;
        Ok(id)
    }

    pub fn update(&self, series: &Series, update_modified_time: bool) -> Result<()> {
        let conn = self.db.rw();
        let mut sql =
            "UPDATE SERIES SET NAME = ?, URL = ?, FILE_LAST_MODIFIED = ?, LIBRARY_ID = ?, \
                   BOOK_COUNT = ?, DELETED_DATE = ?, ONESHOT = ?"
                .to_string();
        if update_modified_time {
            sql.push_str(", LAST_MODIFIED_DATE = ?");
        }
        sql.push_str(" WHERE ID = ?");
        let mut values: Vec<Box<dyn rusqlite::ToSql>> = vec![
            Box::new(series.name.clone()),
            Box::new(series.url.clone()),
            Box::new(time_codec::format_datetime(series.file_last_modified)),
            Box::new(series.library_id.clone()),
            Box::new(series.book_count),
            Box::new(series.deleted_date.map(time_codec::format_datetime)),
            Box::new(series.oneshot),
        ];
        if update_modified_time {
            values.push(Box::new(time_codec::format_datetime(time_codec::now_utc())));
        }
        values.push(Box::new(series.id.clone()));
        conn.execute(&sql, rusqlite::params_from_iter(values))?;
        Ok(())
    }

    pub fn delete(&self, id: &str) -> Result<()> {
        let conn = self.db.rw();
        conn.execute("DELETE FROM SERIES WHERE ID = ?", [id])?;
        Ok(())
    }

    pub fn delete_many(&self, ids: &[String]) -> Result<()> {
        let conn = self.db.rw();
        let mut stmt = conn.prepare("DELETE FROM SERIES WHERE ID = ?")?;
        for id in ids {
            stmt.execute([id])?;
        }
        Ok(())
    }

    pub fn count(&self) -> Result<i64> {
        let conn = self.db.ro();
        let count = conn.query_row("SELECT COUNT(*) FROM SERIES", [], |r| r.get(0))?;
        Ok(count)
    }

    /// `SeriesRepository.findAll(condition, context, pageable)`: domain-level conditional query
    /// (`selectDistinct`, on-demand joins; jOOQ applies no ORDER BY).
    pub fn find_all_by_condition(
        &self,
        condition: Option<&komga_core::search::SearchConditionSeries>,
        ctx: &komga_core::search::SearchContext,
    ) -> Result<Vec<Series>> {
        let w = crate::search_sql::series_condition(condition, ctx);
        let mut join_sql = String::new();
        let mut join_params: Vec<rusqlite::types::Value> = vec![];
        for join in &w.joins {
            match join {
                crate::search_sql::RequiredJoin::Collection(id) => {
                    let alias = crate::search_sql::collection_alias(id);
                    join_sql.push_str(&format!(
                        " LEFT JOIN COLLECTION_SERIES AS \"{alias}\" ON (SERIES.ID = \"{alias}\".SERIES_ID AND \"{alias}\".COLLECTION_ID = ?)"
                    ));
                    join_params.push(rusqlite::types::Value::Text(id.clone()));
                }
                crate::search_sql::RequiredJoin::BookMetadataAggregation => join_sql.push_str(
                    " LEFT JOIN BOOK_METADATA_AGGREGATION ON SERIES.ID = BOOK_METADATA_AGGREGATION.SERIES_ID",
                ),
                crate::search_sql::RequiredJoin::SeriesMetadata => join_sql
                    .push_str(" INNER JOIN SERIES_METADATA ON SERIES.ID = SERIES_METADATA.SERIES_ID"),
                crate::search_sql::RequiredJoin::ReadProgress(user_id) => {
                    join_sql.push_str(
                        " LEFT JOIN READ_PROGRESS_SERIES ON (READ_PROGRESS_SERIES.SERIES_ID = SERIES.ID AND READ_PROGRESS_SERIES.USER_ID = ?)",
                    );
                    join_params.push(rusqlite::types::Value::Text(user_id.clone()));
                }
                _ => {}
            }
        }
        // columns are qualified: the dynamic joins (SERIES_METADATA, ...) share column names
        let columns = SERIES_COLUMNS
            .split(',')
            .map(|c| format!("SERIES.{}", c.trim()))
            .collect::<Vec<_>>()
            .join(", ");
        let mut sql = format!("SELECT DISTINCT {columns} FROM SERIES{join_sql}");
        if !w.sql.is_empty() {
            sql.push_str(&format!(" WHERE {}", w.sql));
        }
        let params: Vec<rusqlite::types::Value> = join_params.into_iter().chain(w.params).collect();
        let conn = self.db.ro();
        let mut stmt = conn.prepare(&sql)?;
        let series = stmt
            .query_map(rusqlite::params_from_iter(params), Self::row_to_series)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(series)
    }

    pub fn find_all_not_deleted_by_library_id_and_url_not_in(
        &self,
        library_id: &str,
        urls: &[String],
    ) -> Result<Vec<Series>> {
        let conn = self.db.ro();
        let sql = if urls.is_empty() {
            format!(
                "SELECT {SERIES_COLUMNS} FROM SERIES WHERE LIBRARY_ID = ? AND DELETED_DATE IS NULL"
            )
        } else {
            let placeholders = urls.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
            format!(
                "SELECT {SERIES_COLUMNS} FROM SERIES WHERE LIBRARY_ID = ? AND DELETED_DATE IS NULL AND URL NOT IN ({placeholders})"
            )
        };
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(library_id.to_string())];
        params.extend(
            urls.iter()
                .map(|u| Box::new(u.clone()) as Box<dyn rusqlite::ToSql>),
        );
        let mut stmt = conn.prepare(&sql)?;
        let series = stmt
            .query_map(rusqlite::params_from_iter(params), Self::row_to_series)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(series)
    }

    pub fn count_grouped_by_library_id(&self) -> Result<HashMap<String, i64>> {
        let conn = self.db.ro();
        let mut stmt =
            conn.prepare("SELECT LIBRARY_ID, COUNT(ID) FROM SERIES GROUP BY LIBRARY_ID")?;
        let map = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?
            .collect::<std::result::Result<HashMap<_, _>, _>>()?;
        Ok(map)
    }
}

pub struct SeriesMetadataDao {
    db: Database,
}

impl SeriesMetadataDao {
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    fn row_to_metadata(row: &Row<'_>) -> rusqlite::Result<SeriesMetadata> {
        let status: String = row.get(1)?;
        let reading_direction: Option<String> = row.get(9)?;
        Ok(SeriesMetadata {
            series_id: row.get(0)?,
            status: SeriesStatus::from_str(&status)
                .ok_or_else(|| super::invalid_column(1, "STATUS", &status))?,
            title: row.get(3)?,
            title_sort: row.get(5)?,
            summary: row.get(7)?,
            reading_direction: reading_direction
                .map(|s| {
                    ReadingDirection::from_str(&s)
                        .ok_or_else(|| super::invalid_column(9, "READING_DIRECTION", &s))
                })
                .transpose()?,
            publisher: row.get(11)?,
            age_rating: row.get(13)?,
            language: row.get(15)?,
            genres: BTreeSet::new(),
            tags: BTreeSet::new(),
            total_book_count: row.get(19)?,
            sharing_labels: BTreeSet::new(),
            links: Vec::new(),
            alternate_titles: Vec::new(),
            status_lock: row.get(2)?,
            title_lock: row.get(4)?,
            title_sort_lock: row.get(6)?,
            summary_lock: row.get(8)?,
            reading_direction_lock: row.get(10)?,
            publisher_lock: row.get(12)?,
            age_rating_lock: row.get(14)?,
            language_lock: row.get(16)?,
            genres_lock: row.get(17)?,
            tags_lock: row.get(18)?,
            total_book_count_lock: row.get(20)?,
            sharing_labels_lock: row.get(21)?,
            links_lock: row.get(22)?,
            alternate_titles_lock: row.get(23)?,
            created_date: get_datetime(row, 24)?,
            last_modified_date: get_datetime(row, 25)?,
        })
    }

    pub fn find_by_id(&self, series_id: &str) -> Result<Option<SeriesMetadata>> {
        let conn = self.db.ro();
        let mut stmt = conn.prepare(&format!(
            "SELECT {METADATA_COLUMNS} FROM SERIES_METADATA WHERE SERIES_ID = ?"
        ))?;
        let metadata = stmt
            .query_map([series_id], Self::row_to_metadata)?
            .collect::<std::result::Result<Vec<_>, _>>()?
            .into_iter()
            .next();
        let Some(mut metadata) = metadata else {
            return Ok(None);
        };
        metadata.genres = query_string_set(
            &conn,
            "SELECT GENRE FROM SERIES_METADATA_GENRE WHERE SERIES_ID = ?",
            series_id,
        )?;
        metadata.tags = query_string_set(
            &conn,
            "SELECT TAG FROM SERIES_METADATA_TAG WHERE SERIES_ID = ?",
            series_id,
        )?;
        metadata.sharing_labels = query_string_set(
            &conn,
            "SELECT LABEL FROM SERIES_METADATA_SHARING WHERE SERIES_ID = ?",
            series_id,
        )?;
        metadata.links = query_pairs(
            &conn,
            "SELECT LABEL, URL FROM SERIES_METADATA_LINK WHERE SERIES_ID = ?",
            series_id,
        )?
        .into_iter()
        .map(|(label, url)| WebLink { label, url })
        .collect();
        metadata.alternate_titles = query_pairs(
            &conn,
            "SELECT LABEL, TITLE FROM SERIES_METADATA_ALTERNATE_TITLE WHERE SERIES_ID = ?",
            series_id,
        )?
        .into_iter()
        .map(|(label, title)| AlternateTitle { label, title })
        .collect();
        Ok(Some(metadata))
    }

    /// Audit columns fall back to DB defaults, matching the jOOQ insert behavior.
    pub fn insert(&self, metadata: &SeriesMetadata) -> Result<()> {
        let conn = self.db.rw();
        conn.execute(
      "INSERT INTO SERIES_METADATA (SERIES_ID, STATUS, STATUS_LOCK, TITLE, TITLE_LOCK, TITLE_SORT, TITLE_SORT_LOCK, \
       SUMMARY, SUMMARY_LOCK, READING_DIRECTION, READING_DIRECTION_LOCK, PUBLISHER, PUBLISHER_LOCK, \
       AGE_RATING, AGE_RATING_LOCK, LANGUAGE, LANGUAGE_LOCK, GENRES_LOCK, TAGS_LOCK, \
       TOTAL_BOOK_COUNT, TOTAL_BOOK_COUNT_LOCK, SHARING_LABELS_LOCK, LINKS_LOCK, ALTERNATE_TITLES_LOCK) \
       VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
      rusqlite::params_from_iter(metadata_params(metadata)),
    )?;
        self.replace_children(&conn, metadata)?;
        Ok(())
    }

    /// Child tables are replaced as a whole, matching the jOOQ update behavior;
    /// LAST_MODIFIED_DATE is set to the current UTC time.
    pub fn update(&self, metadata: &SeriesMetadata) -> Result<()> {
        let conn = self.db.rw();
        // metadata_params' first element is series_id (for the INSERT column order);
        // UPDATE's SET starts at STATUS, so it must be removed
        let mut values = metadata_params(metadata);
        values.remove(0);
        values.push(Box::new(time_codec::format_datetime(time_codec::now_utc())));
        values.push(Box::new(metadata.series_id.clone()));
        conn.execute(
      "UPDATE SERIES_METADATA SET STATUS = ?, STATUS_LOCK = ?, TITLE = ?, TITLE_LOCK = ?, TITLE_SORT = ?, TITLE_SORT_LOCK = ?, \
       SUMMARY = ?, SUMMARY_LOCK = ?, READING_DIRECTION = ?, READING_DIRECTION_LOCK = ?, PUBLISHER = ?, PUBLISHER_LOCK = ?, \
       AGE_RATING = ?, AGE_RATING_LOCK = ?, LANGUAGE = ?, LANGUAGE_LOCK = ?, GENRES_LOCK = ?, TAGS_LOCK = ?, \
       TOTAL_BOOK_COUNT = ?, TOTAL_BOOK_COUNT_LOCK = ?, SHARING_LABELS_LOCK = ?, LINKS_LOCK = ?, ALTERNATE_TITLES_LOCK = ?, \
       LAST_MODIFIED_DATE = ? WHERE SERIES_ID = ?",
      rusqlite::params_from_iter(values),
    )?;
        self.replace_children(&conn, metadata)?;
        Ok(())
    }

    fn replace_children(
        &self,
        conn: &rusqlite::Connection,
        metadata: &SeriesMetadata,
    ) -> Result<()> {
        let series_id = &metadata.series_id;
        conn.execute(
            "DELETE FROM SERIES_METADATA_GENRE WHERE SERIES_ID = ?",
            [series_id],
        )?;
        conn.execute(
            "DELETE FROM SERIES_METADATA_TAG WHERE SERIES_ID = ?",
            [series_id],
        )?;
        conn.execute(
            "DELETE FROM SERIES_METADATA_SHARING WHERE SERIES_ID = ?",
            [series_id],
        )?;
        conn.execute(
            "DELETE FROM SERIES_METADATA_LINK WHERE SERIES_ID = ?",
            [series_id],
        )?;
        conn.execute(
            "DELETE FROM SERIES_METADATA_ALTERNATE_TITLE WHERE SERIES_ID = ?",
            [series_id],
        )?;
        for genre in &metadata.genres {
            conn.execute(
                "INSERT INTO SERIES_METADATA_GENRE (SERIES_ID, GENRE) VALUES (?, ?)",
                params![series_id, genre],
            )?;
        }
        for tag in &metadata.tags {
            conn.execute(
                "INSERT INTO SERIES_METADATA_TAG (SERIES_ID, TAG) VALUES (?, ?)",
                params![series_id, tag],
            )?;
        }
        for label in &metadata.sharing_labels {
            conn.execute(
                "INSERT INTO SERIES_METADATA_SHARING (SERIES_ID, LABEL) VALUES (?, ?)",
                params![series_id, label],
            )?;
        }
        for link in &metadata.links {
            conn.execute(
                "INSERT INTO SERIES_METADATA_LINK (SERIES_ID, LABEL, URL) VALUES (?, ?, ?)",
                params![series_id, link.label, link.url],
            )?;
        }
        for alt in &metadata.alternate_titles {
            conn.execute(
        "INSERT INTO SERIES_METADATA_ALTERNATE_TITLE (SERIES_ID, LABEL, TITLE) VALUES (?, ?, ?)",
        params![series_id, alt.label, alt.title],
      )?;
        }
        Ok(())
    }

    pub fn delete(&self, series_id: &str) -> Result<()> {
        let conn = self.db.rw();
        conn.execute(
            "DELETE FROM SERIES_METADATA_GENRE WHERE SERIES_ID = ?",
            [series_id],
        )?;
        conn.execute(
            "DELETE FROM SERIES_METADATA_TAG WHERE SERIES_ID = ?",
            [series_id],
        )?;
        conn.execute(
            "DELETE FROM SERIES_METADATA_SHARING WHERE SERIES_ID = ?",
            [series_id],
        )?;
        conn.execute(
            "DELETE FROM SERIES_METADATA_LINK WHERE SERIES_ID = ?",
            [series_id],
        )?;
        conn.execute(
            "DELETE FROM SERIES_METADATA_ALTERNATE_TITLE WHERE SERIES_ID = ?",
            [series_id],
        )?;
        conn.execute(
            "DELETE FROM SERIES_METADATA WHERE SERIES_ID = ?",
            [series_id],
        )?;
        Ok(())
    }

    pub fn count(&self) -> Result<i64> {
        let conn = self.db.ro();
        let count = conn.query_row("SELECT COUNT(*) FROM SERIES_METADATA", [], |r| r.get(0))?;
        Ok(count)
    }
}

/// Column order: SERIES_ID, STATUS, STATUS_LOCK, TITLE, TITLE_LOCK, TITLE_SORT, TITLE_SORT_LOCK,
/// SUMMARY, SUMMARY_LOCK, READING_DIRECTION, READING_DIRECTION_LOCK, PUBLISHER, PUBLISHER_LOCK,
/// AGE_RATING, AGE_RATING_LOCK, LANGUAGE, LANGUAGE_LOCK, GENRES_LOCK, TAGS_LOCK,
/// TOTAL_BOOK_COUNT, TOTAL_BOOK_COUNT_LOCK, SHARING_LABELS_LOCK, LINKS_LOCK, ALTERNATE_TITLES_LOCK
fn metadata_params(m: &SeriesMetadata) -> Vec<Box<dyn rusqlite::ToSql>> {
    vec![
        Box::new(m.series_id.clone()),
        Box::new(m.status.as_str().to_string()),
        Box::new(m.status_lock),
        Box::new(m.title.clone()),
        Box::new(m.title_lock),
        Box::new(m.title_sort.clone()),
        Box::new(m.title_sort_lock),
        Box::new(m.summary.clone()),
        Box::new(m.summary_lock),
        Box::new(m.reading_direction.map(|d| d.as_str().to_string())),
        Box::new(m.reading_direction_lock),
        Box::new(m.publisher.clone()),
        Box::new(m.publisher_lock),
        Box::new(m.age_rating),
        Box::new(m.age_rating_lock),
        Box::new(m.language.clone()),
        Box::new(m.language_lock),
        Box::new(m.genres_lock),
        Box::new(m.tags_lock),
        Box::new(m.total_book_count),
        Box::new(m.total_book_count_lock),
        Box::new(m.sharing_labels_lock),
        Box::new(m.links_lock),
        Box::new(m.alternate_titles_lock),
    ]
}

pub struct BookMetadataAggregationDao {
    db: Database,
}

impl BookMetadataAggregationDao {
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    pub fn find_by_id(&self, series_id: &str) -> Result<Option<BookMetadataAggregation>> {
        let conn = self.db.ro();
        let mut stmt = conn.prepare(&format!(
            "SELECT {AGGREGATION_COLUMNS} FROM BOOK_METADATA_AGGREGATION WHERE SERIES_ID = ?"
        ))?;
        let aggregation = stmt
            .query_map([series_id], |row| {
                Ok(BookMetadataAggregation {
                    series_id: row.get(0)?,
                    release_date: get_date(row, 1)?,
                    summary: row.get(2)?,
                    summary_number: row.get(3)?,
                    created_date: get_datetime(row, 4)?,
                    last_modified_date: get_datetime(row, 5)?,
                    authors: Vec::new(),
                    tags: BTreeSet::new(),
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?
            .into_iter()
            .next();
        let Some(mut aggregation) = aggregation else {
            return Ok(None);
        };
        aggregation.authors = query_pairs(
            &conn,
            "SELECT NAME, ROLE FROM BOOK_METADATA_AGGREGATION_AUTHOR WHERE SERIES_ID = ?",
            series_id,
        )?
        .into_iter()
        .map(|(name, role)| Author { name, role })
        .collect();
        aggregation.tags = query_string_set(
            &conn,
            "SELECT TAG FROM BOOK_METADATA_AGGREGATION_TAG WHERE SERIES_ID = ?",
            series_id,
        )?;
        Ok(Some(aggregation))
    }

    /// Audit columns fall back to DB defaults, matching the jOOQ insert behavior.
    pub fn insert(&self, metadata: &BookMetadataAggregation) -> Result<()> {
        let conn = self.db.rw();
        conn.execute(
      "INSERT INTO BOOK_METADATA_AGGREGATION (SERIES_ID, RELEASE_DATE, SUMMARY, SUMMARY_NUMBER) \
       VALUES (?, ?, ?, ?)",
      params![
        metadata.series_id,
        metadata.release_date.map(time_codec::format_date),
        metadata.summary,
        metadata.summary_number,
      ],
    )?;
        self.replace_children(&conn, metadata)?;
        Ok(())
    }

    /// Child tables are replaced as a whole, matching the jOOQ update behavior;
    /// LAST_MODIFIED_DATE is set to the current UTC time.
    pub fn update(&self, metadata: &BookMetadataAggregation) -> Result<()> {
        let conn = self.db.rw();
        conn.execute(
      "UPDATE BOOK_METADATA_AGGREGATION SET SUMMARY = ?, SUMMARY_NUMBER = ?, RELEASE_DATE = ?, LAST_MODIFIED_DATE = ? \
       WHERE SERIES_ID = ?",
      params![
        metadata.summary,
        metadata.summary_number,
        metadata.release_date.map(time_codec::format_date),
        time_codec::format_datetime(time_codec::now_utc()),
        metadata.series_id,
      ],
    )?;
        self.replace_children(&conn, metadata)?;
        Ok(())
    }

    fn replace_children(
        &self,
        conn: &rusqlite::Connection,
        metadata: &BookMetadataAggregation,
    ) -> Result<()> {
        let series_id = &metadata.series_id;
        conn.execute(
            "DELETE FROM BOOK_METADATA_AGGREGATION_AUTHOR WHERE SERIES_ID = ?",
            [series_id],
        )?;
        conn.execute(
            "DELETE FROM BOOK_METADATA_AGGREGATION_TAG WHERE SERIES_ID = ?",
            [series_id],
        )?;
        for author in &metadata.authors {
            conn.execute(
        "INSERT INTO BOOK_METADATA_AGGREGATION_AUTHOR (SERIES_ID, NAME, ROLE) VALUES (?, ?, ?)",
        params![series_id, author.name, author.role],
      )?;
        }
        for tag in &metadata.tags {
            conn.execute(
                "INSERT INTO BOOK_METADATA_AGGREGATION_TAG (SERIES_ID, TAG) VALUES (?, ?)",
                params![series_id, tag],
            )?;
        }
        Ok(())
    }

    pub fn delete(&self, series_id: &str) -> Result<()> {
        let conn = self.db.rw();
        conn.execute(
            "DELETE FROM BOOK_METADATA_AGGREGATION_AUTHOR WHERE SERIES_ID = ?",
            [series_id],
        )?;
        conn.execute(
            "DELETE FROM BOOK_METADATA_AGGREGATION_TAG WHERE SERIES_ID = ?",
            [series_id],
        )?;
        conn.execute(
            "DELETE FROM BOOK_METADATA_AGGREGATION WHERE SERIES_ID = ?",
            [series_id],
        )?;
        Ok(())
    }

    pub fn count(&self) -> Result<i64> {
        let conn = self.db.ro();
        let count = conn.query_row("SELECT COUNT(*) FROM BOOK_METADATA_AGGREGATION", [], |r| {
            r.get(0)
        })?;
        Ok(count)
    }
}

fn query_string_set(
    conn: &rusqlite::Connection,
    sql: &str,
    series_id: &str,
) -> Result<BTreeSet<String>> {
    let mut stmt = conn.prepare(sql)?;
    let set = stmt
        .query_map([series_id], |r| r.get(0))?
        .collect::<std::result::Result<BTreeSet<_>, _>>()?;
    Ok(set)
}

fn query_pairs(
    conn: &rusqlite::Connection,
    sql: &str,
    series_id: &str,
) -> Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare(sql)?;
    let pairs = stmt
        .query_map([series_id], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(pairs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migrate::Migrator;
    use crate::{main_migrations, Placeholders};
    use komga_core::time_codec::now_utc;
    use time::Date;

    fn db() -> Database {
        let db = Database::open_in_memory(true).unwrap();
        let migrations = main_migrations();
        Migrator::new(&migrations, Placeholders::default())
            .migrate(&db.rw())
            .unwrap();
        db
    }

    fn insert_library(db: &Database, id: &str) {
        db.rw()
            .execute(
                "INSERT INTO LIBRARY (ID, NAME, ROOT) VALUES (?, ?, ?)",
                params![id, "lib", "file:/data/"],
            )
            .unwrap();
    }

    fn sample_series(library_id: &str) -> Series {
        Series {
            id: String::new(),
            name: "Berserk".into(),
            url: "file:/data/berserk/".into(),
            file_last_modified: now_utc(),
            library_id: library_id.into(),
            book_count: 0,
            deleted_date: None,
            oneshot: false,
            created_date: now_utc(),
            last_modified_date: now_utc(),
        }
    }

    fn sample_metadata(series_id: &str) -> SeriesMetadata {
        SeriesMetadata {
            series_id: series_id.into(),
            status: SeriesStatus::Ongoing,
            title: "Berserk".into(),
            title_sort: "Berserk".into(),
            summary: "Guts".into(),
            reading_direction: Some(ReadingDirection::RightToLeft),
            publisher: "Hakusensha".into(),
            age_rating: Some(18),
            language: "ja".into(),
            genres: ["action", "dark fantasy"]
                .into_iter()
                .map(String::from)
                .collect(),
            tags: ["seinen"].into_iter().map(String::from).collect(),
            total_book_count: Some(41),
            sharing_labels: ["nsfw"].into_iter().map(String::from).collect(),
            links: vec![WebLink {
                label: "wiki".into(),
                url: "https://example.org/wiki".into(),
            }],
            alternate_titles: vec![AlternateTitle {
                label: "ja".into(),
                title: "ベルセルク".into(),
            }],
            status_lock: false,
            title_lock: true,
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

    fn sample_aggregation(series_id: &str) -> BookMetadataAggregation {
        BookMetadataAggregation {
            series_id: series_id.into(),
            authors: vec![Author::new("Kentaro Miura", "writer")],
            tags: ["seinen"].into_iter().map(String::from).collect(),
            release_date: Date::from_calendar_date(1990, time::Month::January, 1).ok(),
            summary: "Guts".into(),
            summary_number: "1".into(),
            created_date: now_utc(),
            last_modified_date: now_utc(),
        }
    }

    #[test]
    fn series_crud_roundtrip() {
        let db = db();
        insert_library(&db, "lib1");
        let dao = SeriesDao::new(db.clone());

        let id = dao.insert(&sample_series("lib1")).unwrap();
        assert_eq!(id.len(), 13);

        let found = dao.find_by_id(&id).unwrap().expect("not found");
        assert_eq!(found.name, "Berserk");
        assert_eq!(found.url, "file:/data/berserk/");
        assert_eq!(found.library_id, "lib1");
        assert_eq!(found.book_count, 0);
        assert!(!found.oneshot);
        assert!(found.deleted_date.is_none());

        let mut updated = found.clone();
        updated.name = "Berserk Deluxe".into();
        updated.book_count = 3;
        updated.oneshot = true;
        updated.deleted_date = Some(now_utc());
        dao.update(&updated, true).unwrap();

        let found = dao.find_by_id(&id).unwrap().unwrap();
        assert_eq!(found.name, "Berserk Deluxe");
        assert_eq!(found.book_count, 3);
        assert!(found.oneshot);
        assert!(found.deleted_date.is_some());
        assert!(found.last_modified_date >= updated.last_modified_date);

        assert_eq!(dao.find_by_library_id("lib1").unwrap().len(), 1);
        assert_eq!(dao.find_all().unwrap().len(), 1);
        assert_eq!(
            dao.find_all_ids_by_library_id("lib1").unwrap(),
            vec![id.clone()]
        );
        assert_eq!(dao.get_library_id(&id).unwrap().as_deref(), Some("lib1"));
        assert_eq!(dao.count().unwrap(), 1);
        assert_eq!(
            dao.count_grouped_by_library_id().unwrap().get("lib1"),
            Some(&1)
        );

        // after soft-deletion it cannot be found by URL
        assert!(dao
            .find_not_deleted_by_library_id_and_url_or_null("lib1", "file:/data/berserk/")
            .unwrap()
            .is_none());
        let mut restored = found.clone();
        restored.deleted_date = None;
        dao.update(&restored, false).unwrap();
        assert_eq!(
            dao.find_not_deleted_by_library_id_and_url_or_null("lib1", "file:/data/berserk/")
                .unwrap()
                .map(|s| s.id),
            Some(id.clone())
        );

        dao.delete(&id).unwrap();
        assert!(dao.find_by_id(&id).unwrap().is_none());

        dao.insert(&sample_series("lib1")).unwrap();
        let ids = dao.find_all_ids_by_library_id("lib1").unwrap();
        dao.delete_many(&ids).unwrap();
        assert_eq!(dao.count().unwrap(), 0);
    }

    #[test]
    fn metadata_crud_with_children() {
        let db = db();
        insert_library(&db, "lib1");
        let series_dao = SeriesDao::new(db.clone());
        let series_id = series_dao.insert(&sample_series("lib1")).unwrap();
        let dao = SeriesMetadataDao::new(db.clone());

        dao.insert(&sample_metadata(&series_id)).unwrap();
        let found = dao.find_by_id(&series_id).unwrap().expect("not found");
        assert_eq!(found.status, SeriesStatus::Ongoing);
        assert_eq!(found.reading_direction, Some(ReadingDirection::RightToLeft));
        assert_eq!(found.publisher, "Hakusensha");
        assert_eq!(found.age_rating, Some(18));
        assert_eq!(found.total_book_count, Some(41));
        assert_eq!(
            found.genres,
            ["action", "dark fantasy"]
                .into_iter()
                .map(String::from)
                .collect::<BTreeSet<_>>()
        );
        assert_eq!(
            found.tags,
            ["seinen"]
                .into_iter()
                .map(String::from)
                .collect::<BTreeSet<_>>()
        );
        assert_eq!(
            found.sharing_labels,
            ["nsfw"]
                .into_iter()
                .map(String::from)
                .collect::<BTreeSet<_>>()
        );
        assert_eq!(found.links.len(), 1);
        assert_eq!(found.links[0].url, "https://example.org/wiki");
        assert_eq!(found.alternate_titles[0].title, "ベルセルク");
        assert!(found.title_lock);
        assert!(!found.status_lock);

        let mut updated = found.clone();
        updated.status = SeriesStatus::Hiatus;
        updated.reading_direction = None;
        updated.age_rating = None;
        updated.total_book_count = None;
        updated.genres = BTreeSet::new();
        updated.tags = ["josei"].into_iter().map(String::from).collect();
        updated.sharing_labels = BTreeSet::new();
        updated.links = vec![];
        updated.alternate_titles = vec![];
        updated.publisher = "Shueisha".into();
        dao.update(&updated).unwrap();

        let found = dao.find_by_id(&series_id).unwrap().unwrap();
        assert_eq!(found.status, SeriesStatus::Hiatus);
        assert_eq!(found.reading_direction, None);
        assert_eq!(found.age_rating, None);
        assert_eq!(found.total_book_count, None);
        assert!(found.genres.is_empty());
        assert_eq!(
            found.tags,
            ["josei"]
                .into_iter()
                .map(String::from)
                .collect::<BTreeSet<_>>()
        );
        assert!(found.sharing_labels.is_empty());
        assert!(found.links.is_empty());
        assert!(found.alternate_titles.is_empty());
        assert_eq!(found.publisher, "Shueisha");
        assert_eq!(dao.count().unwrap(), 1);

        dao.delete(&series_id).unwrap();
        assert!(dao.find_by_id(&series_id).unwrap().is_none());
        for table in [
            "SERIES_METADATA_GENRE",
            "SERIES_METADATA_TAG",
            "SERIES_METADATA_SHARING",
            "SERIES_METADATA_LINK",
            "SERIES_METADATA_ALTERNATE_TITLE",
        ] {
            let n: i64 = db
                .ro()
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
                .unwrap();
            assert_eq!(n, 0, "{table} not cleaned");
        }
    }

    #[test]
    fn aggregation_crud_with_children() {
        let db = db();
        insert_library(&db, "lib1");
        let series_dao = SeriesDao::new(db.clone());
        let series_id = series_dao.insert(&sample_series("lib1")).unwrap();
        let dao = BookMetadataAggregationDao::new(db.clone());

        dao.insert(&sample_aggregation(&series_id)).unwrap();
        let found = dao.find_by_id(&series_id).unwrap().expect("not found");
        assert_eq!(found.authors.len(), 1);
        assert_eq!(found.authors[0].name, "Kentaro Miura");
        assert_eq!(found.authors[0].role, "writer");
        assert_eq!(found.summary_number, "1");
        assert_eq!(
            found.release_date,
            Date::from_calendar_date(1990, time::Month::January, 1).ok()
        );
        assert_eq!(
            found.tags,
            ["seinen"]
                .into_iter()
                .map(String::from)
                .collect::<BTreeSet<_>>()
        );

        let mut updated = found.clone();
        updated.authors = vec![
            Author::new("Kentaro Miura", "writer"),
            Author::new("Studio Gaga", "penciller"),
        ];
        updated.tags = BTreeSet::new();
        updated.release_date = None;
        updated.summary = "new summary".into();
        dao.update(&updated).unwrap();

        let found = dao.find_by_id(&series_id).unwrap().unwrap();
        assert_eq!(found.authors.len(), 2);
        assert_eq!(found.authors[1].role, "penciller");
        assert!(found.tags.is_empty());
        assert_eq!(found.release_date, None);
        assert_eq!(found.summary, "new summary");
        assert_eq!(dao.count().unwrap(), 1);

        dao.delete(&series_id).unwrap();
        assert!(dao.find_by_id(&series_id).unwrap().is_none());
        for table in [
            "BOOK_METADATA_AGGREGATION_AUTHOR",
            "BOOK_METADATA_AGGREGATION_TAG",
        ] {
            let n: i64 = db
                .ro()
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
                .unwrap();
            assert_eq!(n, 0, "{table} not cleaned");
        }
    }

    #[test]
    fn author_normalization() {
        let author = Author::new("  Kentaro Miura ", " Writer ");
        assert_eq!(author.name, "Kentaro Miura");
        assert_eq!(author.role, "writer");
    }
}
