//! `CollectionDto.kt` (SeriesCollection DTO).

use super::dto_datetime;
use crate::model::collection::SeriesCollection;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionDto {
    pub id: String,
    pub name: String,
    pub ordered: bool,
    pub series_ids: Vec<String>,
    #[serde(with = "dto_datetime")]
    pub created_date: OffsetDateTime,
    #[serde(with = "dto_datetime")]
    pub last_modified_date: OffsetDateTime,
    pub filtered: bool,
}

impl From<&SeriesCollection> for CollectionDto {
    fn from(c: &SeriesCollection) -> Self {
        Self {
            id: c.id.clone(),
            name: c.name.clone(),
            ordered: c.ordered,
            series_ids: c.series_ids.clone(),
            created_date: c.created_date,
            last_modified_date: c.last_modified_date,
            filtered: c.filtered,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_domain_and_shape() {
        let created = crate::time_codec::parse_datetime_utc("2024-01-02 03:04:05").unwrap();
        let collection = SeriesCollection {
            id: "c1".into(),
            name: "Best".into(),
            ordered: true,
            series_ids: vec!["s1".into(), "s2".into()],
            filtered: false,
            created_date: created,
            last_modified_date: created,
        };
        let dto = CollectionDto::from(&collection);
        let json = serde_json::to_value(&dto).unwrap();
        assert_eq!(json["createdDate"], "2024-01-02T03:04:05Z");
        assert_eq!(json["seriesIds"], serde_json::json!(["s1", "s2"]));
        assert_eq!(json["ordered"], true);
        assert_eq!(serde_json::from_value::<CollectionDto>(json).unwrap(), dto);
    }
}
