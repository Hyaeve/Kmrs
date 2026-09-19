//! The DTO-returning queries of `SeriesCollectionDao.kt`.

use super::{search_entity_ids, DtoPage, EntitySearcher, PageRequest, SortOrder};
use crate::dao::get_datetime;
use crate::error::Result;
use crate::pool::Database;
use crate::search_sql::{
    content_restrictions_condition, id_in_or_no_condition, sort_by_values, SqlWhere,
};
use komga_core::dto::collection::CollectionDto;
use komga_core::model::collection::SeriesCollection;
use komga_core::model::user::ContentRestrictions;
use komga_core::task::LuceneEntity;
use rusqlite::types::Value;
use rusqlite::Row;
use std::collections::BTreeSet;
use std::sync::Arc;

const COLUMNS: &str = "COLLECTION.ID, COLLECTION.NAME, COLLECTION.ORDERED, COLLECTION.SERIES_COUNT, COLLECTION.CREATED_DATE, COLLECTION.LAST_MODIFIED_DATE";

pub struct CollectionDtoDao {
    db: Database,
    searcher: Option<Arc<dyn EntitySearcher>>,
}

impl CollectionDtoDao {
    pub fn new(db: Database) -> Self {
        Self { db, searcher: None }
    }

    pub fn with_searcher(mut self, searcher: Option<Arc<dyn EntitySearcher>>) -> Self {
        self.searcher = searcher;
        self
    }

    /// `findAll(belongsToLibraryIds, filterOnLibraryIds, search, pageable, restrictions)`:
    /// `belongs_to` narrows which collections are visible; `authorized` also filters
    /// each collection's member series (driving the `filtered` flag).
    pub fn find_all(
        &self,
        belongs_to_library_ids: Option<&BTreeSet<String>>,
        authorized_library_ids: Option<&BTreeSet<String>>,
        search: Option<&str>,
        page: &PageRequest,
        restrictions: &ContentRestrictions,
    ) -> Result<DtoPage<CollectionDto>> {
        let ids = search_entity_ids(&self.searcher, search, LuceneEntity::Collection);
        let make_conditions = || {
            id_in_or_no_condition("COLLECTION.ID", ids.as_deref())
                .and(set_condition("SERIES.LIBRARY_ID", belongs_to_library_ids))
                .and(set_condition("SERIES.LIBRARY_ID", authorized_library_ids))
                .and(content_restrictions_condition(restrictions))
        };
        // Kotlin skips the id subquery only when nothing but the search term filters
        let needs_query_ids = !(belongs_to_library_ids.is_none()
            && authorized_library_ids.is_none()
            && !restrictions.is_restricted());

        let conn = self.db.ro();

        let total: i64 = if needs_query_ids {
            let w = make_conditions();
            // the id subquery joins SERIES_METADATA unconditionally, unlike the base select
            let sql = format!(
                "SELECT COUNT(*) FROM (SELECT DISTINCT COLLECTION.ID FROM COLLECTION \
         LEFT JOIN COLLECTION_SERIES ON COLLECTION.ID = COLLECTION_SERIES.COLLECTION_ID \
         LEFT JOIN SERIES ON COLLECTION_SERIES.SERIES_ID = SERIES.ID \
         LEFT JOIN SERIES_METADATA ON COLLECTION_SERIES.SERIES_ID = SERIES_METADATA.SERIES_ID{})",
                where_sql(&w)
            );
            conn.query_row(&sql, rusqlite::params_from_iter(w.params), |r| r.get(0))?
        } else {
            let w = make_conditions();
            let sql = format!("SELECT COUNT(*) FROM COLLECTION{}", where_sql(&w));
            conn.query_row(&sql, rusqlite::params_from_iter(w.params), |r| r.get(0))?
        };

        let (order_sql, order_params) = order_by(&page.sort, &ids);

        let mut items_where = make_conditions();
        if needs_query_ids {
            let sub_where = make_conditions();
            let sub = format!(
                "SELECT DISTINCT COLLECTION.ID FROM COLLECTION \
         LEFT JOIN COLLECTION_SERIES ON COLLECTION.ID = COLLECTION_SERIES.COLLECTION_ID \
         LEFT JOIN SERIES ON COLLECTION_SERIES.SERIES_ID = SERIES.ID \
         LEFT JOIN SERIES_METADATA ON COLLECTION_SERIES.SERIES_ID = SERIES_METADATA.SERIES_ID{}",
                where_sql(&sub_where)
            );
            items_where = items_where.and(SqlWhere {
                sql: format!("COLLECTION.ID IN ({sub})"),
                params: sub_where.params,
                joins: BTreeSet::new(),
            });
        }

        let mut sql = format!(
            "{}{}",
            select_base(restrictions.is_restricted()),
            where_sql(&items_where)
        );
        if !order_sql.is_empty() {
            sql.push_str(&format!(" ORDER BY {order_sql}"));
        }
        let mut params = items_where.params;
        params.extend(order_params);
        if !page.unpaged {
            sql.push_str(" LIMIT ? OFFSET ?");
            params.push(Value::Integer(page.size as i64));
            params.push(Value::Integer(page.offset() as i64));
        }

        let rows = conn
            .prepare(&sql)?
            .query_map(rusqlite::params_from_iter(params), row_to_collection)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        drop(conn);

        let collections = self.fetch_and_map(rows, authorized_library_ids, restrictions)?;
        Ok(DtoPage {
            items: collections.iter().map(CollectionDto::from).collect(),
            total,
            sorted: !order_sql.is_empty(),
        })
    }

