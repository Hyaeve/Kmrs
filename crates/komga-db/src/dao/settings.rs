//! DAOs for SERVER_SETTINGS (`ServerSettingsDao`) and CLIENT_SETTINGS_GLOBAL/USER
//! (`ClientSettingsDtoDao`).

use crate::error::Result;
use crate::pool::Database;
use komga_core::model::settings::{ClientSettingGlobal, ClientSettingUser};
use rusqlite::params;

pub struct SettingsDao {
    db: Database,
}

impl SettingsDao {
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    // ---------- SERVER_SETTINGS ----------

    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let conn = self.db.ro();
        Ok(conn
            .query_row(
                "SELECT VALUE FROM SERVER_SETTINGS WHERE KEY = ?",
                [key],
                |r| r.get::<_, Option<String>>(0),
            )
            .ok()
            .flatten())
    }

    /// jOOQ String→Boolean conversion: '1'/'true' (case-insensitive) is true.
    pub fn get_setting_bool(&self, key: &str) -> Result<Option<bool>> {
        Ok(self
            .get_setting(key)?
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true")))
    }

    pub fn get_setting_i64(&self, key: &str) -> Result<Option<i64>> {
        Ok(self.get_setting(key)?.and_then(|v| v.parse().ok()))
    }

    /// upsert (`onDuplicateKeyUpdate`).
    pub fn save_setting(&self, key: &str, value: &str) -> Result<()> {
        let conn = self.db.rw();
        conn.execute(
            "INSERT INTO SERVER_SETTINGS (KEY, VALUE) VALUES (?, ?) \
       ON CONFLICT(KEY) DO UPDATE SET VALUE = excluded.VALUE",
            params![key, value],
        )?;
        Ok(())
    }

    /// Java `saveSetting(key, Boolean)` writes `value.toString()` → "true"/"false".
    pub fn save_setting_bool(&self, key: &str, value: bool) -> Result<()> {
        self.save_setting(key, if value { "true" } else { "false" })
    }

    pub fn save_setting_i64(&self, key: &str, value: i64) -> Result<()> {
        self.save_setting(key, &value.to_string())
    }

    pub fn delete_setting(&self, key: &str) -> Result<()> {
        let conn = self.db.rw();
        conn.execute("DELETE FROM SERVER_SETTINGS WHERE KEY = ?", [key])?;
        Ok(())
    }

    pub fn delete_all_settings(&self) -> Result<()> {
        let conn = self.db.rw();
        conn.execute("DELETE FROM SERVER_SETTINGS", [])?;
        Ok(())
    }

    // ---------- CLIENT_SETTINGS_GLOBAL ----------

    pub fn find_all_global(&self, only_unauthorized: bool) -> Result<Vec<ClientSettingGlobal>> {
        let conn = self.db.ro();
        let sql = if only_unauthorized {
            "SELECT KEY, VALUE, ALLOW_UNAUTHORIZED FROM CLIENT_SETTINGS_GLOBAL WHERE ALLOW_UNAUTHORIZED = 1"
        } else {
            "SELECT KEY, VALUE, ALLOW_UNAUTHORIZED FROM CLIENT_SETTINGS_GLOBAL"
        };
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt
            .query_map([], |r| {
                Ok(ClientSettingGlobal {
                    key: r.get(0)?,
                    value: r.get(1)?,
                    allow_unauthorized: r.get(2)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Note: same as Java — on conflict only VALUE is updated, ALLOW_UNAUTHORIZED is not.
    pub fn save_global(&self, key: &str, value: &str, allow_unauthorized: bool) -> Result<()> {
        let conn = self.db.rw();
        conn.execute(
            "INSERT INTO CLIENT_SETTINGS_GLOBAL (KEY, VALUE, ALLOW_UNAUTHORIZED) VALUES (?, ?, ?) \
       ON CONFLICT(KEY) DO UPDATE SET VALUE = excluded.VALUE",
            params![key, value, allow_unauthorized],
        )?;
        Ok(())
    }

    pub fn delete_global_by_keys(&self, keys: &[String]) -> Result<()> {
        if keys.is_empty() {
            return Ok(());
        }
        let conn = self.db.rw();
        let placeholders = keys.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        conn.execute(
            &format!("DELETE FROM CLIENT_SETTINGS_GLOBAL WHERE KEY IN ({placeholders})"),
            rusqlite::params_from_iter(keys),
        )?;
        Ok(())
    }

    // ---------- CLIENT_SETTINGS_USER ----------

    pub fn find_all_user(&self, user_id: &str) -> Result<Vec<ClientSettingUser>> {
        let conn = self.db.ro();
        let mut stmt =
            conn.prepare("SELECT USER_ID, KEY, VALUE FROM CLIENT_SETTINGS_USER WHERE USER_ID = ?")?;
        let rows = stmt
            .query_map([user_id], |r| {
                Ok(ClientSettingUser {
                    user_id: r.get(0)?,
                    key: r.get(1)?,
                    value: r.get(2)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn save_for_user(&self, user_id: &str, key: &str, value: &str) -> Result<()> {
        let conn = self.db.rw();
        conn.execute(
            "INSERT INTO CLIENT_SETTINGS_USER (USER_ID, KEY, VALUE) VALUES (?, ?, ?) \
       ON CONFLICT(KEY, USER_ID) DO UPDATE SET VALUE = excluded.VALUE",
            params![user_id, key, value],
        )?;
        Ok(())
    }

    pub fn delete_by_user_id_and_keys(&self, user_id: &str, keys: &[String]) -> Result<()> {
        if keys.is_empty() {
            return Ok(());
        }
        let conn = self.db.rw();
        let placeholders = keys.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let mut values: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(user_id.to_string())];
        for k in keys {
            values.push(Box::new(k.clone()));
        }
        conn.execute(
            &format!(
                "DELETE FROM CLIENT_SETTINGS_USER WHERE USER_ID = ? AND KEY IN ({placeholders})"
            ),
            rusqlite::params_from_iter(values),
        )?;
        Ok(())
    }

    pub fn delete_by_user_id(&self, user_id: &str) -> Result<()> {
        let conn = self.db.rw();
        conn.execute(
            "DELETE FROM CLIENT_SETTINGS_USER WHERE USER_ID = ?",
            [user_id],
        )?;
        Ok(())
    }

    pub fn delete_all_client_settings(&self) -> Result<()> {
        let conn = self.db.rw();
        conn.execute("DELETE FROM CLIENT_SETTINGS_GLOBAL", [])?;
        conn.execute("DELETE FROM CLIENT_SETTINGS_USER", [])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migrate::Migrator;
    use crate::{main_migrations, Placeholders};

    fn dao() -> SettingsDao {
        let db = Database::open_in_memory(true).unwrap();
        let migrations = main_migrations();
        Migrator::new(&migrations, Placeholders::default())
            .migrate(&db.rw())
            .unwrap();
        SettingsDao::new(db)
    }

    #[test]
    fn server_settings_kv() {
        let dao = dao();
        // initial keys inserted by the migration: boolean literals are stored as '1' via TEXT affinity
        assert_eq!(
            dao.get_setting("DELETE_EMPTY_COLLECTIONS").unwrap(),
            Some("1".into())
        );
        assert_eq!(
            dao.get_setting_bool("DELETE_EMPTY_COLLECTIONS").unwrap(),
            Some(true)
        );
        assert!(dao.get_setting("REMEMBER_ME_KEY").unwrap().is_some());
        assert_eq!(
            dao.get_setting_i64("REMEMBER_ME_DURATION").unwrap(),
            Some(365)
        );

        dao.save_setting("FOO", "bar").unwrap();
        assert_eq!(dao.get_setting("FOO").unwrap(), Some("bar".into()));
        dao.save_setting("FOO", "baz").unwrap();
        assert_eq!(dao.get_setting("FOO").unwrap(), Some("baz".into()));

        dao.save_setting_bool("FLAG", true).unwrap();
        assert_eq!(dao.get_setting_bool("FLAG").unwrap(), Some(true));
        dao.save_setting_i64("NUM", 42).unwrap();
        assert_eq!(dao.get_setting_i64("NUM").unwrap(), Some(42));

        dao.delete_setting("FOO").unwrap();
        assert_eq!(dao.get_setting("FOO").unwrap(), None);
    }

    #[test]
    fn client_settings_global_and_user() {
        let dao = dao();
        dao.save_global("k1", "v1", false).unwrap();
        dao.save_global("k2", "v2", true).unwrap();
        // on conflict only VALUE is updated; ALLOW_UNAUTHORIZED stays unchanged
        dao.save_global("k2", "v2b", false).unwrap();

        let all = dao.find_all_global(false).unwrap();
        assert_eq!(all.len(), 2);
        let k2 = all.iter().find(|s| s.key == "k2").unwrap();
        assert_eq!(k2.value, "v2b");
        assert!(k2.allow_unauthorized);

        let public = dao.find_all_global(true).unwrap();
        assert_eq!(public.len(), 1);
        assert_eq!(public[0].key, "k2");

        dao.delete_global_by_keys(&["k1".to_string()]).unwrap();
        assert_eq!(dao.find_all_global(false).unwrap().len(), 1);

        // CLIENT_SETTINGS_USER has an FK to USER, so create the users first
        let user_dao = crate::dao::user::UserDao::new(dao.db.clone());
        for id in ["u1", "u2"] {
            user_dao
                .insert(&komga_core::model::user::KomgaUser {
                    id: id.into(),
                    email: format!("{id}@example.org"),
                    password: "x".into(),
                    roles: Default::default(),
                    shared_libraries_ids: Default::default(),
                    shared_all_libraries: true,
                    restrictions: Default::default(),
                    created_date: komga_core::time_codec::now_utc(),
                    last_modified_date: komga_core::time_codec::now_utc(),
                })
                .unwrap();
        }

        dao.save_for_user("u1", "theme", "dark").unwrap();
        dao.save_for_user("u1", "theme", "light").unwrap();
        dao.save_for_user("u2", "theme", "dark").unwrap();
        let user_settings = dao.find_all_user("u1").unwrap();
        assert_eq!(user_settings.len(), 1);
        assert_eq!(user_settings[0].value, "light");

        dao.delete_by_user_id_and_keys("u1", &["theme".to_string()])
            .unwrap();
        assert!(dao.find_all_user("u1").unwrap().is_empty());
        dao.delete_by_user_id("u2").unwrap();
        assert!(dao.find_all_user("u2").unwrap().is_empty());
    }
}
