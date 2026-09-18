//! Thumbnail DTOs: `ThumbnailBookDto.kt`, `ThumbnailSeriesDto.kt`,
//! `ThumbnailSeriesCollectionDto.kt`, `ThumbnailReadListDto.kt`.

use crate::model::thumbnail::{
    ThumbnailBook, ThumbnailReadList, ThumbnailSeries, ThumbnailSeriesCollection,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThumbnailBookDto {
    pub id: String,
    pub book_id: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub selected: bool,
    pub media_type: String,
    pub file_size: i64,
    pub width: i32,
    pub height: i32,
}

impl From<&ThumbnailBook> for ThumbnailBookDto {
    fn from(t: &ThumbnailBook) -> Self {
        Self {
            id: t.id.clone(),
            book_id: t.book_id.clone(),
            type_: t.type_.as_str().to_string(),
            selected: t.selected,
            media_type: t.media_type.clone(),
            file_size: t.file_size,
            width: t.dimension.width,
            height: t.dimension.height,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThumbnailSeriesDto {
    pub id: String,
    pub series_id: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub selected: bool,
    pub media_type: String,
    pub file_size: i64,
    pub width: i32,
    pub height: i32,
}

impl From<&ThumbnailSeries> for ThumbnailSeriesDto {
    fn from(t: &ThumbnailSeries) -> Self {
        Self {
            id: t.id.clone(),
            series_id: t.series_id.clone(),
            type_: t.type_.as_str().to_string(),
            selected: t.selected,
            media_type: t.media_type.clone(),
            file_size: t.file_size,
            width: t.dimension.width,
            height: t.dimension.height,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThumbnailSeriesCollectionDto {
    pub id: String,
    pub collection_id: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub selected: bool,
    pub media_type: String,
    pub file_size: i64,
    pub width: i32,
    pub height: i32,
}

impl From<&ThumbnailSeriesCollection> for ThumbnailSeriesCollectionDto {
    fn from(t: &ThumbnailSeriesCollection) -> Self {
        Self {
            id: t.id.clone(),
            collection_id: t.collection_id.clone(),
            type_: t.type_.as_str().to_string(),
            selected: t.selected,
            media_type: t.media_type.clone(),
            file_size: t.file_size,
            width: t.dimension.width,
            height: t.dimension.height,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThumbnailReadListDto {
    pub id: String,
    pub read_list_id: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub selected: bool,
    pub media_type: String,
    pub file_size: i64,
    pub width: i32,
    pub height: i32,
}

impl From<&ThumbnailReadList> for ThumbnailReadListDto {
    fn from(t: &ThumbnailReadList) -> Self {
        Self {
            id: t.id.clone(),
            read_list_id: t.read_list_id.clone(),
            type_: t.type_.as_str().to_string(),
            selected: t.selected,
            media_type: t.media_type.clone(),
            file_size: t.file_size,
            width: t.dimension.width,
            height: t.dimension.height,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::thumbnail::{Dimension, ThumbnailType};

    #[test]
    fn from_domain_and_shape() {
        let thumbnail = ThumbnailBook {
            id: "t1".into(),
            book_id: "b1".into(),
            thumbnail: Some(vec![1, 2, 3]),
            url: None,
            selected: true,
            type_: ThumbnailType::Generated,
            media_type: "image/jpeg".into(),
            file_size: 3,
            dimension: Dimension {
                width: 300,
                height: 450,
            },
            created_date: crate::time_codec::now_utc(),
            last_modified_date: crate::time_codec::now_utc(),
        };
        let dto = ThumbnailBookDto::from(&thumbnail);
        let json = serde_json::to_value(&dto).unwrap();
        assert_eq!(json["type"], "GENERATED");
        assert_eq!(json["mediaType"], "image/jpeg");
        assert_eq!(json["fileSize"], 3);
        assert_eq!(json["width"], 300);
        assert_eq!(json["selected"], true);
        assert_eq!(
            serde_json::from_value::<ThumbnailBookDto>(json).unwrap(),
            dto
        );
    }
}
