//! `TasksDao.kt`: the TASK table of `tasks.sqlite` (priority queue with group exclusion).
//!
//! Queue semantics:
//! - `save` upserts by task `uniqueId` (ID), refreshing priority/group/payload/last-modified.
//! - `takeFirst` picks the highest-priority, oldest unowned task whose GROUP_ID is not owned by
//!   another worker, and claims it (`OWNER = ?`).
//! - `disown` resets all owners at startup (tasks from a previous run become available again).

use crate::error::Result;
use crate::pool::Database;
use komga_core::task::Task;
use komga_core::time_codec;
use rusqlite::params;
use std::collections::BTreeMap;

/// `tasksAvailableCondition`: unowned, and no other task of the same group is currently owned.
const AVAILABLE: &str = "OWNER IS NULL AND (GROUP_ID NOT IN \
  (SELECT GROUP_ID FROM TASK WHERE OWNER IS NOT NULL AND GROUP_ID IS NOT NULL) OR GROUP_ID IS NULL)";

pub struct TasksDao {
    db: Database,
}

impl TasksDao {
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    pub fn has_available(&self) -> Result<bool> {
        let conn = self.db.ro();
        let exists: bool = conn.query_row(
            &format!("SELECT EXISTS(SELECT 1 FROM TASK WHERE {AVAILABLE})"),
            [],
            |r| r.get(0),
        )?;
        Ok(exists)
    }

    /// Claims the first available task, or None when the queue has nothing runnable.
    /// The read-write pool is a single connection, so the select+update is inherently serialized.
    pub fn take_first(&self, owner: &str) -> Result<Option<Task>> {
        let conn = self.db.rw();
        let task = {
            let mut stmt = conn.prepare(&format!(
                "SELECT CLASS, PAYLOAD FROM TASK WHERE {AVAILABLE} \
                 ORDER BY PRIORITY DESC, LAST_MODIFIED_DATE ASC LIMIT 1"
            ))?;
            let mut rows = stmt.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?;
            match rows.next().transpose()? {
                Some((class, payload)) => match Task::from_payload(&class, &payload) {
                    Some(task) => task,
                    None => {
                        tracing::error!("Could not deserialize object of type: {class}");
                        return Ok(None);
                    }
                },
                None => return Ok(None),
            }
        };
        conn.execute(
            "UPDATE TASK SET OWNER = ? WHERE ID = ?",
            params![owner, task.unique_id()],
        )?;
        Ok(Some(task))
    }

    pub fn find_all(&self) -> Result<Vec<Task>> {
        let conn = self.db.ro();
        let mut stmt = conn.prepare("SELECT CLASS, PAYLOAD FROM TASK")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut tasks = vec![];
        for row in rows {
            let (class, payload) = row?;
            match Task::from_payload(&class, &payload) {
                Some(task) => tasks.push(task),
                None => tracing::error!("Could not deserialize object of type: {class}"),
            }
        }
        Ok(tasks)
    }

    pub fn find_all_grouped_by_owner(&self) -> Result<BTreeMap<Option<String>, Vec<Task>>> {
        let conn = self.db.ro();
        let mut stmt = conn.prepare("SELECT OWNER, CLASS, PAYLOAD FROM TASK")?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, Option<String>>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        let mut map: BTreeMap<Option<String>, Vec<Task>> = BTreeMap::new();
        for row in rows {
            let (owner, class, payload) = row?;
            match Task::from_payload(&class, &payload) {
                Some(task) => map.entry(owner).or_default().push(task),
                None => tracing::error!("Could not deserialize object of type: {class}"),
            }
        }
        Ok(map)
    }

    pub fn count(&self) -> Result<i64> {
        let conn = self.db.ro();
        let count = conn.query_row("SELECT COUNT(*) FROM TASK", [], |r| r.get(0))?;
        Ok(count)
    }

    pub fn count_by_simple_type(&self) -> Result<BTreeMap<String, i64>> {
        let conn = self.db.ro();
        let mut stmt =
            conn.prepare("SELECT SIMPLE_TYPE, COUNT(SIMPLE_TYPE) FROM TASK GROUP BY SIMPLE_TYPE")?;
        let map = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?
            .collect::<std::result::Result<BTreeMap<_, _>, _>>()?;
        Ok(map)
    }

    pub fn save(&self, task: &Task) -> Result<()> {
        self.save_many(std::slice::from_ref(task))
    }

    /// Upsert by unique ID (jOOQ `onDuplicateKeyUpdate`): LAST_MODIFIED_DATE moves to now.
    pub fn save_many(&self, tasks: &[Task]) -> Result<()> {
        let conn = self.db.rw();
        let mut stmt = conn.prepare(
            "INSERT INTO TASK (ID, PRIORITY, GROUP_ID, CLASS, SIMPLE_TYPE, PAYLOAD) \
             VALUES (?, ?, ?, ?, ?, ?) \
             ON CONFLICT(ID) DO UPDATE SET \
             GROUP_ID = excluded.GROUP_ID, PRIORITY = excluded.PRIORITY, CLASS = excluded.CLASS, \
             SIMPLE_TYPE = excluded.SIMPLE_TYPE, PAYLOAD = excluded.PAYLOAD, \
             LAST_MODIFIED_DATE = ?",
        )?;
        for task in tasks {
            stmt.execute(params![
                task.unique_id(),
                task.priority(),
                task.group_id(),
                task.class_name(),
                task.simple_type(),
                task.to_payload().to_string(),
                time_codec::format_datetime(time_codec::now_utc()),
            ])?;
        }
        Ok(())
    }

