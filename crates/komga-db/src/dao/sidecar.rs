//! DAO for SIDECAR (PK is the URL, no FK).

use super::get_datetime;
use crate::error::Result;
use crate::pool::Database;
use komga_core::model::sidecar::SidecarStored;
use komga_core::time_codec;
use rusqlite::{params, Row};

const COLUMNS: &str = "URL, PARENT_URL, LAST_MODIFIED_TIME, LIBRARY_ID";

pub struct SidecarDao {
    db: Database,
}

impl SidecarDao {
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    fn row_to_sidecar(row: &Row<'_>) -> rusqlite::Result<SidecarStored> {
        Ok(SidecarStored {
            url: row.get(0)?,
            parent_url: row.get(1)?,
            last_modified_time: get_datetime(row, 2)?,
            library_id: row.get(3)?,
        })
    }

    pub fn find_all(&self) -> Result<Vec<SidecarStored>> {
        let conn = self.db.ro();
        let mut stmt = conn.prepare(&format!("SELECT {COLUMNS} FROM SIDECAR"))?;
        let rows = stmt
            .query_map([], Self::row_to_sidecar)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Upsert by URL (corresponds to jOOQ `onDuplicateKeyUpdate`).
    pub fn save(&self, sidecar: &SidecarStored) -> Result<()> {
        let conn = self.db.rw();
        conn.execute(
            &format!(
                "INSERT INTO SIDECAR ({COLUMNS}) VALUES (?,?,?,?) \
         ON CONFLICT(URL) DO UPDATE SET PARENT_URL = excluded.PARENT_URL, \
         LAST_MODIFIED_TIME = excluded.LAST_MODIFIED_TIME, LIBRARY_ID = excluded.LIBRARY_ID"
            ),
            params![
                sidecar.url,
                sidecar.parent_url,
                time_codec::format_datetime(sidecar.last_modified_time),
                sidecar.library_id,
            ],
        )?;
        Ok(())
    }

    pub fn delete_by_library_id_and_urls(&self, library_id: &str, urls: &[String]) -> Result<()> {
        if urls.is_empty() {
            return Ok(());
        }
        let conn = self.db.rw();
        let mut stmt = conn.prepare("DELETE FROM SIDECAR WHERE LIBRARY_ID = ? AND URL = ?")?;
        for url in urls {
            stmt.execute(params![library_id, url])?;
        }
        Ok(())
    }

    pub fn delete_by_library_id(&self, library_id: &str) -> Result<()> {
        let conn = self.db.rw();
        conn.execute("DELETE FROM SIDECAR WHERE LIBRARY_ID = ?", [library_id])?;
        Ok(())
    }

    pub fn count_grouped_by_library_id(&self) -> Result<std::collections::HashMap<String, i64>> {
        let conn = self.db.ro();
        let mut stmt =
            conn.prepare("SELECT LIBRARY_ID, COUNT(*) FROM SIDECAR GROUP BY LIBRARY_ID")?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?
            .collect::<std::result::Result<std::collections::HashMap<_, _>, _>>()?;
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migrate::Migrator;
    use crate::{main_migrations, Placeholders};
    use komga_core::time_codec::now_utc;

    fn dao() -> SidecarDao {
        let db = Database::open_in_memory(true).unwrap();
        let migrations = main_migrations();
        Migrator::new(&migrations, Placeholders::default())
            .migrate(&db.rw())
            .unwrap();
        SidecarDao::new(db)
    }

    fn sample(url: &str, library_id: &str) -> SidecarStored {
        SidecarStored {
            url: url.into(),
            parent_url: "file:/l/s/".into(),
            last_modified_time: now_utc(),
            library_id: library_id.into(),
        }
    }

    #[test]
    fn save_upserts_by_url() {
        let dao = dao();
        dao.save(&sample("file:/l/s/cover.jpg", "lib1")).unwrap();
        dao.save(&sample("file:/l/s/series.json", "lib1")).unwrap();
        dao.save(&sample("file:/l2/cover.jpg", "lib2")).unwrap();
        assert_eq!(dao.find_all().unwrap().len(), 3);

        let mut updated = sample("file:/l/s/cover.jpg", "lib1");
        updated.parent_url = "file:/changed/".into();
        dao.save(&updated).unwrap();
        assert_eq!(dao.find_all().unwrap().len(), 3);
        assert!(dao
            .find_all()
            .unwrap()
            .iter()
            .any(|s| s.parent_url == "file:/changed/"));

        let counts = dao.count_grouped_by_library_id().unwrap();
        assert_eq!(counts.get("lib1"), Some(&2));
        assert_eq!(counts.get("lib2"), Some(&1));

        dao.delete_by_library_id_and_urls("lib1", &["file:/l/s/cover.jpg".to_string()])
            .unwrap();
        assert_eq!(dao.find_all().unwrap().len(), 2);
        dao.delete_by_library_id("lib2").unwrap();
        assert_eq!(dao.find_all().unwrap().len(), 1);
        dao.delete_by_library_id_and_urls("lib1", &[]).unwrap();
        assert_eq!(dao.find_all().unwrap().len(), 1);
    }
}
