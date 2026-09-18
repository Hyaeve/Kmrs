//! Mihon (Tachiyomi) DTOs: `TachiyomiReadProgressDto.kt`, `TachiyomiReadProgressV2Dto.kt`,
//! `TachiyomiReadProgressUpdateDto.kt`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TachiyomiReadProgressDto {
    pub books_count: i32,
    pub books_read_count: i32,
    pub books_unread_count: i32,
    pub books_in_progress_count: i32,
    pub last_read_continuous_index: i32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TachiyomiReadProgressV2Dto {
    pub books_count: i32,
    pub books_read_count: i32,
    pub books_unread_count: i32,
    pub books_in_progress_count: i32,
    pub last_read_continuous_number_sort: f32,
    pub max_number_sort: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TachiyomiReadProgressUpdateDto {
    /// `@PositiveOrZero`
    pub last_book_read: i32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TachiyomiReadProgressUpdateV2Dto {
    pub last_book_number_sort_read: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shape() {
        let dto = TachiyomiReadProgressV2Dto {
            books_count: 3,
            books_read_count: 1,
            books_unread_count: 1,
            books_in_progress_count: 1,
            last_read_continuous_number_sort: 1.5,
            max_number_sort: 3.0,
        };
        let json = serde_json::to_value(&dto).unwrap();
        assert_eq!(json["lastReadContinuousNumberSort"], 1.5);
        assert_eq!(json["maxNumberSort"], 3.0);
        assert_eq!(
            serde_json::from_value::<TachiyomiReadProgressV2Dto>(json).unwrap(),
            dto
        );

        let update: TachiyomiReadProgressUpdateDto =
            serde_json::from_str(r#"{"lastBookRead": 2}"#).unwrap();
        assert_eq!(update.last_book_read, 2);
    }
}
