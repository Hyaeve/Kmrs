//! `BookDtoDao.kt`: book DTO queries, plus the on-deck query from `BookCommonDao.kt`.
//!
//! The base select layout is BOOK (14 columns) + MEDIA (11) + BOOK_METADATA (18) +
//! READ_PROGRESS (10) + SERIES_METADATA.TITLE (1); column offsets below are tied to
//! `SELECT_CLAUSE` and must stay in sync with it.

use super::{search_entity_ids, DtoPage, EntitySearcher, PageRequest};
use crate::dao::{get_date, get_datetime, get_datetime_opt};
use crate::error::Result;
use crate::pool::Database;
use crate::search_sql::{
    book_condition, content_restrictions_condition, id_in_or_no_condition, readlist_alias,
    sort_by_values, RequiredJoin, SqlWhere,
};
use komga_core::dto::book::{BookDto, BookMetadataDto, MediaDto, ReadProgressDto};
use komga_core::dto::common::{AuthorDto, WebLinkDto};
use komga_core::dto::url_to_file_path;
use komga_core::model::readlist::ReadList;
use komga_core::model::user::ContentRestrictions;
use komga_core::search::{BookSearch, SearchContext};
use komga_core::task::LuceneEntity;
use rusqlite::types::Value;
use rusqlite::{params_from_iter, Connection, OptionalExtension, Row};
use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

const SELECT_CLAUSE: &str = "SELECT \
 BOOK.ID, BOOK.NAME, BOOK.URL, BOOK.FILE_LAST_MODIFIED, BOOK.SERIES_ID, BOOK.LIBRARY_ID, \
 BOOK.FILE_SIZE, BOOK.NUMBER, BOOK.FILE_HASH, BOOK.FILE_HASH_KOREADER, BOOK.DELETED_DATE, \
 BOOK.ONESHOT, BOOK.CREATED_DATE, BOOK.LAST_MODIFIED_DATE, \
 MEDIA.BOOK_ID, MEDIA.STATUS, MEDIA.MEDIA_TYPE, MEDIA.COMMENT, MEDIA.PAGE_COUNT, \
 MEDIA.EPUB_DIVINA_COMPATIBLE, MEDIA.EPUB_IS_KEPUB, MEDIA.EXTENSION_CLASS, \
 MEDIA.EXTENSION_VALUE_BLOB, MEDIA.CREATED_DATE, MEDIA.LAST_MODIFIED_DATE, \
 BOOK_METADATA.BOOK_ID, BOOK_METADATA.TITLE, BOOK_METADATA.TITLE_LOCK, BOOK_METADATA.SUMMARY, \
 BOOK_METADATA.SUMMARY_LOCK, BOOK_METADATA.NUMBER, BOOK_METADATA.NUMBER_LOCK, \
 BOOK_METADATA.NUMBER_SORT, BOOK_METADATA.NUMBER_SORT_LOCK, BOOK_METADATA.RELEASE_DATE, \
 BOOK_METADATA.RELEASE_DATE_LOCK, BOOK_METADATA.AUTHORS_LOCK, BOOK_METADATA.TAGS_LOCK, \
 BOOK_METADATA.ISBN, BOOK_METADATA.ISBN_LOCK, BOOK_METADATA.LINKS_LOCK, \
 BOOK_METADATA.CREATED_DATE, BOOK_METADATA.LAST_MODIFIED_DATE, \
 READ_PROGRESS.BOOK_ID, READ_PROGRESS.USER_ID, READ_PROGRESS.PAGE, READ_PROGRESS.COMPLETED, \
 READ_PROGRESS.READ_DATE, READ_PROGRESS.DEVICE_ID, READ_PROGRESS.DEVICE_NAME, \
 READ_PROGRESS.LOCATOR, READ_PROGRESS.CREATED_DATE, READ_PROGRESS.LAST_MODIFIED_DATE, \
 SERIES_METADATA.TITLE";

// column offsets within a base-select row
const B_NAME: usize = 1;
const B_URL: usize = 2;
const B_FILE_LAST_MODIFIED: usize = 3;
const B_SERIES_ID: usize = 4;
const B_LIBRARY_ID: usize = 5;
const B_FILE_SIZE: usize = 6;
const B_NUMBER: usize = 7;
const B_FILE_HASH: usize = 8;
const B_DELETED_DATE: usize = 10;
const B_ONESHOT: usize = 11;
const B_CREATED_DATE: usize = 12;
const B_LAST_MODIFIED_DATE: usize = 13;
const M_STATUS: usize = 15;
const M_MEDIA_TYPE: usize = 16;
const M_COMMENT: usize = 17;
const M_PAGE_COUNT: usize = 18;
const M_EPUB_DIVINA_COMPATIBLE: usize = 19;
const M_EPUB_IS_KEPUB: usize = 20;
const D_TITLE: usize = 26;
const D_TITLE_LOCK: usize = 27;
const D_SUMMARY: usize = 28;
const D_SUMMARY_LOCK: usize = 29;
const D_NUMBER: usize = 30;
const D_NUMBER_LOCK: usize = 31;
const D_NUMBER_SORT: usize = 32;
const D_NUMBER_SORT_LOCK: usize = 33;
const D_RELEASE_DATE: usize = 34;
const D_RELEASE_DATE_LOCK: usize = 35;
const D_AUTHORS_LOCK: usize = 36;
const D_TAGS_LOCK: usize = 37;
const D_ISBN: usize = 38;
const D_ISBN_LOCK: usize = 39;
const D_LINKS_LOCK: usize = 40;
const D_CREATED_DATE: usize = 41;
const D_LAST_MODIFIED_DATE: usize = 42;
const R_USER_ID: usize = 44;
const R_PAGE: usize = 45;
const R_COMPLETED: usize = 46;
const R_READ_DATE: usize = 47;
const R_DEVICE_ID: usize = 48;
const R_DEVICE_NAME: usize = 49;
const R_CREATED_DATE: usize = 51;
const R_LAST_MODIFIED_DATE: usize = 52;
const SD_TITLE: usize = 53;

pub struct BookDtoDao {
    db: Database,
    searcher: Option<Arc<dyn EntitySearcher>>,
}

impl BookDtoDao {
    pub fn new(db: Database) -> Self {
        Self { db, searcher: None }
    }

    pub fn with_searcher(mut self, searcher: Option<Arc<dyn EntitySearcher>>) -> Self {
        self.searcher = searcher;
        self
    }

    pub fn find_all(
        &self,
        search: &BookSearch,
        ctx: &SearchContext,
        page: &PageRequest,
    ) -> Result<DtoPage<BookDto>> {
        // mirrors Kotlin's requireNotNull on the search context user id
        let user_id = ctx
            .user_id
            .as_deref()
            .expect("Missing userId in search context");
        let ids = search_entity_ids(
            &self.searcher,
            search.full_text_search.as_deref(),
            LuceneEntity::Book,
        );
        let conditions = book_condition(search.condition.as_ref(), ctx)
            .and(id_in_or_no_condition("BOOK.ID", ids.as_deref()));
        let conn = self.db.ro();

        let total = count(&conn, &conditions, user_id)?;

        let (orders, order_params) = build_orders(page, &conditions.joins, ids.as_deref());
        let (from, mut params) = select_from(user_id, &conditions.joins);
        let mut sql = format!("{SELECT_CLAUSE} {from}");
        if !conditions.sql.is_empty() {
            sql.push_str(&format!(" WHERE {}", conditions.sql));
        }
        params.extend(conditions.params.iter().cloned());
        if !orders.is_empty() {
            sql.push_str(&format!(" ORDER BY {}", orders.join(", ")));
        }
        params.extend(order_params);
        if !page.unpaged {
            sql.push_str(" LIMIT ? OFFSET ?");
            params.push(Value::Integer(page.size as i64));
            params.push(Value::Integer(page.offset() as i64));
        }
        let items = fetch_and_map(&conn, &sql, params)?;
        Ok(DtoPage {
            items,
            total,
            sorted: !orders.is_empty(),
        })
    }

    pub fn find_by_id(&self, book_id: &str, user_id: &str) -> Result<Option<BookDto>> {
        let conn = self.db.ro();
        let (from, mut params) = select_from(user_id, &BTreeSet::new());
        let sql = format!("{SELECT_CLAUSE} {from} WHERE BOOK.ID = ?");
        params.push(Value::Text(book_id.to_string()));
        Ok(fetch_and_map(&conn, &sql, params)?.into_iter().next())
    }

    pub fn find_previous_in_series(&self, book_id: &str, user_id: &str) -> Result<Option<BookDto>> {
        self.find_sibling_series(book_id, user_id, false)
    }

    pub fn find_next_in_series(&self, book_id: &str, user_id: &str) -> Result<Option<BookDto>> {
        self.find_sibling_series(book_id, user_id, true)
    }

    pub fn find_previous_in_readlist(
        &self,
        readlist: &ReadList,
        book_id: &str,
        user_id: &str,
        filter_library_ids: Option<&BTreeSet<String>>,
        restrictions: &ContentRestrictions,
    ) -> Result<Option<BookDto>> {
        self.find_sibling_readlist(
            readlist,
            book_id,
            user_id,
            filter_library_ids,
            restrictions,
            false,
        )
    }

