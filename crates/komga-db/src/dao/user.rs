//! DAOs for USER, USER_ROLE, USER_LIBRARY_SHARING, USER_SHARING, USER_API_KEY,
//! ANNOUNCEMENTS_READ, AUTHENTICATION_ACTIVITY (`KomgaUserDao` + `AuthenticationActivityDao`).

use super::get_datetime;
use crate::error::Result;
use crate::pool::Database;
use komga_core::model::user::{
    AgeRestriction, AllowExclude, ApiKey, AuthenticationActivity, ContentRestrictions, KomgaUser,
    UserRole,
};
use komga_core::time_codec::{self, now_utc};
use komga_core::tsid::TsidFactory;
use rusqlite::{params, Row};
use std::collections::BTreeSet;

const USER_COLUMNS: &str = "ID, EMAIL, PASSWORD, SHARED_ALL_LIBRARIES, AGE_RESTRICTION, AGE_RESTRICTION_ALLOW_ONLY, CREATED_DATE, LAST_MODIFIED_DATE";

const API_KEY_COLUMNS: &str = "ID, USER_ID, API_KEY, COMMENT, CREATED_DATE, LAST_MODIFIED_DATE";

const ACTIVITY_COLUMNS: &str = "USER_ID, EMAIL, API_KEY_ID, API_KEY_COMMENT, IP, USER_AGENT, SUCCESS, ERROR, DATE_TIME, SOURCE";

pub struct UserDao {
    db: Database,
    tsid: TsidFactory,
}

impl UserDao {
    pub fn new(db: Database) -> Self {
        Self {
            db,
            tsid: TsidFactory::new_random_node(),
        }
    }

    // ---------- KomgaUser ----------

    fn row_to_user(row: &Row<'_>) -> rusqlite::Result<KomgaUser> {
        let age_restriction: Option<i64> = row.get(4)?;
        let age_restriction_allow_only: Option<bool> = row.get(5)?;
        Ok(KomgaUser {
            id: row.get(0)?,
            email: row.get(1)?,
            password: row.get(2)?,
            shared_all_libraries: row.get(3)?,
            restrictions: ContentRestrictions::new(
                match (age_restriction, age_restriction_allow_only) {
                    (Some(age), Some(allow_only)) => Some(AgeRestriction {
                        age: age as i32,
                        restriction: if allow_only {
                            AllowExclude::AllowOnly
                        } else {
                            AllowExclude::Exclude
                        },
                    }),
                    _ => None,
                },
                BTreeSet::new(),
                BTreeSet::new(),
            ),
            roles: BTreeSet::new(),
            shared_libraries_ids: BTreeSet::new(),
            created_date: get_datetime(row, 6)?,
            last_modified_date: get_datetime(row, 7)?,
        })
    }

