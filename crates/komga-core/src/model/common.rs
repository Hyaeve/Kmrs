//! Basic types shared by multiple entities.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Author {
    pub name: String,
    pub role: String,
}

impl Author {
    /// Kotlin `Author` construction semantics: name is trimmed, role is trimmed + lowercased.
    pub fn new(name: &str, role: &str) -> Self {
        Self {
            name: name.trim().to_string(),
            role: role.trim().to_lowercase(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebLink {
    pub label: String,
    pub url: String,
}
