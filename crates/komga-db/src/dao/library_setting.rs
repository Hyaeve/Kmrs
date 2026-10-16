//! Per-library kmrs-only settings in the separate `kmrs.sqlite` database.

use crate::pool::Database;
use crate::Result;
use komga_core::time_codec;
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LibrarySettings {
    pub scan_strm: bool,
    pub real_time_scan: bool,
}

pub struct LibrarySettingDao {
    db: Database,
}

impl LibrarySettingDao {
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    pub fn get(&self, library_id: &str) -> Result<LibrarySettings> {
        let conn = self.db.ro()?;
        let mut stmt = conn.prepare(
            "SELECT SCAN_STRM, REAL_TIME_SCAN FROM LIBRARY_SETTING WHERE LIBRARY_ID = ?",
        )?;
        let mut rows = stmt.query([library_id])?;
        match rows.next()? {
            Some(row) => Ok(LibrarySettings {
                scan_strm: row.get(0)?,
                real_time_scan: row.get(1)?,
            }),
            None => Ok(LibrarySettings::default()),
        }
    }

    pub fn set(&self, library_id: &str, settings: LibrarySettings) -> Result<()> {
        if !settings.scan_strm && !settings.real_time_scan {
            return self.delete(library_id);
        }
        let now = time_codec::format_datetime(time_codec::now_utc());
        self.db.rw()?.execute(
            "INSERT INTO LIBRARY_SETTING \
             (LIBRARY_ID, SCAN_STRM, REAL_TIME_SCAN, CREATED_DATE, LAST_MODIFIED_DATE) \
             VALUES (?, ?, ?, ?, ?) \
             ON CONFLICT (LIBRARY_ID) DO UPDATE SET \
                 SCAN_STRM = excluded.SCAN_STRM, \
                 REAL_TIME_SCAN = excluded.REAL_TIME_SCAN, \
                 LAST_MODIFIED_DATE = excluded.LAST_MODIFIED_DATE",
            rusqlite::params![
                library_id,
                settings.scan_strm,
                settings.real_time_scan,
                now,
                now
            ],
        )?;
        Ok(())
    }

    pub fn delete(&self, library_id: &str) -> Result<()> {
        self.db.rw()?.execute(
            "DELETE FROM LIBRARY_SETTING WHERE LIBRARY_ID = ?",
            [library_id],
        )?;
        Ok(())
    }

    pub fn scan_strm_library_ids(&self) -> Result<BTreeSet<String>> {
        let conn = self.db.ro()?;
        let mut stmt =
            conn.prepare("SELECT LIBRARY_ID FROM LIBRARY_SETTING WHERE SCAN_STRM = 1")?;
        let rows = stmt.query_map([], |row| row.get(0))?;
        Ok(rows.collect::<std::result::Result<BTreeSet<_>, _>>()?)
    }

    pub fn real_time_scan_library_ids(&self) -> Result<BTreeSet<String>> {
        let conn = self.db.ro()?;
        let mut stmt =
            conn.prepare("SELECT LIBRARY_ID FROM LIBRARY_SETTING WHERE REAL_TIME_SCAN = 1")?;
        let rows = stmt.query_map([], |row| row.get(0))?;
        Ok(rows.collect::<std::result::Result<BTreeSet<_>, _>>()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Migrator, Placeholders};

    fn test_dao() -> LibrarySettingDao {
        let db = Database::open_in_memory(false).unwrap();
        Migrator::new(&crate::kmrs_migrations(), Placeholders::default())
            .migrate(&db.rw().unwrap())
            .unwrap();
        LibrarySettingDao::new(db)
    }

    #[test]
    fn defaults_false_and_round_trips_enabled_setting() {
        let dao = test_dao();
        assert_eq!(dao.get("l1").unwrap(), LibrarySettings::default());
        assert!(dao.scan_strm_library_ids().unwrap().is_empty());

        dao.set(
            "l1",
            LibrarySettings {
                scan_strm: true,
                real_time_scan: false,
            },
        )
        .unwrap();
        assert_eq!(
            dao.get("l1").unwrap(),
            LibrarySettings {
                scan_strm: true,
                real_time_scan: false,
            }
        );
        assert_eq!(
            dao.scan_strm_library_ids().unwrap(),
            ["l1".to_string()].into_iter().collect()
        );
    }

    #[test]
    fn disabling_setting_removes_row() {
        let dao = test_dao();
        dao.set(
            "l1",
            LibrarySettings {
                scan_strm: true,
                real_time_scan: false,
            },
        )
        .unwrap();
        dao.set(
            "l1",
            LibrarySettings {
                scan_strm: false,
                real_time_scan: false,
            },
        )
        .unwrap();
        assert_eq!(dao.get("l1").unwrap(), LibrarySettings::default());
        assert!(dao.scan_strm_library_ids().unwrap().is_empty());
    }

    #[test]
    fn round_trips_real_time_scan() {
        let dao = test_dao();
        dao.set(
            "l1",
            LibrarySettings {
                scan_strm: true,
                real_time_scan: true,
            },
        )
        .unwrap();
        assert_eq!(
            dao.get("l1").unwrap(),
            LibrarySettings {
                scan_strm: true,
                real_time_scan: true,
            }
        );
        assert_eq!(
            dao.real_time_scan_library_ids().unwrap(),
            ["l1".to_string()].into_iter().collect()
        );
    }
}
