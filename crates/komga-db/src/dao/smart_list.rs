//! DAO for the kmrs-only `SMART_LIST` + `SMART_LIST_SHARE` tables: user-owned persisted
//! search filters with a private/public/shared visibility scope.
//! Membership is never stored — evaluation happens against the main database on each
//! request, so only the filter document lives here.

use super::{get_datetime, invalid_column};
use crate::error::Result;
use crate::pool::Database;
use komga_core::model::smart_list::{SmartList, SmartListTarget, SmartListVisibility};
use komga_core::time_codec;
use komga_core::tsid::TsidFactory;
use rusqlite::{params, Row};

const COLUMNS: &str =
    "ID, NAME, SUMMARY, OWNER_USER_ID, TARGET, VISIBILITY, SEARCH_JSON, CREATED_DATE, LAST_MODIFIED_DATE";

pub struct SmartListDao {
    db: Database,
    tsid: TsidFactory,
}

fn row_to_smart_list(row: &Row<'_>) -> rusqlite::Result<SmartList> {
    let target: String = row.get(4)?;
    let visibility: String = row.get(5)?;
    Ok(SmartList {
        id: row.get(0)?,
        name: row.get(1)?,
        summary: row.get(2)?,
        owner_user_id: row.get(3)?,
        target: SmartListTarget::parse(&target)
            .ok_or_else(|| invalid_column(row, 4, "smart list target", &target))?,
        visibility: SmartListVisibility::parse(&visibility)
            .ok_or_else(|| invalid_column(row, 5, "smart list visibility", &visibility))?,
        search_json: row.get(6)?,
        created_date: get_datetime(row, 7)?,
        last_modified_date: get_datetime(row, 8)?,
    })
}

impl SmartListDao {
    pub fn new(db: Database) -> Self {
        Self {
            db,
            tsid: TsidFactory::new_random_node(),
        }
    }

