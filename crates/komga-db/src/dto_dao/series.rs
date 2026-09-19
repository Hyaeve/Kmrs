//! `SeriesDtoDao.kt`: series DTO queries with read-progress aggregation.

use super::{search_entity_ids, DtoPage, EntitySearcher, PageRequest, SortOrder};
use crate::dao::{get_date, get_datetime, get_datetime_opt};
use crate::error::Result;
use crate::pool::Database;
use crate::search_sql::{
    collection_alias, id_in_or_no_condition, series_condition, series_regex_condition,
    sort_by_values, RequiredJoin, SqlWhere,
};
use komga_core::dto::common::{AlternateTitleDto, AuthorDto, GroupCountDto, WebLinkDto};
use komga_core::dto::series::{BookMetadataAggregationDto, SeriesDto, SeriesMetadataDto};
use komga_core::dto::url_to_file_path;
use komga_core::search::{SearchContext, SearchField, SeriesSearch};
use komga_core::task::LuceneEntity;
use rusqlite::types::Value;
use rusqlite::{params_from_iter, Row};
use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use time::{Date, OffsetDateTime};

// Selected column lists, qualified: SERIES, SERIES_METADATA, BOOK_METADATA_AGGREGATION, and
// READ_PROGRESS_SERIES share column names, and ORDER BY/GROUP BY reference the same expressions.
const SERIES_COLUMNS: &str = "SERIES.ID, SERIES.NAME, SERIES.URL, SERIES.FILE_LAST_MODIFIED, SERIES.LIBRARY_ID, \
 SERIES.BOOK_COUNT, SERIES.DELETED_DATE, SERIES.ONESHOT, SERIES.CREATED_DATE, SERIES.LAST_MODIFIED_DATE";
const METADATA_COLUMNS: &str = "SERIES_METADATA.SERIES_ID, SERIES_METADATA.STATUS, SERIES_METADATA.STATUS_LOCK, \
 SERIES_METADATA.TITLE, SERIES_METADATA.TITLE_LOCK, SERIES_METADATA.TITLE_SORT, SERIES_METADATA.TITLE_SORT_LOCK, \
 SERIES_METADATA.SUMMARY, SERIES_METADATA.SUMMARY_LOCK, SERIES_METADATA.READING_DIRECTION, \
 SERIES_METADATA.READING_DIRECTION_LOCK, SERIES_METADATA.PUBLISHER, SERIES_METADATA.PUBLISHER_LOCK, \
 SERIES_METADATA.AGE_RATING, SERIES_METADATA.AGE_RATING_LOCK, SERIES_METADATA.LANGUAGE, SERIES_METADATA.LANGUAGE_LOCK, \
 SERIES_METADATA.GENRES_LOCK, SERIES_METADATA.TAGS_LOCK, SERIES_METADATA.TOTAL_BOOK_COUNT, \
 SERIES_METADATA.TOTAL_BOOK_COUNT_LOCK, SERIES_METADATA.SHARING_LABELS_LOCK, SERIES_METADATA.LINKS_LOCK, \
 SERIES_METADATA.ALTERNATE_TITLES_LOCK, SERIES_METADATA.CREATED_DATE, SERIES_METADATA.LAST_MODIFIED_DATE";
const AGGREGATION_COLUMNS: &str = "BOOK_METADATA_AGGREGATION.SERIES_ID, BOOK_METADATA_AGGREGATION.RELEASE_DATE, \
 BOOK_METADATA_AGGREGATION.SUMMARY, BOOK_METADATA_AGGREGATION.SUMMARY_NUMBER, BOOK_METADATA_AGGREGATION.CREATED_DATE, \
 BOOK_METADATA_AGGREGATION.LAST_MODIFIED_DATE";
const READ_PROGRESS_COLUMNS: &str = "READ_PROGRESS_SERIES.SERIES_ID, READ_PROGRESS_SERIES.USER_ID, \
 READ_PROGRESS_SERIES.READ_COUNT, READ_PROGRESS_SERIES.IN_PROGRESS_COUNT, READ_PROGRESS_SERIES.MOST_RECENT_READ_DATE, \
 READ_PROGRESS_SERIES.LAST_MODIFIED_DATE";

fn select_columns() -> String {
    format!("{SERIES_COLUMNS}, {METADATA_COLUMNS}, {AGGREGATION_COLUMNS}, {READ_PROGRESS_COLUMNS}")
}

const FROM_BASE: &str = "FROM SERIES \
 LEFT JOIN SERIES_METADATA ON (SERIES.ID = SERIES_METADATA.SERIES_ID) \
 LEFT JOIN BOOK_METADATA_AGGREGATION ON (SERIES.ID = BOOK_METADATA_AGGREGATION.SERIES_ID) \
 LEFT JOIN READ_PROGRESS_SERIES ON (SERIES.ID = READ_PROGRESS_SERIES.SERIES_ID \
 AND (READ_PROGRESS_SERIES.USER_ID = ? OR READ_PROGRESS_SERIES.USER_ID IS NULL))";

pub struct SeriesDtoDao {
    db: Database,
    searcher: Option<Arc<dyn EntitySearcher>>,
}

impl SeriesDtoDao {
    pub fn new(db: Database) -> Self {
        Self { db, searcher: None }
    }

    pub fn with_searcher(mut self, searcher: Option<Arc<dyn EntitySearcher>>) -> Self {
        self.searcher = searcher;
        self
    }
}

