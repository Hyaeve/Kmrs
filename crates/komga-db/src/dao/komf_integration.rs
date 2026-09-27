//! The `KOMF_INTEGRATION` row in the separate `kmrs.sqlite` database: single-row state
//! of the komf integration (target URLs, minted key reference, provisioning outcome),
//! so reconnects and retries survive restarts without touching the main database.

use crate::pool::Database;
use crate::Result;
use komga_core::time_codec;
use rusqlite::Row;
use time::OffsetDateTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KomfIntegrationState {
    Pending,
    Connected,
    Error,
}

impl KomfIntegrationState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Connected => "connected",
            Self::Error => "error",
        }
    }

    fn from_column(row: &Row<'_>, idx: usize) -> rusqlite::Result<Self> {
        let s: String = row.get(idx)?;
        match s.as_str() {
            "pending" => Ok(Self::Pending),
            "connected" => Ok(Self::Connected),
            "error" => Ok(Self::Error),
            _ => Err(super::invalid_column(idx, "state", &s)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KomfIntegration {
    pub url: String,
    pub base_url: String,
    /// owner of the minted API key; set once provisioning succeeded at least once
    pub owner_user_id: Option<String>,
    pub api_key_id: Option<String>,
    pub state: KomfIntegrationState,
    pub last_error: Option<String>,
    pub created_date: OffsetDateTime,
    pub last_modified_date: OffsetDateTime,
}

pub struct KomfIntegrationDao {
    db: Database,
}

const COLUMNS: &str =
    "URL, BASE_URL, OWNER_USER_ID, API_KEY_ID, STATE, LAST_ERROR, CREATED_DATE, LAST_MODIFIED_DATE";

fn row_to_integration(row: &Row<'_>) -> rusqlite::Result<KomfIntegration> {
    Ok(KomfIntegration {
        url: row.get(0)?,
        base_url: row.get(1)?,
        owner_user_id: row.get(2)?,
        api_key_id: row.get(3)?,
        state: KomfIntegrationState::from_column(row, 4)?,
        last_error: row.get(5)?,
        created_date: super::get_datetime(row, 6)?,
        last_modified_date: super::get_datetime(row, 7)?,
    })
}

impl KomfIntegrationDao {
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    pub fn get(&self) -> Result<Option<KomfIntegration>> {
        let conn = self.db.ro();
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLUMNS} FROM KOMF_INTEGRATION WHERE ID = 1"
        ))?;
        let mut rows = stmt.query_map([], row_to_integration)?;
        Ok(rows.next().transpose()?)
    }

    /// Re-arms provisioning on (re)configuration. The minted key reference is kept so
    /// the next provision can revoke it before minting a fresh one.
    pub fn upsert(&self, url: &str, base_url: &str) -> Result<()> {
        let now = time_codec::format_datetime(time_codec::now_utc());
        self.db.rw().execute(
            r#"
            INSERT INTO KOMF_INTEGRATION (ID, URL, BASE_URL, STATE, CREATED_DATE, LAST_MODIFIED_DATE)
            VALUES (1, ?, ?, 'pending', ?, ?)
            ON CONFLICT (ID) DO UPDATE SET
                URL = excluded.URL,
                BASE_URL = excluded.BASE_URL,
                STATE = 'pending',
                LAST_ERROR = NULL,
                LAST_MODIFIED_DATE = excluded.LAST_MODIFIED_DATE
            "#,
            rusqlite::params![url, base_url, now, now],
        )?;
        Ok(())
    }

    pub fn mark_connected(&self, owner_user_id: &str, api_key_id: &str) -> Result<()> {
        self.db.rw().execute(
            "UPDATE KOMF_INTEGRATION \
             SET STATE = 'connected', LAST_ERROR = NULL, OWNER_USER_ID = ?, API_KEY_ID = ?, LAST_MODIFIED_DATE = ? \
             WHERE ID = 1",
            rusqlite::params![
                owner_user_id,
                api_key_id,
                time_codec::format_datetime(time_codec::now_utc())
            ],
        )?;
        Ok(())
    }

    pub fn mark_error(&self, last_error: &str) -> Result<()> {
        self.db.rw().execute(
            "UPDATE KOMF_INTEGRATION SET STATE = 'error', LAST_ERROR = ?, LAST_MODIFIED_DATE = ? WHERE ID = 1",
            rusqlite::params![
                last_error,
                time_codec::format_datetime(time_codec::now_utc())
            ],
        )?;
        Ok(())
    }

    pub fn delete(&self) -> Result<()> {
        self.db
            .rw()
            .execute("DELETE FROM KOMF_INTEGRATION WHERE ID = 1", [])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Migrator;
    use crate::Placeholders;

    fn test_dao() -> KomfIntegrationDao {
        let db = Database::open_in_memory(false).unwrap();
        Migrator::new(&crate::kmrs_migrations(), Placeholders::default())
            .migrate(&db.rw())
            .unwrap();
        KomfIntegrationDao::new(db)
    }

    #[test]
    fn upsert_creates_pending_row_and_reupsert_keeps_key_reference() {
        let dao = test_dao();
        assert!(dao.get().unwrap().is_none());

        dao.upsert("http://komf:8085", "http://kmrs:25600").unwrap();
        let row = dao.get().unwrap().unwrap();
        assert_eq!(row.url, "http://komf:8085");
        assert_eq!(row.base_url, "http://kmrs:25600");
        assert_eq!(row.state, KomfIntegrationState::Pending);
        assert_eq!(row.owner_user_id, None);
        assert_eq!(row.api_key_id, None);
        assert_eq!(row.last_error, None);

        dao.mark_connected("user-1", "key-1").unwrap();
        dao.upsert("http://komf2:8085", "http://kmrs2:25600")
            .unwrap();
        let row = dao.get().unwrap().unwrap();
        assert_eq!(row.url, "http://komf2:8085");
        assert_eq!(row.state, KomfIntegrationState::Pending);
        // the previous key reference survives so provision can revoke it
        assert_eq!(row.owner_user_id.as_deref(), Some("user-1"));
        assert_eq!(row.api_key_id.as_deref(), Some("key-1"));
    }

    #[test]
    fn mark_connected_backfills_owner_and_key() {
        let dao = test_dao();
        dao.upsert("http://komf:8085", "http://kmrs:25600").unwrap();
        dao.mark_error("boom").unwrap();
        dao.mark_connected("user-1", "key-1").unwrap();
        let row = dao.get().unwrap().unwrap();
        assert_eq!(row.state, KomfIntegrationState::Connected);
        assert_eq!(row.owner_user_id.as_deref(), Some("user-1"));
        assert_eq!(row.api_key_id.as_deref(), Some("key-1"));
        assert_eq!(row.last_error, None);
    }

    #[test]
    fn mark_error_records_message() {
        let dao = test_dao();
        dao.upsert("http://komf:8085", "http://kmrs:25600").unwrap();
        dao.mark_error("connection refused").unwrap();
        let row = dao.get().unwrap().unwrap();
        assert_eq!(row.state, KomfIntegrationState::Error);
        assert_eq!(row.last_error.as_deref(), Some("connection refused"));
    }

    #[test]
    fn delete_removes_the_row() {
        let dao = test_dao();
        dao.upsert("http://komf:8085", "http://kmrs:25600").unwrap();
        dao.delete().unwrap();
        assert!(dao.get().unwrap().is_none());
        // deleting twice is a no-op
        dao.delete().unwrap();
    }
}
