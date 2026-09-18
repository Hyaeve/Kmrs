//! DAO for PAGE_HASH + PAGE_HASH_THUMBNAIL.

use super::{get_datetime, invalid_column};
use crate::error::Result;
use crate::pool::Database;
use komga_core::model::page_hash::{PageHashAction, PageHashKnown};
use komga_core::time_codec;
use rusqlite::{params, Row};

const COLUMNS: &str = "HASH, SIZE, ACTION, DELETE_COUNT, CREATED_DATE, LAST_MODIFIED_DATE";

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
