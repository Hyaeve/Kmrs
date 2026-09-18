//! The DTO-returning queries of `ReadListDao.kt`.

use super::{lucene_ids_stub, DtoPage, PageRequest, SortOrder};
use crate::dao::get_datetime;
use crate::error::Result;
use crate::pool::Database;
use crate::search_sql::{
    content_restrictions_condition, id_in_or_no_condition, sort_by_values, SqlWhere,
};
use komga_core::dto::readlist::ReadListDto;
use komga_core::model::readlist::ReadList;
use komga_core::model::user::ContentRestrictions;
use rusqlite::types::Value;
use rusqlite::Row;
use std::collections::{BTreeMap, BTreeSet};

const COLUMNS: &str = "READLIST.ID, READLIST.NAME, READLIST.SUMMARY, READLIST.ORDERED, READLIST.BOOK_COUNT, READLIST.CREATED_DATE, READLIST.LAST_MODIFIED_DATE";

pub struct ReadListDtoDao {
    db: Database,
}

impl ReadListDtoDao {
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    /// `findAll(belongsToLibraryIds, filterOnLibraryIds, search, pageable, restrictions)`:
    /// `belongs_to` narrows which read lists are visible; `authorized` also filters
    /// each list's member books (driving the `filtered` flag).
    pub fn find_all(
        &self,
        belongs_to_library_ids: Option<&BTreeSet<String>>,
        authorized_library_ids: Option<&BTreeSet<String>>,
        search: Option<&str>,
        page: &PageRequest,
        restrictions: &ContentRestrictions,
    ) -> Result<DtoPage<ReadListDto>> {
        let ids = lucene_ids_stub(search);
        let make_conditions = || {
            id_in_or_no_condition("READLIST.ID", ids.as_deref())
                .and(set_condition("BOOK.LIBRARY_ID", belongs_to_library_ids))
                .and(set_condition("BOOK.LIBRARY_ID", authorized_library_ids))
                .and(content_restrictions_condition(restrictions))
        };
        // Kotlin skips the id subquery only when nothing but the search term filters
        let needs_query_ids = !(belongs_to_library_ids.is_none()
            && authorized_library_ids.is_none()
            && !restrictions.is_restricted());

        let conn = self.db.ro();

        let total: i64 = if needs_query_ids {
            let w = make_conditions();
            let sql = format!(
                "SELECT COUNT(*) FROM (SELECT DISTINCT READLIST.ID FROM READLIST \
         LEFT JOIN READLIST_BOOK ON READLIST.ID = READLIST_BOOK.READLIST_ID \
         LEFT JOIN BOOK ON READLIST_BOOK.BOOK_ID = BOOK.ID{}{})",
                sd_join(restrictions.is_restricted()),
                where_sql(&w)
            );
            conn.query_row(&sql, rusqlite::params_from_iter(w.params), |r| r.get(0))?
        } else {
            let w = make_conditions();
            let sql = format!("SELECT COUNT(*) FROM READLIST{}", where_sql(&w));
            conn.query_row(&sql, rusqlite::params_from_iter(w.params), |r| r.get(0))?
        };

        let (order_sql, order_params) = order_by(&page.sort, &ids);

        let mut items_where = make_conditions();
        if needs_query_ids {
            let sub_where = make_conditions();
            let sub = format!(
                "SELECT DISTINCT READLIST.ID FROM READLIST \
         LEFT JOIN READLIST_BOOK ON READLIST.ID = READLIST_BOOK.READLIST_ID \
         LEFT JOIN BOOK ON READLIST_BOOK.BOOK_ID = BOOK.ID{}{}",
                sd_join(restrictions.is_restricted()),
                where_sql(&sub_where)
            );
            items_where = items_where.and(SqlWhere {
                sql: format!("READLIST.ID IN ({sub})"),
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
            .query_map(rusqlite::params_from_iter(params), row_to_readlist)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        drop(conn);

        let readlists = self.fetch_and_map(rows, authorized_library_ids, restrictions)?;
        Ok(DtoPage {
            items: readlists.iter().map(ReadListDto::from).collect(),
            total,
            sorted: !order_sql.is_empty(),
        })
    }

    /// `findByIdOrNull(readListId, filterOnLibraryIds, restrictions)`
    pub fn find_by_id(
        &self,
        id: &str,
        authorized_library_ids: Option<&BTreeSet<String>>,
        restrictions: &ContentRestrictions,
    ) -> Result<Option<ReadList>> {
        let w = single("READLIST.ID", id)
            .and(set_condition("BOOK.LIBRARY_ID", authorized_library_ids))
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
            .query_map(rusqlite::params_from_iter(w.params), row_to_readlist)?
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

    /// `findAllContainingBookId(containsBookId, filterOnLibraryIds, restrictions)`
    pub fn find_all_containing_book_id(
        &self,
        book_id: &str,
        authorized_library_ids: Option<&BTreeSet<String>>,
        restrictions: &ContentRestrictions,
    ) -> Result<Vec<ReadList>> {
        let sub_where = single("READLIST_BOOK.BOOK_ID", book_id)
            .and(content_restrictions_condition(restrictions));
        let sub = format!(
            "SELECT READLIST.ID FROM READLIST \
       LEFT JOIN READLIST_BOOK ON READLIST.ID = READLIST_BOOK.READLIST_ID{} \
       WHERE {}",
            containing_joins(restrictions.is_restricted()),
            sub_where.sql
        );
        let w = SqlWhere {
            sql: format!("READLIST.ID IN ({sub})"),
            params: sub_where.params,
            joins: BTreeSet::new(),
        }
        .and(set_condition("BOOK.LIBRARY_ID", authorized_library_ids))
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
            .query_map(rusqlite::params_from_iter(w.params), row_to_readlist)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        self.fetch_and_map(rows, authorized_library_ids, restrictions)
    }

    /// `fetchAndMap`: per read list, member books filtered by authorized libraries and
    /// restrictions; `filtered` when the persisted BOOK_COUNT differs from the visible members.
    fn fetch_and_map(
        &self,
        rows: Vec<(ReadList, i32)>,
        authorized_library_ids: Option<&BTreeSet<String>>,
        restrictions: &ContentRestrictions,
    ) -> Result<Vec<ReadList>> {
        let conn = self.db.ro();
        let mut out = Vec::with_capacity(rows.len());
        for (mut readlist, book_count) in rows {
            let w = single("READLIST_BOOK.READLIST_ID", &readlist.id)
                .and(set_condition("BOOK.LIBRARY_ID", authorized_library_ids))
                .and(content_restrictions_condition(restrictions));
            let sql = format!(
                "SELECT READLIST_BOOK.NUMBER, READLIST_BOOK.BOOK_ID FROM READLIST_BOOK \
         LEFT JOIN BOOK ON READLIST_BOOK.BOOK_ID = BOOK.ID{} \
         WHERE {} ORDER BY READLIST_BOOK.NUMBER ASC",
                sd_join(restrictions.is_restricted()),
                w.sql
            );
            readlist.book_ids = conn
                .prepare(&sql)?
                .query_map(rusqlite::params_from_iter(w.params), |r| {
                    Ok((r.get::<_, i32>(0)?, r.get::<_, String>(1)?))
                })?
                .collect::<std::result::Result<BTreeMap<_, _>, _>>()?;
            readlist.filtered = book_count as usize != readlist.book_ids.len();
            out.push(readlist);
        }
        Ok(out)
    }
}

fn row_to_readlist(row: &Row<'_>) -> rusqlite::Result<(ReadList, i32)> {
    Ok((
        ReadList {
            id: row.get(0)?,
            name: row.get(1)?,
            summary: row.get(2)?,
            ordered: row.get(3)?,
            book_ids: BTreeMap::new(),
            filtered: false,
            created_date: get_datetime(row, 5)?,
            last_modified_date: get_datetime(row, 6)?,
        },
        row.get(4)?,
    ))
}

fn select_base(join_series_metadata: bool) -> String {
    format!(
        "SELECT DISTINCT {COLUMNS} FROM READLIST \
     LEFT JOIN READLIST_BOOK ON READLIST.ID = READLIST_BOOK.READLIST_ID \
     LEFT JOIN BOOK ON READLIST_BOOK.BOOK_ID = BOOK.ID{}",
        sd_join(join_series_metadata)
    )
}

fn sd_join(join: bool) -> &'static str {
    if join {
        " LEFT JOIN SERIES_METADATA ON SERIES_METADATA.SERIES_ID = BOOK.SERIES_ID"
    } else {
        ""
    }
}

/// `findAllContainingBookId` joins BOOK and SERIES_METADATA only when restricted
fn containing_joins(join_series_metadata: bool) -> &'static str {
    if join_series_metadata {
        " LEFT JOIN BOOK ON READLIST_BOOK.BOOK_ID = BOOK.ID \
     LEFT JOIN SERIES_METADATA ON SERIES_METADATA.SERIES_ID = BOOK.SERIES_ID"
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
                let (sql, p) = sort_by_values("READLIST.ID", ids, !order.descending);
                parts.push(sql);
                params.extend(p);
            }
            continue;
        }
        let expr = match order.property.as_str() {
            "name" => "READLIST.NAME COLLATE COLLATION_UNICODE_3",
            "createdDate" => "READLIST.CREATED_DATE",
            "lastModifiedDate" => "READLIST.LAST_MODIFIED_DATE",
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

    fn seed_book(db: &Database, id: &str, series_id: &str, library_id: &str) {
        let conn = db.rw();
        conn.execute(
            "INSERT OR IGNORE INTO SERIES (ID, NAME, URL, FILE_LAST_MODIFIED, LIBRARY_ID) \
         VALUES (?, ?, 'file:/l/s/', '2020-01-01 00:00:00.0', ?)",
            rusqlite::params![series_id, series_id, library_id],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO BOOK (ID, NAME, URL, FILE_LAST_MODIFIED, SERIES_ID, LIBRARY_ID) \
         VALUES (?, ?, 'file:/l/s/b.cbz', '2020-01-01 00:00:00.0', ?, ?)",
            rusqlite::params![id, id, series_id, library_id],
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

    fn seed_readlist(db: &Database, id: &str, name: &str, book_ids: &[(i32, &str)]) {
        let conn = db.rw();
        conn.execute(
            "INSERT INTO READLIST (ID, NAME, SUMMARY, ORDERED, BOOK_COUNT) VALUES (?, ?, '', 1, ?)",
            rusqlite::params![id, name, book_ids.len() as i64],
        )
        .unwrap();
        for (number, bid) in book_ids {
            conn.execute(
                "INSERT INTO READLIST_BOOK (READLIST_ID, BOOK_ID, NUMBER) VALUES (?, ?, ?)",
                rusqlite::params![id, bid, number],
            )
            .unwrap();
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

    fn sorted_by_name(descending: bool) -> PageRequest {
        PageRequest {
            sort: vec![SortOrder {
                property: "name".to_string(),
                descending,
            }],
            ..page()
        }
    }

    fn ids(dtos: &[ReadListDto]) -> Vec<String> {
        let mut ids: Vec<String> = dtos.iter().map(|r| r.id.clone()).collect();
        ids.sort();
        ids
    }

    #[test]
    fn find_all_sorted_paged_and_member_order() {
        let db = db();
        let dao = ReadListDtoDao::new(db.clone());
        seed_library(&db, "lib1");
        for b in ["b1", "b2", "b3", "b4"] {
            seed_book(&db, b, "s1", "lib1");
        }
        seed_readlist(&db, "rl1", "alpha", &[(0, "b1"), (1, "b2")]);
        // sparse ordinals are kept as-is
        seed_readlist(&db, "rl2", "Beta", &[(2, "b3"), (0, "b4")]);
        seed_readlist(&db, "rl3", "gamma", &[]);

        let result = dao
            .find_all(
                None,
                None,
                None,
                &sorted_by_name(false),
                &ContentRestrictions::default(),
            )
            .unwrap();
        assert_eq!(result.total, 3);
        assert!(result.sorted);
        let names: Vec<&str> = result.items.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["alpha", "Beta", "gamma"]);
        // DTO book_ids follow READLIST_BOOK.NUMBER order
        let rl2 = result.items.iter().find(|r| r.id == "rl2").unwrap();
        assert_eq!(rl2.book_ids, vec!["b4", "b3"]);

        let result = dao
            .find_all(
                None,
                None,
                None,
                &PageRequest {
                    size: 2,
                    ..sorted_by_name(true)
                },
                &ContentRestrictions::default(),
            )
            .unwrap();
        let names: Vec<&str> = result.items.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["gamma", "Beta"]);
    }

    #[test]
    fn find_all_library_filtering_and_filtered_flag() {
        let db = db();
        let dao = ReadListDtoDao::new(db.clone());
        seed_library(&db, "lib1");
        seed_library(&db, "lib2");
        seed_book(&db, "b1", "s1", "lib1");
        seed_book(&db, "b2", "s2", "lib2");
        seed_readlist(&db, "rl1", "one", &[(0, "b1"), (1, "b2")]);
        seed_readlist(&db, "rl2", "two", &[(0, "b2")]);
        seed_readlist(&db, "rl3", "empty", &[]);

        let lib1: BTreeSet<String> = ["lib1".to_string()].into_iter().collect();

        // belongs_to narrows visible lists; members stay complete without authorized filter
        let result = dao
            .find_all(
                Some(&lib1),
                None,
                None,
                &page(),
                &ContentRestrictions::default(),
            )
            .unwrap();
        assert_eq!(ids(&result.items), vec!["rl1"]);
        let rl1 = &result.items[0];
        assert_eq!(rl1.book_ids, vec!["b1", "b2"]);
        assert!(!rl1.filtered);

        // authorized also filters members: rl1 loses b2 and becomes filtered
        let result = dao
            .find_all(
                None,
                Some(&lib1),
                None,
                &page(),
                &ContentRestrictions::default(),
            )
            .unwrap();
        let rl1 = &result.items[0];
        assert_eq!(rl1.book_ids, vec!["b1"]);
        assert!(rl1.filtered);

        // no filters: the empty list shows up
        let result = dao
            .find_all(None, None, None, &page(), &ContentRestrictions::default())
            .unwrap();
        assert_eq!(result.total, 3);
    }

    #[test]
    fn find_all_with_content_restrictions() {
        let db = db();
        let dao = ReadListDtoDao::new(db.clone());
        seed_library(&db, "lib1");
        seed_book(&db, "b1", "s1", "lib1");
        seed_book(&db, "b2", "s2", "lib1");
        seed_metadata(&db, "s1", Some(10));
        seed_metadata(&db, "s2", Some(18));
        seed_readlist(&db, "rl1", "mixed", &[(0, "b1"), (1, "b2")]);
        seed_readlist(&db, "rl2", "kids", &[(0, "b1")]);
        seed_readlist(&db, "rl3", "empty", &[]);

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
        // the empty list joins no SERIES_METADATA row and is not visible when restricted
        assert_eq!(ids(&result.items), vec!["rl1", "rl2"]);
        let rl1 = result.items.iter().find(|r| r.id == "rl1").unwrap();
        assert_eq!(rl1.book_ids, vec!["b1"]);
        assert!(rl1.filtered);
    }

    #[test]
    fn find_by_id_and_containing_book() {
        let db = db();
        let dao = ReadListDtoDao::new(db.clone());
        seed_library(&db, "lib1");
        seed_library(&db, "lib2");
        seed_book(&db, "b1", "s1", "lib1");
        seed_book(&db, "b2", "s2", "lib2");
        seed_readlist(&db, "rl1", "one", &[(0, "b1"), (1, "b2")]);
        seed_readlist(&db, "rl2", "two", &[(0, "b2")]);

        let lib1: BTreeSet<String> = ["lib1".to_string()].into_iter().collect();

        let found = dao
            .find_by_id("rl1", Some(&lib1), &ContentRestrictions::default())
            .unwrap()
            .expect("not found");
        assert_eq!(found.book_ids, BTreeMap::from([(0, "b1".to_string())]));
        assert!(found.filtered);

        assert!(dao
            .find_by_id("rl2", Some(&lib1), &ContentRestrictions::default())
            .unwrap()
            .is_none());

        let found = dao
            .find_all_containing_book_id("b2", None, &ContentRestrictions::default())
            .unwrap();
        let mut found_ids: Vec<String> = found.iter().map(|r| r.id.clone()).collect();
        found_ids.sort();
        assert_eq!(found_ids, vec!["rl1", "rl2"]);

        // authorized filter shrinks members of the containing lists
        let found = dao
            .find_all_containing_book_id("b2", Some(&lib1), &ContentRestrictions::default())
            .unwrap();
        let rl1 = found.iter().find(|r| r.id == "rl1").unwrap();
        assert_eq!(rl1.book_ids, BTreeMap::from([(0, "b1".to_string())]));
        assert!(rl1.filtered);
    }

    #[test]
    fn find_all_search_returns_nothing_before_m6() {
        let db = db();
        let dao = ReadListDtoDao::new(db.clone());
        seed_readlist(&db, "rl1", "one", &[]);
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
}