/// Scalar fields of one joined row; child-table values are fetched separately.
struct SeriesRecord {
    id: String,
    name: String,
    url: String,
    file_last_modified: OffsetDateTime,
    library_id: String,
    book_count: i32,
    deleted_date: Option<OffsetDateTime>,
    oneshot: bool,
    created_date: OffsetDateTime,
    last_modified_date: OffsetDateTime,
    status: String,
    status_lock: bool,
    title: String,
    title_lock: bool,
    title_sort: String,
    title_sort_lock: bool,
    summary: String,
    summary_lock: bool,
    reading_direction: Option<String>,
    reading_direction_lock: bool,
    publisher: String,
    publisher_lock: bool,
    age_rating: Option<i32>,
    age_rating_lock: bool,
    language: String,
    language_lock: bool,
    genres_lock: bool,
    tags_lock: bool,
    total_book_count: Option<i32>,
    total_book_count_lock: bool,
    sharing_labels_lock: bool,
    links_lock: bool,
    alternate_titles_lock: bool,
    meta_created: OffsetDateTime,
    meta_last_modified: OffsetDateTime,
    agg_release_date: Option<Date>,
    agg_summary: String,
    agg_summary_number: String,
    agg_created: OffsetDateTime,
    agg_last_modified: OffsetDateTime,
    read_count: Option<i32>,
    in_progress_count: Option<i32>,
}

#[derive(Default)]
struct ChildrenMaps {
    genres: HashMap<String, Vec<String>>,
    tags: HashMap<String, Vec<String>>,
    sharing_labels: HashMap<String, Vec<String>>,
    links: HashMap<String, Vec<WebLinkDto>>,
    alternate_titles: HashMap<String, Vec<AlternateTitleDto>>,
    aggregated_authors: HashMap<String, Vec<AuthorDto>>,
    aggregated_tags: HashMap<String, Vec<String>>,
}

impl SeriesDtoDao {
    pub fn find_all(
        &self,
        search: &SeriesSearch,
        regex_search: Option<(&str, SearchField)>,
        ctx: &SearchContext,
        page: &PageRequest,
    ) -> Result<DtoPage<SeriesDto>> {
        let user_id = ctx
            .user_id
            .as_deref()
            .expect("Missing userId in search context");
        let lucene_ids = search_entity_ids(
            &self.searcher,
            search.full_text_search.as_deref(),
            LuceneEntity::Series,
        );
        let mut w = series_condition(search.condition.as_ref(), ctx);
        if let Some((regex, field)) = regex_search {
            w = w.and(series_regex_condition(regex, field));
        }
        if let Some(ids) = &lucene_ids {
            w = w.and(id_in_or_no_condition("SERIES.ID", Some(ids)));
        }
        self.find_all_internal(w, user_id, page, &lucene_ids)
    }

    pub fn find_all_recently_updated(
        &self,
        search: &SeriesSearch,
        ctx: &SearchContext,
        page: &PageRequest,
    ) -> Result<DtoPage<SeriesDto>> {
        let user_id = ctx
            .user_id
            .as_deref()
            .expect("Missing userId in search context");
        let lucene_ids = search_entity_ids(
            &self.searcher,
            search.full_text_search.as_deref(),
            LuceneEntity::Series,
        );
        let mut w = series_condition(search.condition.as_ref(), ctx).and(SqlWhere {
            sql: "SERIES.CREATED_DATE <> SERIES.LAST_MODIFIED_DATE".to_string(),
            params: vec![],
            joins: BTreeSet::new(),
        });
        if let Some(ids) = &lucene_ids {
            w = w.and(id_in_or_no_condition("SERIES.ID", Some(ids)));
        }
        self.find_all_internal(w, user_id, page, &lucene_ids)
    }