    fn fill_user_children(&self, users: &mut [KomgaUser]) -> Result<()> {
        if users.is_empty() {
            return Ok(());
        }
        let conn = self.db.ro();
        let mut roles_stmt = conn.prepare("SELECT USER_ID, ROLE FROM USER_ROLE")?;
        let mut roles_map: std::collections::HashMap<String, BTreeSet<UserRole>> =
            std::collections::HashMap::new();
        for row in
            roles_stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        {
            let (user_id, role) = row?;
            if let Ok(role) = role.parse::<UserRole>() {
                roles_map.entry(user_id).or_default().insert(role);
            }
        }
        let mut sharing_stmt =
            conn.prepare("SELECT USER_ID, LIBRARY_ID FROM USER_LIBRARY_SHARING")?;
        let mut sharing_map: std::collections::HashMap<String, BTreeSet<String>> =
            std::collections::HashMap::new();
        for row in
            sharing_stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        {
            let (user_id, library_id) = row?;
            sharing_map.entry(user_id).or_default().insert(library_id);
        }
        let mut labels_stmt = conn.prepare("SELECT USER_ID, LABEL, ALLOW FROM USER_SHARING")?;
        let mut allow_map: std::collections::HashMap<String, BTreeSet<String>> =
            std::collections::HashMap::new();
        let mut exclude_map: std::collections::HashMap<String, BTreeSet<String>> =
            std::collections::HashMap::new();
        for row in labels_stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, bool>(2)?,
            ))
        })? {
            let (user_id, label, allow) = row?;
            if allow {
                allow_map.entry(user_id).or_default().insert(label);
            } else {
                exclude_map.entry(user_id).or_default().insert(label);
            }
        }
        for user in users.iter_mut() {
            user.roles = roles_map.remove(&user.id).unwrap_or_default();
            user.shared_libraries_ids = sharing_map.remove(&user.id).unwrap_or_default();
            user.restrictions = ContentRestrictions::new(
                user.restrictions.age_restriction,
                allow_map.remove(&user.id).unwrap_or_default(),
                exclude_map.remove(&user.id).unwrap_or_default(),
            );
        }
        Ok(())
    }

    pub fn count(&self) -> Result<i64> {
        Ok(self
            .db
            .ro()
            .query_row("SELECT COUNT(*) FROM USER", [], |r| r.get(0))?)
    }

    pub fn find_all(&self) -> Result<Vec<KomgaUser>> {
        let conn = self.db.ro();
        let mut stmt = conn.prepare(&format!("SELECT {USER_COLUMNS} FROM USER ORDER BY EMAIL"))?;
        let mut users = stmt
            .query_map([], Self::row_to_user)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        drop(stmt);
        drop(conn);
        self.fill_user_children(&mut users)?;
        Ok(users)
    }

    pub fn find_by_id(&self, id: &str) -> Result<Option<KomgaUser>> {
        let conn = self.db.ro();
        let mut stmt = conn.prepare(&format!("SELECT {USER_COLUMNS} FROM USER WHERE ID = ?"))?;
        let mut users = stmt
            .query_map([id], Self::row_to_user)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        drop(stmt);
        drop(conn);
        self.fill_user_children(&mut users)?;
        Ok(users.into_iter().next())
    }

    /// jOOQ `equalIgnoreCase` → `LOWER(EMAIL) = LOWER(?)`.
    pub fn find_by_email_ignore_case(&self, email: &str) -> Result<Option<KomgaUser>> {
        let conn = self.db.ro();
        let mut stmt = conn.prepare(&format!(
            "SELECT {USER_COLUMNS} FROM USER WHERE LOWER(EMAIL) = LOWER(?)"
        ))?;
        let mut users = stmt
            .query_map([email], Self::row_to_user)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        drop(stmt);
        drop(conn);
        self.fill_user_children(&mut users)?;
        Ok(users.into_iter().next())
    }

    pub fn exists_by_email_ignore_case(&self, email: &str) -> Result<bool> {
        let conn = self.db.ro();
        Ok(conn.query_row(
            "SELECT COUNT(*) FROM USER WHERE LOWER(EMAIL) = LOWER(?)",
            [email],
            |r| r.get::<_, i64>(0),
        )? > 0)
    }

    pub fn insert(&self, user: &KomgaUser) -> Result<String> {
        let conn = self.db.rw();
        let id = if user.id.is_empty() {
            self.tsid.create_string()
        } else {
            user.id.clone()
        };
        conn.execute(
            &format!("INSERT INTO USER ({USER_COLUMNS}) VALUES (?,?,?,?,?,?,?,?)"),
            rusqlite::params_from_iter(user_params(&id, user)),
        )?;
        self.insert_user_children(&conn, &id, user)?;
        Ok(id)
    }

    /// Corresponds to `KomgaUserDao.update`: LAST_MODIFIED_DATE is forced to the
    /// current time (UTC) by the DAO.
    pub fn update(&self, user: &KomgaUser) -> Result<()> {
        let conn = self.db.rw();
        conn.execute(
            "UPDATE USER SET EMAIL = ?, PASSWORD = ?, SHARED_ALL_LIBRARIES = ?, \
       AGE_RESTRICTION = ?, AGE_RESTRICTION_ALLOW_ONLY = ?, LAST_MODIFIED_DATE = ? WHERE ID = ?",
            params![
                user.email,
                user.password,
                user.shared_all_libraries,
                user.restrictions.age_restriction.map(|a| a.age),
                user.restrictions
                    .age_restriction
                    .map(|a| a.restriction == AllowExclude::AllowOnly),
                time_codec::format_datetime(now_utc()),
                user.id,
            ],
        )?;
        conn.execute("DELETE FROM USER_ROLE WHERE USER_ID = ?", [&user.id])?;
        conn.execute(
            "DELETE FROM USER_LIBRARY_SHARING WHERE USER_ID = ?",
            [&user.id],
        )?;
        conn.execute("DELETE FROM USER_SHARING WHERE USER_ID = ?", [&user.id])?;
        self.insert_user_children(&conn, &user.id, user)?;
        Ok(())
    }

    /// Follows the deletion order of `KomgaUserDao.delete`.
    pub fn delete(&self, user_id: &str) -> Result<()> {
        let conn = self.db.rw();
        conn.execute("DELETE FROM USER_API_KEY WHERE USER_ID = ?", [user_id])?;
        conn.execute(
            "DELETE FROM ANNOUNCEMENTS_READ WHERE USER_ID = ?",
            [user_id],
        )?;
        conn.execute("DELETE FROM USER_SHARING WHERE USER_ID = ?", [user_id])?;
        conn.execute(
            "DELETE FROM USER_LIBRARY_SHARING WHERE USER_ID = ?",
            [user_id],
        )?;
        conn.execute("DELETE FROM USER_ROLE WHERE USER_ID = ?", [user_id])?;
        conn.execute("DELETE FROM USER WHERE ID = ?", [user_id])?;
        Ok(())
    }

    pub fn delete_all(&self) -> Result<()> {
        let conn = self.db.rw();
        for table in [
            "USER_API_KEY",
            "ANNOUNCEMENTS_READ",
            "USER_SHARING",
            "USER_LIBRARY_SHARING",
            "USER_ROLE",
            "USER",
        ] {
            conn.execute(&format!("DELETE FROM {table}"), [])?;
        }
        Ok(())
    }

    fn insert_user_children(
        &self,
        conn: &rusqlite::Connection,
        user_id: &str,
        user: &KomgaUser,
    ) -> Result<()> {
        for role in &user.roles {
            conn.execute(
                "INSERT INTO USER_ROLE (USER_ID, ROLE) VALUES (?, ?)",
                params![user_id, role.as_str()],
            )?;
        }
        for library_id in &user.shared_libraries_ids {
            conn.execute(
                "INSERT INTO USER_LIBRARY_SHARING (USER_ID, LIBRARY_ID) VALUES (?, ?)",
                params![user_id, library_id],
            )?;
        }
        for label in &user.restrictions.labels_allow {
            conn.execute(
                "INSERT INTO USER_SHARING (USER_ID, ALLOW, LABEL) VALUES (?, 1, ?)",
                params![user_id, label],
            )?;
        }
        for label in &user.restrictions.labels_exclude {
            conn.execute(
                "INSERT INTO USER_SHARING (USER_ID, ALLOW, LABEL) VALUES (?, 0, ?)",
                params![user_id, label],
            )?;
        }
        Ok(())
    }

    // ---------- ANNOUNCEMENTS_READ ----------

    pub fn find_announcement_ids_read(&self, user_id: &str) -> Result<BTreeSet<String>> {
        let conn = self.db.ro();
        let mut stmt =
            conn.prepare("SELECT ANNOUNCEMENT_ID FROM ANNOUNCEMENTS_READ WHERE USER_ID = ?")?;
        let ids = stmt
            .query_map([user_id], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<BTreeSet<_>, _>>()?;
        Ok(ids)
    }

    pub fn save_announcement_ids_read(
        &self,
        user_id: &str,
        announcement_ids: &BTreeSet<String>,
    ) -> Result<()> {
        let conn = self.db.rw();
        for id in announcement_ids {
            conn.execute(
                "INSERT OR IGNORE INTO ANNOUNCEMENTS_READ (USER_ID, ANNOUNCEMENT_ID) VALUES (?, ?)",
                params![user_id, id],
            )?;
        }
        Ok(())
    }

    // ---------- USER_API_KEY ----------

    fn row_to_api_key(row: &Row<'_>) -> rusqlite::Result<ApiKey> {
        Ok(ApiKey {
            id: row.get(0)?,
            user_id: row.get(1)?,
            key: row.get(2)?,
            comment: row.get(3)?,
            created_date: get_datetime(row, 4)?,
            last_modified_date: get_datetime(row, 5)?,
        })
    }

    pub fn find_api_keys_by_user_id(&self, user_id: &str) -> Result<Vec<ApiKey>> {
        let conn = self.db.ro();
        let mut stmt = conn.prepare(&format!(
            "SELECT {API_KEY_COLUMNS} FROM USER_API_KEY WHERE USER_ID = ?"
        ))?;
        let rows = stmt
            .query_map([user_id], Self::row_to_api_key)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// `findByApiKeyOrNull`: finds the user and key record by key (SHA-512 hex).
    pub fn find_by_api_key(&self, api_key: &str) -> Result<Option<(KomgaUser, ApiKey)>> {
        let conn = self.db.ro();
        let mut key_stmt = conn.prepare(&format!(
            "SELECT {API_KEY_COLUMNS} FROM USER_API_KEY WHERE API_KEY = ?"
        ))?;
        let key = key_stmt
            .query_map([api_key], Self::row_to_api_key)?
            .collect::<std::result::Result<Vec<_>, _>>()?
            .into_iter()
            .next();
        drop(key_stmt);
        drop(conn);
        let Some(key) = key else { return Ok(None) };
        let Some(user) = self.find_by_id(&key.user_id)? else {
            return Ok(None);
        };
        Ok(Some((user, key)))
    }

    pub fn exists_api_key_by_id_and_user_id(
        &self,
        api_key_id: &str,
        user_id: &str,
    ) -> Result<bool> {
        let conn = self.db.ro();
        Ok(conn.query_row(
            "SELECT COUNT(*) FROM USER_API_KEY WHERE ID = ? AND USER_ID = ?",
            params![api_key_id, user_id],
            |r| r.get::<_, i64>(0),
        )? > 0)
    }

    pub fn exists_api_key_by_comment_and_user_id(
        &self,
        comment: &str,
        user_id: &str,
    ) -> Result<bool> {
        let conn = self.db.ro();
        Ok(conn.query_row(
            "SELECT COUNT(*) FROM USER_API_KEY WHERE LOWER(COMMENT) = LOWER(?) AND USER_ID = ?",
            params![comment, user_id],
            |r| r.get::<_, i64>(0),
        )? > 0)
    }

    pub fn insert_api_key(&self, api_key: &ApiKey) -> Result<String> {
        let conn = self.db.rw();
        let id = if api_key.id.is_empty() {
            self.tsid.create_string()
        } else {
            api_key.id.clone()
        };
        conn.execute(
            &format!("INSERT INTO USER_API_KEY ({API_KEY_COLUMNS}) VALUES (?,?,?,?,?,?)"),
            params![
                id,
                api_key.user_id,
                api_key.key,
                api_key.comment,
                time_codec::format_datetime(api_key.created_date),
                time_codec::format_datetime(api_key.last_modified_date),
            ],
        )?;
        Ok(id)
    }

    pub fn delete_api_key_by_id_and_user_id(&self, api_key_id: &str, user_id: &str) -> Result<()> {
        let conn = self.db.rw();
        conn.execute(
            "DELETE FROM USER_API_KEY WHERE ID = ? AND USER_ID = ?",
            params![api_key_id, user_id],
        )?;
        Ok(())
    }

    pub fn delete_api_keys_by_user_id(&self, user_id: &str) -> Result<()> {
        let conn = self.db.rw();
        conn.execute("DELETE FROM USER_API_KEY WHERE USER_ID = ?", [user_id])?;
        Ok(())
    }

    // ---------- AUTHENTICATION_ACTIVITY ----------

    fn row_to_activity(row: &Row<'_>) -> rusqlite::Result<AuthenticationActivity> {
        Ok(AuthenticationActivity {
            user_id: row.get(0)?,
            email: row.get(1)?,
            api_key_id: row.get(2)?,
            api_key_comment: row.get(3)?,
            ip: row.get(4)?,
            user_agent: row.get(5)?,
            success: row.get(6)?,
            error: row.get(7)?,
            date_time: get_datetime(row, 8)?,
            source: row.get(9)?,
        })
    }

    pub fn insert_activity(&self, activity: &AuthenticationActivity) -> Result<()> {
        let conn = self.db.rw();
        conn.execute(
      "INSERT INTO AUTHENTICATION_ACTIVITY \
       (USER_ID, EMAIL, API_KEY_ID, API_KEY_COMMENT, IP, USER_AGENT, SUCCESS, ERROR, DATE_TIME, SOURCE) \
       VALUES (?,?,?,?,?,?,?,?,?,?)",
      params![
        activity.user_id,
        activity.email,
        activity.api_key_id,
        activity.api_key_comment,
        activity.ip,
        activity.user_agent,
        activity.success,
        activity.error,
        time_codec::format_datetime(activity.date_time),
        activity.source,
      ],
    )?;
        Ok(())
    }

    /// `AuthenticationActivityRepository.deleteOlderThan`
    pub fn delete_activity_older_than(&self, cutoff: time::OffsetDateTime) -> Result<i64> {
        let conn = self.db.rw();
        let n = conn.execute(
            "DELETE FROM AUTHENTICATION_ACTIVITY WHERE DATE_TIME < ?",
            [time_codec::format_datetime(cutoff)],
        )?;
        Ok(n as i64)
    }

    /// `findAllByUser`: `USER_ID = ? OR EMAIL = ?`, ordered by DATE_TIME DESC,
    /// returns (items, total).
    pub fn find_activities_by_user(
        &self,
        user_id: &str,
        email: &str,
        limit: Option<u32>,
        offset: u32,
    ) -> Result<(Vec<AuthenticationActivity>, i64)> {
        let conn = self.db.ro();
        let total: i64 = conn.query_row(
            "SELECT COUNT(*) FROM AUTHENTICATION_ACTIVITY WHERE USER_ID = ? OR EMAIL = ?",
            params![user_id, email],
            |r| r.get(0),
        )?;
        let mut sql = format!(
      "SELECT {ACTIVITY_COLUMNS} FROM AUTHENTICATION_ACTIVITY WHERE USER_ID = ? OR EMAIL = ? ORDER BY DATE_TIME DESC"
    );
        if limit.is_some() {
            sql.push_str(" LIMIT ? OFFSET ?");
        }
        let mut stmt = conn.prepare(&sql)?;
        let rows: Vec<AuthenticationActivity> = match limit {
            Some(limit) => stmt
                .query_map(
                    params![user_id, email, limit, offset],
                    Self::row_to_activity,
                )?
                .collect::<std::result::Result<Vec<_>, _>>()?,
            None => stmt
                .query_map(params![user_id, email], Self::row_to_activity)?
                .collect::<std::result::Result<Vec<_>, _>>()?,
        };
        Ok((rows, total))
    }

    pub fn find_all_activities(
        &self,
        limit: Option<u32>,
        offset: u32,
    ) -> Result<(Vec<AuthenticationActivity>, i64)> {
        let conn = self.db.ro();
        let total: i64 =
            conn.query_row("SELECT COUNT(*) FROM AUTHENTICATION_ACTIVITY", [], |r| {
                r.get(0)
            })?;
        let mut sql = format!(
            "SELECT {ACTIVITY_COLUMNS} FROM AUTHENTICATION_ACTIVITY ORDER BY DATE_TIME DESC"
        );
        if limit.is_some() {
            sql.push_str(" LIMIT ? OFFSET ?");
        }
        let mut stmt = conn.prepare(&sql)?;
        let rows: Vec<AuthenticationActivity> = match limit {
            Some(limit) => stmt
                .query_map(params![limit, offset], Self::row_to_activity)?
                .collect::<std::result::Result<Vec<_>, _>>()?,
            None => stmt
                .query_map([], Self::row_to_activity)?
                .collect::<std::result::Result<Vec<_>, _>>()?,
        };
        Ok((rows, total))
    }

    pub fn find_most_recent_activity_by_user(
        &self,
        user_id: &str,
        email: &str,
        api_key_id: Option<&str>,
    ) -> Result<Option<AuthenticationActivity>> {
        let conn = self.db.ro();
        let mut sql = format!(
      "SELECT {ACTIVITY_COLUMNS} FROM AUTHENTICATION_ACTIVITY WHERE (USER_ID = ? OR EMAIL = ?)"
    );
        if api_key_id.is_some() {
            sql.push_str(" AND API_KEY_ID = ?");
        }
        sql.push_str(" ORDER BY DATE_TIME DESC LIMIT 1");
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = match api_key_id {
            Some(api_key_id) => stmt
                .query_map(params![user_id, email, api_key_id], Self::row_to_activity)?
                .collect::<std::result::Result<Vec<_>, _>>()?,
            None => stmt
                .query_map(params![user_id, email], Self::row_to_activity)?
                .collect::<std::result::Result<Vec<_>, _>>()?,
        };
        Ok(rows.pop())
    }

    pub fn delete_activities_by_user(&self, user_id: &str, email: &str) -> Result<()> {
        let conn = self.db.rw();
        conn.execute(
            "DELETE FROM AUTHENTICATION_ACTIVITY WHERE USER_ID = ? OR EMAIL = ?",
            params![user_id, email],
        )?;
        Ok(())
    }

    pub fn delete_activities_older_than(&self, date_time: time::OffsetDateTime) -> Result<usize> {
        let conn = self.db.rw();
        Ok(conn.execute(
            "DELETE FROM AUTHENTICATION_ACTIVITY WHERE DATE_TIME < ?",
            [time_codec::format_datetime(date_time)],
        )?)
    }
}

fn user_params(id: &str, u: &KomgaUser) -> Vec<Box<dyn rusqlite::ToSql>> {
    vec![
        Box::new(id.to_string()),
        Box::new(u.email.clone()),
        Box::new(u.password.clone()),
        Box::new(u.shared_all_libraries),
        Box::new(u.restrictions.age_restriction.map(|a| a.age)),
        Box::new(
            u.restrictions
                .age_restriction
                .map(|a| a.restriction == AllowExclude::AllowOnly),
        ),
        Box::new(time_codec::format_datetime(u.created_date)),
        Box::new(time_codec::format_datetime(u.last_modified_date)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migrate::Migrator;
    use crate::{main_migrations, Placeholders};

    fn dao() -> UserDao {
        let db = Database::open_in_memory(true).unwrap();
        let migrations = main_migrations();
        Migrator::new(&migrations, Placeholders::default())
            .migrate(&db.rw())
            .unwrap();
        UserDao::new(db)
    }

    fn sample_user() -> KomgaUser {
        KomgaUser {
            id: String::new(),
            email: "Admin@Example.org".into(),
            password: "bcrypt-hash".into(),
            roles: [UserRole::Admin, UserRole::FileDownload]
                .into_iter()
                .collect(),
            shared_libraries_ids: BTreeSet::new(),
            shared_all_libraries: true,
            restrictions: ContentRestrictions::new(
                Some(AgeRestriction {
                    age: 15,
                    restriction: AllowExclude::Exclude,
                }),
                ["kids"].into_iter().map(String::from).collect(),
                ["horror"].into_iter().map(String::from).collect(),
            ),
            created_date: now_utc(),
            last_modified_date: now_utc(),
        }
    }

    #[test]
    fn user_crud_roundtrip() {
        let dao = dao();
        let user = sample_user();
        let id = dao.insert(&user).unwrap();
        assert_eq!(id.len(), 13);

        let found = dao.find_by_id(&id).unwrap().expect("not found");
        assert_eq!(found.email, "Admin@Example.org");
        assert!(found.roles.contains(&UserRole::Admin));
        assert!(found.roles.contains(&UserRole::FileDownload));
        assert!(found.shared_all_libraries);
        let ar = found.restrictions.age_restriction.expect("age restriction");
        assert_eq!(ar.age, 15);
        assert_eq!(ar.restriction, AllowExclude::Exclude);
        assert!(found.restrictions.labels_allow.contains("kids"));
        assert!(found.restrictions.labels_exclude.contains("horror"));

        // email matching is case-insensitive
        let by_email = dao.find_by_email_ignore_case("admin@example.org").unwrap();
        assert!(by_email.is_some());
        assert!(dao
            .exists_by_email_ignore_case("ADMIN@EXAMPLE.ORG")
            .unwrap());

        // update: roles/sharing/labels are replaced, last_modified is refreshed
        // USER_LIBRARY_SHARING has an FK to LIBRARY, so create the library first
        crate::dao::library::LibraryDao::new(dao.db.clone())
            .insert(&komga_core::model::library::Library {
                id: "lib1".into(),
                name: "lib1".into(),
                root: "file:/tmp/lib1/".into(),
                import_comicinfo_book: true,
                import_comicinfo_series: true,
                import_comicinfo_collection: true,
                import_comicinfo_readlist: true,
                import_comicinfo_series_append_volume: true,
                import_epub_book: true,
                import_epub_series: true,
                import_mylar_series: true,
                import_local_artwork: true,
                import_barcode_isbn: true,
                scan_force_modified_time: false,
                scan_on_startup: false,
                scan_interval: komga_core::model::library::ScanInterval::Every6H,
                scan_cbx: true,
                scan_pdf: true,
                scan_epub: true,
                scan_directory_exclusions: vec![],
                repair_extensions: false,
                convert_to_cbz: false,
                empty_trash_after_scan: false,
                series_cover: komga_core::model::library::SeriesCover::First,
                hash_files: true,
                hash_pages: false,
                hash_koreader: false,
                analyze_dimensions: true,
                oneshots_directory: None,
                unavailable_date: None,
                created_date: now_utc(),
                last_modified_date: now_utc(),
            })
            .unwrap();

        let mut updated = found.clone();
        updated.roles = [UserRole::PageStreaming].into_iter().collect();
        updated.shared_all_libraries = false;
        updated.shared_libraries_ids = ["lib1".to_string()].into_iter().collect();
        updated.restrictions = ContentRestrictions::new(None, BTreeSet::new(), BTreeSet::new());
        dao.update(&updated).unwrap();

        let found = dao.find_by_id(&id).unwrap().unwrap();
        assert_eq!(found.roles.len(), 1);
        assert!(found.roles.contains(&UserRole::PageStreaming));
        assert!(!found.shared_all_libraries);
        assert!(found.shared_libraries_ids.contains("lib1"));
        assert!(found.restrictions.age_restriction.is_none());
        assert!(found.restrictions.labels_allow.is_empty());
        assert!(found.last_modified_date >= updated.last_modified_date);

        assert_eq!(dao.count().unwrap(), 1);
        assert_eq!(dao.find_all().unwrap().len(), 1);

        dao.delete(&id).unwrap();
        assert!(dao.find_by_id(&id).unwrap().is_none());
        for table in ["USER_ROLE", "USER_LIBRARY_SHARING", "USER_SHARING"] {
            let n: i64 = dao
                .db
                .ro()
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
                .unwrap();
            assert_eq!(n, 0, "{table} not empty after delete");
        }
    }

    #[test]
    fn announcements_read() {
        let dao = dao();
        let id = dao.insert(&sample_user()).unwrap();
        let ids: BTreeSet<String> = ["a1".to_string(), "a2".to_string()].into_iter().collect();
        dao.save_announcement_ids_read(&id, &ids).unwrap();
        // duplicate inserts are ignored
        dao.save_announcement_ids_read(&id, &ids).unwrap();
        assert_eq!(dao.find_announcement_ids_read(&id).unwrap(), ids);
    }

    #[test]
    fn api_key_lifecycle() {
        let dao = dao();
        let user_id = dao.insert(&sample_user()).unwrap();
        let key = ApiKey {
            id: String::new(),
            user_id: user_id.clone(),
            key: "sha512hex".into(),
            comment: "kobo".into(),
            created_date: now_utc(),
            last_modified_date: now_utc(),
        };
        let key_id = dao.insert_api_key(&key).unwrap();
        assert!(dao
            .exists_api_key_by_id_and_user_id(&key_id, &user_id)
            .unwrap());
        assert!(dao
            .exists_api_key_by_comment_and_user_id("KOBO", &user_id)
            .unwrap());

        let (user, found_key) = dao
            .find_by_api_key("sha512hex")
            .unwrap()
            .expect("not found");
        assert_eq!(user.id, user_id);
        assert_eq!(found_key.comment, "kobo");
        assert_eq!(dao.find_api_keys_by_user_id(&user_id).unwrap().len(), 1);

        dao.delete_api_key_by_id_and_user_id(&key_id, &user_id)
            .unwrap();
        assert!(dao.find_by_api_key("sha512hex").unwrap().is_none());

        // deleting the user cascades to the keys
        dao.insert_api_key(&key).unwrap();
        dao.delete(&user_id).unwrap();
        let n: i64 = dao
            .db
            .ro()
            .query_row("SELECT COUNT(*) FROM USER_API_KEY", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn authentication_activity() {
        let dao = dao();
        let user_id = dao.insert(&sample_user()).unwrap();
        for (i, success) in [true, false, true].into_iter().enumerate() {
            dao.insert_activity(&AuthenticationActivity {
                user_id: Some(user_id.clone()),
                email: Some("admin@example.org".into()),
                api_key_id: None,
                api_key_comment: None,
                ip: Some("127.0.0.1".into()),
                user_agent: Some("test".into()),
                success,
                error: if success {
                    None
                } else {
                    Some("bad credentials".into())
                },
                date_time: now_utc() + time::Duration::seconds(i as i64),
                source: Some("Password".into()),
            })
            .unwrap();
        }

        let (items, total) = dao
            .find_activities_by_user(&user_id, "admin@example.org", None, 0)
            .unwrap();
        assert_eq!(total, 3);
        assert_eq!(items.len(), 3);
        // descending order: newest first
        assert!(items[0].date_time >= items[1].date_time);

        let (page, total) = dao
            .find_activities_by_user(&user_id, "admin@example.org", Some(2), 2)
            .unwrap();
        assert_eq!(total, 3);
        assert_eq!(page.len(), 1);

        let recent = dao
            .find_most_recent_activity_by_user(&user_id, "admin@example.org", None)
            .unwrap()
            .expect("no activity");
        assert!(recent.success);

        let (all, _) = dao.find_all_activities(None, 0).unwrap();
        assert_eq!(all.len(), 3);

        // threshold is now+1s: activities i=0 and i=1 are deleted, i=2 is kept
        dao.delete_activities_older_than(now_utc() + time::Duration::seconds(1))
            .unwrap();
        let (_, total) = dao.find_all_activities(None, 0).unwrap();
        assert_eq!(total, 1);
    }
}
