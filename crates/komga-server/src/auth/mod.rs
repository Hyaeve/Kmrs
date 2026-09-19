//! Authentication resolution and middleware: tries X-API-Key → Basic → X-Auth-Token/session cookie → remember-me, in that order.
//! Aligned with `SecurityConfiguration.kt`: successful Basic/remember-me authentication establishes a session (IF_REQUIRED);
//! any authenticated request with a `remember-me=true` parameter → issue a remember-me cookie.

pub mod remember_me;
pub mod session;

use crate::state::AppState;
use axum::extract::{Request, State};
use axum::http::request::Parts;
use axum::middleware::Next;
use axum::response::Response;
use komga_core::model::user::{KomgaUser, UserRole};
use komga_db::dao::user::UserDao;
use sha2::{Digest, Sha512};

pub use session::{SessionStore, SESSION_COOKIE_NAME, SESSION_HEADER_NAME};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthSource {
    Password,
    ApiKey,
    RememberMe,
    Session,
}

#[derive(Debug, Clone)]
pub struct Auth {
    pub user: KomgaUser,
    #[allow(dead_code)] // used by OPDS/Kobo/KOReader in M3+
    pub source: AuthSource,
    #[allow(dead_code)]
    pub api_key_id: Option<String>,
}

impl Auth {
    pub fn is_admin(&self) -> bool {
        self.user.is_admin()
    }

    pub fn require_admin(&self) -> Result<(), crate::error::ApiError> {
        if self.is_admin() {
            Ok(())
        } else {
            Err(crate::error::ApiError::forbidden(""))
        }
    }

    #[allow(dead_code)] // used by endpoints starting from M3
    pub fn require_role(&self, role: UserRole) -> Result<(), crate::error::ApiError> {
        if self.user.roles.contains(&role) || self.is_admin() {
            Ok(())
        } else {
            Err(crate::error::ApiError::forbidden(""))
        }
    }
}

pub fn sha512_hex(input: &str) -> String {
    format!("{:x}", Sha512::digest(input.as_bytes()))
}

pub struct ActivityDraft {
    pub user_id: Option<String>,
    pub email: Option<String>,
    pub api_key_id: Option<String>,
    pub api_key_comment: Option<String>,
    pub success: bool,
    pub error: Option<String>,
    pub source: String,
}

/// Authentication resolution result stored in request extensions.
#[derive(Clone)]
pub(crate) struct ResolvedAuth {
    pub auth: Option<Auth>,
}

fn parse_basic_authorization(value: &str) -> Option<(String, String)> {
    use base64::Engine;
    let encoded = value.strip_prefix("Basic ")?;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(encoded.trim())
        .ok()?;
    let decoded = String::from_utf8(decoded).ok()?;
    let (email, password) = decoded.split_once(':')?;
    Some((email.to_string(), password.to_string()))
}

struct Outcome {
    auth: Option<Auth>,
    new_session_id: Option<String>,
    activity: Option<ActivityDraft>,
}

