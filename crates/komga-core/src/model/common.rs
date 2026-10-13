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

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NotableEntity {
    pub kind: String,
    pub name: String,
}

impl NotableEntity {
    /// Kotlin `NotableEntity` construction semantics: both fields are trimmed, kind is
    /// additionally lowercased; equality is (kind, name).
    pub fn new(kind: &str, name: &str) -> Self {
        Self {
            kind: kind.trim().to_lowercase(),
            name: name.trim().to_string(),
        }
    }
}
