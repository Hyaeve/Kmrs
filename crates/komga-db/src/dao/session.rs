//! `JdbcIndexedSessionRepository` (plus komga's `JdbcSessionConfiguration` sqlite
//! upserts): the `SPRING_SESSION` tables of `sessions.sqlite`.
//!
//! Row semantics mirror the Java repository:
//! - `PRIMARY_ID` is a UUID distinct from `SESSION_ID` (Java generates both).
//! - `EXPIRY_TIME` is `LAST_ACCESS_TIME + MAX_INACTIVE_INTERVAL`, in millis; a
//!   negative interval never expires (`Long.MAX_VALUE`), and such rows are
//!   excluded from both single deletes and the expiry cleanup (the default
//!   queries' `MAX_INACTIVE_INTERVAL >= 0` guard). kmrs only writes positive
//!   intervals.
//! - the only attribute komga sessions carry is the security context; kmrs stores
//!   its JSON payload under the same `SPRING_SECURITY_CONTEXT` name. The bytes are
//!   kmrs-encoded (Java uses JDK serialization), so a session written by one
//!   implementation is unreadable by the other: both sides treat it as an unknown
//!   session and the user re-authenticates.
//! - `PRINCIPAL_NAME` holds the kmrs user id (Java holds the login email). kmrs
//!   invalidation and the actuator `?username=` filter both key off the user id and
//!   resolve the email lazily, exactly like the in-memory store, so the observable
//!   behavior is identical across backends.

use crate::error::Result;
use crate::pool::Database;
use rusqlite::params;

/// Attribute name komga sessions carry (Spring's security-context attribute).
pub const SECURITY_CONTEXT_ATTRIBUTE: &str = "SPRING_SECURITY_CONTEXT";

pub struct SessionRow {
    pub primary_id: String,
    pub id: String,
    pub created_at: i64,
    pub last_accessed_at: i64,
    pub max_inactive_secs: i64,
    pub principal: Option<String>,
    /// raw `SPRING_SECURITY_CONTEXT` bytes, if the session carries them.
    pub payload: Option<Vec<u8>>,
}

impl SessionRow {
    pub fn expiry_time(&self) -> i64 {
        if self.max_inactive_secs < 0 {
            i64::MAX
        } else {
            self.last_accessed_at
                .saturating_add(self.max_inactive_secs.saturating_mul(1000))
        }
    }

    pub fn is_expired(&self, now_millis: i64) -> bool {
        self.expiry_time() <= now_millis
    }
}

pub struct SessionDao {
    db: Database,
}

impl SessionDao {
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    /// Inserts a fresh session row plus its security-context attribute.
    pub fn insert(
        &self,
        primary_id: &str,
        id: &str,
        now_millis: i64,
        max_inactive_secs: i64,
        principal: &str,
        payload: &[u8],
    ) -> Result<()> {
        let conn = self.db.rw()?;
        let tx = conn.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO SPRING_SESSION \
             (PRIMARY_ID, SESSION_ID, CREATION_TIME, LAST_ACCESS_TIME, \
              MAX_INACTIVE_INTERVAL, EXPIRY_TIME, PRINCIPAL_NAME) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
            params![
                primary_id,
                id,
                now_millis,
                now_millis,
                max_inactive_secs,
                expiry_time(now_millis, max_inactive_secs),
                principal,
            ],
        )?;
        tx.execute(
            "INSERT INTO SPRING_SESSION_ATTRIBUTES \
             (SESSION_PRIMARY_ID, ATTRIBUTE_NAME, ATTRIBUTE_BYTES) \
             VALUES (?, ?, ?)",
            params![primary_id, SECURITY_CONTEXT_ATTRIBUTE, payload],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn find_by_id(&self, id: &str) -> Result<Option<SessionRow>> {
        let conn = self.db.ro()?;
        let mut stmt = conn.prepare(
            "SELECT S.PRIMARY_ID, S.SESSION_ID, S.CREATION_TIME, S.LAST_ACCESS_TIME, \
                    S.MAX_INACTIVE_INTERVAL, S.PRINCIPAL_NAME, SA.ATTRIBUTE_BYTES \
             FROM SPRING_SESSION S \
             LEFT JOIN SPRING_SESSION_ATTRIBUTES SA \
               ON S.PRIMARY_ID = SA.SESSION_PRIMARY_ID \
               AND SA.ATTRIBUTE_NAME = 'SPRING_SECURITY_CONTEXT' \
             WHERE S.SESSION_ID = ?",
        )?;
        let mut rows = stmt.query_map([id], map_row)?;
        match rows.next().transpose()? {
            Some(row) => Ok(Some(row)),
            None => Ok(None),
        }
    }

    /// Refreshes the sliding expiration after the session was accessed.
    pub fn touch(&self, id: &str, now_millis: i64, max_inactive_secs: i64) -> Result<()> {
        let conn = self.db.rw()?;
        conn.execute(
            "UPDATE SPRING_SESSION SET LAST_ACCESS_TIME = ?, EXPIRY_TIME = ? \
             WHERE SESSION_ID = ?",
            params![now_millis, expiry_time(now_millis, max_inactive_secs), id],
        )?;
        Ok(())
    }

    /// `DELETE_SESSION_QUERY`: immortal sessions (negative interval) survive.
    pub fn delete_by_id(&self, id: &str) -> Result<()> {
        let conn = self.db.rw()?;
        conn.execute(
            "DELETE FROM SPRING_SESSION WHERE SESSION_ID = ? AND MAX_INACTIVE_INTERVAL >= 0",
            [id],
        )?;
        Ok(())
    }

    /// Live sessions for the actuator listing, in a stable order.
    pub fn list_live(&self, now_millis: i64) -> Result<Vec<SessionRow>> {
        let conn = self.db.ro()?;
        let mut stmt = conn.prepare(
            "SELECT S.PRIMARY_ID, S.SESSION_ID, S.CREATION_TIME, S.LAST_ACCESS_TIME, \
                    S.MAX_INACTIVE_INTERVAL, S.PRINCIPAL_NAME, SA.ATTRIBUTE_BYTES \
             FROM SPRING_SESSION S \
             LEFT JOIN SPRING_SESSION_ATTRIBUTES SA \
               ON S.PRIMARY_ID = SA.SESSION_PRIMARY_ID \
               AND SA.ATTRIBUTE_NAME = 'SPRING_SECURITY_CONTEXT' \
             WHERE S.EXPIRY_TIME > ? \
             ORDER BY S.SESSION_ID",
        )?;
        let rows = stmt.query_map([now_millis], map_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    /// Hourly expired-session cleanup (`cleanup-cron: "0 0 * * * *"`).
    pub fn delete_expired(&self, now_millis: i64) -> Result<u64> {
        let conn = self.db.rw()?;
        let deleted = conn.execute(
            "DELETE FROM SPRING_SESSION WHERE EXPIRY_TIME < ?",
            [now_millis],
        )?;
        Ok(deleted as u64)
    }
}

fn expiry_time(now_millis: i64, max_inactive_secs: i64) -> i64 {
    if max_inactive_secs < 0 {
        i64::MAX
    } else {
        now_millis.saturating_add(max_inactive_secs.saturating_mul(1000))
    }
}

fn map_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SessionRow> {
    Ok(SessionRow {
        primary_id: row.get(0)?,
        id: row.get(1)?,
        created_at: row.get(2)?,
        last_accessed_at: row.get(3)?,
        max_inactive_secs: row.get(4)?,
        principal: row.get(5)?,
        payload: row.get(6)?,
    })
}