fn resolve(state: &AppState, parts: &Parts) -> Outcome {
    let headers = &parts.headers;
    let user_dao = UserDao::new(state.db.clone());

    // 1. X-API-Key
    if let Some(key) = headers.get("X-API-Key").and_then(|v| v.to_str().ok()) {
        let hashed = sha512_hex(key.trim());
        return match user_dao.find_by_api_key(&hashed) {
            Ok(Some((user, api_key))) => {
                // LoginListener records user.id/user.email on success; nulls are for failures
                let activity = ActivityDraft {
                    user_id: Some(api_key.user_id.clone()),
                    email: Some(user.email.clone()),
                    api_key_id: Some(api_key.id.clone()),
                    api_key_comment: Some(api_key.comment),
                    success: true,
                    error: None,
                    source: "ApiKey".into(),
                };
                Outcome {
                    auth: Some(Auth {
                        user,
                        source: AuthSource::ApiKey,
                        api_key_id: activity.api_key_id.clone(),
                    }),
                    new_session_id: None,
                    activity: Some(activity),
                }
            }
            _ => Outcome {
                auth: None,
                new_session_id: None,
                activity: Some(ActivityDraft {
                    user_id: None,
                    email: None,
                    api_key_id: None,
                    api_key_comment: None,
                    success: false,
                    error: Some("Invalid API key".into()),
                    source: "ApiKey".into(),
                }),
            },
        };
    }

    // 2. Basic
    if let Some(value) = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    {
        if let Some((email, password)) = parse_basic_authorization(value) {
            let verified = user_dao
                .find_by_email_ignore_case(&email)
                .ok()
                .flatten()
                .filter(|user| bcrypt::verify(&password, &user.password).unwrap_or(false));
            return match verified {
                Some(user) => {
                    let activity = ActivityDraft {
                        user_id: Some(user.id.clone()),
                        email: Some(user.email.clone()),
                        api_key_id: None,
                        api_key_comment: None,
                        success: true,
                        error: None,
                        source: "Password".into(),
                    };
                    Outcome {
                        new_session_id: Some(state.sessions.create(&user.id)),
                        auth: Some(Auth {
                            user,
                            source: AuthSource::Password,
                            api_key_id: None,
                        }),
                        activity: Some(activity),
                    }
                }
                None => Outcome {
                    auth: None,
                    new_session_id: None,
                    activity: Some(ActivityDraft {
                        user_id: None,
                        email: Some(email),
                        api_key_id: None,
                        api_key_comment: None,
                        success: false,
                        error: Some("Bad credentials".into()),
                        source: "Password".into(),
                    }),
                },
            };
        }
    }

    Outcome {
        auth: None,
        new_session_id: None,
        activity: None,
    }
}

async fn resolve_session_and_remember(
    state: &AppState,
    parts: &Parts,
    session_id: Option<String>,
) -> Outcome {
    let user_dao = UserDao::new(state.db.clone());

    // 3. session (X-Auth-Token header takes precedence over cookie)
    if let Some(session_id) = session_id {
        if let Some(user_id) = state.sessions.get(&session_id) {
            if let Ok(Some(user)) = user_dao.find_by_id(&user_id) {
                return Outcome {
                    auth: Some(Auth {
                        user,
                        source: AuthSource::Session,
                        api_key_id: None,
                    }),
                    new_session_id: None,
                    activity: None,
                };
            }
        }
    }

    // 4. remember-me cookie
    if let Some(token) = cookie_value(parts, remember_me::REMEMBER_ME_COOKIE) {
        use base64::Engine;
        let now_millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        if let Ok(Some(key)) = state.settings.get_setting("REMEMBER_ME_KEY") {
            if let Some(decoded) = base64::engine::general_purpose::STANDARD
                .decode(&token)
                .ok()
                .and_then(|b| String::from_utf8(b).ok())
            {
                if let Some(email) = decoded.split(':').next() {
                    if let Ok(Some(user)) = user_dao.find_by_email_ignore_case(email) {
                        if remember_me::decode_token(&token, &user, &key, now_millis).is_some() {
                            let activity = ActivityDraft {
                                user_id: Some(user.id.clone()),
                                email: Some(user.email.clone()),
                                api_key_id: None,
                                api_key_comment: None,
                                success: true,
                                error: None,
                                source: "RememberMe".into(),
                            };
                            return Outcome {
                                new_session_id: Some(state.sessions.create(&user.id)),
                                auth: Some(Auth {
                                    user,
                                    source: AuthSource::RememberMe,
                                    api_key_id: None,
                                }),
                                activity: Some(activity),
                            };
                        }
                    }
                }
            }
        }
    }

    Outcome {
        auth: None,
        new_session_id: None,
        activity: None,
    }
}