    pub fn delete(&self, unique_id: &str) -> Result<()> {
        let conn = self.db.rw();
        conn.execute("DELETE FROM TASK WHERE ID = ?", [unique_id])?;
        Ok(())
    }

    pub fn delete_all(&self) -> Result<()> {
        let conn = self.db.rw();
        conn.execute("DELETE FROM TASK", [])?;
        Ok(())
    }

    pub fn delete_all_without_owner(&self) -> Result<i64> {
        let conn = self.db.rw();
        let n = conn.execute("DELETE FROM TASK WHERE OWNER IS NULL", [])?;
        Ok(n as i64)
    }

    pub fn disown(&self) -> Result<i64> {
        let conn = self.db.rw();
        let n = conn.execute("UPDATE TASK SET OWNER = NULL WHERE OWNER IS NOT NULL", [])?;
        Ok(n as i64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migrate::Migrator;
    use crate::{tasks_migrations, Placeholders};
    use komga_core::task::{BookTaskKind, DEFAULT_PRIORITY, HIGHEST_PRIORITY, LOWEST_PRIORITY};

    fn db() -> Database {
        let db = Database::open_in_memory(false).unwrap();
        let migrations = tasks_migrations();
        Migrator::new(&migrations, Placeholders::default())
            .migrate(&db.rw())
            .unwrap();
        db
    }

    fn dao() -> TasksDao {
        TasksDao::new(db())
    }

    #[test]
    fn save_upserts_by_unique_id() {
        let dao = dao();
        let task = Task::scan_library("lib1", false, DEFAULT_PRIORITY);
        dao.save(&task).unwrap();
        assert_eq!(dao.count().unwrap(), 1);

        // same unique id: payload is replaced, not duplicated
        let updated = Task::scan_library("lib1", false, HIGHEST_PRIORITY);
        dao.save(&updated).unwrap();
        assert_eq!(dao.count().unwrap(), 1);
        let all = dao.find_all().unwrap();
        assert_eq!(all[0].priority(), HIGHEST_PRIORITY);
    }

    #[test]
    fn take_first_orders_by_priority_then_last_modified() {
        let dao = dao();
        dao.save(&Task::scan_library("lib1", false, LOWEST_PRIORITY))
            .unwrap();
        dao.save(&Task::scan_library("lib2", false, HIGHEST_PRIORITY))
            .unwrap();
        dao.save(&Task::book(BookTaskKind::HashBook, "b1", DEFAULT_PRIORITY, None))
            .unwrap();

        let first = dao.take_first("worker-1").unwrap().unwrap();
        assert_eq!(first.unique_id(), "SCAN_LIBRARY_lib2_DEEP_false");
        // claimed now; DEFAULT_PRIORITY outranks LOWEST_PRIORITY
        let second = dao.take_first("worker-1").unwrap().unwrap();
        assert_eq!(second.unique_id(), "HASH_BOOK_b1");
        let third = dao.take_first("worker-1").unwrap().unwrap();
        assert_eq!(third.unique_id(), "SCAN_LIBRARY_lib1_DEEP_false");
        assert!(dao.take_first("worker-1").unwrap().is_none());
    }

    #[test]
    fn group_exclusion() {
        let dao = dao();
        dao.save(&Task::analyze_book("b1", DEFAULT_PRIORITY, "s1".into()))
            .unwrap();
        dao.save(&Task::analyze_book("b2", DEFAULT_PRIORITY, "s1".into()))
            .unwrap();
        dao.save(&Task::analyze_book("b3", DEFAULT_PRIORITY, "s2".into()))
            .unwrap();

        let first = dao.take_first("worker-1").unwrap().unwrap();
        assert_eq!(first.unique_id(), "ANALYZE_BOOK_b1");
        // b2 is in the same owned group: only b3 is available
        assert!(dao.has_available().unwrap());
        let second = dao.take_first("worker-2").unwrap().unwrap();
        assert_eq!(second.unique_id(), "ANALYZE_BOOK_b3");
        assert!(!dao.has_available().unwrap());
        assert!(dao.take_first("worker-3").unwrap().is_none());

        // finishing b1 frees the group
        dao.delete("ANALYZE_BOOK_b1").unwrap();
        assert!(dao.has_available().unwrap());
    }

    #[test]
    fn disown_resets_claimed_tasks() {
        let dao = dao();
        dao.save(&Task::scan_library("lib1", false, DEFAULT_PRIORITY))
            .unwrap();
        assert!(dao.take_first("worker-1").unwrap().is_some());
        assert_eq!(dao.disown().unwrap(), 1);
        assert!(dao.has_available().unwrap());
    }

    #[test]
    fn count_by_simple_type_and_owner_grouping() {
        let dao = dao();
        dao.save(&Task::scan_library("lib1", false, DEFAULT_PRIORITY))
            .unwrap();
        dao.save(&Task::book(BookTaskKind::HashBook, "b1", DEFAULT_PRIORITY, None))
            .unwrap();
        dao.save(&Task::book(BookTaskKind::HashBook, "b2", DEFAULT_PRIORITY, None))
            .unwrap();
        dao.take_first("worker-1").unwrap();

        let counts = dao.count_by_simple_type().unwrap();
        assert_eq!(counts.get("ScanLibrary"), Some(&1));
        assert_eq!(counts.get("HashBook"), Some(&2));

        let grouped = dao.find_all_grouped_by_owner().unwrap();
        assert_eq!(grouped.get(&Some("worker-1".to_string())).unwrap().len(), 1);
        assert_eq!(grouped.get(&None).unwrap().len(), 2);
    }
}
