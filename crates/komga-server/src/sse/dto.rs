//! SSE payload DTOs, ported from `interfaces/sse/dto/`. All serialize with camelCase keys,
//! matching the Jackson output byte for byte (compact single-line JSON).

use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibrarySseDto {
    pub library_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesSseDto {
    pub series_id: String,
    pub library_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BookSseDto {
    pub book_id: String,
    pub series_id: String,
    pub library_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BookImportSseDto {
    pub book_id: Option<String>,
    pub source_file: String,
    pub success: bool,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadListSseDto {
    pub read_list_id: String,
    pub book_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionSseDto {
    pub collection_id: String,
    pub series_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadProgressSseDto {
    pub book_id: String,
    pub user_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadProgressSeriesSseDto {
    pub series_id: String,
    pub user_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThumbnailBookSseDto {
    pub book_id: String,
    pub series_id: String,
    pub selected: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThumbnailSeriesSseDto {
    pub series_id: String,
    pub selected: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThumbnailSeriesCollectionSseDto {
    pub collection_id: String,
    pub selected: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThumbnailReadListSseDto {
    pub read_list_id: String,
    pub selected: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionExpiredDto {
    pub user_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskQueueSseDto {
    pub count: i64,
    pub count_by_type: BTreeMap<String, i64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_shapes() {
        assert_eq!(
            serde_json::to_string(&BookSseDto {
                book_id: "b1".into(),
                series_id: "s1".into(),
                library_id: "l1".into(),
            })
            .unwrap(),
            r#"{"bookId":"b1","seriesId":"s1","libraryId":"l1"}"#
        );
        assert_eq!(
            serde_json::to_string(&TaskQueueSseDto {
                count: 2,
                count_by_type: [("ScanLibrary".to_string(), 2)].into_iter().collect(),
            })
            .unwrap(),
            r#"{"count":2,"countByType":{"ScanLibrary":2}}"#
        );
        assert_eq!(
            serde_json::to_string(&BookImportSseDto {
                book_id: None,
                source_file: "/data/x.cbz".into(),
                success: false,
                message: None,
            })
            .unwrap(),
            r#"{"bookId":null,"sourceFile":"/data/x.cbz","success":false,"message":null}"#
        );
    }
}
