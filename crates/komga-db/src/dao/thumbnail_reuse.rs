//! `THUMBNAIL_REUSE` rows in the separate `kmrs.sqlite` database: one-shot markers
//! granting a book's next `GenerateBookThumbnail` task permission to reuse (downscale)
//! its stored generated thumbnail. Only the user-initiated
//! `FindBookThumbnailsToRegenerate` expansion sets markers, so the analysis-derived
//! regeneration after a book file replacement always re-reads the file.

use crate::pool::Database;
use crate::Result;
use komga_core::time_codec;

pub struct ThumbnailReuseDao {
    db: Database,
}

impl ThumbnailReuseDao {
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    pub fn allow(&self, book_id: &str) -> Result<()> {
        self.db.rw()?.execute(
            "INSERT INTO THUMBNAIL_REUSE (BOOK_ID, CREATED_DATE) VALUES (?, ?) \
             ON CONFLICT (BOOK_ID) DO NOTHING",
            rusqlite::params![book_id, time_codec::format_datetime(time_codec::now_utc())],
        )?;
        Ok(())
    }

    /// Consumes the marker: returns true when reuse was allowed for this book.
    pub fn take(&self, book_id: &str) -> Result<bool> {
        let n = self
            .db
            .rw()?
            .execute("DELETE FROM THUMBNAIL_REUSE WHERE BOOK_ID = ?", [book_id])?;
        Ok(n > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Migrator, Placeholders};

    fn test_dao() -> ThumbnailReuseDao {
        let db = Database::open_in_memory(false).unwrap();
        Migrator::new(&crate::kmrs_migrations(), Placeholders::default())
            .migrate(&db.rw().unwrap())
            .unwrap();
        ThumbnailReuseDao::new(db)
    }

    #[test]
    fn take_consumes_the_marker_once() {
        let dao = test_dao();
        assert!(!dao.take("b1").unwrap());
        dao.allow("b1").unwrap();
        dao.allow("b1").unwrap();
        assert!(dao.take("b1").unwrap());
        assert!(!dao.take("b1").unwrap());
    }
}