    /// `findByIdOrNull(collectionId, filterOnLibraryIds, restrictions)`
    pub fn find_by_id(
        &self,
        id: &str,
        authorized_library_ids: Option<&BTreeSet<String>>,
        restrictions: &ContentRestrictions,
    ) -> Result<Option<SeriesCollection>> {
        let w = single("COLLECTION.ID", id)
            .and(set_condition("SERIES.LIBRARY_ID", authorized_library_ids))
            .and(content_restrictions_condition(restrictions));
        let sql = format!(
            "{}{}",
            select_base(restrictions.is_restricted()),
            where_sql(&w)
        );
        let row = self
            .db
            .ro()
            .prepare(&sql)?
            .query_map(rusqlite::params_from_iter(w.params), row_to_collection)?
            .next()
            .transpose()?;
        match row {
            None => Ok(None),
            Some(row) => Ok(self
                .fetch_and_map(vec![row], authorized_library_ids, restrictions)?
                .into_iter()
                .next()),
        }
    }

    /// `findAllContainingSeriesId(containsSeriesId, filterOnLibraryIds, restrictions)`
    pub fn find_all_containing_series_id(
        &self,
        series_id: &str,
        authorized_library_ids: Option<&BTreeSet<String>>,
        restrictions: &ContentRestrictions,
    ) -> Result<Vec<SeriesCollection>> {
        let sub_where = single("COLLECTION_SERIES.SERIES_ID", series_id)
            .and(content_restrictions_condition(restrictions));
        let sub = format!(
            "SELECT COLLECTION.ID FROM COLLECTION \
       LEFT JOIN COLLECTION_SERIES ON COLLECTION.ID = COLLECTION_SERIES.COLLECTION_ID{} \
       WHERE {}",
            sd_join(restrictions.is_restricted()),
            sub_where.sql
        );
        let w = SqlWhere {
            sql: format!("COLLECTION.ID IN ({sub})"),
            params: sub_where.params,
            joins: BTreeSet::new(),
        }
        .and(set_condition("SERIES.LIBRARY_ID", authorized_library_ids))
        .and(content_restrictions_condition(restrictions));
        let sql = format!(
            "{}{}",
            select_base(restrictions.is_restricted()),
            where_sql(&w)
        );
        let rows = self
            .db
            .ro()
            .prepare(&sql)?
            .query_map(rusqlite::params_from_iter(w.params), row_to_collection)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        self.fetch_and_map(rows, authorized_library_ids, restrictions)
    }

