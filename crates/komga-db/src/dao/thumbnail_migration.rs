//! `THUMBNAIL_STORAGE_MIGRATION` rows in the separate `kmrs.sqlite` database:
//! per-kind completion markers for the one-time blob→file thumbnail migration, so an
//! interrupted migration resumes instead of restarting.

use crate::pool::Database;
use crate::Result;
use komga_core::time_codec;

pub struct ThumbnailMigrationDao {
    db: Database,
}

impl ThumbnailMigrationDao {
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    pub fn is_done(&self, kind: &str) -> Result<bool> {
        let n: i64 = self.db.ro().query_row(
            "SELECT COUNT(*) FROM THUMBNAIL_STORAGE_MIGRATION WHERE KIND = ?",
            [kind],
            |r| r.get(0),
        )?;
        Ok(n > 0)
    }

    pub fn mark_done(&self, kind: &str) -> Result<()> {
        self.db.rw().execute(
            "INSERT INTO THUMBNAIL_STORAGE_MIGRATION (KIND, COMPLETED_DATE) VALUES (?, ?) \
             ON CONFLICT (KIND) DO UPDATE SET COMPLETED_DATE = excluded.COMPLETED_DATE",
            rusqlite::params![kind, time_codec::format_datetime(time_codec::now_utc())],
        )?;
        Ok(())
    }

    pub fn clear(&self, kind: &str) -> Result<()> {
        self.db.rw().execute(
            "DELETE FROM THUMBNAIL_STORAGE_MIGRATION WHERE KIND = ?",
            [kind],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Migrator, Placeholders};

    fn test_dao() -> ThumbnailMigrationDao {
        let db = Database::open_in_memory(false).unwrap();
        Migrator::new(&crate::kmrs_migrations(), Placeholders::default())
            .migrate(&db.rw())
            .unwrap();
        ThumbnailMigrationDao::new(db)
    }

    #[test]
    fn mark_done_upserts_per_kind() {
        let dao = test_dao();
        assert!(!dao.is_done("book").unwrap());
        dao.mark_done("book").unwrap();
        assert!(dao.is_done("book").unwrap());
        assert!(!dao.is_done("series").unwrap());
        dao.mark_done("book").unwrap();
        assert!(dao.is_done("book").unwrap());
        dao.clear("book").unwrap();
        assert!(!dao.is_done("book").unwrap());
        // clearing an absent marker is a no-op
        dao.clear("book").unwrap();
    }
}
