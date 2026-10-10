//! The `TRACKER_LINK` table in the separate `kmrs.sqlite` database: which
//! platform entry (per provider) tracks a komga series for one user. Komga has
//! no tracker concept, so this mapping is kmrs-private state; the platform
//! side (OAuth tokens, ledger) lives in komf.

use crate::pool::Database;
use crate::Result;
use komga_core::time_codec;
use komga_core::tracker::TrackMode;
use rusqlite::Row;
use time::OffsetDateTime;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackerLink {
    pub series_id: String,
    pub user_id: String,
    pub provider: String,
    pub track_id: String,
    pub title: Option<String>,
    pub track_mode: TrackMode,
    pub chapter_offset: i32,
    pub created_date: OffsetDateTime,
    pub last_modified_date: OffsetDateTime,
}

/// A binding to create or replace an existing one for the same
/// (series, user, provider).
#[derive(Debug, Clone)]
pub struct NewTrackerLink<'a> {
    pub series_id: &'a str,
    pub user_id: &'a str,
    pub provider: &'a str,
    pub track_id: &'a str,
    pub title: Option<&'a str>,
    pub track_mode: TrackMode,
    pub chapter_offset: i32,
}

pub struct TrackerLinkDao {
    db: Database,
}

const COLUMNS: &str = "SERIES_ID, USER_ID, PROVIDER, TRACK_ID, TITLE, TRACK_MODE, \
                       CHAPTER_OFFSET, CREATED_DATE, LAST_MODIFIED_DATE";

fn track_mode_from_column(row: &Row<'_>, idx: usize) -> rusqlite::Result<TrackMode> {
    let s: String = row.get(idx)?;
    TrackMode::parse(&s).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            idx,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("invalid TRACK_MODE '{s}'"),
            )),
        )
    })
}

fn row_to_link(row: &Row<'_>) -> rusqlite::Result<TrackerLink> {
    Ok(TrackerLink {
        series_id: row.get(0)?,
        user_id: row.get(1)?,
        provider: row.get(2)?,
        track_id: row.get(3)?,
        title: row.get(4)?,
        track_mode: track_mode_from_column(row, 5)?,
        chapter_offset: row.get(6)?,
        created_date: super::get_datetime(row, 7)?,
        last_modified_date: super::get_datetime(row, 8)?,
    })
}

