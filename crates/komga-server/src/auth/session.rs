//! Session store: aligned with spring-session-caffeine.
//! - Cookie name `KOMGA-SESSION`; the `X-Auth-Token` header is also accepted for passing the session id both ways.
//! - In-memory storage with sliding expiration (default 7 days, idle-based).
//! - Session data stores only the user id; user info is fetched from the DB each time (immediate effect of permission changes is handled by explicit invalidation).

use moka::sync::Cache;
use std::time::Duration;

pub const SESSION_COOKIE_NAME: &str = "KOMGA-SESSION";
pub const SESSION_HEADER_NAME: &str = "X-Auth-Token";

#[derive(Clone)]
pub struct SessionStore {
    cache: Cache<String, String>,
}

impl SessionStore {
    pub fn new(timeout: Duration) -> Self {
        Self {
            // sliding expiration (time_to_idle = expire after access)
            cache: Cache::builder().time_to_idle(timeout).build(),
        }
    }

    pub fn create(&self, user_id: &str) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        self.cache.insert(id.clone(), user_id.to_string());
        id
    }

    pub fn get(&self, id: &str) -> Option<String> {
        self.cache.get(id)
    }

    pub fn invalidate(&self, id: &str) {
        self.cache.invalidate(id);
    }

    /// Invalidates all sessions of a user (on password change / permission change / user deletion).
    pub fn invalidate_user(&self, user_id: &str) {
        let ids: Vec<String> = self
            .cache
            .iter()
            .filter(|(_, v)| v.as_str() == user_id)
            .map(|(k, _)| (*k).clone())
            .collect();
        for id in ids {
            self.cache.invalidate(&id);
        }
    }
}
