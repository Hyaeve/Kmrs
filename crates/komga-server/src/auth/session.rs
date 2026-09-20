//! Session store: aligned with spring-session-caffeine.
//! - Cookie name `KOMGA-SESSION`; the `X-Auth-Token` header is also accepted for passing the session id both ways.
//! - In-memory storage with sliding expiration (default 7 days, idle-based).
//! - Session data stores the user id plus the API-key mark left by API-key authentication
//!   (Spring persists the replaced SecurityContext into the session); user info is fetched
//!   from the DB each time (immediate effect of permission changes is handled by explicit invalidation).

use moka::sync::Cache;
use std::time::Duration;

pub const SESSION_COOKIE_NAME: &str = "KOMGA-SESSION";
pub const SESSION_HEADER_NAME: &str = "X-Auth-Token";

/// Set once an API-key authentication replaces the session's security context
/// (Java stores the `ApiKeyAuthenticationToken`, whose name is the masked key hash, in the session).
#[derive(Clone)]
pub struct SessionApiKey {
    pub id: String,
    pub hash: String,
}

#[derive(Clone)]
pub struct SessionData {
    pub user_id: String,
    pub api_key: Option<SessionApiKey>,
    /// Epoch millis; tracked for the actuator `/actuator/sessions` descriptors.
    pub created_at: u64,
    pub last_accessed_at: u64,
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

#[derive(Clone)]
pub struct SessionStore {
    cache: Cache<String, SessionData>,
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
        let now = now_millis();
        self.cache.insert(
            id.clone(),
            SessionData {
                user_id: user_id.to_string(),
                api_key: None,
                created_at: now,
                last_accessed_at: now,
            },
        );
        id
    }

    pub fn get(&self, id: &str) -> Option<SessionData> {
        let data = self.cache.get(id)?;
        let mut data = data;
        data.last_accessed_at = now_millis();
        self.cache.insert(id.to_string(), data.clone());
        Some(data)
    }

    /// All live sessions, for the actuator `/actuator/sessions` listing.
    pub fn all(&self) -> Vec<(String, SessionData)> {
        self.cache.iter().map(|(k, v)| ((*k).clone(), v)).collect()
    }

    /// Replaces the session's identity with the API-key authentication, like Spring's
    /// `SecurityContextRepository` saving the new context. Unknown session ids are ignored:
    /// the caller creates the session first when the request had none.
    pub fn mark_api_key(&self, id: &str, user_id: &str, key_id: &str, key_hash: &str) {
        if let Some(existing) = self.cache.get(id) {
            self.cache.insert(
                id.to_string(),
                SessionData {
                    user_id: user_id.to_string(),
                    api_key: Some(SessionApiKey {
                        id: key_id.to_string(),
                        hash: key_hash.to_string(),
                    }),
                    created_at: existing.created_at,
                    last_accessed_at: now_millis(),
                },
            );
        }
    }

    pub fn invalidate(&self, id: &str) {
        self.cache.invalidate(id);
    }

    /// Invalidates all sessions of a user (on password change / permission change / user deletion).
    pub fn invalidate_user(&self, user_id: &str) {
        let ids: Vec<String> = self
            .cache
            .iter()
            .filter(|(_, v)| v.user_id == user_id)
            .map(|(k, _)| (*k).clone())
            .collect();
        for id in ids {
            self.cache.invalidate(&id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mark_api_key_replaces_identity_and_ignores_unknown_sessions() {
        let store = SessionStore::new(Duration::from_secs(60));
        store.mark_api_key("ghost", "u1", "k1", "h1");
        assert!(store.get("ghost").is_none());

        let id = store.create("u1");
        store.mark_api_key(&id, "u2", "k1", "h1");
        let session = store.get(&id).unwrap();
        assert_eq!(session.user_id, "u2");
        let key = session.api_key.unwrap();
        assert_eq!(key.id, "k1");
        assert_eq!(key.hash, "h1");

        store.invalidate_user("u2");
        assert!(store.get(&id).is_none());
    }
}
