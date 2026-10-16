//! Remember-me: aligned with Spring `TokenBasedRememberMeServices`.
//! token = base64(username ":" expiryMillis ":" md5Hex(username ":" expiryMillis ":" password ":" key)).
//! Cookie name `komga-remember-me`, path=/; validity is determined by SERVER_SETTINGS.REMEMBER_ME_DURATION (days).

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use komga_core::model::user::KomgaUser;

pub const REMEMBER_ME_COOKIE: &str = "komga-remember-me";
pub const REMEMBER_ME_PARAM: &str = "remember-me";

pub fn encode_token(user: &KomgaUser, key: &str, expiry_millis: u64) -> String {
    let signature = md5_hex(&format!(
        "{}:{}:{}:{}",
        user.email, expiry_millis, user.password, key
    ));
    B64.encode(format!("{}:{}:{}", user.email, expiry_millis, signature))
}

/// Returns (email, expiry_millis) on successful validation.
pub fn decode_token(token: &str, user: &KomgaUser, key: &str, now_millis: u64) -> Option<u64> {
    let decoded = String::from_utf8(B64.decode(token).ok()?).ok()?;
    let mut parts = decoded.splitn(3, ':');
    let email = parts.next()?;
    let expiry: u64 = parts.next()?.parse().ok()?;
    let signature = parts.next()?;
    if email != user.email || expiry < now_millis {
        return None;
    }
    let expected = md5_hex(&format!("{}:{}:{}:{}", email, expiry, user.password, key));
    if signature != expected {
        return None;
    }
    Some(expiry)
}

fn md5_hex(input: &str) -> String {
    format!("{:x}", md5::compute(input.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use komga_core::model::user::*;

    fn user() -> KomgaUser {
        KomgaUser {
            id: "u1".into(),
            email: "a@b.c".into(),
            password: "$2a$10$hash".into(),
            roles: Default::default(),
            shared_all_libraries: true,
            shared_libraries_ids: Default::default(),
            restrictions: ContentRestrictions::default(),
            created_date: time::OffsetDateTime::now_utc(),
            last_modified_date: time::OffsetDateTime::now_utc(),
        }
    }

    #[test]
    fn roundtrip() {
        let token = encode_token(&user(), "secret", 1_900_000_000_000);
        let expiry = decode_token(&token, &user(), "secret", 1_000_000).unwrap();
        assert_eq!(expiry, 1_900_000_000_000);
    }

    #[test]
    fn rejects_wrong_key_and_expired() {
        let token = encode_token(&user(), "secret", 1_900_000_000_000);
        assert!(decode_token(&token, &user(), "other", 1_000_000).is_none());
        assert!(decode_token(&token, &user(), "secret", 2_000_000_000_000).is_none());
    }
}