    pub fn count_by_first_character(
        &self,
        search: &SeriesSearch,
        regex_search: Option<(&str, SearchField)>,
        ctx: &SearchContext,
    ) -> Result<Vec<GroupCountDto>> {
        let user_id = ctx
            .user_id
            .as_deref()
            .expect("Missing userId in search context");
        let lucene_ids = search_entity_ids(
            &self.searcher,
            search.full_text_search.as_deref(),
            LuceneEntity::Series,
        );
        let mut w = series_condition(search.condition.as_ref(), ctx);
        if let Some((regex, field)) = regex_search {
            w = w.and(series_regex_condition(regex, field));
        }
        if let Some(ids) = &lucene_ids {
            w = w.and(id_in_or_no_condition("SERIES.ID", Some(ids)));
        }

        let (join_sql, join_params) = render_collection_joins(&w.joins);
        let first_char = "LOWER(SUBSTR(SERIES_METADATA.TITLE_SORT, 1, 1))";
        let sql = format!(
            "SELECT {first_char}, COUNT(*) {FROM_BASE} {join_sql} {} GROUP BY {first_char}",
            where_clause(&w)
        );
        let conn = self.db.ro();
        let mut stmt = conn.prepare(&sql)?;
        let groups = stmt
            .query_map(
                params_from_iter(
                    [Value::Text(user_id.to_string())]
                        .into_iter()
                        .chain(join_params)
                        .chain(w.params),
                ),
                |row| {
                    Ok(GroupCountDto {
                        group: row.get(0)?,
                        count: row.get(1)?,
                    })
                },
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(groups)
    }

    pub fn find_by_id(&self, series_id: &str, user_id: &str) -> Result<Option<SeriesDto>> {
        let columns = select_columns();
        let sql = format!("SELECT {columns} {FROM_BASE} WHERE SERIES.ID = ? GROUP BY {columns}");
        let conn = self.db.ro();
        let mut stmt = conn.prepare(&sql)?;
        let records = stmt
            .query_map(
                params_from_iter([
                    Value::Text(user_id.to_string()),
                    Value::Text(series_id.to_string()),
                ]),
                map_record,
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let children = fetch_children(&conn, &records)?;
        Ok(records.into_iter().map(|r| r.into_dto(&children)).next())
    }

    fn find_all_internal(
        &self,
        w: SqlWhere,
        user_id: &str,
        page: &PageRequest,
        lucene_ids: &Option<Vec<String>>,
    ) -> Result<DtoPage<SeriesDto>> {
        let conn = self.db.ro();
        let (join_sql, join_params) = render_collection_joins(&w.joins);
        let base_params = || {
            [Value::Text(user_id.to_string())]
                .into_iter()
                .chain(join_params.clone())
                .chain(w.params.clone())
        };

        let count_sql = format!(
            "SELECT COUNT(DISTINCT SERIES.ID) {FROM_BASE} {join_sql} {}",
            where_clause(&w)
        );
        let total: i64 =
            conn.query_row(&count_sql, params_from_iter(base_params()), |r| r.get(0))?;

        let (order_sql, order_params) = build_order_by(&page.sort, &w.joins, lucene_ids);
        let sorted = !order_sql.is_empty();
        let mut sql = format!(
            "SELECT {} {FROM_BASE} {join_sql} {}",
            select_columns(),
            where_clause(&w)
        );
        if sorted {
            sql.push_str(&format!(" ORDER BY {}", order_sql.join(", ")));
        }
        let mut params: Vec<Value> = base_params().collect();
        params.extend(order_params);
        if !page.unpaged {
            sql.push_str(" LIMIT ? OFFSET ?");
            params.push(Value::Integer(page.size as i64));
            params.push(Value::Integer(page.offset() as i64));
        }

        let mut stmt = conn.prepare(&sql)?;
        let records = stmt
            .query_map(params_from_iter(params), map_record)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let children = fetch_children(&conn, &records)?;
        let items = records.into_iter().map(|r| r.into_dto(&children)).collect();
        Ok(DtoPage {
            items,
            total,
            sorted,
        })
    }
}

fn where_clause(w: &SqlWhere) -> String {
    if w.sql.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", w.sql)
    }
}

/// Dynamic joins for `SearchCondition.CollectionId(Is)`: the ON parameter precedes WHERE
/// parameters in the SQL text, so it is returned separately and chained first.
fn render_collection_joins(joins: &BTreeSet<RequiredJoin>) -> (String, Vec<Value>) {
    let mut sql = String::new();
    let mut params = vec![];
    for join in joins {
        if let RequiredJoin::Collection(id) = join {
            let alias = collection_alias(id);
            sql.push_str(&format!(
                " LEFT JOIN COLLECTION_SERIES AS \"{alias}\" ON (SERIES.ID = \"{alias}\".SERIES_ID AND \"{alias}\".COLLECTION_ID = ?)"
            ));
            params.push(Value::Text(id.clone()));
        }
    }
    (sql, params)
}

/// Property → ORDER BY expressions, mirroring the `sorts` map plus the special
/// `collection.number` and `relevance` handling. Unknown properties are dropped.
fn build_order_by(
    sort: &[SortOrder],
    joins: &BTreeSet<RequiredJoin>,
    lucene_ids: &Option<Vec<String>>,
) -> (Vec<String>, Vec<Value>) {
    let mut expressions = vec![];
    let mut params = vec![];
    for order in sort {
        if order.property == "relevance" {
            // jOOQ sorts by the case expression itself; the direction is in the multiplier
            if let Some(ids) = lucene_ids.as_ref().filter(|ids| !ids.is_empty()) {
                let (expr, mut p) = sort_by_values("SERIES.ID", ids, !order.descending);
                expressions.push(expr);
                params.append(&mut p);
            }
            continue;
        }
        if order.property == "collection.number" {
            if let Some(id) = joins.iter().find_map(|j| match j {
                RequiredJoin::Collection(id) => Some(id),
                _ => None,
            }) {
                let field = format!("{}.NUMBER", collection_alias(id));
                expressions.push(direction(&field, order.descending));
            }
            continue;
        }
        let field = match order.property.as_str() {
            "metadata.titleSort" => "SERIES_METADATA.TITLE_SORT COLLATE COLLATION_UNICODE_3",
            "createdDate" | "created" => "SERIES.CREATED_DATE",
            "lastModifiedDate" | "lastModified" => "SERIES.LAST_MODIFIED_DATE",
            "booksMetadata.releaseDate" => "BOOK_METADATA_AGGREGATION.RELEASE_DATE",
            "readDate" => "READ_PROGRESS_SERIES.MOST_RECENT_READ_DATE",
            "name" => "SERIES.NAME COLLATE COLLATION_UNICODE_3",
            "booksCount" => "SERIES.BOOK_COUNT",
            "random" => "RANDOM()",
            _ => continue,
        };
        expressions.push(direction(field, order.descending));
    }
    (expressions, params)
}

fn direction(field: &str, descending: bool) -> String {
    if descending {
        format!("{field} DESC")
    } else {
        format!("{field} ASC")
    }
}

fn map_record(row: &Row<'_>) -> rusqlite::Result<SeriesRecord> {
    Ok(SeriesRecord {
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
        status: row.get(11)?,
        status_lock: row.get(12)?,
        title: row.get(13)?,
        title_lock: row.get(14)?,
        title_sort: row.get(15)?,
        title_sort_lock: row.get(16)?,
        summary: row.get(17)?,
        summary_lock: row.get(18)?,
        reading_direction: row.get(19)?,
        reading_direction_lock: row.get(20)?,
        publisher: row.get(21)?,
        publisher_lock: row.get(22)?,
        age_rating: row.get(23)?,
        age_rating_lock: row.get(24)?,
        language: row.get(25)?,
        language_lock: row.get(26)?,
        genres_lock: row.get(27)?,
        tags_lock: row.get(28)?,
        total_book_count: row.get(29)?,
        total_book_count_lock: row.get(30)?,
        sharing_labels_lock: row.get(31)?,
        links_lock: row.get(32)?,
        alternate_titles_lock: row.get(33)?,
        meta_created: get_datetime(row, 34)?,
        meta_last_modified: get_datetime(row, 35)?,
        agg_release_date: get_date(row, 37)?,
        agg_summary: row.get(38)?,
        agg_summary_number: row.get(39)?,
        agg_created: get_datetime(row, 40)?,
        agg_last_modified: get_datetime(row, 41)?,
        read_count: row.get(44)?,
        in_progress_count: row.get(45)?,
    })
}

impl SeriesRecord {
    fn into_dto(self, children: &ChildrenMaps) -> SeriesDto {
        let books_read_count = self.read_count.unwrap_or(0);
        let books_in_progress_count = self.in_progress_count.unwrap_or(0);
        let books_unread_count = self.book_count - books_read_count - books_in_progress_count;
        let metadata = SeriesMetadataDto {
            status: self.status,
            status_lock: self.status_lock,
            title: self.title,
            title_lock: self.title_lock,
            title_sort: self.title_sort,
            title_sort_lock: self.title_sort_lock,
            summary: self.summary,
            summary_lock: self.summary_lock,
            reading_direction: self.reading_direction.unwrap_or_default(),
            reading_direction_lock: self.reading_direction_lock,
            publisher: self.publisher,
            publisher_lock: self.publisher_lock,
            age_rating: self.age_rating,
            age_rating_lock: self.age_rating_lock,
            language: self.language,
            language_lock: self.language_lock,
            genres: take(&children.genres, &self.id),
            genres_lock: self.genres_lock,
            tags: take(&children.tags, &self.id),
            tags_lock: self.tags_lock,
            total_book_count: self.total_book_count,
            total_book_count_lock: self.total_book_count_lock,
            sharing_labels: take(&children.sharing_labels, &self.id),
            sharing_labels_lock: self.sharing_labels_lock,
            links: children.links.get(&self.id).cloned().unwrap_or_default(),
            links_lock: self.links_lock,
            alternate_titles: children
                .alternate_titles
                .get(&self.id)
                .cloned()
                .unwrap_or_default(),
            alternate_titles_lock: self.alternate_titles_lock,
            created: self.meta_created,
            last_modified: self.meta_last_modified,
        };
        let books_metadata = BookMetadataAggregationDto {
            authors: children
                .aggregated_authors
                .get(&self.id)
                .cloned()
                .unwrap_or_default(),
            tags: take(&children.aggregated_tags, &self.id),
            release_date: self.agg_release_date,
            summary: self.agg_summary,
            summary_number: self.agg_summary_number,
            created: self.agg_created,
            last_modified: self.agg_last_modified,
        };
        SeriesDto {
            id: self.id,
            library_id: self.library_id,
            name: self.name,
            url: url_to_file_path(&self.url),
            created: self.created_date,
            last_modified: self.last_modified_date,
            file_last_modified: self.file_last_modified,
            books_count: self.book_count,
            books_read_count,
            books_unread_count,
            books_in_progress_count,
            metadata,
            books_metadata,
            deleted: self.deleted_date.is_some(),
            oneshot: self.oneshot,
        }
    }
}

fn take(map: &HashMap<String, Vec<String>>, id: &str) -> BTreeSet<String> {
    map.get(id)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .collect()
}

/// Batched child-table lookups for the fetched ids (`fetchAndMap`); chunked to stay under
/// SQLite's parameter limit.
fn fetch_children(conn: &rusqlite::Connection, records: &[SeriesRecord]) -> Result<ChildrenMaps> {
    let mut maps = ChildrenMaps::default();
    let ids: Vec<&str> = records.iter().map(|r| r.id.as_str()).collect();
    for chunk in ids.chunks(500) {
        let ph = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
        let mut stmt = conn.prepare(&format!(
            "SELECT SERIES_ID, GENRE FROM SERIES_METADATA_GENRE WHERE SERIES_ID IN ({ph})"
        ))?;
        for row in stmt.query_map(params_from_iter(chunk.iter()), |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })? {
            let (id, v) = row?;
            maps.genres.entry(id).or_default().push(v);
        }

        let mut stmt = conn.prepare(&format!(
            "SELECT SERIES_ID, TAG FROM SERIES_METADATA_TAG WHERE SERIES_ID IN ({ph})"
        ))?;
        for row in stmt.query_map(params_from_iter(chunk.iter()), |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })? {
            let (id, v) = row?;
            maps.tags.entry(id).or_default().push(v);
        }