    pub fn find_next_in_readlist(
        &self,
        readlist: &ReadList,
        book_id: &str,
        user_id: &str,
        filter_library_ids: Option<&BTreeSet<String>>,
        restrictions: &ContentRestrictions,
    ) -> Result<Option<BookDto>> {
        self.find_sibling_readlist(
            readlist,
            book_id,
            user_id,
            filter_library_ids,
            restrictions,
            true,
        )
    }

    fn find_sibling_series(
        &self,
        book_id: &str,
        user_id: &str,
        next: bool,
    ) -> Result<Option<BookDto>> {
        let conn = self.db.ro();
        // Kotlin uses fetchOne()!! here: an unknown book id is an internal error (500), not a 404
        let (series_id, number_sort): (String, Option<f32>) = conn.query_row(
            "SELECT BOOK.SERIES_ID, BOOK_METADATA.NUMBER_SORT FROM BOOK \
             LEFT JOIN BOOK_METADATA ON (BOOK.ID = BOOK_METADATA.BOOK_ID) WHERE BOOK.ID = ?",
            [book_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        // jOOQ `.seek(null)` renders a comparison against NULL, which never matches
        let Some(number_sort) = number_sort else {
            return Ok(None);
        };
        let (cmp, dir) = if next { (">", "ASC") } else { ("<", "DESC") };
        let (from, mut params) = select_from(user_id, &BTreeSet::new());
        let sql = format!(
            "{SELECT_CLAUSE} {from} WHERE BOOK.SERIES_ID = ? AND BOOK_METADATA.NUMBER_SORT {cmp} ? \
             ORDER BY BOOK_METADATA.NUMBER_SORT {dir} LIMIT 1"
        );
        params.push(Value::Text(series_id));
        params.push(Value::Real(number_sort as f64));
        Ok(fetch_and_map(&conn, &sql, params)?.into_iter().next())
    }

    fn find_sibling_readlist(
        &self,
        readlist: &ReadList,
        book_id: &str,
        user_id: &str,
        filter_library_ids: Option<&BTreeSet<String>>,
        restrictions: &ContentRestrictions,
        next: bool,
    ) -> Result<Option<BookDto>> {
        let conn = self.db.ro();
        let library_vec: Option<Vec<String>> =
            filter_library_ids.map(|ids| ids.iter().cloned().collect());
        if readlist.ordered {
            let mut sql = String::from(
                "SELECT READLIST_BOOK.NUMBER FROM BOOK \
                 LEFT JOIN READLIST_BOOK ON (BOOK.ID = READLIST_BOOK.BOOK_ID) \
                 WHERE BOOK.ID = ? AND READLIST_BOOK.READLIST_ID = ?",
            );
            let mut params = vec![
                Value::Text(book_id.to_string()),
                Value::Text(readlist.id.clone()),
            ];
            if let Some(ids) = &library_vec {
                let w = id_in_or_no_condition("BOOK.LIBRARY_ID", Some(ids));
                sql.push_str(&format!(" AND {}", w.sql));
                params.extend(w.params);
            }
            let number: Option<Option<i32>> = conn
                .query_row(&sql, params_from_iter(params), |r| r.get(0))
                .optional()?;
            // jOOQ `.seek(null)` never matches
            let Some(Some(number)) = number else {
                return Ok(None);
            };

            let mut joins = BTreeSet::new();
            joins.insert(RequiredJoin::ReadList(readlist.id.clone()));
            let (from, mut params) = select_from(user_id, &joins);
            let mut conditions = SqlWhere::no_condition();
            if restrictions.is_restricted() {
                conditions = conditions.and(content_restrictions_condition(restrictions));
            }
            if let Some(ids) = &library_vec {
                conditions = conditions.and(id_in_or_no_condition("BOOK.LIBRARY_ID", Some(ids)));
            }
            let alias = readlist_alias(&readlist.id);
            let (cmp, dir) = if next { (">", "ASC") } else { ("<", "DESC") };
            conditions = conditions.and(SqlWhere {
                sql: format!("{alias}.NUMBER {cmp} ?"),
                params: vec![Value::Integer(number as i64)],
                joins: BTreeSet::new(),
            });
            let mut sql = format!("{SELECT_CLAUSE} {from}");
            if !conditions.sql.is_empty() {
                sql.push_str(&format!(" WHERE {}", conditions.sql));
            }
            params.extend(conditions.params);
            sql.push_str(&format!(" ORDER BY {alias}.NUMBER {dir} LIMIT 1"));
            Ok(fetch_and_map(&conn, &sql, params)?.into_iter().next())
        } else {
            // a seek by release date is impossible (null and non-unique values), so the whole
            // list is pulled and the sibling is located in memory, as in the Kotlin code
            let mut sql = String::from(
                "SELECT BOOK.ID FROM BOOK \
                 LEFT JOIN READLIST_BOOK ON (BOOK.ID = READLIST_BOOK.BOOK_ID) \
                 LEFT JOIN BOOK_METADATA ON (BOOK.ID = BOOK_METADATA.BOOK_ID)",
            );
            if restrictions.is_restricted() {
                sql.push_str(
                    " LEFT JOIN SERIES_METADATA ON (BOOK.SERIES_ID = SERIES_METADATA.SERIES_ID)",
                );
            }
            let mut conditions = SqlWhere {
                sql: "READLIST_BOOK.READLIST_ID = ?".to_string(),
                params: vec![Value::Text(readlist.id.clone())],
                joins: BTreeSet::new(),
            };
            if restrictions.is_restricted() {
                conditions = conditions.and(content_restrictions_condition(restrictions));
            }
            if let Some(ids) = &library_vec {
                conditions = conditions.and(id_in_or_no_condition("BOOK.LIBRARY_ID", Some(ids)));
            }
            sql.push_str(&format!(
                " WHERE {} ORDER BY BOOK_METADATA.RELEASE_DATE",
                conditions.sql
            ));
            let mut stmt = conn.prepare(&sql)?;
            let book_ids: Vec<String> = stmt
                .query_map(params_from_iter(conditions.params), |r| r.get(0))?
                .collect::<std::result::Result<_, _>>()?;
            let Some(index) = book_ids.iter().position(|id| id == book_id) else {
                return Ok(None);
            };
            let sibling = if next {
                book_ids.get(index + 1)
            } else {
                index.checked_sub(1).and_then(|i| book_ids.get(i))
            };
            let Some(sibling_id) = sibling else {
                return Ok(None);
            };

            let (from, mut params) = select_from(user_id, &BTreeSet::new());
            let mut conditions = SqlWhere {
                sql: "BOOK.ID = ?".to_string(),
                params: vec![Value::Text(sibling_id.clone())],
                joins: BTreeSet::new(),
            };
            if let Some(ids) = &library_vec {
                conditions = conditions.and(id_in_or_no_condition("BOOK.LIBRARY_ID", Some(ids)));
            }
            let sql = format!("{SELECT_CLAUSE} {from} WHERE {} LIMIT 1", conditions.sql);
            params.extend(conditions.params);
            Ok(fetch_and_map(&conn, &sql, params)?.into_iter().next())
        }
    }

    /// On Deck: the first unread book of each series that has at least one book read and none
    /// in progress (`BookCommonDao.getBooksOnDeckQuery`).
    pub fn find_all_on_deck(
        &self,
        user_id: &str,
        filter_library_ids: Option<&BTreeSet<String>>,
        restrictions: &ContentRestrictions,
        page: &PageRequest,
    ) -> Result<DtoPage<BookDto>> {
        let conn = self.db.ro();
        let library_vec: Option<Vec<String>> =
            filter_library_ids.map(|ids| ids.iter().cloned().collect());
        let mut cte_conditions = SqlWhere {
            sql: "READ_PROGRESS_SERIES.IN_PROGRESS_COUNT = 0 \
                  AND READ_PROGRESS_SERIES.READ_COUNT <> SERIES.BOOK_COUNT"
                .to_string(),
            params: vec![],
            joins: BTreeSet::new(),
        };
        // Kotlin ANDs the restriction condition unconditionally; it is noCondition when unrestricted
        cte_conditions = cte_conditions.and(content_restrictions_condition(restrictions));
        if let Some(ids) = &library_vec {
            cte_conditions =
                cte_conditions.and(id_in_or_no_condition("SERIES.LIBRARY_ID", Some(ids)));
        }
        let query = format!(
            "WITH cte_series AS MATERIALIZED ( \
               SELECT SERIES.ID, READ_PROGRESS_SERIES.MOST_RECENT_READ_DATE FROM SERIES \
               INNER JOIN READ_PROGRESS_SERIES \
                 ON (SERIES.ID = READ_PROGRESS_SERIES.SERIES_ID AND READ_PROGRESS_SERIES.USER_ID = ?) \
               INNER JOIN SERIES_METADATA ON (SERIES.ID = SERIES_METADATA.SERIES_ID) \
               WHERE {} \
             ), \
             cte_books AS MATERIALIZED ( \
               SELECT BOOK.ID AS cte_books_book_id, BOOK.SERIES_ID AS cte_books_series_id, \
                      BOOK_METADATA.NUMBER_SORT AS cte_books_number_sort, \
                      ROW_NUMBER() OVER ( \
                        PARTITION BY BOOK.SERIES_ID \
                        ORDER BY BOOK_METADATA.NUMBER_SORT, BOOK.ID \
                      ) AS cte_books_rn \
               FROM BOOK \
               INNER JOIN BOOK_METADATA ON (BOOK.ID = BOOK_METADATA.BOOK_ID) \
               LEFT JOIN READ_PROGRESS \
                 ON (BOOK.ID = READ_PROGRESS.BOOK_ID AND READ_PROGRESS.USER_ID = ?) \
               WHERE READ_PROGRESS.COMPLETED IS NULL \
                 AND BOOK.SERIES_ID IN (SELECT ID FROM cte_series) \
             ) \
             {SELECT_CLAUSE}, COUNT(*) OVER () AS total_count FROM cte_series \
             INNER JOIN cte_books AS b1 ON (cte_series.ID = b1.cte_books_series_id) \
             INNER JOIN BOOK ON (b1.cte_books_book_id = BOOK.ID) \
             INNER JOIN MEDIA ON (BOOK.ID = MEDIA.BOOK_ID) \
             INNER JOIN BOOK_METADATA ON (BOOK.ID = BOOK_METADATA.BOOK_ID) \
             INNER JOIN SERIES_METADATA ON (BOOK.SERIES_ID = SERIES_METADATA.SERIES_ID) \
             LEFT OUTER JOIN READ_PROGRESS ON (1 = 0) \
             WHERE b1.cte_books_rn = 1",
            cte_conditions.sql
        );
        let mut params = vec![Value::Text(user_id.to_string())];
        params.extend(cte_conditions.params);
        params.push(Value::Text(user_id.to_string()));

        let mut page_params = params.clone();
        let mut sql = query.clone();
        sql.push_str(" ORDER BY cte_series.MOST_RECENT_READ_DATE DESC");
        if !page.unpaged {
            sql.push_str(" LIMIT ? OFFSET ?");
            page_params.push(Value::Integer(page.size as i64));
            page_params.push(Value::Integer(page.offset() as i64));
        }
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(&page_params), |row| {
            Ok((row_to_dto(row)?, row.get::<_, i64>("total_count")?))
        })?;
        let mut total: i64 = 0;
        let mut items = Vec::new();
        for row in rows {
            let (dto, count) = row?;
            total = count;
            items.push(dto);
        }
        fill_children(&conn, &mut items)?;
        if items.is_empty() && page.offset() > 0 {
            // an out-of-range page returns no row to read the windowed total from
            total = conn.query_row(
                &format!("SELECT COUNT(*) FROM ({query}) AS \"count\""),
                params_from_iter(&params),
                |r| r.get(0),
            )?;
        }
        // the Kotlin PageImpl is built with Sort.unsorted() for on-deck
        Ok(DtoPage {
            items,
            total,
            sorted: false,
        })
    }

    pub fn find_all_duplicates(
        &self,
        user_id: &str,
        page: &PageRequest,
    ) -> Result<DtoPage<BookDto>> {
        let conn = self.db.ro();
        let mut stmt = conn.prepare(
            "SELECT FILE_HASH, COUNT(ID) FROM BOOK WHERE FILE_HASH <> '' \
             GROUP BY FILE_HASH, FILE_SIZE HAVING COUNT(ID) > 1",
        )?;
        let hashes: Vec<(String, i64)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<_, _>>()?;
        let total: i64 = hashes.iter().map(|(_, c)| c).sum();
        let hash_list: Vec<String> = hashes.into_iter().map(|(h, _)| h).collect();

        let mut orders = vec![];
        for o in &page.sort {
            if let Some(expr) = sort_expr(&o.property) {
                orders.push(format!("{expr} {}", dir(o.descending)));
            }
        }
        let (from, mut params) = select_from(user_id, &BTreeSet::new());
        let hash_condition = id_in_or_no_condition("BOOK.FILE_HASH", Some(&hash_list));
        let mut sql = format!("{SELECT_CLAUSE} {from} WHERE {}", hash_condition.sql);
        params.extend(hash_condition.params);
        if !orders.is_empty() {
            sql.push_str(&format!(" ORDER BY {}", orders.join(", ")));
        }
        if !page.unpaged {
            sql.push_str(" LIMIT ? OFFSET ?");
            params.push(Value::Integer(page.size as i64));
            params.push(Value::Integer(page.offset() as i64));
        }
        let items = fetch_and_map(&conn, &sql, params)?;
        Ok(DtoPage {
            items,
            total,
            sorted: !orders.is_empty(),
        })
    }
}

fn dir(descending: bool) -> &'static str {
    if descending {
        "DESC"
    } else {
        "ASC"
    }
}

