//! DAO for HISTORICAL_EVENT + HISTORICAL_EVENT_PROPERTIES. Events are insert-only.

use super::{get_datetime, invalid_column};
use crate::error::Result;
use crate::pool::Database;
use komga_core::model::history::{HistoricalEvent, HistoricalEventType};
use komga_core::time_codec;
use komga_core::tsid::TsidFactory;
use rusqlite::params;
use std::collections::BTreeMap;

pub struct HistoricalEventDao {
    db: Database,
    tsid: TsidFactory,
}

impl HistoricalEventDao {
    pub fn new(db: Database) -> Self {
        Self {
            db,
            tsid: TsidFactory::new_random_node(),
        }
    }

    pub fn insert(&self, event: &HistoricalEvent) -> Result<String> {
        let conn = self.db.rw();
        let id = if event.id.is_empty() {
            self.tsid.create_string()
        } else {
            event.id.clone()
        };
        conn.execute(
      "INSERT INTO HISTORICAL_EVENT (ID, TYPE, BOOK_ID, SERIES_ID, TIMESTAMP) VALUES (?,?,?,?,?)",
      params![
        id,
        event.type_.as_str(),
        event.book_id,
        event.series_id,
        time_codec::format_datetime(event.timestamp),
      ],
    )?;
        for (key, value) in &event.properties {
            conn.execute(
                "INSERT INTO HISTORICAL_EVENT_PROPERTIES (ID, KEY, VALUE) VALUES (?,?,?)",
                params![id, key, value],
            )?;
        }
        Ok(id)
    }

    pub fn find_by_id(&self, id: &str) -> Result<Option<HistoricalEvent>> {
        let conn = self.db.ro();
        let event = {
            let mut stmt = conn.prepare(
                "SELECT ID, TYPE, BOOK_ID, SERIES_ID, TIMESTAMP FROM HISTORICAL_EVENT WHERE ID = ?",
            )?;
            let mut rows = stmt.query_map([id], |row| {
                let type_: String = row.get(1)?;
                Ok(HistoricalEvent {
                    id: row.get(0)?,
                    type_: HistoricalEventType::from_str(&type_)
                        .ok_or_else(|| invalid_column(1, "TYPE", &type_))?,
                    book_id: row.get(2)?,
                    series_id: row.get(3)?,
                    timestamp: get_datetime(row, 4)?,
                    properties: BTreeMap::new(), // filled in below
                })
            })?;
            rows.next().transpose()?
        };
        let Some(mut event) = event else {
            return Ok(None);
        };
        let mut stmt =
            conn.prepare("SELECT KEY, VALUE FROM HISTORICAL_EVENT_PROPERTIES WHERE ID = ?")?;
        let properties = stmt
            .query_map([id], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?
            .collect::<std::result::Result<BTreeMap<_, _>, _>>()?;
        event.properties = properties;
        Ok(Some(event))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migrate::Migrator;
    use crate::{main_migrations, Placeholders};
    use komga_core::time_codec::now_utc;

    fn dao() -> HistoricalEventDao {
        let db = Database::open_in_memory(true).unwrap();
        let migrations = main_migrations();
        Migrator::new(&migrations, Placeholders::default())
            .migrate(&db.rw())
            .unwrap();
        HistoricalEventDao::new(db)
    }

    #[test]
    fn insert_and_read_with_properties() {
        let dao = dao();
        let event = HistoricalEvent {
            id: String::new(),
            type_: HistoricalEventType::DuplicatePageDeleted,
            book_id: Some("b1".into()),
            series_id: Some("s1".into()),
            properties: BTreeMap::from([
                ("name".to_string(), "/l/s/b.cbz".to_string()),
                ("page number".to_string(), "3".to_string()),
            ]),
            timestamp: now_utc(),
        };
        let id = dao.insert(&event).unwrap();
        assert_eq!(id.len(), 13);

        let found = dao.find_by_id(&id).unwrap().unwrap();
        assert_eq!(found.type_, HistoricalEventType::DuplicatePageDeleted);
        assert_eq!(found.book_id.as_deref(), Some("b1"));
        assert_eq!(
            found.properties.get("page number").map(String::as_str),
            Some("3")
        );
        assert_eq!(found.properties.len(), 2);

        // event without properties
        let event2 = HistoricalEvent {
            id: String::new(),
            type_: HistoricalEventType::SeriesFolderDeleted,
            book_id: None,
            series_id: Some("s2".into()),
            properties: BTreeMap::new(),
            timestamp: now_utc(),
        };
        let id2 = dao.insert(&event2).unwrap();
        let found2 = dao.find_by_id(&id2).unwrap().unwrap();
        assert!(found2.properties.is_empty());
        assert!(found2.book_id.is_none());

        assert!(dao.find_by_id("nonexistent").unwrap().is_none());
    }
}
