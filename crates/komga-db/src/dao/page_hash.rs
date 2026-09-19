//! DAO for PAGE_HASH + PAGE_HASH_THUMBNAIL.

use super::{get_datetime, invalid_column};
use crate::error::Result;
use crate::pool::Database;
use komga_core::model::page_hash::{PageHashAction, PageHashKnown};
use komga_core::time_codec;
use rusqlite::{params, Row};

const COLUMNS: &str = "HASH, SIZE, ACTION, DELETE_COUNT, CREATED_DATE, LAST_MODIFIED_DATE";

/// Row shape of `findMatchesByHash` (`PageHashMatch` domain model).
#[derive(Debug, Clone, PartialEq)]
pub struct PageHashMatchRow {
    pub book_id: String,
    pub url: String,
    /// 1-based (NUMBER + 1, as in the jOOQ mapping)
    pub page_number: i32,
    pub file_name: String,
    pub file_size: i64,
    pub media_type: String,
}

pub struct PageHashDao {
    db: Database,
}

impl PageHashDao {
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    fn row_to_known(row: &Row<'_>, match_count: i32) -> rusqlite::Result<PageHashKnown> {
        let action: String = row.get(2)?;
        Ok(PageHashKnown {
            hash: row.get(0)?,
            size: PageHashKnown::normalize_size(row.get(1)?),
            action: PageHashAction::from_str(&action)
                .ok_or_else(|| invalid_column(2, "ACTION", &action))?,
            delete_count: row.get(3)?,
            match_count,
            created_date: get_datetime(row, 4)?,
            last_modified_date: get_datetime(row, 5)?,
        })
    }

    pub fn find_known(&self, hash: &str) -> Result<Option<PageHashKnown>> {
        let conn = self.db.ro();
        let mut stmt = conn.prepare(&format!("SELECT {COLUMNS} FROM PAGE_HASH WHERE HASH = ?"))?;
        let mut rows = stmt.query_map([hash], |row| Self::row_to_known(row, 0))?;
        Ok(rows.next().transpose()?)
    }

