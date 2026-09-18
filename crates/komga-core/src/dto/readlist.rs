//! `ReadListDto.kt`.

use super::dto_datetime;
use crate::model::readlist::ReadList;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadListDto {
    pub id: String,
    pub name: String,
    pub summary: String,
    pub ordered: bool,
    pub book_ids: Vec<String>,
    #[serde(with = "dto_datetime")]
    pub created_date: OffsetDateTime,
    #[serde(with = "dto_datetime")]
    pub last_modified_date: OffsetDateTime,
    pub filtered: bool,
}

impl From<&ReadList> for ReadListDto {
    fn from(r: &ReadList) -> Self {
        Self {
            id: r.id.clone(),
            name: r.name.clone(),
            summary: r.summary.clone(),
            ordered: r.ordered,
            book_ids: r.book_ids.values().cloned().collect(),
            created_date: r.created_date,
            last_modified_date: r.last_modified_date,
            filtered: r.filtered,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_domain_book_ids_ordered() {
        let created = crate::time_codec::parse_datetime_utc("2024-01-02 03:04:05").unwrap();
        let readlist = ReadList {
            id: "r1".into(),
            name: "Marvel".into(),
            summary: "reading order".into(),
            ordered: true,
            book_ids: [
                (2, "b2".to_string()),
                (0, "b0".to_string()),
                (1, "b1".to_string()),
            ]
            .into_iter()
            .collect(),
            filtered: false,
            created_date: created,
            last_modified_date: created,
        };
        let dto = ReadListDto::from(&readlist);
        assert_eq!(dto.book_ids, vec!["b0", "b1", "b2"]);
        let json = serde_json::to_value(&dto).unwrap();
        assert_eq!(json["createdDate"], "2024-01-02T03:04:05Z");
        assert_eq!(serde_json::from_value::<ReadListDto>(json).unwrap(), dto);
    }
}