/// `sorts` from `BookDtoDao.kt`; unknown properties are dropped from the ORDER BY
fn sort_expr(property: &str) -> Option<&'static str> {
    Some(match property {
        "name" => "BOOK.NAME COLLATE COLLATION_UNICODE_3",
        "series" => "SERIES_METADATA.TITLE_SORT COLLATE COLLATION_UNICODE_3",
        "created" | "createdDate" => "BOOK.CREATED_DATE",
        "lastModified" | "lastModifiedDate" => "BOOK.LAST_MODIFIED_DATE",
        "fileSize" | "size" => "BOOK.FILE_SIZE",
        "fileHash" => "BOOK.FILE_HASH",
        "url" => "BOOK.URL COLLATE NOCASE",
        "media.status" => "MEDIA.STATUS COLLATE NOCASE",
        "media.comment" => "MEDIA.COMMENT COLLATE NOCASE",
        "media.mediaType" => "MEDIA.MEDIA_TYPE COLLATE NOCASE",
        "media.pagesCount" => "MEDIA.PAGE_COUNT",
        "metadata.title" => "BOOK_METADATA.TITLE COLLATE COLLATION_UNICODE_3",
        "metadata.numberSort" => "BOOK_METADATA.NUMBER_SORT",
        "metadata.releaseDate" => "BOOK_METADATA.RELEASE_DATE",
        "readProgress.lastModified" => "READ_PROGRESS.LAST_MODIFIED_DATE",
        "readProgress.readDate" => "READ_PROGRESS.READ_DATE",
        _ => return None,
    })
}

fn build_orders(
    page: &PageRequest,
    joins: &BTreeSet<RequiredJoin>,
    lucene_ids: Option<&[String]>,
) -> (Vec<String>, Vec<Value>) {
    let mut orders = vec![];
    let mut params = vec![];
    for o in &page.sort {
        if o.property == "relevance" {
            // only meaningful with lucene hits; otherwise dropped like the Kotlin mapNotNull
            if let Some(ids) = lucene_ids {
                if !ids.is_empty() {
                    let (case, case_params) = sort_by_values("BOOK.ID", ids, !o.descending);
                    orders.push(case);
                    params.extend(case_params);
                }
            }
            continue;
        }
        if o.property == "readList.number" {
            // Kotlin uses the first read-list join; without one the order is dropped
            if let Some(id) = joins.iter().find_map(|j| match j {
                RequiredJoin::ReadList(id) => Some(id),
                _ => None,
            }) {
                orders.push(format!(
                    "{}.NUMBER {}",
                    readlist_alias(id),
                    dir(o.descending)
                ));
            }
            continue;
        }
        if let Some(expr) = sort_expr(&o.property) {
            orders.push(format!("{expr} {}", dir(o.descending)));
        }
    }
    (orders, params)
}

/// The shared FROM/JOIN skeleton (`selectBase`). Bind parameters come in SQL text order:
/// the read-progress user id first, then the read-list join ids.
fn select_from(user_id: &str, joins: &BTreeSet<RequiredJoin>) -> (String, Vec<Value>) {
    let mut sql = String::from(
        "FROM BOOK \
         LEFT JOIN MEDIA ON (BOOK.ID = MEDIA.BOOK_ID) \
         LEFT JOIN BOOK_METADATA ON (BOOK.ID = BOOK_METADATA.BOOK_ID) \
         LEFT JOIN READ_PROGRESS ON (BOOK.ID = READ_PROGRESS.BOOK_ID AND READ_PROGRESS.USER_ID = ?) \
         LEFT JOIN SERIES_METADATA ON (BOOK.SERIES_ID = SERIES_METADATA.SERIES_ID)",
    );
    let mut params = vec![Value::Text(user_id.to_string())];
    for join in joins {
        if let RequiredJoin::ReadList(id) = join {
            let alias = readlist_alias(id);
            sql.push_str(&format!(
                " LEFT JOIN READLIST_BOOK AS \"{alias}\" \
                 ON ({alias}.BOOK_ID = BOOK.ID AND {alias}.READLIST_ID = ?)"
            ));
            params.push(Value::Text(id.clone()));
        }
    }
    (sql, params)
}

/// jOOQ `fetchCount` over the grouped id subquery
fn count(conn: &Connection, conditions: &SqlWhere, user_id: &str) -> Result<i64> {
    let (from, mut params) = select_from(user_id, &conditions.joins);
    let mut sql = format!("SELECT COUNT(*) FROM (SELECT BOOK.ID {from}");
    if !conditions.sql.is_empty() {
        sql.push_str(&format!(" WHERE {}", conditions.sql));
    }
    sql.push_str(" GROUP BY BOOK.ID) AS \"count\"");
    params.extend(conditions.params.iter().cloned());
    let total = conn.query_row(&sql, params_from_iter(params), |r| r.get(0))?;
    Ok(total)
}