    /// List of known hashes; match_count is the number of occurrences in MEDIA_PAGE
    /// (corresponds to jOOQ's leftJoin count).
    pub fn find_all_known(&self, actions: Option<&[PageHashAction]>) -> Result<Vec<PageHashKnown>> {
        let conn = self.db.ro();
        let filter = match actions {
            Some(actions) if !actions.is_empty() => format!(
                "WHERE ph.ACTION IN ({})",
                actions
                    .iter()
                    .map(|a| format!("'{}'", a.as_str()))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            _ => String::new(),
        };
        let sql = format!(
            "SELECT {cols}, COUNT(p.FILE_HASH) FROM PAGE_HASH ph \
       LEFT JOIN MEDIA_PAGE p ON ph.HASH = p.FILE_HASH \
       {filter} GROUP BY ph.HASH",
            cols = COLUMNS
                .split(',')
                .map(|c| format!("ph.{}", c.trim()))
                .collect::<Vec<_>>()
                .join(", "),
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt
            .query_map([], |row| {
                let known = Self::row_to_known(row, 0)?;
                let match_count: i64 = row.get(6)?;
                Ok((known, match_count as i32))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows
            .into_iter()
            .map(|(mut k, count)| {
                k.match_count = count;
                k
            })
            .collect())
    }

    /// Corresponds to the jOOQ insert: DELETE_COUNT and the dates use DB defaults.
    pub fn insert(&self, page_hash: &PageHashKnown, thumbnail: Option<&[u8]>) -> Result<()> {
        let conn = self.db.rw();
        conn.execute(
            "INSERT INTO PAGE_HASH (HASH, SIZE, ACTION) VALUES (?,?,?)",
            params![page_hash.hash, page_hash.size, page_hash.action.as_str()],
        )?;
        if let Some(thumbnail) = thumbnail {
            conn.execute(
                "INSERT INTO PAGE_HASH_THUMBNAIL (HASH, THUMBNAIL) VALUES (?,?)",
                params![page_hash.hash, thumbnail],
            )?;
        }
        Ok(())
    }

    /// Corresponds to the jOOQ update: LAST_MODIFIED_DATE is set to the current UTC time.
    pub fn update(&self, page_hash: &PageHashKnown) -> Result<()> {
        let conn = self.db.rw();
        conn.execute(
      "UPDATE PAGE_HASH SET ACTION = ?, SIZE = ?, DELETE_COUNT = ?, LAST_MODIFIED_DATE = ? WHERE HASH = ?",
      params![
        page_hash.action.as_str(),
        page_hash.size,
        page_hash.delete_count,
        time_codec::format_datetime(time_codec::now_utc()),
        page_hash.hash,
      ],
    )?;
        Ok(())
    }

    pub fn get_known_thumbnail(&self, hash: &str) -> Result<Option<Vec<u8>>> {
        let conn = self.db.ro();
        let mut stmt = conn.prepare("SELECT THUMBNAIL FROM PAGE_HASH_THUMBNAIL WHERE HASH = ?")?;
        let mut rows = stmt.query_map([hash], |r| r.get(0))?;
        Ok(rows.next().transpose()?)
    }

    /// Paged variant of `findAllKnown`, with the jOOQ sort mapping.
    pub fn find_all_known_paged(
        &self,
        actions: Option<&[PageHashAction]>,
        page: &crate::dto_dao::PageRequest,
    ) -> Result<crate::dto_dao::DtoPage<PageHashKnown>> {
        let filter = match actions {
            Some(actions) if !actions.is_empty() => format!(
                "WHERE ph.ACTION IN ({})",
                actions
                    .iter()
                    .map(|a| format!("'{}'", a.as_str()))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            _ => String::new(),
        };
        let count_sql = format!(
            "SELECT COUNT(*) FROM (SELECT ph.HASH FROM PAGE_HASH ph LEFT JOIN MEDIA_PAGE p ON ph.HASH = p.FILE_HASH {filter} GROUP BY ph.HASH)"
        );
        let conn = self.db.ro();
        let total: i64 = conn.query_row(&count_sql, [], |r| r.get(0))?;

        let order_sql = page
            .sort
            .iter()
            .filter_map(|o| {
                let expr = match o.property.as_str() {
                    "hash" => "ph.HASH",
                    "matchCount" => "count",
                    "deleteCount" => "ph.DELETE_COUNT",
                    "deleteSize" => "ph.SIZE * ph.DELETE_COUNT",
                    "fileSize" | "size" => "ph.SIZE",
                    "createdDate" | "created" => "ph.CREATED_DATE",
                    "lastModifiedDate" | "lastModified" => "ph.LAST_MODIFIED_DATE",
                    _ => return None,
                };
                Some(format!(
                    "{expr} {}",
                    if o.descending { "DESC" } else { "ASC" }
                ))
            })
            .collect::<Vec<_>>();

        let cols = COLUMNS
            .split(',')
            .map(|c| format!("ph.{}", c.trim()))
            .collect::<Vec<_>>()
            .join(", ");
        let mut sql = format!(
            "SELECT {cols}, COUNT(p.FILE_HASH) AS count FROM PAGE_HASH ph LEFT JOIN MEDIA_PAGE p ON ph.HASH = p.FILE_HASH {filter} GROUP BY ph.HASH"
        );
        if !order_sql.is_empty() {
            sql.push_str(&format!(" ORDER BY {}", order_sql.join(", ")));
        }
        if !page.unpaged {
            sql.push_str(&format!(" LIMIT {} OFFSET {}", page.size, page.offset()));
        }
        let mut stmt = conn.prepare(&sql)?;
        let items = stmt
            .query_map([], |row| {
                let known = Self::row_to_known(row, 0)?;
                let match_count: i64 = row.get(6)?;
                Ok((known, match_count as i32))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?
            .into_iter()
            .map(|(mut k, count)| {
                k.match_count = count;
                k
            })
            .collect();
        Ok(crate::dto_dao::DtoPage {
            items,
            total,
            sorted: !order_sql.is_empty(),
        })
    }

    /// `findAllUnknown`: hashes seen more than once in MEDIA_PAGE and not yet registered in PAGE_HASH.
    pub fn find_all_unknown_paged(
        &self,
        page: &crate::dto_dao::PageRequest,
    ) -> Result<crate::dto_dao::DtoPage<komga_core::model::page_hash::PageHashUnknown>> {
        let conn = self.db.ro();
        let total: i64 = conn.query_row(
            "SELECT COUNT(*) FROM (SELECT FILE_HASH FROM MEDIA_PAGE WHERE FILE_HASH != '' AND NOT EXISTS (SELECT 1 FROM PAGE_HASH WHERE PAGE_HASH.HASH = MEDIA_PAGE.FILE_HASH) GROUP BY FILE_HASH HAVING COUNT(BOOK_ID) > 1)",
            [],
            |r| r.get(0),
        )?;

        let order_sql = page
            .sort
            .iter()
            .filter_map(|o| {
                let expr = match o.property.as_str() {
                    "hash" => "p.FILE_HASH",
                    "fileSize" | "size" => "p.FILE_SIZE",
                    "matchCount" => "count",
                    "totalSize" => "totalSize",
                    "url" => "b.URL",
                    "bookId" => "b.ID",
                    "pageNumber" => "p.NUMBER",
                    _ => return None,
                };
                Some(format!(
                    "{expr} {}",
                    if o.descending { "DESC" } else { "ASC" }
                ))
            })
            .collect::<Vec<_>>();

        let mut sql = String::from(
            "SELECT p.FILE_HASH, p.FILE_SIZE, COUNT(p.BOOK_ID) AS count, COUNT(p.BOOK_ID) * p.FILE_SIZE AS totalSize \
             FROM MEDIA_PAGE p WHERE p.FILE_HASH != '' \
             AND NOT EXISTS (SELECT 1 FROM PAGE_HASH ph WHERE ph.HASH = p.FILE_HASH) \
             GROUP BY p.FILE_HASH HAVING COUNT(p.BOOK_ID) > 1",
        );
        if !order_sql.is_empty() {
            sql.push_str(&format!(" ORDER BY {}", order_sql.join(", ")));
        }
        if !page.unpaged {
            sql.push_str(&format!(" LIMIT {} OFFSET {}", page.size, page.offset()));
        }
        let mut stmt = conn.prepare(&sql)?;
        let items = stmt
            .query_map([], |row| {
                Ok(komga_core::model::page_hash::PageHashUnknown {
                    hash: row.get(0)?,
                    size: komga_core::model::page_hash::PageHashKnown::normalize_size(row.get(1)?),
                    match_count: row.get::<_, i64>(2)? as i32,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(crate::dto_dao::DtoPage {
            items,
            total,
            sorted: !order_sql.is_empty(),
        })
    }

    /// `findMatchesByHash`: every book page carrying this hash.
    pub fn find_matches_by_hash_paged(
        &self,
        hash: &str,
        page: &crate::dto_dao::PageRequest,
    ) -> Result<crate::dto_dao::DtoPage<PageHashMatchRow>> {
        let conn = self.db.ro();
        let total: i64 = conn.query_row(
            "SELECT COUNT(*) FROM MEDIA_PAGE p WHERE p.FILE_HASH = ?",
            [hash],
            |r| r.get(0),
        )?;

        let order_sql = page
            .sort
            .iter()
            .filter_map(|o| {
                let expr = match o.property.as_str() {
                    "hash" => "p.FILE_HASH",
                    "fileSize" | "size" => "p.FILE_SIZE",
                    "matchCount" => "count",
                    "totalSize" => "totalSize",
                    "url" => "b.URL",
                    "bookId" => "b.ID",
                    "pageNumber" => "p.NUMBER",
                    _ => return None,
                };
                Some(format!(
                    "{expr} {}",
                    if o.descending { "DESC" } else { "ASC" }
                ))
            })
            .collect::<Vec<_>>();

        let mut sql = String::from(
            "SELECT p.BOOK_ID, b.URL, p.NUMBER, p.FILE_NAME, p.FILE_SIZE, p.MEDIA_TYPE \
             FROM MEDIA_PAGE p LEFT JOIN BOOK b ON p.BOOK_ID = b.ID WHERE p.FILE_HASH = ?",
        );
        if !order_sql.is_empty() {
            sql.push_str(&format!(" ORDER BY {}", order_sql.join(", ")));
        }
        if !page.unpaged {
            sql.push_str(&format!(" LIMIT {} OFFSET {}", page.size, page.offset()));
        }
        let mut stmt = conn.prepare(&sql)?;
        let items = stmt
            .query_map([hash], |row| {
                Ok(PageHashMatchRow {
                    book_id: row.get(0)?,
                    url: row.get(1)?,
                    page_number: row.get::<_, i32>(2)? + 1,
                    file_name: row.get(3)?,
                    file_size: row.get(4)?,
                    media_type: row.get(5)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(crate::dto_dao::DtoPage {
            items,
            total,
            sorted: !order_sql.is_empty(),
        })
    }

    /// `findMatchesByKnownHashAction`: pages whose hash is registered with one of the given actions,
    /// grouped by book id.
    pub fn find_matches_by_known_hash_action(
        &self,
        actions: &[PageHashAction],
        library_id: Option<&str>,
    ) -> Result<std::collections::BTreeMap<String, Vec<komga_core::task::BookPageNumbered>>> {
        if actions.is_empty() {
            return Ok(std::collections::BTreeMap::new());
        }
        let action_list = actions
            .iter()
            .map(|a| format!("'{}'", a.as_str()))
            .collect::<Vec<_>>()
            .join(", ");
        let mut sql =
            "SELECT p.BOOK_ID, p.FILE_NAME, p.NUMBER, p.FILE_HASH, p.MEDIA_TYPE, p.FILE_SIZE \
             FROM MEDIA_PAGE p INNER JOIN PAGE_HASH ph ON p.FILE_HASH = ph.HASH"
                .to_string();
        if library_id.is_some() {
            sql.push_str(" INNER JOIN BOOK b ON b.ID = p.BOOK_ID");
        }
        sql.push_str(&format!(" WHERE ph.ACTION IN ({action_list})"));
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![];
        if let Some(library_id) = library_id {
            sql.push_str(" AND b.LIBRARY_ID = ?");
            params.push(Box::new(library_id.to_string()));
        }
        let conn = self.db.ro();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(params), |row| {
            Ok((
                row.get::<_, String>(0)?,
                komga_core::task::BookPageNumbered {
                    file_name: row.get(1)?,
                    page_number: row.get::<_, i32>(2)? + 1,
                    file_hash: row.get(3)?,
                    media_type: row.get(4)?,
                    file_size: row.get(5)?,
                    width: None,
                    height: None,
                },
            ))
        })?;
        let mut map: std::collections::BTreeMap<String, Vec<komga_core::task::BookPageNumbered>> =
            std::collections::BTreeMap::new();
        for row in rows {
            let (book_id, page) = row?;
            map.entry(book_id).or_default().push(page);
        }
        Ok(map)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migrate::Migrator;
    use crate::{main_migrations, Placeholders};

    fn dao() -> PageHashDao {
        let db = Database::open_in_memory(true).unwrap();
        let migrations = main_migrations();
        Migrator::new(&migrations, Placeholders::default())
            .migrate(&db.rw())
            .unwrap();
        PageHashDao::new(db)
    }

    fn sample(hash: &str, action: PageHashAction) -> PageHashKnown {
        PageHashKnown {
            hash: hash.into(),
            size: Some(1024),
            action,
            delete_count: 0,
            match_count: 0,
            created_date: time_codec::now_utc(),
            last_modified_date: time_codec::now_utc(),
        }
    }

    #[test]
    fn crud_and_thumbnail() {
        let dao = dao();
        dao.insert(&sample("h1", PageHashAction::DeleteAuto), Some(&[1, 2, 3]))
            .unwrap();
        dao.insert(&sample("h2", PageHashAction::Ignore), None)
            .unwrap();

        let found = dao.find_known("h1").unwrap().unwrap();
        assert_eq!(found.action, PageHashAction::DeleteAuto);
        assert_eq!(found.size, Some(1024));
        assert_eq!(found.delete_count, 0);

        assert_eq!(dao.get_known_thumbnail("h1").unwrap(), Some(vec![1, 2, 3]));
        assert_eq!(dao.get_known_thumbnail("h2").unwrap(), None);

        let all = dao.find_all_known(None).unwrap();
        assert_eq!(all.len(), 2);
        let filtered = dao
            .find_all_known(Some(&[PageHashAction::DeleteAuto]))
            .unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].hash, "h1");

        let mut updated = sample("h1", PageHashAction::DeleteManual);
        updated.delete_count = 5;
        updated.size = Some(-1); // negative values normalize to None
        dao.update(&updated).unwrap();
        let found = dao.find_known("h1").unwrap().unwrap();
        assert_eq!(found.action, PageHashAction::DeleteManual);
        assert_eq!(found.delete_count, 5);
        assert_eq!(found.size, None);

        assert!(dao.find_known("nope").unwrap().is_none());
    }
}