    /// everything the user may see: own lists plus public ones plus lists shared with them
    pub fn find_visible_for_user(&self, user_id: &str) -> Result<Vec<SmartList>> {
        let conn = self.db.ro()?;
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLUMNS} FROM SMART_LIST WHERE OWNER_USER_ID = ? OR VISIBILITY = 'PUBLIC' \
             OR (VISIBILITY = 'SHARED' AND ID IN (SELECT SMART_LIST_ID FROM SMART_LIST_SHARE WHERE USER_ID = ?)) \
             ORDER BY NAME"
        ))?;
        let rows = stmt
            .query_map(rusqlite::params![user_id, user_id], row_to_smart_list)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// admin view: every list, optionally narrowed to one owner's
    pub fn find_all_admin(&self, owner_user_id: Option<&str>) -> Result<Vec<SmartList>> {
        let conn = self.db.ro()?;
        let (sql, param) = match owner_user_id {
            Some(owner) => (
                format!("SELECT {COLUMNS} FROM SMART_LIST WHERE OWNER_USER_ID = ? ORDER BY NAME"),
                Some(owner),
            ),
            None => (
                format!("SELECT {COLUMNS} FROM SMART_LIST ORDER BY NAME"),
                None,
            ),
        };
        let mut stmt = conn.prepare(&sql)?;
        let rows = match param {
            Some(p) => stmt.query_map([p], row_to_smart_list)?,
            None => stmt.query_map([], row_to_smart_list)?,
        };
        let rows = rows.collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// whether the list is shared with (not owned by) the given user
    pub fn is_shared_with(&self, smart_list_id: &str, user_id: &str) -> Result<bool> {
        let conn = self.db.ro()?;
        Ok(conn.query_row(
            "SELECT COUNT(*) > 0 FROM SMART_LIST_SHARE WHERE SMART_LIST_ID = ? AND USER_ID = ?",
            [smart_list_id, user_id],
            |r| r.get(0),
        )?)
    }

    /// replaces the share scope of a list as one transaction, never a partial scope
    pub fn set_shares(&self, smart_list_id: &str, user_ids: &[String]) -> Result<()> {
        let mut conn = self.db.rw()?;
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM SMART_LIST_SHARE WHERE SMART_LIST_ID = ?",
            [smart_list_id],
        )?;
        for user_id in user_ids {
            tx.execute(
                "INSERT INTO SMART_LIST_SHARE (SMART_LIST_ID, USER_ID) VALUES (?,?)",
                params![smart_list_id, user_id],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn find_share_targets(&self, smart_list_id: &str) -> Result<Vec<String>> {
        let conn = self.db.ro()?;
        let mut stmt = conn.prepare(
            "SELECT USER_ID FROM SMART_LIST_SHARE WHERE SMART_LIST_ID = ? ORDER BY USER_ID",
        )?;
        let rows = stmt
            .query_map([smart_list_id], |r| r.get(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn find_by_id(&self, id: &str) -> Result<Option<SmartList>> {
        let conn = self.db.ro()?;
        let mut stmt = conn.prepare(&format!("SELECT {COLUMNS} FROM SMART_LIST WHERE ID = ?"))?;
        let found = stmt
            .query_map([id], row_to_smart_list)?
            .next()
            .transpose()?;
        Ok(found)
    }

    /// Uniqueness is per owner: different users may reuse a name.
    pub fn exists_by_name(&self, owner_user_id: &str, name: &str) -> Result<bool> {
        let conn = self.db.ro()?;
        Ok(conn.query_row(
            "SELECT COUNT(*) > 0 FROM SMART_LIST WHERE OWNER_USER_ID = ? AND lower(NAME) = lower(?)",
            [owner_user_id, name],
            |r| r.get(0),
        )?)
    }

    pub fn insert(&self, smart_list: &SmartList) -> Result<String> {
        let conn = self.db.rw()?;
        let id = if smart_list.id.is_empty() {
            self.tsid.create_string()
        } else {
            smart_list.id.clone()
        };
        conn.execute(
            "INSERT INTO SMART_LIST (ID, NAME, SUMMARY, OWNER_USER_ID, TARGET, VISIBILITY, SEARCH_JSON) \
             VALUES (?,?,?,?,?,?,?)",
            params![
                id,
                smart_list.name,
                smart_list.summary,
                smart_list.owner_user_id,
                smart_list.target.as_str(),
                smart_list.visibility.as_str(),
                smart_list.search_json,
            ],
        )?;
        Ok(id)
    }

    pub fn update(&self, smart_list: &SmartList) -> Result<()> {
        let conn = self.db.rw()?;
        conn.execute(
            "UPDATE SMART_LIST SET NAME = ?, SUMMARY = ?, TARGET = ?, VISIBILITY = ?, SEARCH_JSON = ?, \
             LAST_MODIFIED_DATE = ? WHERE ID = ?",
            params![
                smart_list.name,
                smart_list.summary,
                smart_list.target.as_str(),
                smart_list.visibility.as_str(),
                smart_list.search_json,
                time_codec::format_datetime(time_codec::now_utc()),
                smart_list.id,
            ],
        )?;
        Ok(())
    }

    pub fn delete(&self, id: &str) -> Result<()> {
        let conn = self.db.rw()?;
        conn.execute("DELETE FROM SMART_LIST_SHARE WHERE SMART_LIST_ID = ?", [id])?;
        conn.execute("DELETE FROM SMART_LIST WHERE ID = ?", [id])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{kmrs_migrations, Migrator, Placeholders};

    fn dao() -> SmartListDao {
        let db = Database::open_in_memory(false).unwrap();
        Migrator::new(&kmrs_migrations(), Placeholders::default())
            .migrate(&db.rw().unwrap())
            .unwrap();
        SmartListDao::new(db)
    }

    fn sample(owner: &str, name: &str, target: SmartListTarget) -> SmartList {
        SmartList {
            id: String::new(),
            name: name.into(),
            summary: String::new(),
            owner_user_id: owner.into(),
            target,
            visibility: SmartListVisibility::Private,
            search_json: r#"{"condition": null}"#.into(),
            created_date: time_codec::now_utc(),
            last_modified_date: time_codec::now_utc(),
        }
    }

    #[test]
    fn crud_roundtrip() {
        let dao = dao();
        let id = dao
            .insert(&sample("u1", "Unread", SmartListTarget::Book))
            .unwrap();
        let found = dao.find_by_id(&id).unwrap().expect("not found");
        assert_eq!(found.name, "Unread");
        assert_eq!(found.owner_user_id, "u1");
        assert_eq!(found.target, SmartListTarget::Book);
        assert_eq!(found.visibility, SmartListVisibility::Private);
        assert_eq!(found.search_json, r#"{"condition": null}"#);
        assert_eq!(found.summary, "");

        let mut updated = found.clone();
        updated.name = "Unread manga".into();
        updated.summary = "ongoing only".into();
        updated.target = SmartListTarget::Series;
        updated.visibility = SmartListVisibility::Shared;
        updated.search_json = r#"{"fullTextSearch": "manga"}"#.into();
        dao.update(&updated).unwrap();
        let found = dao.find_by_id(&id).unwrap().unwrap();
        assert_eq!(found.name, "Unread manga");
        assert_eq!(found.summary, "ongoing only");
        assert_eq!(found.target, SmartListTarget::Series);
        assert_eq!(found.visibility, SmartListVisibility::Shared);
        assert_eq!(found.search_json, r#"{"fullTextSearch": "manga"}"#);

        dao.delete(&id).unwrap();
        assert!(dao.find_by_id(&id).unwrap().is_none());
    }

    #[test]
    fn name_uniqueness_is_per_owner() {
        let dao = dao();
        dao.insert(&sample("u1", "Favorites", SmartListTarget::Book))
            .unwrap();
        assert!(dao.exists_by_name("u1", "FAVORITES").unwrap());
        assert!(!dao.exists_by_name("u2", "Favorites").unwrap());

        dao.insert(&sample("u2", "Favorites", SmartListTarget::Book))
            .unwrap();
        assert_eq!(dao.find_all_admin(Some("u1")).unwrap().len(), 1);
        assert_eq!(dao.find_all_admin(Some("u2")).unwrap().len(), 1);
        assert_eq!(dao.find_all_admin(None).unwrap().len(), 2);
    }

    #[test]
    fn visibility_scopes_and_share_targets() {
        let dao = dao();
        let private_id = dao
            .insert(&sample("u1", "p", SmartListTarget::Book))
            .unwrap();
        let mut public = sample("u1", "pub", SmartListTarget::Book);
        public.visibility = SmartListVisibility::Public;
        let public_id = dao.insert(&public).unwrap();
        let mut shared = sample("u1", "sh", SmartListTarget::Book);
        shared.visibility = SmartListVisibility::Shared;
        let shared_id = dao.insert(&shared).unwrap();
        dao.set_shares(&shared_id, &["u2".to_string(), "u3".to_string()])
            .unwrap();

        // u2 sees the public and shared-with-them lists, but not the private one
        let visible = dao.find_visible_for_user("u2").unwrap();
        let ids: Vec<&str> = visible.iter().map(|l| l.id.as_str()).collect();
        assert!(!ids.contains(&private_id.as_str()));
        assert!(ids.contains(&public_id.as_str()));
        assert!(ids.contains(&shared_id.as_str()));

        assert!(dao.is_shared_with(&shared_id, "u2").unwrap());
        assert!(!dao.is_shared_with(&shared_id, "u4").unwrap());
        assert_eq!(dao.find_share_targets(&shared_id).unwrap(), ["u2", "u3"]);

        // narrowing the scope replaces it
        dao.set_shares(&shared_id, &["u4".to_string()]).unwrap();
        assert!(!dao.is_shared_with(&shared_id, "u2").unwrap());
        assert!(dao.is_shared_with(&shared_id, "u4").unwrap());

        // deleting the list cascades the shares
        dao.delete(&shared_id).unwrap();
        assert!(!dao.is_shared_with(&shared_id, "u4").unwrap());
    }
}
