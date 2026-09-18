//! Models for SERVER_SETTINGS (kv) and CLIENT_SETTINGS_GLOBAL/USER.

/// A SERVER_SETTINGS row. bool/int values are stored as strings (jOOQ behavior: '1'/'true' → true).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerSetting {
    pub key: String,
    pub value: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientSettingGlobal {
    pub key: String,
    pub value: String,
    pub allow_unauthorized: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientSettingUser {
    pub user_id: String,
    pub key: String,
    pub value: String,
}