    /// `fetchAndMap`: per collection, member series filtered by authorized libraries and
    /// restrictions; `filtered` when the persisted SERIES_COUNT differs from the visible members.
    fn fetch_and_map(
        &self,
        rows: Vec<(SeriesCollection, i32)>,
        authorized_library_ids: Option<&BTreeSet<String>>,
        restrictions: &ContentRestrictions,
    ) -> Result<Vec<SeriesCollection>> {
        let conn = self.db.ro();
        let mut out = Vec::with_capacity(rows.len());
        for (mut collection, series_count) in rows {
            let w = single("COLLECTION_SERIES.COLLECTION_ID", &collection.id)
                .and(set_condition("SERIES.LIBRARY_ID", authorized_library_ids))
                .and(content_restrictions_condition(restrictions));
            let sql = format!(
                "SELECT COLLECTION_SERIES.SERIES_ID FROM COLLECTION_SERIES \
         LEFT JOIN SERIES ON COLLECTION_SERIES.SERIES_ID = SERIES.ID{} \
         WHERE {} ORDER BY COLLECTION_SERIES.NUMBER ASC",
                sd_join(restrictions.is_restricted()),
                w.sql
            );
            collection.series_ids = conn
                .prepare(&sql)?
                .query_map(rusqlite::params_from_iter(w.params), |r| {
                    r.get::<_, String>(0)
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            collection.filtered = series_count as usize != collection.series_ids.len();
            out.push(collection);
        }
        Ok(out)
    }
}

fn row_to_collection(row: &Row<'_>) -> rusqlite::Result<(SeriesCollection, i32)> {
    Ok((
        SeriesCollection {
            id: row.get(0)?,
            name: row.get(1)?,
            ordered: row.get(2)?,
            series_ids: Vec::new(),
            filtered: false,
            created_date: get_datetime(row, 4)?,
            last_modified_date: get_datetime(row, 5)?,
        },
        row.get(3)?,
    ))
}

fn select_base(join_series_metadata: bool) -> String {
    format!(
        "SELECT DISTINCT {COLUMNS} FROM COLLECTION \
     LEFT JOIN COLLECTION_SERIES ON COLLECTION.ID = COLLECTION_SERIES.COLLECTION_ID \
     LEFT JOIN SERIES ON COLLECTION_SERIES.SERIES_ID = SERIES.ID{}",
        sd_join(join_series_metadata)
    )
}

fn sd_join(join: bool) -> &'static str {
    if join {
        " LEFT JOIN SERIES_METADATA ON COLLECTION_SERIES.SERIES_ID = SERIES_METADATA.SERIES_ID"
    } else {
        ""
    }
}

fn where_sql(w: &SqlWhere) -> String {
    if w.sql.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", w.sql)
    }
}

fn single(field: &str, value: &str) -> SqlWhere {
    SqlWhere {
        sql: format!("{field} IN (?)"),
        params: vec![Value::Text(value.to_string())],
        joins: BTreeSet::new(),
    }
}

fn set_condition(field: &str, ids: Option<&BTreeSet<String>>) -> SqlWhere {
    match ids {
        None => SqlWhere::no_condition(),
        Some(ids) => {
            let ids: Vec<String> = ids.iter().cloned().collect();
            id_in_or_no_condition(field, Some(&ids))
        }
    }
}

