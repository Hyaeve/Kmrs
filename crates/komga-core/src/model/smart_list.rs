//! kmrs-only model: a smart list is a persisted `BookSearch`/`SeriesSearch` filter owned
//! by a user, evaluated live on each request through the same query path as the
//! `/list` endpoints — no stored membership, so per-user conditions (read status,
//! content restrictions) always reflect the requesting user.

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SmartListTarget {
    #[serde(rename = "BOOK")]
    Book,
    #[serde(rename = "SERIES")]
    Series,
}

impl SmartListTarget {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Book => "BOOK",
            Self::Series => "SERIES",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "BOOK" => Some(Self::Book),
            "SERIES" => Some(Self::Series),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SmartListVisibility {
    #[default]
    #[serde(rename = "PRIVATE")]
    Private,
    #[serde(rename = "PUBLIC")]
    Public,
    #[serde(rename = "SHARED")]
    Shared,
}

impl SmartListVisibility {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Private => "PRIVATE",
            Self::Public => "PUBLIC",
            Self::Shared => "SHARED",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "PRIVATE" => Some(Self::Private),
            "PUBLIC" => Some(Self::Public),
            "SHARED" => Some(Self::Shared),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SmartList {
    pub id: String,
    pub name: String,
    pub summary: String,
    pub owner_user_id: String,
    pub target: SmartListTarget,
    pub visibility: SmartListVisibility,
    /// JSON-serialized `BookSearch` or `SeriesSearch`, per `target`
    pub search_json: String,
    pub created_date: OffsetDateTime,
    pub last_modified_date: OffsetDateTime,
}
