//! User-related helpers shared by the users endpoint and integrations.

use komga_core::model::user::ApiKey;
use komga_core::time_codec::now_utc;
use komga_db::dao::user::UserDao;
use komga_db::pool::Database;

/// komga retries generation up to 10 times (guards against unique key conflicts)
const MINT_ATTEMPTS: usize = 10;

/// Mint an API key for `user_id`; on success returns the stored key (id filled,
/// `key` holding the SHA-512 hex) plus the plaintext, which is only available here.
pub fn mint_api_key(
    db: Database,
    user_id: &str,
    comment: &str,
) -> anyhow::Result<(ApiKey, String)> {
    let dao = UserDao::new(db);
    for _ in 0..MINT_ATTEMPTS {
        let plain = uuid::Uuid::new_v4().simple().to_string();
        let mut api_key = ApiKey {
            id: String::new(),
            user_id: user_id.to_string(),
            key: crate::auth::sha512_hex(&plain),
            comment: comment.to_string(),
            created_date: now_utc(),
            last_modified_date: now_utc(),
        };
        match dao.insert_api_key(&api_key) {
            Ok(id) => {
                api_key.id = id;
                return Ok((api_key, plain));
            }
            Err(_) => continue,
        }
    }
    anyhow::bail!("failed to mint API key after {MINT_ATTEMPTS} attempts")
}
