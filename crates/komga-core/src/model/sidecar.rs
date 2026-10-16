//! Equivalent model for `Sidecar.kt`. The SIDECAR table only stores `SidecarStored`; type/source are derived by the scanner from file rules and are not persisted.

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

#[derive(Debug, Clone, PartialEq)]
pub struct SidecarStored {
    pub url: String,
    pub parent_url: String,
    pub last_modified_time: OffsetDateTime,
    pub library_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SidecarType {
    #[serde(rename = "ARTWORK")]
    Artwork,
    #[serde(rename = "METADATA")]
    Metadata,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SidecarSource {
    #[serde(rename = "SERIES")]
    Series,
    #[serde(rename = "BOOK")]
    Book,
}
