//! The `TRACKER_PREFERENCES` table in the separate `kmrs.sqlite` database:
//! per-user display preferences for the tracker module.
//! - `LIBRARY_IDS`: JSON array of library ids; empty means "every library".
//! - `DEFAULT_TRACKER`: the provider preselected in the bind dialog, if any.

use crate::pool::Database;
use crate::Result;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrackerPreferences {
    pub libraries: Vec<String>,
    pub default_tracker: Option<String>,
}

pub struct TrackerPreferencesDao {
    db: Database,
}

impl TrackerPreferencesDao {
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    pub fn get(&self, user_id: &str) -> Result<TrackerPreferences> {
        let conn = self.db.ro()?;
        let row: Option<(String, Option<String>)> = conn
            .query_row(
                "SELECT LIBRARY_IDS, DEFAULT_TRACKER FROM TRACKER_PREFERENCES WHERE USER_ID = ?",
                rusqlite::params![user_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .ok();
        let (libraries, default) = row.unwrap_or_default();
        Ok(TrackerPreferences {
            libraries: serde_json::from_str(&libraries).unwrap_or_default(),
            default_tracker: default,
        })
    }

    pub fn set(&self, user_id: &str, prefs: &TrackerPreferences) -> Result<()> {
        let libraries =
            serde_json::to_string(&prefs.libraries).unwrap_or_else(|_| "[]".to_string());
        let now = komga_core::time_codec::format_datetime(komga_core::time_codec::now_utc());
        self.db.rw()?.execute(
            r#"
            INSERT INTO TRACKER_PREFERENCES
                (USER_ID, LIBRARY_IDS, DEFAULT_TRACKER, LAST_MODIFIED_DATE)
            VALUES (?, ?, ?, ?)
            ON CONFLICT (USER_ID) DO UPDATE SET
                LIBRARY_IDS = excluded.LIBRARY_IDS,
                DEFAULT_TRACKER = excluded.DEFAULT_TRACKER,
                LAST_MODIFIED_DATE = excluded.LAST_MODIFIED_DATE
            "#,
            rusqlite::params![user_id, libraries, prefs.default_tracker, now],
        )?;
        Ok(())
    }

    /// The libraries the tracker module shows in; empty = all libraries.
    pub fn get_libraries(&self, user_id: &str) -> Result<Vec<String>> {
        Ok(self.get(user_id)?.libraries)
    }

    pub fn set_libraries(&self, user_id: &str, library_ids: &[String]) -> Result<()> {
        let mut prefs = self.get(user_id)?;
        prefs.libraries = library_ids.to_vec();
        self.set(user_id, &prefs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Migrator;
    use crate::Placeholders;

    fn test_dao() -> TrackerPreferencesDao {
        let db = Database::open_in_memory(false).unwrap();
        Migrator::new(&crate::kmrs_migrations(), Placeholders::default())
            .migrate(&db.rw().unwrap())
            .unwrap();
        TrackerPreferencesDao::new(db)
    }

    #[test]
    fn empty_by_default_and_roundtrip() {
        let dao = test_dao();
        assert_eq!(
            dao.get("alice").unwrap(),
            TrackerPreferences {
                libraries: vec![],
                default_tracker: None,
            }
        );

        dao.set_libraries("alice", &["lib-1".into(), "lib-2".into()])
            .unwrap();
        assert_eq!(dao.get_libraries("alice").unwrap(), vec!["lib-1", "lib-2"]);

        // clearing back to empty restores the unrestricted default
        dao.set_libraries("alice", &[]).unwrap();
        assert_eq!(dao.get_libraries("alice").unwrap(), Vec::<String>::new());
    }

    #[test]
    fn preferences_are_per_user() {
        let dao = test_dao();
        dao.set_libraries("alice", &["lib-1".into()]).unwrap();
        dao.set_libraries("bob", &["lib-9".into()]).unwrap();
        assert_eq!(dao.get_libraries("alice").unwrap(), vec!["lib-1"]);
        assert_eq!(dao.get_libraries("bob").unwrap(), vec!["lib-9"]);
    }

    #[test]
    fn default_tracker_roundtrip_without_touching_libraries() {
        let dao = test_dao();
        dao.set_libraries("alice", &["lib-1".into()]).unwrap();
        dao.set(
            "alice",
            &TrackerPreferences {
                libraries: vec!["lib-1".into()],
                default_tracker: Some("bangumi".into()),
            },
        )
        .unwrap();
        let prefs = dao.get("alice").unwrap();
        assert_eq!(prefs.default_tracker.as_deref(), Some("bangumi"));
        // libraries survived the partial-form update
        assert_eq!(prefs.libraries, vec!["lib-1"]);
    }

    #[test]
    fn unknown_user_gets_empty_preferences() {
        let dao = test_dao();
        let prefs = dao.get("nobody").unwrap();
        assert_eq!(prefs, TrackerPreferences::default());
    }
}