fn fetch_and_map(conn: &Connection, sql: &str, params: Vec<Value>) -> Result<Vec<BookDto>> {
    let mut stmt = conn.prepare(sql)?;
    let mut dtos = stmt
        .query_map(params_from_iter(params), row_to_dto)?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    fill_children(conn, &mut dtos)?;
    Ok(dtos)
}

/// Second-pass batch fetch of authors/tags/links for the page's books (`fetchAndMap`)
fn fill_children(conn: &Connection, dtos: &mut [BookDto]) -> Result<()> {
    if dtos.is_empty() {
        return Ok(());
    }
    let ids: Vec<String> = dtos.iter().map(|d| d.id.clone()).collect();
    let mut authors: HashMap<String, Vec<AuthorDto>> = HashMap::new();
    let mut tags: HashMap<String, Vec<String>> = HashMap::new();
    let mut links: HashMap<String, Vec<WebLinkDto>> = HashMap::new();
    for chunk in ids.chunks(500) {
        let ph = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
        let params: Vec<Value> = chunk.iter().map(|i| Value::Text(i.clone())).collect();

        let mut stmt = conn.prepare(&format!(
            "SELECT BOOK_ID, NAME, ROLE FROM BOOK_METADATA_AUTHOR WHERE BOOK_ID IN ({ph})"
        ))?;
        let rows = stmt.query_map(params_from_iter(&params), |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?;
        for row in rows {
            let (book_id, name, role) = row?;
            // Kotlin filters out null names
            if let Some(name) = name {
                authors
                    .entry(book_id)
                    .or_default()
                    .push(AuthorDto { name, role });
            }
        }

        let mut stmt = conn.prepare(&format!(
            "SELECT BOOK_ID, TAG FROM BOOK_METADATA_TAG WHERE BOOK_ID IN ({ph})"
        ))?;
        let rows = stmt.query_map(params_from_iter(&params), |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (book_id, tag) = row?;
            tags.entry(book_id).or_default().push(tag);
        }

        let mut stmt = conn.prepare(&format!(
            "SELECT BOOK_ID, LABEL, URL FROM BOOK_METADATA_LINK WHERE BOOK_ID IN ({ph})"
        ))?;
        let rows = stmt.query_map(params_from_iter(&params), |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?;
        for row in rows {
            let (book_id, label, url) = row?;
            links
                .entry(book_id)
                .or_default()
                .push(WebLinkDto { label, url });
        }
    }
    for dto in dtos.iter_mut() {
        if let Some(a) = authors.remove(&dto.id) {
            dto.metadata.authors = a;
        }
        if let Some(t) = tags.remove(&dto.id) {
            dto.metadata.tags = t.into_iter().collect();
        }
        if let Some(l) = links.remove(&dto.id) {
            dto.metadata.links = l;
        }
    }
    Ok(())
}

fn row_to_dto(row: &Row<'_>) -> rusqlite::Result<BookDto> {
    let media_type: Option<String> = row.get(M_MEDIA_TYPE)?;
    let media_type = media_type.unwrap_or_default();
    let comment: Option<String> = row.get(M_COMMENT)?;
    let read_progress = match row.get::<_, Option<String>>(R_USER_ID)? {
        Some(_) => Some(ReadProgressDto {
            page: row.get(R_PAGE)?,
            completed: row.get(R_COMPLETED)?,
            read_date: get_datetime(row, R_READ_DATE)?,
            created: get_datetime(row, R_CREATED_DATE)?,
            last_modified: get_datetime(row, R_LAST_MODIFIED_DATE)?,
            device_id: row.get(R_DEVICE_ID)?,
            device_name: row.get(R_DEVICE_NAME)?,
        }),
        None => None,
    };
    let series_title: Option<String> = row.get(SD_TITLE)?;
    let url: String = row.get(B_URL)?;
    let file_size: i64 = row.get(B_FILE_SIZE)?;
    let deleted_date = get_datetime_opt(row, B_DELETED_DATE)?;
    Ok(BookDto {
        id: row.get(0)?,
        series_id: row.get(B_SERIES_ID)?,
        // jOOQ feeds the (nullable) join column into a non-null DTO field and would NPE;
        // an empty string keeps the JSON contract instead
        series_title: series_title.unwrap_or_default(),
        library_id: row.get(B_LIBRARY_ID)?,
        name: row.get(B_NAME)?,
        url: url_to_file_path(&url),
        number: row.get(B_NUMBER)?,
        created: get_datetime(row, B_CREATED_DATE)?,
        last_modified: get_datetime(row, B_LAST_MODIFIED_DATE)?,
        file_last_modified: get_datetime(row, B_FILE_LAST_MODIFIED)?,
        size_bytes: file_size,
        size: BookDto::size_of(file_size),
        media: MediaDto {
            status: row.get(M_STATUS)?,
            media_profile: MediaDto::media_profile_of(&media_type),
            media_type,
            pages_count: row.get(M_PAGE_COUNT)?,
            comment: comment.unwrap_or_default(),
            epub_divina_compatible: row.get(M_EPUB_DIVINA_COMPATIBLE)?,
            epub_is_kepub: row.get(M_EPUB_IS_KEPUB)?,
        },
        metadata: BookMetadataDto {
            title: row.get(D_TITLE)?,
            title_lock: row.get(D_TITLE_LOCK)?,
            summary: row.get(D_SUMMARY)?,
            summary_lock: row.get(D_SUMMARY_LOCK)?,
            number: row.get(D_NUMBER)?,
            number_lock: row.get(D_NUMBER_LOCK)?,
            number_sort: row.get(D_NUMBER_SORT)?,
            number_sort_lock: row.get(D_NUMBER_SORT_LOCK)?,
            release_date: get_date(row, D_RELEASE_DATE)?,
            release_date_lock: row.get(D_RELEASE_DATE_LOCK)?,
            authors: vec![],
            authors_lock: row.get(D_AUTHORS_LOCK)?,
            tags: BTreeSet::new(),
            tags_lock: row.get(D_TAGS_LOCK)?,
            isbn: row.get(D_ISBN)?,
            isbn_lock: row.get(D_ISBN_LOCK)?,
            links: vec![],
            links_lock: row.get(D_LINKS_LOCK)?,
            created: get_datetime(row, D_CREATED_DATE)?,
            last_modified: get_datetime(row, D_LAST_MODIFIED_DATE)?,
        },
        read_progress,
        deleted: deleted_date.is_some(),
        file_hash: row.get(B_FILE_HASH)?,
        oneshot: row.get(B_ONESHOT)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto_dao::SortOrder;
    use crate::migrate::Migrator;
    use crate::{main_migrations, Placeholders};
    use komga_core::model::user::{AgeRestriction, AllowExclude};
    use komga_core::search::*;
    use rusqlite::params;
    use time::{Date, Month};

    fn db() -> Database {
        let db = Database::open_in_memory(true).unwrap();
        let migrations = main_migrations();
        Migrator::new(&migrations, Placeholders::default())
            .migrate(&db.rw())
            .unwrap();
        db
    }

    fn insert_library(conn: &Connection, id: &str) {
        conn.execute(
            "INSERT INTO LIBRARY (ID, NAME, ROOT) VALUES (?, ?, ?)",
            params![id, format!("lib-{id}"), format!("file:/{id}/")],
        )
        .unwrap();
    }

    fn insert_series(
        conn: &Connection,
        id: &str,
        library_id: &str,
        book_count: i32,
        oneshot: bool,
    ) {
        conn.execute(
            "INSERT INTO SERIES (ID, NAME, URL, FILE_LAST_MODIFIED, LIBRARY_ID, BOOK_COUNT, ONESHOT) \
             VALUES (?, ?, ?, '2020-01-01 00:00:00.0', ?, ?, ?)",
            params![id, format!("series-{id}"), format!("file:/lib/{id}/"), library_id, book_count, oneshot],
        )
        .unwrap();
    }

    fn insert_series_metadata(
        conn: &Connection,
        series_id: &str,
        title_sort: &str,
        publisher: &str,
        age_rating: Option<i32>,
    ) {
        conn.execute(
            "INSERT INTO SERIES_METADATA (SERIES_ID, STATUS, TITLE, TITLE_SORT, PUBLISHER, AGE_RATING) \
             VALUES (?, 'ONGOING', ?, ?, ?, ?)",
            params![series_id, format!("title-{series_id}"), title_sort, publisher, age_rating],
        )
        .unwrap();
    }

    #[allow(clippy::too_many_arguments)]
    fn insert_book(
        conn: &Connection,
        id: &str,
        series_id: &str,
        library_id: &str,
        file_size: i64,
        file_hash: &str,
        deleted: bool,
        oneshot: bool,
    ) {
        conn.execute(
            "INSERT INTO BOOK (ID, NAME, URL, FILE_LAST_MODIFIED, SERIES_ID, LIBRARY_ID, FILE_SIZE, \
             FILE_HASH, DELETED_DATE, ONESHOT) \
             VALUES (?, ?, ?, '2020-01-01 00:00:00.0', ?, ?, ?, ?, ?, ?)",
            params![
                id,
                format!("book-{id}"),
                format!("file:/lib/{id}.cbz"),
                series_id,
                library_id,
                file_size,
                file_hash,
                if deleted { Some("2021-01-01 00:00:00.0") } else { None },
                oneshot,
            ],
        )
        .unwrap();
    }

    fn insert_media(
        conn: &Connection,
        book_id: &str,
        status: &str,
        media_type: &str,
        page_count: i32,
    ) {
        conn.execute(
            "INSERT INTO MEDIA (BOOK_ID, STATUS, MEDIA_TYPE, PAGE_COUNT) VALUES (?, ?, ?, ?)",
            params![book_id, status, media_type, page_count],
        )
        .unwrap();
    }

    fn insert_book_metadata(
        conn: &Connection,
        book_id: &str,
        title: &str,
        number_sort: f32,
        release_date: Option<&str>,
    ) {
        conn.execute(
            "INSERT INTO BOOK_METADATA (BOOK_ID, TITLE, NUMBER, NUMBER_SORT, RELEASE_DATE) \
             VALUES (?, ?, ?, ?, ?)",
            params![
                book_id,
                title,
                number_sort.to_string(),
                number_sort,
                release_date
            ],
        )
        .unwrap();
    }

    fn insert_read_progress(
        conn: &Connection,
        book_id: &str,
        user_id: &str,
        page: i32,
        completed: bool,
    ) {
        conn.execute(
            "INSERT INTO READ_PROGRESS (BOOK_ID, USER_ID, PAGE, COMPLETED) VALUES (?, ?, ?, ?)",
            params![book_id, user_id, page, completed],
        )
        .unwrap();
    }

    fn insert_read_progress_series(
        conn: &Connection,
        series_id: &str,
        user_id: &str,
        read: i32,
        in_progress: i32,
        most_recent: Option<&str>,
    ) {
        conn.execute(
            "INSERT INTO READ_PROGRESS_SERIES (SERIES_ID, USER_ID, READ_COUNT, IN_PROGRESS_COUNT, \
             MOST_RECENT_READ_DATE) VALUES (?, ?, ?, ?, ?)",
            params![series_id, user_id, read, in_progress, most_recent],
        )
        .unwrap();
    }

    /// Base dataset: l1/s1(b1,b2,b3) + l1/s2(b4,b5) + l2/s3(b6 oneshot+deleted)
    fn base_db() -> Database {
        let db = db();
        let conn = db.rw();
        insert_library(&conn, "l1");
        insert_library(&conn, "l2");
        conn.execute(
            "INSERT INTO USER (ID, EMAIL, PASSWORD) VALUES ('u1', 'u1@example.org', 'x')",
            [],
        )
        .unwrap();
        for (id, lib, count, oneshot) in [
            ("s1", "l1", 3, false),
            ("s2", "l1", 2, false),
            ("s3", "l2", 1, true),
        ] {
            insert_series(&conn, id, lib, count, oneshot);
        }
        insert_series_metadata(&conn, "s1", "Alpha", "P1", None);
        insert_series_metadata(&conn, "s2", "beta", "p2", Some(18));
        insert_series_metadata(&conn, "s3", "Gamma", "P1", None);
        conn.execute(
            "INSERT INTO SERIES_METADATA_SHARING (SERIES_ID, LABEL) VALUES ('s2', 'kids')",
            [],
        )
        .unwrap();

        insert_book(&conn, "b1", "s1", "l1", 100, "h1", false, false);
        insert_book(&conn, "b2", "s1", "l1", 100, "h1", false, false);
        insert_book(&conn, "b3", "s1", "l1", 200, "h2", false, false);
        insert_book(&conn, "b4", "s2", "l1", 999, "h1", false, false);
        insert_book(&conn, "b5", "s2", "l1", 300, "", false, false);
        insert_book(&conn, "b6", "s3", "l2", 300, "h3", true, true);

        insert_media(&conn, "b1", "READY", "application/zip", 10);
        insert_media(&conn, "b2", "ERROR", "application/zip", 0);
        insert_media(&conn, "b3", "READY", "application/pdf", 5);
        insert_media(&conn, "b4", "READY", "application/epub+zip", 7);
        insert_media(&conn, "b5", "UNKNOWN", "application/zip", 0);
        insert_media(&conn, "b6", "READY", "application/zip", 12);

        insert_book_metadata(&conn, "b1", "Book One", 1.0, Some("2020-01-01"));
        insert_book_metadata(&conn, "b2", "Book Two", 2.0, None);
        insert_book_metadata(&conn, "b3", "Book Three", 3.0, Some("2019-01-01"));
        insert_book_metadata(&conn, "b4", "Book Four", 1.0, None);
        insert_book_metadata(&conn, "b5", "Book Five", 2.0, None);
        insert_book_metadata(&conn, "b6", "Book Six", 1.0, None);

        conn.execute(
            "INSERT INTO BOOK_METADATA_AUTHOR (BOOK_ID, NAME, ROLE) VALUES ('b1', 'Miura', 'writer')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO BOOK_METADATA_TAG (BOOK_ID, TAG) VALUES ('b1', 'action')",
            [],
        )
        .unwrap();

        insert_read_progress(&conn, "b1", "u1", 10, true);
        insert_read_progress(&conn, "b2", "u1", 3, false);
        insert_read_progress(&conn, "b4", "u1", 7, true);
        insert_read_progress_series(&conn, "s1", "u1", 1, 1, Some("2021-01-01 00:00:00.0"));
        insert_read_progress_series(&conn, "s2", "u1", 1, 0, Some("2021-06-01 00:00:00.0"));

        conn.execute(
            "INSERT INTO READLIST (ID, NAME, BOOK_COUNT, ORDERED) VALUES ('rl1', 'rl1', 3, 1)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO READLIST_BOOK (READLIST_ID, BOOK_ID, NUMBER) \
             VALUES ('rl1', 'b1', 0), ('rl1', 'b3', 1), ('rl1', 'b5', 2)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO READLIST (ID, NAME, BOOK_COUNT, ORDERED) VALUES ('rl2', 'rl2', 3, 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO READLIST_BOOK (READLIST_ID, BOOK_ID, NUMBER) \
             VALUES ('rl2', 'b1', 0), ('rl2', 'b3', 1), ('rl2', 'b5', 2)",
            [],
        )
        .unwrap();
        db
    }

    fn ctx_user() -> SearchContext {
        SearchContext {
            user_id: Some("u1".to_string()),
            restrictions: ContentRestrictions::default(),
            library_ids: None,
        }
    }

    fn dao(db: &Database) -> BookDtoDao {
        BookDtoDao::new(db.clone())
    }

    fn paged(page: u32, size: u32) -> PageRequest {
        PageRequest {
            page,
            size,
            unpaged: false,
            sort: vec![],
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

    fn search(condition: Option<SearchConditionBook>) -> BookSearch {
        BookSearch {
            condition,
            full_text_search: None,
        }
    }

    fn is(value: &str) -> Equality<String> {
        Equality::Is {
            value: value.to_string(),
        }
    }

    fn is_not(value: &str) -> Equality<String> {
        Equality::IsNot {
            value: value.to_string(),
        }
    }

    fn ids(page: &DtoPage<BookDto>) -> Vec<String> {
        page.items.iter().map(|b| b.id.clone()).collect()
    }

    fn id_set(page: &DtoPage<BookDto>) -> BTreeSet<String> {
        ids(page).into_iter().collect()
    }

    fn set(ids: &[&str]) -> BTreeSet<String> {
        ids.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn find_all_unpaged_returns_everything_including_deleted() {
        let db = base_db();
        let page = dao(&db)
            .find_all(&search(None), &ctx_user(), &unpaged())
            .unwrap();
        assert_eq!(page.total, 6);
        assert_eq!(page.items.len(), 6);
        assert!(!page.sorted);
    }

    #[test]
    fn find_all_paged_with_sort() {
        let db = base_db();
        let mut page = paged(0, 2);
        page.sort = vec![SortOrder {
            property: "name".to_string(),
            descending: false,
        }];
        let result = dao(&db)
            .find_all(&search(None), &ctx_user(), &page)
            .unwrap();
        assert_eq!(result.total, 6);
        assert!(result.sorted);
        assert_eq!(ids(&result), ["b1", "b2"]);

        let mut page = paged(2, 2);
        page.sort = vec![SortOrder {
            property: "name".to_string(),
            descending: false,
        }];
        let result = dao(&db)
            .find_all(&search(None), &ctx_user(), &page)
            .unwrap();
        assert_eq!(ids(&result), ["b5", "b6"]);
    }

    #[test]
    fn find_all_unknown_sort_property_is_dropped() {
        let db = base_db();
        let mut page = unpaged();
        page.sort = vec![SortOrder {
            property: "bogus".to_string(),
            descending: false,
        }];
        let result = dao(&db)
            .find_all(&search(None), &ctx_user(), &page)
            .unwrap();
        assert_eq!(result.items.len(), 6);
        assert!(!result.sorted);
    }

    #[test]
    fn find_all_filter_library_id() {
        let db = base_db();
        let page = dao(&db)
            .find_all(
                &search(Some(SearchConditionBook::LibraryId { operator: is("l1") })),
                &ctx_user(),
                &unpaged(),
            )
            .unwrap();
        assert_eq!(id_set(&page), set(&["b1", "b2", "b3", "b4", "b5"]));

        let mut ctx = ctx_user();
        ctx.library_ids = Some(set(&["l2"]));
        let page = dao(&db).find_all(&search(None), &ctx, &unpaged()).unwrap();
        assert_eq!(id_set(&page), set(&["b6"]));
    }

    #[test]
    fn find_all_filter_series_id() {
        let db = base_db();
        let page = dao(&db)
            .find_all(
                &search(Some(SearchConditionBook::SeriesId { operator: is("s1") })),
                &ctx_user(),
                &unpaged(),
            )
            .unwrap();
        assert_eq!(id_set(&page), set(&["b1", "b2", "b3"]));
    }

    #[test]
    fn find_all_filter_media_status() {
        let db = base_db();
        let page = dao(&db)
            .find_all(
                &search(Some(SearchConditionBook::MediaStatus {
                    operator: Equality::Is {
                        value: komga_core::model::media::MediaStatus::Ready,
                    },
                })),
                &ctx_user(),
                &unpaged(),
            )
            .unwrap();
        assert_eq!(id_set(&page), set(&["b1", "b3", "b4", "b6"]));

        let page = dao(&db)
            .find_all(
                &search(Some(SearchConditionBook::MediaStatus {
                    operator: Equality::IsNot {
                        value: komga_core::model::media::MediaStatus::Ready,
                    },
                })),
                &ctx_user(),
                &unpaged(),
            )
            .unwrap();
        assert_eq!(id_set(&page), set(&["b2", "b5"]));
    }

    #[test]
    fn find_all_filter_read_status() {
        let db = base_db();
        let by = |status: ReadStatus, negate: bool| {
            let operator = if negate {
                Equality::IsNot { value: status }
            } else {
                Equality::Is { value: status }
            };
            SearchConditionBook::ReadStatus { operator }
        };
        let d = dao(&db);
        let page = d
            .find_all(
                &search(Some(by(ReadStatus::Read, false))),
                &ctx_user(),
                &unpaged(),
            )
            .unwrap();
        assert_eq!(id_set(&page), set(&["b1", "b4"]));
        let page = d
            .find_all(
                &search(Some(by(ReadStatus::Unread, false))),
                &ctx_user(),
                &unpaged(),
            )
            .unwrap();
        assert_eq!(id_set(&page), set(&["b3", "b5", "b6"]));
        let page = d
            .find_all(
                &search(Some(by(ReadStatus::InProgress, false))),
                &ctx_user(),
                &unpaged(),
            )
            .unwrap();
        assert_eq!(id_set(&page), set(&["b2"]));
        let page = d
            .find_all(
                &search(Some(by(ReadStatus::Unread, true))),
                &ctx_user(),
                &unpaged(),
            )
            .unwrap();
        assert_eq!(id_set(&page), set(&["b1", "b2", "b4"]));
    }

    #[test]
    fn find_all_filter_tag() {
        let db = base_db();
        let page = dao(&db)
            .find_all(
                &search(Some(SearchConditionBook::Tag {
                    tag: EqualityNullable::Is {
                        value: "action".to_string(),
                    },
                })),
                &ctx_user(),
                &unpaged(),
            )
            .unwrap();
        assert_eq!(id_set(&page), set(&["b1"]));

        let page = dao(&db)
            .find_all(
                &search(Some(SearchConditionBook::Tag {
                    tag: EqualityNullable::IsNull,
                })),
                &ctx_user(),
                &unpaged(),
            )
            .unwrap();
        assert_eq!(page.items.len(), 5);
    }

    #[test]
    fn find_all_filter_author() {
        let db = base_db();
        let by = |name: Option<&str>, role: Option<&str>| SearchConditionBook::Author {
            author: Equality::Is {
                value: AuthorMatch {
                    name: name.map(String::from),
                    role: role.map(String::from),
                },
            },
        };
        let d = dao(&db);
        let page = d
            .find_all(
                &search(Some(by(Some("Miura"), None))),
                &ctx_user(),
                &unpaged(),
            )
            .unwrap();
        assert_eq!(id_set(&page), set(&["b1"]));
        let page = d
            .find_all(
                &search(Some(by(Some("Miura"), Some("writer")))),
                &ctx_user(),
                &unpaged(),
            )
            .unwrap();
        assert_eq!(id_set(&page), set(&["b1"]));
        let page = d
            .find_all(
                &search(Some(by(Some("Miura"), Some("editor")))),
                &ctx_user(),
                &unpaged(),
            )
            .unwrap();
        assert!(page.items.is_empty());
    }

    #[test]
    fn find_all_filter_number_sort() {
        let db = base_db();
        let page = dao(&db)
            .find_all(
                &search(Some(SearchConditionBook::NumberSort {
                    operator: Numeric::GreaterThan { value: 1.5 },
                })),
                &ctx_user(),
                &unpaged(),
            )
            .unwrap();
        // greaterThan maps to >= in komga
        assert_eq!(id_set(&page), set(&["b2", "b3", "b5"]));

        let page = dao(&db)
            .find_all(
                &search(Some(SearchConditionBook::NumberSort {
                    operator: Numeric::Is { value: 1.0 },
                })),
                &ctx_user(),
                &unpaged(),
            )
            .unwrap();
        assert_eq!(id_set(&page), set(&["b1", "b4", "b6"]));
    }

    #[test]
    fn find_all_filter_release_date() {
        let db = base_db();
        let dt = time::OffsetDateTime::parse(
            "2019-06-01T00:00:00Z",
            &time::format_description::well_known::Iso8601::DEFAULT,
        )
        .unwrap();
        let page = dao(&db)
            .find_all(
                &search(Some(SearchConditionBook::ReleaseDate {
                    operator: DateOp::After { date_time: dt },
                })),
                &ctx_user(),
                &unpaged(),
            )
            .unwrap();
        assert_eq!(id_set(&page), set(&["b1"]));

        let page = dao(&db)
            .find_all(
                &search(Some(SearchConditionBook::ReleaseDate {
                    operator: DateOp::Before { date_time: dt },
                })),
                &ctx_user(),
                &unpaged(),
            )
            .unwrap();
        assert_eq!(id_set(&page), set(&["b3"]));
    }

    #[test]
    fn find_all_filter_media_profile() {
        let db = base_db();
        let by = |profile: MediaProfile| SearchConditionBook::MediaProfile {
            operator: Equality::Is { value: profile },
        };
        let d = dao(&db);
        let page = d
            .find_all(
                &search(Some(by(MediaProfile::Divina))),
                &ctx_user(),
                &unpaged(),
            )
            .unwrap();
        assert_eq!(id_set(&page), set(&["b1", "b2", "b5", "b6"]));
        let page = d
            .find_all(
                &search(Some(by(MediaProfile::Pdf))),
                &ctx_user(),
                &unpaged(),
            )
            .unwrap();
        assert_eq!(id_set(&page), set(&["b3"]));
        let page = d
            .find_all(
                &search(Some(by(MediaProfile::Epub))),
                &ctx_user(),
                &unpaged(),
            )
            .unwrap();
        assert_eq!(id_set(&page), set(&["b4"]));
    }

    #[test]
    fn find_all_filter_oneshot_and_deleted() {
        let db = base_db();
        let d = dao(&db);
        let page = d
            .find_all(
                &search(Some(SearchConditionBook::OneShot {
                    operator: BooleanOp::IsTrue,
                })),
                &ctx_user(),
                &unpaged(),
            )
            .unwrap();
        assert_eq!(id_set(&page), set(&["b6"]));

        let page = d
            .find_all(
                &search(Some(SearchConditionBook::Deleted {
                    deleted: BooleanOp::IsTrue,
                })),
                &ctx_user(),
                &unpaged(),
            )
            .unwrap();
        assert_eq!(id_set(&page), set(&["b6"]));

        let page = d
            .find_all(
                &search(Some(SearchConditionBook::Deleted {
                    deleted: BooleanOp::IsFalse,
                })),
                &ctx_user(),
                &unpaged(),
            )
            .unwrap();
        assert_eq!(page.items.len(), 5);
    }

    #[test]
    fn find_all_filter_poster() {
        let db = base_db();
        db.rw()
            .execute(
                "INSERT INTO THUMBNAIL_BOOK (ID, BOOK_ID, TYPE, SELECTED) VALUES ('t1', 'b3', 'GENERATED', 1)",
                [],
            )
            .unwrap();
        let d = dao(&db);
        let page = d
            .find_all(
                &search(Some(SearchConditionBook::Poster {
                    poster: Equality::Is {
                        value: PosterMatch {
                            type_: None,
                            selected: Some(true),
                        },
                    },
                })),
                &ctx_user(),
                &unpaged(),
            )
            .unwrap();
        assert_eq!(id_set(&page), set(&["b3"]));

        let page = d
            .find_all(
                &search(Some(SearchConditionBook::Poster {
                    poster: Equality::IsNot {
                        value: PosterMatch {
                            type_: None,
                            selected: Some(true),
                        },
                    },
                })),
                &ctx_user(),
                &unpaged(),
            )
            .unwrap();
        assert_eq!(page.items.len(), 5);
    }

    #[test]
    fn find_all_sort_number_sort_then_name() {
        let db = base_db();
        let mut page = unpaged();
        page.sort = vec![
            SortOrder {
                property: "metadata.numberSort".to_string(),
                descending: false,
            },
            SortOrder {
                property: "name".to_string(),
                descending: false,
            },
        ];
        let result = dao(&db)
            .find_all(&search(None), &ctx_user(), &page)
            .unwrap();
        assert_eq!(ids(&result), ["b1", "b4", "b6", "b2", "b5", "b3"]);
    }

    #[test]
    fn find_all_sort_series_title() {
        let db = base_db();
        let mut page = unpaged();
        page.sort = vec![SortOrder {
            property: "series".to_string(),
            descending: false,
        }];
        let result = dao(&db)
            .find_all(&search(None), &ctx_user(), &page)
            .unwrap();
        let result_ids = ids(&result);
        // ICU tertiary: Alpha < beta < Gamma
        assert_eq!(&result_ids[0..3], &["b1", "b2", "b3"]);
        assert_eq!(&result_ids[3..5], &["b4", "b5"]);
        assert_eq!(result_ids[5], "b6");
    }

    #[test]
    fn find_all_with_content_restrictions() {
        let db = base_db();
        let restricted = |labels: &[&str]| {
            let mut ctx = ctx_user();
            ctx.restrictions = ContentRestrictions::new(
                None,
                labels.iter().map(|s| s.to_string()).collect(),
                BTreeSet::new(),
            );
            ctx
        };
        let page = dao(&db)
            .find_all(&search(None), &restricted(&["kids"]), &unpaged())
            .unwrap();
        assert_eq!(id_set(&page), set(&["b4", "b5"]));

        let mut ctx = ctx_user();
        ctx.restrictions = ContentRestrictions::new(
            None,
            BTreeSet::new(),
            ["kids".to_string()].into_iter().collect(),
        );
        let page = dao(&db).find_all(&search(None), &ctx, &unpaged()).unwrap();
        assert_eq!(id_set(&page), set(&["b1", "b2", "b3", "b6"]));

        let mut ctx = ctx_user();
        ctx.restrictions = ContentRestrictions::new(
            Some(AgeRestriction {
                age: 16,
                restriction: AllowExclude::AllowOnly,
            }),
            BTreeSet::new(),
            BTreeSet::new(),
        );
        let page = dao(&db).find_all(&search(None), &ctx, &unpaged()).unwrap();
        assert!(page.items.is_empty());

        let mut ctx = ctx_user();
        ctx.restrictions = ContentRestrictions::new(
            Some(AgeRestriction {
                age: 18,
                restriction: AllowExclude::Exclude,
            }),
            BTreeSet::new(),
            BTreeSet::new(),
        );
        let page = dao(&db).find_all(&search(None), &ctx, &unpaged()).unwrap();
        assert_eq!(id_set(&page), set(&["b1", "b2", "b3", "b6"]));
    }

    #[test]
    fn find_all_full_text_stub_returns_empty() {
        let db = base_db();
        let mut s = search(None);
        s.full_text_search = Some("anything".to_string());
        let page = dao(&db).find_all(&s, &ctx_user(), &unpaged()).unwrap();
        assert_eq!(page.total, 0);
        assert!(page.items.is_empty());
    }

    struct StubSearcher(Option<Vec<String>>);

    impl EntitySearcher for StubSearcher {
        fn search_entity_ids(
            &self,
            _term: Option<&str>,
            entity: LuceneEntity,
        ) -> Option<Vec<String>> {
            match entity {
                LuceneEntity::Book => self.0.clone(),
                _ => None,
            }
        }
    }

    #[test]
    fn find_all_full_text_filters_by_searcher_ids() {
        let db = base_db();
        let dao_with =
            |ids: Vec<String>| dao(&db).with_searcher(Some(Arc::new(StubSearcher(Some(ids)))));
        let mut s = search(None);
        s.full_text_search = Some("anything".to_string());
        let page = dao_with(vec!["b3".to_string(), "b1".to_string()])
            .find_all(&s, &ctx_user(), &unpaged())
            .unwrap();
        assert_eq!(page.total, 2);
        assert_eq!(id_set(&page), set(&["b1", "b3"]));

        // searcher returning nothing yields an empty page
        let page = dao_with(vec![])
            .find_all(&s, &ctx_user(), &unpaged())
            .unwrap();
        assert_eq!(page.total, 0);

        // relevance order follows the searcher's id order
        let page = dao_with(vec!["b3".to_string(), "b1".to_string()])
            .find_all(
                &s,
                &ctx_user(),
                &PageRequest {
                    page: 0,
                    size: 20,
                    unpaged: false,
                    sort: vec![crate::dto_dao::SortOrder {
                        property: "relevance".to_string(),
                        descending: false,
                    }],
                },
            )
            .unwrap();
        assert_eq!(ids(&page), vec!["b3".to_string(), "b1".to_string()]);
    }

    #[test]
    fn find_by_id_maps_all_fields() {
        let db = base_db();
        let book = dao(&db)
            .find_by_id("b1", "u1")
            .unwrap()
            .expect("book not found");
        assert_eq!(book.series_id, "s1");
        assert_eq!(book.series_title, "title-s1");
        assert_eq!(book.library_id, "l1");
        assert_eq!(book.name, "book-b1");
        assert_eq!(book.url, "/lib/b1.cbz");
        assert_eq!(book.size_bytes, 100);
        assert_eq!(book.size, "100 B");
        assert!(!book.deleted);
        assert!(!book.oneshot);
        assert_eq!(book.file_hash, "h1");
        assert_eq!(book.media.status, "READY");
        assert_eq!(book.media.media_type, "application/zip");
        assert_eq!(book.media.media_profile, "DIVINA");
        assert_eq!(book.media.pages_count, 10);
        assert_eq!(book.metadata.title, "Book One");
        assert_eq!(book.metadata.number_sort, 1.0);
        assert_eq!(
            book.metadata.release_date,
            Some(Date::from_calendar_date(2020, Month::January, 1).unwrap())
        );
        assert_eq!(
            book.metadata.authors,
            vec![AuthorDto {
                name: "Miura".to_string(),
                role: "writer".to_string(),
            }]
        );
        assert_eq!(
            book.metadata.tags,
            ["action".to_string()].into_iter().collect::<BTreeSet<_>>()
        );
        let progress = book.read_progress.expect("read progress missing");
        assert!(progress.completed);
        assert_eq!(progress.page, 10);

        let book = dao(&db).find_by_id("b3", "u1").unwrap().unwrap();
        assert!(book.read_progress.is_none());

        assert!(dao(&db).find_by_id("nope", "u1").unwrap().is_none());
    }

    #[test]
    fn sibling_series_navigation() {
        let db = base_db();
        let d = dao(&db);
        assert_eq!(
            d.find_next_in_series("b1", "u1").unwrap().map(|b| b.id),
            Some("b2".to_string())
        );
        assert!(d.find_next_in_series("b3", "u1").unwrap().is_none());
        assert_eq!(
            d.find_previous_in_series("b3", "u1").unwrap().map(|b| b.id),
            Some("b2".to_string())
        );
        assert!(d.find_previous_in_series("b1", "u1").unwrap().is_none());
        // unknown book id is an internal error, mirroring Kotlin's fetchOne()!!
        assert!(d.find_next_in_series("nope", "u1").is_err());
    }

    fn readlist(id: &str, ordered: bool) -> ReadList {
        ReadList {
            id: id.to_string(),
            name: id.to_string(),
            summary: String::new(),
            ordered,
            book_ids: Default::default(),
            filtered: false,
            created_date: komga_core::time_codec::now_utc(),
            last_modified_date: komga_core::time_codec::now_utc(),
        }
    }

    #[test]
    fn sibling_readlist_ordered_navigation() {
        let db = base_db();
        let d = dao(&db);
        let rl = readlist("rl1", true);
        let none = ContentRestrictions::default();
        assert_eq!(
            d.find_next_in_readlist(&rl, "b1", "u1", None, &none)
                .unwrap()
                .map(|b| b.id),
            Some("b3".to_string())
        );
        assert_eq!(
            d.find_next_in_readlist(&rl, "b3", "u1", None, &none)
                .unwrap()
                .map(|b| b.id),
            Some("b5".to_string())
        );
        assert!(d
            .find_next_in_readlist(&rl, "b5", "u1", None, &none)
            .unwrap()
            .is_none());
        assert_eq!(
            d.find_previous_in_readlist(&rl, "b5", "u1", None, &none)
                .unwrap()
                .map(|b| b.id),
            Some("b3".to_string())
        );
        assert!(d
            .find_previous_in_readlist(&rl, "b1", "u1", None, &none)
            .unwrap()
            .is_none());
    }

    #[test]
    fn sibling_readlist_ordered_with_library_filter_and_restrictions() {
        let db = base_db();
        let d = dao(&db);
        let rl = readlist("rl1", true);
        let none = ContentRestrictions::default();
        // b5 is in l1: filtering on l2 hides it
        assert!(d
            .find_next_in_readlist(&rl, "b3", "u1", Some(&set(&["l2"])), &none)
            .unwrap()
            .is_none());
        // b5's series has the "kids" sharing label: allowing "other" hides it
        let restrictions = ContentRestrictions::new(
            None,
            ["other".to_string()].into_iter().collect(),
            BTreeSet::new(),
        );
        assert!(d
            .find_next_in_readlist(&rl, "b3", "u1", None, &restrictions)
            .unwrap()
            .is_none());
        let restrictions = ContentRestrictions::new(
            None,
            ["kids".to_string()].into_iter().collect(),
            BTreeSet::new(),
        );
        assert_eq!(
            d.find_next_in_readlist(&rl, "b3", "u1", None, &restrictions)
                .unwrap()
                .map(|b| b.id),
            Some("b5".to_string())
        );
    }

    #[test]
    fn sibling_readlist_unordered_uses_release_date() {
        let db = base_db();
        let d = dao(&db);
        let rl = readlist("rl2", false);
        let none = ContentRestrictions::default();
        // release dates: b5 NULL (first), b3 2019-01-01, b1 2020-01-01
        assert_eq!(
            d.find_next_in_readlist(&rl, "b5", "u1", None, &none)
                .unwrap()
                .map(|b| b.id),
            Some("b3".to_string())
        );
        assert_eq!(
            d.find_next_in_readlist(&rl, "b3", "u1", None, &none)
                .unwrap()
                .map(|b| b.id),
            Some("b1".to_string())
        );
        assert!(d
            .find_next_in_readlist(&rl, "b1", "u1", None, &none)
            .unwrap()
            .is_none());
        assert_eq!(
            d.find_previous_in_readlist(&rl, "b1", "u1", None, &none)
                .unwrap()
                .map(|b| b.id),
            Some("b3".to_string())
        );
    }

    #[test]
    fn on_deck_returns_first_unread_of_partially_read_series() {
        let db = base_db();
        // a second on-deck series with a newer read date to exercise the ordering
        let conn = db.rw();
        insert_series(&conn, "s4", "l1", 2, false);
        insert_series_metadata(&conn, "s4", "Delta", "P1", None);
        insert_book(&conn, "b7", "s4", "l1", 50, "h4", false, false);
        insert_book(&conn, "b8", "s4", "l1", 60, "h5", false, false);
        insert_media(&conn, "b7", "READY", "application/zip", 1);
        insert_media(&conn, "b8", "READY", "application/zip", 1);
        insert_book_metadata(&conn, "b7", "Book Seven", 1.0, None);
        insert_book_metadata(&conn, "b8", "Book Eight", 2.0, None);
        insert_read_progress(&conn, "b7", "u1", 1, true);
        insert_read_progress_series(&conn, "s4", "u1", 1, 0, Some("2022-01-01 00:00:00.0"));
        drop(conn);

        let d = dao(&db);
        let none = ContentRestrictions::default();
        let page = d.find_all_on_deck("u1", None, &none, &unpaged()).unwrap();
        assert_eq!(page.total, 2);
        // most recently read series first
        assert_eq!(ids(&page), ["b8", "b5"]);
        assert!(!page.sorted);
        for book in &page.items {
            assert!(book.read_progress.is_none());
        }

        // s2 has the "kids" label: allowing only "other" empties the deck entirely
        let restrictions = ContentRestrictions::new(
            None,
            ["other".to_string()].into_iter().collect(),
            BTreeSet::new(),
        );
        let page = d
            .find_all_on_deck("u1", None, &restrictions, &unpaged())
            .unwrap();
        assert!(page.items.is_empty());

        let restrictions = ContentRestrictions::new(
            None,
            ["kids".to_string()].into_iter().collect(),
            BTreeSet::new(),
        );
        let page = d
            .find_all_on_deck("u1", None, &restrictions, &unpaged())
            .unwrap();
        assert_eq!(id_set(&page), set(&["b5"]));

        // library filter: b5 and b8 are both in l1
        let page = d
            .find_all_on_deck("u1", Some(&set(&["l2"])), &none, &unpaged())
            .unwrap();
        assert!(page.items.is_empty());
    }

    #[test]
    fn on_deck_picks_lowest_number_sort_then_book_id() {
        let db = base_db();
        // s5 has three unread books: b10 sorts last; b11 and b12 tie on number_sort,
        // so the smaller book id must win
        let conn = db.rw();
        insert_series(&conn, "s5", "l1", 4, false);
        insert_series_metadata(&conn, "s5", "Echo", "P1", None);
        for (id, size, hash, sort) in [
            ("b9", 70, "h6", 1.0),
            ("b10", 71, "h7", 2.0),
            ("b11", 72, "h8", 1.0),
            ("b12", 73, "h9", 1.0),
        ] {
            insert_book(&conn, id, "s5", "l1", size, hash, false, false);
            insert_media(&conn, id, "READY", "application/zip", 1);
            insert_book_metadata(&conn, id, &format!("Book {id}"), sort, None);
        }
        insert_read_progress(&conn, "b9", "u1", 1, true);
        insert_read_progress_series(&conn, "s5", "u1", 1, 0, Some("2023-01-01 00:00:00.0"));
        drop(conn);

        let page = dao(&db)
            .find_all_on_deck("u1", None, &ContentRestrictions::default(), &unpaged())
            .unwrap();
        assert_eq!(page.total, 2);
        assert_eq!(ids(&page), ["b11", "b5"]);
    }

    #[test]
    fn on_deck_out_of_range_page_keeps_total() {
        let db = base_db();
        let page = dao(&db)
            .find_all_on_deck(
                "u1",
                None,
                &ContentRestrictions::default(),
                &PageRequest {
                    page: 5,
                    size: 20,
                    unpaged: false,
                    sort: vec![],
                },
            )
            .unwrap();
        assert!(page.items.is_empty());
        assert_eq!(page.total, 1);
    }

    #[test]
    fn on_deck_paged_total_exceeds_page_size() {
        let db = base_db();
        let conn = db.rw();
        insert_series(&conn, "s4", "l1", 2, false);
        insert_series_metadata(&conn, "s4", "Delta", "P1", None);
        insert_book(&conn, "b7", "s4", "l1", 50, "h4", false, false);
        insert_book(&conn, "b8", "s4", "l1", 60, "h5", false, false);
        insert_media(&conn, "b7", "READY", "application/zip", 1);
        insert_media(&conn, "b8", "READY", "application/zip", 1);
        insert_book_metadata(&conn, "b7", "Book Seven", 1.0, None);
        insert_book_metadata(&conn, "b8", "Book Eight", 2.0, None);
        insert_read_progress(&conn, "b7", "u1", 1, true);
        insert_read_progress_series(&conn, "s4", "u1", 1, 0, Some("2022-01-01 00:00:00.0"));
        drop(conn);

        let paged = |page: u32| PageRequest {
            page,
            size: 1,
            unpaged: false,
            sort: vec![],
        };
        let d = dao(&db);
        let none = ContentRestrictions::default();
        // two on-deck series, one book per page: total must reflect the full
        // filtered result, not the page size
        let first = d.find_all_on_deck("u1", None, &none, &paged(0)).unwrap();
        assert_eq!(first.total, 2);
        assert_eq!(ids(&first), ["b8"]);
        let second = d.find_all_on_deck("u1", None, &none, &paged(1)).unwrap();
        assert_eq!(second.total, 2);
        assert_eq!(ids(&second), ["b5"]);
    }

    #[test]
    fn duplicates_grouped_by_hash_and_size() {
        let db = base_db();
        let mut page = unpaged();
        page.sort = vec![SortOrder {
            property: "fileHash".to_string(),
            descending: false,
        }];
        let result = dao(&db).find_all_duplicates("u1", &page).unwrap();
        // (h1, 100) is the only duplicate (hash, size) group, so total counts only b1+b2;
        // but the listing filters on hash alone, so b4 (h1 with a different size) is listed too —
        // same quirk as the Kotlin query
        assert_eq!(result.total, 2);
        assert_eq!(id_set(&result), set(&["b1", "b2", "b4"]));
        assert!(result.sorted);
    }

    #[test]
    fn find_all_sort_by_readlist_number() {
        let db = base_db();
        let mut page = unpaged();
        page.sort = vec![SortOrder {
            property: "readList.number".to_string(),
            descending: false,
        }];
        let result = dao(&db)
            .find_all(
                &search(Some(SearchConditionBook::ReadListId {
                    operator: is("rl1"),
                })),
                &ctx_user(),
                &page,
            )
            .unwrap();
        assert_eq!(ids(&result), ["b1", "b3", "b5"]);
        assert!(result.sorted);
    }

    #[test]
    fn find_all_readlist_is_not() {
        let db = base_db();
        let page = dao(&db)
            .find_all(
                &search(Some(SearchConditionBook::ReadListId {
                    operator: is_not("rl1"),
                })),
                &ctx_user(),
                &unpaged(),
            )
            .unwrap();
        assert_eq!(id_set(&page), set(&["b2", "b4", "b6"]));
    }
}