        let mut stmt = conn.prepare(&format!(
            "SELECT SERIES_ID, LABEL FROM SERIES_METADATA_SHARING WHERE SERIES_ID IN ({ph})"
        ))?;
        for row in stmt.query_map(params_from_iter(chunk.iter()), |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })? {
            let (id, v) = row?;
            maps.sharing_labels.entry(id).or_default().push(v);
        }

        let mut stmt = conn.prepare(&format!(
            "SELECT SERIES_ID, LABEL, URL FROM SERIES_METADATA_LINK WHERE SERIES_ID IN ({ph})"
        ))?;
        for row in stmt.query_map(params_from_iter(chunk.iter()), |r| {
            Ok((
                r.get::<_, String>(0)?,
                WebLinkDto {
                    label: r.get(1)?,
                    url: r.get(2)?,
                },
            ))
        })? {
            let (id, v) = row?;
            maps.links.entry(id).or_default().push(v);
        }

        let mut stmt = conn.prepare(&format!(
            "SELECT SERIES_ID, LABEL, TITLE FROM SERIES_METADATA_ALTERNATE_TITLE WHERE SERIES_ID IN ({ph})"
        ))?;
        for row in stmt.query_map(params_from_iter(chunk.iter()), |r| {
            Ok((
                r.get::<_, String>(0)?,
                AlternateTitleDto {
                    label: r.get(1)?,
                    title: r.get(2)?,
                },
            ))
        })? {
            let (id, v) = row?;
            maps.alternate_titles.entry(id).or_default().push(v);
        }

        let mut stmt = conn.prepare(&format!(
            "SELECT SERIES_ID, NAME, ROLE FROM BOOK_METADATA_AGGREGATION_AUTHOR WHERE SERIES_ID IN ({ph}) AND NAME IS NOT NULL"
        ))?;
        for row in stmt.query_map(params_from_iter(chunk.iter()), |r| {
            Ok((
                r.get::<_, String>(0)?,
                AuthorDto {
                    name: r.get(1)?,
                    role: r.get(2)?,
                },
            ))
        })? {
            let (id, v) = row?;
            maps.aggregated_authors.entry(id).or_default().push(v);
        }

        let mut stmt = conn.prepare(&format!(
            "SELECT SERIES_ID, TAG FROM BOOK_METADATA_AGGREGATION_TAG WHERE SERIES_ID IN ({ph})"
        ))?;
        for row in stmt.query_map(params_from_iter(chunk.iter()), |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })? {
            let (id, v) = row?;
            maps.aggregated_tags.entry(id).or_default().push(v);
        }
    }
    Ok(maps)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dao::series::{BookMetadataAggregationDao, SeriesDao, SeriesMetadataDao};
    use crate::migrate::Migrator;
    use crate::{main_migrations, Placeholders};
    use komga_core::model::common::{Author, WebLink};
    use komga_core::model::series::{
        AlternateTitle, BookMetadataAggregation, ReadingDirection, Series, SeriesMetadata,
        SeriesStatus,
    };
    use komga_core::model::user::{AgeRestriction, AllowExclude, ContentRestrictions};
    use komga_core::search::*;
    use komga_core::time_codec::now_utc;
    use rusqlite::params;

    fn db() -> Database {
        let db = Database::open_in_memory(true).unwrap();
        let migrations = main_migrations();
        Migrator::new(&migrations, Placeholders::default())
            .migrate(&db.rw())
            .unwrap();
        db
    }

    fn ctx(user_id: &str) -> SearchContext {
        SearchContext {
            user_id: Some(user_id.to_string()),
            restrictions: ContentRestrictions::default(),
            library_ids: None,
        }
    }

    fn page(sort: &[(&str, bool)]) -> PageRequest {
        PageRequest {
            page: 0,
            size: 20,
            unpaged: false,
            sort: sort
                .iter()
                .map(|(p, desc)| SortOrder {
                    property: p.to_string(),
                    descending: *desc,
                })
                .collect(),
        }
    }

    fn unpaged() -> PageRequest {
        PageRequest {
            page: 0,
            size: 20,
            unpaged: true,
            sort: vec![],
        }
    }

    fn insert_library(db: &Database, id: &str) {
        db.rw()
            .execute(
                "INSERT INTO LIBRARY (ID, NAME, ROOT) VALUES (?, ?, ?)",
                params![id, format!("lib-{id}"), format!("file:/{id}/")],
            )
            .unwrap();
    }

    fn insert_series(db: &Database, id: &str, library_id: &str, name: &str, book_count: i32) {
        let dao = SeriesDao::new(db.clone());
        let id_ = dao
            .insert(&Series {
                id: id.to_string(),
                name: name.to_string(),
                url: format!("file:/{library_id}/{name}/"),
                file_last_modified: now_utc(),
                library_id: library_id.to_string(),
                book_count: 0,
                deleted_date: None,
                oneshot: false,
                created_date: now_utc(),
                last_modified_date: now_utc(),
            })
            .unwrap();
        assert_eq!(id_, id);
        // insert falls back to the BOOK_COUNT column default; set the intended value explicitly
        let mut series = dao.find_by_id(id).unwrap().unwrap();
        series.book_count = book_count;
        dao.update(&series, false).unwrap();
    }

    #[allow(clippy::too_many_arguments)]
    fn insert_metadata(
        db: &Database,
        series_id: &str,
        title_sort: &str,
        publisher: &str,
        age_rating: Option<i32>,
        total_book_count: Option<i32>,
        genres: &[&str],
        tags: &[&str],
        sharing_labels: &[&str],
    ) {
        SeriesMetadataDao::new(db.clone())
            .insert(&SeriesMetadata {
                series_id: series_id.to_string(),
                status: SeriesStatus::Ongoing,
                title: title_sort.to_string(),
                title_sort: title_sort.to_string(),
                summary: String::new(),
                reading_direction: Some(ReadingDirection::LeftToRight),
                publisher: publisher.to_string(),
                age_rating,
                language: "en".to_string(),
                genres: genres.iter().map(|s| s.to_string()).collect(),
                tags: tags.iter().map(|s| s.to_string()).collect(),
                total_book_count,
                sharing_labels: sharing_labels.iter().map(|s| s.to_string()).collect(),
                links: vec![WebLink {
                    label: "wiki".to_string(),
                    url: "https://example.org".to_string(),
                }],
                alternate_titles: vec![AlternateTitle {
                    label: "ja".to_string(),
                    title: "タイトル".to_string(),
                }],
                status_lock: false,
                title_lock: false,
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
            })
            .unwrap();
    }

    fn insert_aggregation(db: &Database, series_id: &str, tags: &[&str], authors: &[(&str, &str)]) {
        BookMetadataAggregationDao::new(db.clone())
            .insert(&BookMetadataAggregation {
                series_id: series_id.to_string(),
                authors: authors
                    .iter()
                    .map(|(name, role)| Author::new(name, role))
                    .collect(),
                tags: tags.iter().map(|s| s.to_string()).collect(),
                release_date: None,
                summary: String::new(),
                summary_number: String::new(),
                created_date: now_utc(),
                last_modified_date: now_utc(),
            })
            .unwrap();
    }

    fn insert_read_progress_series(
        db: &Database,
        series_id: &str,
        user_id: &str,
        read: i32,
        in_progress: i32,
    ) {
        db.rw()
            .execute(
                "INSERT INTO READ_PROGRESS_SERIES (SERIES_ID, USER_ID, READ_COUNT, IN_PROGRESS_COUNT) \
                 VALUES (?, ?, ?, ?)",
                params![series_id, user_id, read, in_progress],
            )
            .unwrap();
    }

    fn insert_collection(db: &Database, id: &str, entries: &[(&str, i32)]) {
        db.rw()
            .execute(
                "INSERT INTO COLLECTION (ID, NAME, ORDERED, SERIES_COUNT) VALUES (?, ?, 1, ?)",
                params![id, format!("col-{id}"), entries.len() as i32],
            )
            .unwrap();
        for (series_id, number) in entries {
            db.rw()
                .execute(
                    "INSERT INTO COLLECTION_SERIES (COLLECTION_ID, SERIES_ID, NUMBER) VALUES (?, ?, ?)",
                    params![id, series_id, number],
                )
                .unwrap();
        }
    }

    /// lib1: s1 (Berserk, complete, tag seinen, age 18, label nsfw, read 3/3),
    ///       s2 (akira, incomplete, aggregation tag, age 12, in progress 1/2);
    /// lib2: s3 (20th Century Boys, deleted, oneshot, unread).
    fn fixtures() -> Database {
        let db = db();
        db.rw()
            .execute(
                "INSERT INTO USER (ID, EMAIL, PASSWORD) VALUES ('u1', 'u@x.y', 'x')",
                [],
            )
            .unwrap();
        insert_library(&db, "lib1");
        insert_library(&db, "lib2");

        insert_series(&db, "s1", "lib1", "Berserk", 3);
        insert_metadata(
            &db,
            "s1",
            "Berserk",
            "Hakusensha",
            Some(18),
            Some(3),
            &["action"],
            &["seinen"],
            &["nsfw"],
        );
        insert_aggregation(&db, "s1", &["seinen"], &[("Kentaro Miura", "writer")]);
        insert_read_progress_series(&db, "s1", "u1", 3, 0);

        insert_series(&db, "s2", "lib1", "Akira", 2);
        insert_metadata(
            &db,
            "s2",
            "akira",
            "Kodansha",
            Some(12),
            Some(5),
            &[],
            &[],
            &[],
        );
        insert_aggregation(&db, "s2", &["cyberpunk"], &[]);
        insert_read_progress_series(&db, "s2", "u1", 1, 0);

        insert_series(&db, "s3", "lib2", "20th Century Boys", 1);
        insert_metadata(
            &db,
            "s3",
            "20th Century Boys",
            "Shogakukan",
            None,
            None,
            &[],
            &[],
            &[],
        );
        insert_aggregation(&db, "s3", &[], &[]);
        db.rw()
            .execute(
                "UPDATE SERIES SET DELETED_DATE = '2024-01-01 00:00:00', ONESHOT = 1 WHERE ID = 's3'",
                [],
            )
            .unwrap();

        insert_collection(&db, "c1", &[("s2", 1), ("s1", 2)]);
        db
    }

    fn search(condition: Option<SearchConditionSeries>) -> SeriesSearch {
        SeriesSearch {
            condition,
            full_text_search: None,
        }
    }

    #[test]
    fn find_all_pagination_and_titlesort_order() {
        let db = fixtures();
        let dao = SeriesDtoDao::new(db);
        let result = dao
            .find_all(
                &search(None),
                None,
                &ctx("u1"),
                &page(&[("metadata.titleSort", false)]),
            )
            .unwrap();
        assert_eq!(result.total, 3);
        assert!(result.sorted);
        let ids: Vec<&str> = result.items.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["s3", "s2", "s1"]);

        let page2 = dao
            .find_all(
                &search(None),
                None,
                &ctx("u1"),
                &PageRequest {
                    page: 1,
                    size: 2,
                    unpaged: false,
                    sort: page(&[("metadata.titleSort", false)]).sort,
                },
            )
            .unwrap();
        assert_eq!(page2.total, 3);
        assert_eq!(page2.items.len(), 1);
        assert_eq!(page2.items[0].id, "s1");
    }

    #[test]
    fn find_all_unpaged_and_name_desc() {
        let db = fixtures();
        let dao = SeriesDtoDao::new(db);
        let result = dao
            .find_all(&search(None), None, &ctx("u1"), &unpaged())
            .unwrap();
        assert_eq!(result.total, 3);
        assert_eq!(result.items.len(), 3);
        assert!(!result.sorted);

        let result = dao
            .find_all(&search(None), None, &ctx("u1"), &page(&[("name", true)]))
            .unwrap();
        let ids: Vec<&str> = result.items.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["s1", "s2", "s3"]);
    }

    #[test]
    fn dto_fields_are_mapped() {
        let db = fixtures();
        let dao = SeriesDtoDao::new(db);
        let dto = dao.find_by_id("s1", "u1").unwrap().unwrap();
        assert_eq!(dto.library_id, "lib1");
        assert_eq!(dto.url, "/lib1/Berserk");
        assert_eq!(dto.books_count, 3);
        assert_eq!(dto.books_read_count, 3);
        assert_eq!(dto.books_in_progress_count, 0);
        assert_eq!(dto.books_unread_count, 0);
        assert!(!dto.deleted);
        assert!(!dto.oneshot);
        assert_eq!(dto.metadata.status, "ONGOING");
        assert_eq!(dto.metadata.reading_direction, "LEFT_TO_RIGHT");
        assert_eq!(dto.metadata.age_rating, Some(18));
        assert_eq!(dto.metadata.total_book_count, Some(3));
        assert_eq!(
            dto.metadata.genres,
            ["action".to_string()].into_iter().collect()
        );
        assert_eq!(
            dto.metadata.tags,
            ["seinen".to_string()].into_iter().collect()
        );
        assert_eq!(
            dto.metadata.sharing_labels,
            ["nsfw".to_string()].into_iter().collect()
        );
        assert_eq!(dto.metadata.links.len(), 1);
        assert_eq!(dto.metadata.alternate_titles.len(), 1);
        assert_eq!(dto.books_metadata.authors.len(), 1);
        assert_eq!(dto.books_metadata.authors[0].name, "Kentaro Miura");
        assert_eq!(
            dto.books_metadata.tags,
            ["seinen".to_string()].into_iter().collect()
        );

        let s3 = dao.find_by_id("s3", "u1").unwrap().unwrap();
        assert!(s3.deleted);
        assert!(s3.oneshot);
        assert_eq!(s3.books_unread_count, 1);
        // series without a READ_PROGRESS_SERIES row gets zero counts
        assert_eq!(s3.books_read_count, 0);
    }

    #[test]
    fn filter_by_library_id() {
        let db = fixtures();
        let dao = SeriesDtoDao::new(db);
        let condition: SearchConditionSeries =
            serde_json::from_str(r#"{"libraryId":{"operator":"is","value":"lib1"}}"#).unwrap();
        let result = dao
            .find_all(&search(Some(condition)), None, &ctx("u1"), &unpaged())
            .unwrap();
        assert_eq!(result.total, 2);
        assert!(result.items.iter().all(|s| s.library_id == "lib1"));
    }

    #[test]
    fn filter_by_read_status() {
        let db = fixtures();
        let dao = SeriesDtoDao::new(db);
        let by_status = |value: &str, op: &str| {
            let condition: SearchConditionSeries = serde_json::from_str(&format!(
                r#"{{"readStatus":{{"operator":"{op}","value":"{value}"}}}}"#
            ))
            .unwrap();
            dao.find_all(&search(Some(condition)), None, &ctx("u1"), &unpaged())
                .unwrap()
                .items
                .into_iter()
                .map(|s| s.id)
                .collect::<Vec<_>>()
        };
        assert_eq!(by_status("READ", "is"), ["s1"]);
        assert_eq!(by_status("UNREAD", "is"), ["s3"]);
        assert_eq!(by_status("IN_PROGRESS", "is"), ["s2"]);
        assert_eq!(by_status("READ", "isNot").len(), 2);
        assert_eq!(by_status("UNREAD", "isNot"), ["s1", "s2"]);
    }

    #[test]
    fn filter_by_tag_unions_metadata_and_aggregation() {
        let db = fixtures();
        let dao = SeriesDtoDao::new(db);
        let by_tag = |tag: &str| {
            let condition: SearchConditionSeries =
                serde_json::from_str(&format!(r#"{{"tag":{{"operator":"is","value":"{tag}"}}}}"#))
                    .unwrap();
            dao.find_all(&search(Some(condition)), None, &ctx("u1"), &unpaged())
                .unwrap()
                .items
                .into_iter()
                .map(|s| s.id)
                .collect::<Vec<_>>()
        };
        // series metadata tag
        assert_eq!(by_tag("seinen"), ["s1"]);
        // book aggregation tag
        assert_eq!(by_tag("cyberpunk"), ["s2"]);
        assert!(by_tag("nonexistent").is_empty());

        let condition: SearchConditionSeries =
            serde_json::from_str(r#"{"tag":{"operator":"isNull"}}"#).unwrap();
        let result = dao
            .find_all(&search(Some(condition)), None, &ctx("u1"), &unpaged())
            .unwrap();
        assert_eq!(result.items.len(), 1);
        assert_eq!(result.items[0].id, "s3");
    }

    #[test]
    fn filter_publisher_is_case_insensitive() {
        let db = fixtures();
        let dao = SeriesDtoDao::new(db);
        let condition: SearchConditionSeries =
            serde_json::from_str(r#"{"publisher":{"operator":"is","value":"hakusensha"}}"#)
                .unwrap();
        let result = dao
            .find_all(&search(Some(condition)), None, &ctx("u1"), &unpaged())
            .unwrap();
        assert_eq!(result.items.len(), 1);
        assert_eq!(result.items[0].id, "s1");
    }

    #[test]
    fn filter_deleted_oneshot_complete() {
        let db = fixtures();
        let dao = SeriesDtoDao::new(db);
        let ids = |json: &str| {
            let condition: SearchConditionSeries = serde_json::from_str(json).unwrap();
            dao.find_all(&search(Some(condition)), None, &ctx("u1"), &unpaged())
                .unwrap()
                .items
                .into_iter()
                .map(|s| s.id)
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(r#"{"deleted":{"operator":"isTrue"}}"#), ["s3"]);
        assert_eq!(ids(r#"{"deleted":{"operator":"isFalse"}}"#).len(), 2);
        assert_eq!(ids(r#"{"oneShot":{"operator":"isTrue"}}"#), ["s3"]);
        assert_eq!(ids(r#"{"complete":{"operator":"isTrue"}}"#), ["s1"]);
        assert_eq!(ids(r#"{"complete":{"operator":"isFalse"}}"#), ["s2"]);
    }

    #[test]
    fn content_restrictions_filter_in_sql() {
        let db = fixtures();
        let dao = SeriesDtoDao::new(db);
        // allow only age <= 15 or label "kids": s1 (18, nsfw) and s3 (no age) are denied
        let restrictions = ContentRestrictions::new(
            Some(AgeRestriction {
                age: 15,
                restriction: AllowExclude::AllowOnly,
            }),
            ["kids".to_string()].into_iter().collect(),
            std::collections::BTreeSet::new(),
        );
        let restricted = SearchContext {
            user_id: Some("u1".to_string()),
            restrictions,
            library_ids: None,
        };
        let result = dao
            .find_all(&search(None), None, &restricted, &unpaged())
            .unwrap();
        assert_eq!(result.items.len(), 1);
        assert_eq!(result.items[0].id, "s2");
    }

    #[test]
    fn authorized_libraries_filter_in_sql() {
        let db = fixtures();
        let dao = SeriesDtoDao::new(db);
        let limited = SearchContext {
            library_ids: Some(["lib2".to_string()].into_iter().collect()),
            ..ctx("u1")
        };
        let result = dao
            .find_all(&search(None), None, &limited, &unpaged())
            .unwrap();
        assert_eq!(result.items.len(), 1);
        assert_eq!(result.items[0].id, "s3");

        let none = SearchContext {
            library_ids: Some(std::collections::BTreeSet::new()),
            ..ctx("u1")
        };
        assert_eq!(
            dao.find_all(&search(None), None, &none, &unpaged())
                .unwrap()
                .total,
            0
        );
    }

    #[test]
    fn count_by_first_character_groups() {
        let db = fixtures();
        let dao = SeriesDtoDao::new(db);
        let mut groups = dao
            .count_by_first_character(&search(None), None, &ctx("u1"))
            .unwrap();
        groups.sort_by(|a, b| a.group.cmp(&b.group));
        assert_eq!(
            groups,
            vec![
                GroupCountDto {
                    group: "2".into(),
                    count: 1
                },
                GroupCountDto {
                    group: "a".into(),
                    count: 1
                },
                GroupCountDto {
                    group: "b".into(),
                    count: 1
                },
            ]
        );
    }

    #[test]
    fn regex_search_filters() {
        let db = fixtures();
        let dao = SeriesDtoDao::new(db);
        let result = dao
            .find_all(
                &search(None),
                Some(("^Ber", SearchField::Title)),
                &ctx("u1"),
                &unpaged(),
            )
            .unwrap();
        assert_eq!(result.items.len(), 1);
        assert_eq!(result.items[0].id, "s1");

        let result = dao
            .find_all(
                &search(None),
                Some(("^xyz", SearchField::Title)),
                &ctx("u1"),
                &unpaged(),
            )
            .unwrap();
        assert_eq!(result.total, 0);
    }

    #[test]
    fn recently_updated_excludes_unmodified() {
        let db = fixtures();
        db.rw()
            .execute(
                "UPDATE SERIES SET LAST_MODIFIED_DATE = '2030-01-01 00:00:00' WHERE ID = 's1'",
                [],
            )
            .unwrap();
        let dao = SeriesDtoDao::new(db);
        let result = dao
            .find_all_recently_updated(&search(None), &ctx("u1"), &unpaged())
            .unwrap();
        assert_eq!(result.items.len(), 1);
        assert_eq!(result.items[0].id, "s1");
    }

    #[test]
    fn collection_join_condition_and_number_sort() {
        let db = fixtures();
        let dao = SeriesDtoDao::new(db);
        let condition: SearchConditionSeries =
            serde_json::from_str(r#"{"collectionId":{"operator":"is","value":"c1"}}"#).unwrap();
        let result = dao
            .find_all(
                &search(Some(condition)),
                None,
                &ctx("u1"),
                &page(&[("collection.number", false)]),
            )
            .unwrap();
        assert_eq!(result.total, 2);
        let ids: Vec<&str> = result.items.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["s2", "s1"]);

        // isNot needs no join
        let condition: SearchConditionSeries =
            serde_json::from_str(r#"{"collectionId":{"operator":"isNot","value":"c1"}}"#).unwrap();
        let result = dao
            .find_all(&search(Some(condition)), None, &ctx("u1"), &unpaged())
            .unwrap();
        assert_eq!(result.items.len(), 1);
        assert_eq!(result.items[0].id, "s3");
    }

    #[test]
    fn full_text_search_stub_returns_nothing() {
        let db = fixtures();
        let dao = SeriesDtoDao::new(db);
        let result = dao
            .find_all(
                &SeriesSearch {
                    condition: None,
                    full_text_search: Some("berserk".to_string()),
                },
                None,
                &ctx("u1"),
                &unpaged(),
            )
            .unwrap();
        assert_eq!(result.total, 0);
    }
}