pub(crate) fn cookie_value(parts: &Parts, name: &str) -> Option<String> {
    let header = parts.headers.get(axum::http::header::COOKIE)?;
    let header = header.to_str().ok()?;
    for pair in header.split(';') {
        let pair = pair.trim();
        if let Some((k, v)) = pair.split_once('=') {
            if k == name {
                return Some(v.to_string());
            }
        }
    }
    None
}

/// Per-request authentication middleware: resolve identity → store in extensions → record authentication activity → attach session/remember-me to the response.
pub async fn auth_middleware(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let (mut parts, body) = request.into_parts();
    let session_header = parts
        .headers
        .get(SESSION_HEADER_NAME)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let session_cookie = cookie_value(&parts, SESSION_COOKIE_NAME);
    let session_via_header = session_header.is_some();

    let mut outcome = resolve(&state, &parts);
    if outcome.auth.is_none() && outcome.activity.is_none() {
        outcome =
            resolve_session_and_remember(&state, &parts, session_header.or(session_cookie)).await;
    }

    state.record_activity(&outcome.activity, &parts).await;

    let issue_remember_me = outcome.auth.is_some()
        && crate::http::pagination::QueryExt::first(
            &crate::http::pagination::parse_query_multi(parts.uri.query().unwrap_or("")),
            remember_me::REMEMBER_ME_PARAM,
        )
        .map(|v| v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);

    // the remember-me cookie is built on the request side first (needs the user and settings)
    let remember_me_cookie = if issue_remember_me {
        outcome.auth.as_ref().map(|auth| {
            let settings = state.settings.get();
            let expiry = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64
                + settings.remember_me_duration.as_millis() as u64;
            let token = remember_me::encode_token(&auth.user, &settings.remember_me_key, expiry);
            format!(
                "{}={}; Path=/; Max-Age={}; HttpOnly; SameSite=Lax",
                remember_me::REMEMBER_ME_COOKIE,
                token,
                settings.remember_me_duration.as_secs(),
            )
        })
    } else {
        None
    };

    let new_session_id = outcome.new_session_id.clone();
    parts.extensions.insert(ResolvedAuth { auth: outcome.auth });

    let request = Request::from_parts(parts, body);
    let mut response = next.run(request).await;

    // session transport matches the request: if it came via X-Auth-Token → write the header, otherwise write the cookie
    if let Some(session_id) = new_session_id {
        if session_via_header {
            response.headers_mut().insert(
                axum::http::HeaderName::from_static("x-auth-token"),
                axum::http::HeaderValue::from_str(&session_id).unwrap(),
            );
        } else {
            append_set_cookie(
                &mut response,
                &format!("{SESSION_COOKIE_NAME}={session_id}; Path=/; HttpOnly; SameSite=Lax"),
            );
        }
    }

    if let Some(cookie) = remember_me_cookie {
        append_set_cookie(&mut response, &cookie);
    }

    response
}

pub(crate) fn append_set_cookie(response: &mut Response, cookie: &str) {
    response.headers_mut().append(
        axum::http::header::SET_COOKIE,
        axum::http::HeaderValue::from_str(cookie).unwrap(),
    );
}

/// Extracts the authenticated user; unauthenticated → 401 (empty body + WWW-Authenticate).
pub struct RequireAuth(pub Auth);

impl axum::extract::FromRequestParts<AppState> for RequireAuth {
    type Rejection = crate::error::ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        match parts
            .extensions
            .get::<ResolvedAuth>()
            .and_then(|r| r.auth.clone())
        {
            Some(auth) => Ok(RequireAuth(auth)),
            None => Err(crate::error::ApiError::unauthorized()),
        }
    }
}

/// Optional authentication: for anonymous endpoints.
pub struct MaybeAuth(#[allow(dead_code)] pub Option<Auth>);

impl axum::extract::FromRequestParts<AppState> for MaybeAuth {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        Ok(MaybeAuth(
            parts
                .extensions
                .get::<ResolvedAuth>()
                .and_then(|r| r.auth.clone()),
        ))
    }
}