/// `sorts` map from the Kotlin DAO plus `relevance` handling; unknown properties are dropped
fn order_by(sort: &[SortOrder], ids: &Option<Vec<String>>) -> (String, Vec<Value>) {
    let mut parts = vec![];
    let mut params = vec![];
    for order in sort {
        if order.property == "relevance" {
            if let Some(ids) = ids.as_ref().filter(|i| !i.is_empty()) {
                let (sql, p) = sort_by_values("COLLECTION.ID", ids, !order.descending);
                parts.push(sql);
                params.extend(p);
            }
            continue;
        }
        let expr = match order.property.as_str() {
            "name" => "COLLECTION.NAME COLLATE COLLATION_UNICODE_3",
            "createdDate" => "COLLECTION.CREATED_DATE",
            "lastModifiedDate" => "COLLECTION.LAST_MODIFIED_DATE",
            _ => continue,
        };
        parts.push(format!(
            "{expr} {}",
            if order.descending { "DESC" } else { "ASC" }
        ));
    }
    (parts.join(", "), params)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migrate::Migrator;
    use crate::{main_migrations, Placeholders};
    use komga_core::model::user::{AgeRestriction, AllowExclude};

    fn db() -> Database {
        let db = Database::open_in_memory(true).unwrap();
        let migrations = main_migrations();
        Migrator::new(&migrations, Placeholders::default())
            .migrate(&db.rw())
            .unwrap();
        db
    }

    fn seed_library(db: &Database, id: &str) {
        db.rw()
            .execute(
                "INSERT INTO LIBRARY (ID, NAME, ROOT) VALUES (?, 'L', 'file:/l/')",
                [id],
            )
            .unwrap();
    }

    fn seed_series(db: &Database, id: &str, library_id: &str) {
        db.rw()
            .execute(
                "INSERT INTO SERIES (ID, NAME, URL, FILE_LAST_MODIFIED, LIBRARY_ID) \
         VALUES (?, ?, 'file:/l/s/', '2020-01-01 00:00:00.0', ?)",
                rusqlite::params![id, id, library_id],
            )
            .unwrap();
    }

    fn seed_metadata(db: &Database, series_id: &str, age_rating: Option<i32>) {
        db.rw()
            .execute(
                "INSERT INTO SERIES_METADATA (SERIES_ID, STATUS, TITLE, TITLE_SORT, AGE_RATING) \
         VALUES (?, 'ONGOING', ?, ?, ?)",
                rusqlite::params![series_id, series_id, series_id, age_rating],
            )
            .unwrap();
    }

    fn seed_sharing_label(db: &Database, series_id: &str, label: &str) {
        db.rw()
            .execute(
                "INSERT INTO SERIES_METADATA_SHARING (SERIES_ID, LABEL) VALUES (?, ?)",
                rusqlite::params![series_id, label],
            )
            .unwrap();
    }

    fn seed_collection(db: &Database, id: &str, name: &str, series_ids: &[&str]) {
        let conn = db.rw();
        conn.execute(
            "INSERT INTO COLLECTION (ID, NAME, ORDERED, SERIES_COUNT) VALUES (?, ?, 0, ?)",
            rusqlite::params![id, name, series_ids.len() as i64],
        )
        .unwrap();
        for (i, sid) in series_ids.iter().enumerate() {
            conn.execute(
                "INSERT INTO COLLECTION_SERIES (COLLECTION_ID, SERIES_ID, NUMBER) VALUES (?, ?, ?)",
                rusqlite::params![id, sid, i as i64],
            )
            .unwrap();
        }
    }

    fn sorted(page: PageRequest, property: &str, descending: bool) -> PageRequest {
        PageRequest {
            sort: vec![SortOrder {
                property: property.to_string(),
                descending,
            }],
            ..page
        }
    }

    fn page() -> PageRequest {
        PageRequest {
            page: 0,
            size: 20,
            unpaged: false,
            sort: vec![],
        }
    }

    fn ids(collections: &[SeriesCollection]) -> Vec<String> {
        collections.iter().map(|c| c.id.clone()).collect()
    }

    #[test]
    fn find_all_unfiltered_with_sort_and_paging() {
        let db = db();
        let dao = CollectionDtoDao::new(db.clone());
        seed_collection(&db, "c1", "gamma", &[]);
        seed_collection(&db, "c2", "alpha", &[]);
        seed_collection(&db, "c3", "Beta", &[]);

        let by_name = sorted(page(), "name", false);
        let result = dao
            .find_all(None, None, None, &by_name, &ContentRestrictions::default())
            .unwrap();
        assert_eq!(result.total, 3);
        assert!(result.sorted);
        // COLLATION_UNICODE_3 orders case-insensitively by base letters
        let names: Vec<&str> = result.items.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["alpha", "Beta", "gamma"]);

        let result = dao
            .find_all(
                None,
                None,
                None,
                &sorted(PageRequest { size: 2, ..page() }, "name", true),
                &ContentRestrictions::default(),
            )
            .unwrap();
        let names: Vec<&str> = result.items.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["gamma", "Beta"]);
        assert_eq!(result.total, 3);

        let result = dao
            .find_all(
                None,
                None,
                None,
                &PageRequest {
                    sort: vec![],
                    ..page()
                },
                &ContentRestrictions::default(),
            )
            .unwrap();
        assert!(!result.sorted);
    }

    #[test]
    fn find_all_library_filtering_and_filtered_flag() {
        let db = db();
        let dao = CollectionDtoDao::new(db.clone());
        seed_library(&db, "lib1");
        seed_library(&db, "lib2");
        seed_series(&db, "s1", "lib1");
        seed_series(&db, "s2", "lib2");
        seed_collection(&db, "c1", "one", &["s1"]);
        seed_collection(&db, "c2", "two", &["s2"]);
        seed_collection(&db, "c3", "three", &["s1", "s2"]);
        seed_collection(&db, "c4", "empty", &[]);

        let lib1: BTreeSet<String> = ["lib1".to_string()].into_iter().collect();

        // belongs_to narrows visible collections; members stay complete without authorized filter
        let result = dao
            .find_all(
                Some(&lib1),
                None,
                None,
                &page(),
                &ContentRestrictions::default(),
            )
            .unwrap();
        assert_eq!(ids_as_strings(&result.items), vec!["c1", "c3"]);
        // the empty collection has no series row to match the library condition
        let c3 = result.items.iter().find(|c| c.id == "c3").unwrap();
        assert_eq!(c3.series_ids, vec!["s1", "s2"]);
        assert!(!c3.filtered);

        // authorized also filters members: c3 loses s2 and becomes filtered
        let result = dao
            .find_all(
                None,
                Some(&lib1),
                None,
                &page(),
                &ContentRestrictions::default(),
            )
            .unwrap();
        let c3 = result.items.iter().find(|c| c.id == "c3").unwrap();
        assert_eq!(c3.series_ids, vec!["s1"]);
        assert!(c3.filtered);

        // empty authorized set means nothing is visible
        let empty: BTreeSet<String> = BTreeSet::new();
        let result = dao
            .find_all(
                None,
                Some(&empty),
                None,
                &page(),
                &ContentRestrictions::default(),
            )
            .unwrap();
        assert_eq!(result.total, 0);
        assert!(result.items.is_empty());

        // no filters at all: the empty collection shows up
        let result = dao
            .find_all(None, None, None, &page(), &ContentRestrictions::default())
            .unwrap();
        assert_eq!(result.total, 4);
    }

    #[test]
    fn find_all_with_content_restrictions() {
        let db = db();
        let dao = CollectionDtoDao::new(db.clone());
        seed_library(&db, "lib1");
        seed_series(&db, "s1", "lib1");
        seed_series(&db, "s2", "lib1");
        seed_metadata(&db, "s1", Some(10));
        seed_metadata(&db, "s2", Some(18));
        seed_collection(&db, "c1", "mixed", &["s1", "s2"]);
        seed_collection(&db, "c2", "kids", &["s1"]);
        seed_collection(&db, "c3", "empty", &[]);

        let restrictions = ContentRestrictions::new(
            Some(AgeRestriction {
                age: 12,
                restriction: AllowExclude::AllowOnly,
            }),
            BTreeSet::new(),
            BTreeSet::new(),
        );
        let result = dao
            .find_all(None, None, None, &page(), &restrictions)
            .unwrap();
        // the empty collection joins no SERIES_METADATA row and is not visible when restricted
        assert_eq!(ids_as_strings(&result.items), vec!["c1", "c2"]);
        let c1 = result.items.iter().find(|c| c.id == "c1").unwrap();
        assert_eq!(c1.series_ids, vec!["s1"]);
        assert!(c1.filtered);
        let c2 = result.items.iter().find(|c| c.id == "c2").unwrap();
        assert!(!c2.filtered);

        // label-based allowance
        seed_sharing_label(&db, "s2", "kids");
        let restrictions = ContentRestrictions::new(
            None,
            ["kids".to_string()].into_iter().collect(),
            BTreeSet::new(),
        );
        let result = dao
            .find_all(None, None, None, &page(), &restrictions)
            .unwrap();
        assert_eq!(ids_as_strings(&result.items), vec!["c1"]);
    }

    #[test]
    fn find_by_id_authorization() {
        let db = db();
        let dao = CollectionDtoDao::new(db.clone());
        seed_library(&db, "lib1");
        seed_library(&db, "lib2");
        seed_series(&db, "s1", "lib1");
        seed_series(&db, "s2", "lib2");
        seed_collection(&db, "c1", "one", &["s1", "s2"]);
        seed_collection(&db, "c2", "two", &["s2"]);

        let lib1: BTreeSet<String> = ["lib1".to_string()].into_iter().collect();

        let found = dao
            .find_by_id("c1", Some(&lib1), &ContentRestrictions::default())
            .unwrap()
            .expect("not found");
        assert_eq!(found.series_ids, vec!["s1"]);
        assert!(found.filtered);

        // all members outside the authorized libraries: not visible
        assert!(dao
            .find_by_id("c2", Some(&lib1), &ContentRestrictions::default())
            .unwrap()
            .is_none());

        // unrestricted: full members
        let found = dao
            .find_by_id("c1", None, &ContentRestrictions::default())
            .unwrap()
            .unwrap();
        assert_eq!(found.series_ids, vec!["s1", "s2"]);
        assert!(!found.filtered);
    }

    #[test]
    fn find_all_containing_series() {
        let db = db();
        let dao = CollectionDtoDao::new(db.clone());
        seed_library(&db, "lib1");
        seed_series(&db, "s1", "lib1");
        seed_series(&db, "s2", "lib1");
        seed_series(&db, "s3", "lib1");
        seed_collection(&db, "c1", "one", &["s1", "s2"]);
        seed_collection(&db, "c2", "two", &["s2"]);
        seed_collection(&db, "c3", "three", &["s3"]);

        let found = dao
            .find_all_containing_series_id("s2", None, &ContentRestrictions::default())
            .unwrap();
        let mut found_ids = ids(&found);
        found_ids.sort();
        assert_eq!(found_ids, vec!["c1", "c2"]);
        assert!(dao
            .find_all_containing_series_id("nope", None, &ContentRestrictions::default())
            .unwrap()
            .is_empty());

        // authorized filter shrinks members of the containing collections
        let lib1: BTreeSet<String> = ["lib1".to_string()].into_iter().collect();
        seed_library(&db, "lib2");
        seed_series(&db, "s4", "lib2");
        seed_collection(&db, "c4", "four", &["s2", "s4"]);
        let found = dao
            .find_all_containing_series_id("s2", Some(&lib1), &ContentRestrictions::default())
            .unwrap();
        let c4 = found.iter().find(|c| c.id == "c4").unwrap();
        assert_eq!(c4.series_ids, vec!["s2"]);
        assert!(c4.filtered);
    }

    #[test]
    fn find_all_search_returns_nothing_before_m6() {
        let db = db();
        let dao = CollectionDtoDao::new(db.clone());
        seed_collection(&db, "c1", "one", &[]);
        let result = dao
            .find_all(
                None,
                None,
                Some("one"),
                &page(),
                &ContentRestrictions::default(),
            )
            .unwrap();
        assert_eq!(result.total, 0);
        assert!(result.items.is_empty());
    }

    fn ids_as_strings(dtos: &[CollectionDto]) -> Vec<String> {
        let mut ids: Vec<String> = dtos.iter().map(|c| c.id.clone()).collect();
        ids.sort();
        ids
    }
}