impl TrackerLinkDao {
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    pub fn list_by_series_and_user(
        &self,
        series_id: &str,
        user_id: &str,
    ) -> Result<Vec<TrackerLink>> {
        let conn = self.db.ro()?;
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLUMNS} FROM TRACKER_LINK WHERE SERIES_ID = ? AND USER_ID = ? \
             ORDER BY PROVIDER"
        ))?;
        let rows = stmt.query_map(rusqlite::params![series_id, user_id], row_to_link)?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn list_by_user(&self, user_id: &str) -> Result<Vec<TrackerLink>> {
        let conn = self.db.ro()?;
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLUMNS} FROM TRACKER_LINK WHERE USER_ID = ? ORDER BY SERIES_ID, PROVIDER"
        ))?;
        let rows = stmt.query_map(rusqlite::params![user_id], row_to_link)?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    /// Inserts the binding or replaces the target entry and options of an
    /// existing (series, user, provider) binding.
    pub fn upsert(&self, link: &NewTrackerLink<'_>) -> Result<()> {
        let now = time_codec::format_datetime(time_codec::now_utc());
        self.db.rw()?.execute(
            r#"
            INSERT INTO TRACKER_LINK
                (SERIES_ID, USER_ID, PROVIDER, TRACK_ID, TITLE, TRACK_MODE,
                 CHAPTER_OFFSET, CREATED_DATE, LAST_MODIFIED_DATE)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT (SERIES_ID, USER_ID, PROVIDER) DO UPDATE SET
                TRACK_ID = excluded.TRACK_ID,
                TITLE = excluded.TITLE,
                TRACK_MODE = excluded.TRACK_MODE,
                CHAPTER_OFFSET = excluded.CHAPTER_OFFSET,
                LAST_MODIFIED_DATE = excluded.LAST_MODIFIED_DATE
            "#,
            rusqlite::params![
                link.series_id,
                link.user_id,
                link.provider,
                link.track_id,
                link.title,
                link.track_mode.as_str(),
                link.chapter_offset,
                now,
                now,
            ],
        )?;
        Ok(())
    }

    /// Removes one binding; deleting a non-existent binding is a no-op.
    pub fn delete(&self, series_id: &str, user_id: &str, provider: &str) -> Result<()> {
        self.db.rw()?.execute(
            "DELETE FROM TRACKER_LINK WHERE SERIES_ID = ? AND USER_ID = ? AND PROVIDER = ?",
            rusqlite::params![series_id, user_id, provider],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Migrator;
    use crate::Placeholders;

    fn test_dao() -> TrackerLinkDao {
        let db = Database::open_in_memory(false).unwrap();
        Migrator::new(&crate::kmrs_migrations(), Placeholders::default())
            .migrate(&db.rw().unwrap())
            .unwrap();
        TrackerLinkDao::new(db)
    }

    fn link<'a>(
        series: &'a str,
        user: &'a str,
        provider: &'a str,
        track_id: &'a str,
    ) -> NewTrackerLink<'a> {
        NewTrackerLink {
            series_id: series,
            user_id: user,
            provider,
            track_id,
            title: None,
            track_mode: TrackMode::Auto,
            chapter_offset: 0,
        }
    }

    #[test]
    fn upsert_lists_and_replaces_per_user_provider() {
        let dao = test_dao();
        dao.upsert(&link("ser-1", "alice", "anilist", "42"))
            .unwrap();
        dao.upsert(&link("ser-1", "alice", "mal", "99")).unwrap();
        dao.upsert(&link("ser-1", "bob", "anilist", "7")).unwrap();

        let alice = dao.list_by_series_and_user("ser-1", "alice").unwrap();
        assert_eq!(alice.len(), 2);
        // one series can be tracked on every provider independently
        assert_eq!(
            alice
                .iter()
                .map(|l| l.provider.as_str())
                .collect::<Vec<_>>(),
            vec!["anilist", "mal"]
        );
        // per-user isolation
        assert_eq!(
            dao.list_by_series_and_user("ser-1", "bob").unwrap().len(),
            1
        );
        assert_eq!(
            dao.list_by_series_and_user("ser-2", "alice").unwrap(),
            vec![]
        );

        // replacing the same (series, user, provider) keeps one row
        dao.upsert(&NewTrackerLink {
            title: Some("New Title"),
            track_mode: TrackMode::Volume,
            chapter_offset: -3,
            ..link("ser-1", "alice", "anilist", "43")
        })
        .unwrap();
        let alice = dao.list_by_series_and_user("ser-1", "alice").unwrap();
        assert_eq!(alice.len(), 2);
        let anilist = alice.iter().find(|l| l.provider == "anilist").unwrap();
        assert_eq!(anilist.track_id, "43");
        assert_eq!(anilist.title.as_deref(), Some("New Title"));
        assert_eq!(anilist.track_mode, TrackMode::Volume);
        assert_eq!(anilist.chapter_offset, -3);
    }

    #[test]
    fn list_by_user_spans_series() {
        let dao = test_dao();
        dao.upsert(&link("ser-1", "alice", "anilist", "1")).unwrap();
        dao.upsert(&link("ser-2", "alice", "bangumi", "2")).unwrap();
        dao.upsert(&link("ser-1", "bob", "anilist", "3")).unwrap();
        let all = dao.list_by_user("alice").unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].series_id, "ser-1");
        assert_eq!(all[1].series_id, "ser-2");
    }

    #[test]
    fn delete_removes_only_the_target_binding() {
        let dao = test_dao();
        dao.upsert(&link("ser-1", "alice", "anilist", "1")).unwrap();
        dao.upsert(&link("ser-1", "alice", "mal", "2")).unwrap();

        dao.delete("ser-1", "alice", "anilist").unwrap();
        let rest = dao.list_by_series_and_user("ser-1", "alice").unwrap();
        assert_eq!(rest.len(), 1);
        assert_eq!(rest[0].provider, "mal");

        // deleting again is a no-op
        dao.delete("ser-1", "alice", "anilist").unwrap();
        assert_eq!(
            dao.list_by_series_and_user("ser-1", "alice").unwrap().len(),
            1
        );
    }
}
