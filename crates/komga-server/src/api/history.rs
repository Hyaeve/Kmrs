//! `HistoricalEventController.kt`: historical events list (admin only).

use crate::auth::RequireAuth;
use crate::dto::common::Page;
use crate::error::ApiError;
use crate::http::pagination::QueryPageable;
use crate::state::AppState;
use axum::extract::State;
use axum::{routing, Json, Router};
use komga_core::dto::dto_datetime;
use komga_core::model::history::HistoricalEvent;
use komga_db::dao::history::HistoricalEventDao;
use komga_db::dto_dao::{DtoPage, PageRequest};
use serde::Serialize;
use std::collections::BTreeMap;

pub fn router() -> Router<AppState> {
    Router::new().route("/api/v1/history", routing::get(get_historical_events))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoricalEventDto {
    pub id: String,
    #[serde(rename = "type")]
    pub type_: String,
    #[serde(with = "dto_datetime")]
    pub timestamp: time::OffsetDateTime,
    pub book_id: Option<String>,
    pub series_id: Option<String>,
    pub properties: BTreeMap<String, String>,
}

impl From<&HistoricalEvent> for HistoricalEventDto {
    fn from(e: &HistoricalEvent) -> Self {
        Self {
            id: e.id.clone(),
            type_: e.type_.as_str().to_string(),
            timestamp: e.timestamp,
            book_id: e.book_id.clone(),
            series_id: e.series_id.clone(),
            properties: e.properties.clone(),
        }
    }
}

async fn get_historical_events(
    State(state): State<AppState>,
    auth: RequireAuth,
    query: QueryPageable,
) -> Result<Json<Page<HistoricalEventDto>>, ApiError> {
    auth.0.require_admin()?;
    let sort = if query.pageable.sort.is_empty() {
        vec![crate::dto::common::SortOrder {
            property: "timestamp".into(),
            descending: true,
        }]
    } else {
        query.pageable.sort.clone()
    };
    let page = HistoricalEventDao::new(state.db.clone()).find_all_paged(&PageRequest {
        page: query.pageable.page,
        size: query.pageable.size,
        unpaged: query.pageable.unpaged,
        sort: sort
            .iter()
            .map(|s| komga_db::dto_dao::SortOrder {
                property: s.property.clone(),
                descending: s.descending,
            })
            .collect(),
    })?;
    let mut p = query.pageable.clone();
    // Spring echoes the effective sort in the pageable
    p.sort = if page.sorted { sort } else { vec![] };
    let total = page.total;
    Ok(Json(Page::of(
        map_items(page, |e| HistoricalEventDto::from(&e)).items,
        total.max(0) as u64,
        &p,
    )))
}

fn map_items<T, U>(page: DtoPage<T>, f: impl Fn(T) -> U) -> DtoPage<U> {
    DtoPage {
        items: page.items.into_iter().map(f).collect(),
        total: page.total,
        sorted: page.sorted,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::collections::tests::{call, get, insert_user, test_state};
    use axum::http::StatusCode;
    use komga_core::model::history::{HistoricalEvent, HistoricalEventType};
    use komga_core::time_codec::now_utc;

    fn seed_admin(state: &AppState) -> String {
        insert_user(
            &state.db,
            "a@b.c",
            &[komga_core::model::user::UserRole::Admin],
            &[],
            Default::default(),
            "k",
        );
        "k".to_string()
    }

    #[tokio::test]
    async fn list_with_default_sort() {
        let state = test_state();
        let key = seed_admin(&state);
        let dao = HistoricalEventDao::new(state.db.clone());
        dao.insert(&HistoricalEvent {
            id: String::new(),
            type_: HistoricalEventType::BookFileDeleted,
            book_id: Some("b1".into()),
            series_id: Some("s1".into()),
            properties: BTreeMap::from([("name".to_string(), "/l/b.cbz".to_string())]),
            timestamp: now_utc(),
        })
        .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        dao.insert(&HistoricalEvent {
            id: String::new(),
            type_: HistoricalEventType::SeriesFolderDeleted,
            book_id: None,
            series_id: Some("s2".into()),
            properties: BTreeMap::new(),
            timestamp: now_utc(),
        })
        .unwrap();

        let (status, json) = {
            let (s, _, b) = call(&state, router(), get("/api/v1/history", &key)).await;
            (s, serde_json::from_slice::<serde_json::Value>(&b).unwrap())
        };
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["totalElements"], 2);
        // default sort is timestamp desc: the later event comes first
        assert_eq!(json["content"][0]["type"], "SeriesFolderDeleted");
        assert_eq!(json["content"][1]["type"], "BookFileDeleted");
        assert_eq!(json["content"][1]["properties"]["name"], "/l/b.cbz");
        assert_eq!(json["content"][1]["bookId"], "b1");
        assert_eq!(json["pageable"]["sort"]["sorted"], true);

        // non-admin → 403
        insert_user(&state.db, "u@b.c", &[], &[], Default::default(), "u");
        let (status, _, _) = call(&state, router(), get("/api/v1/history", "u")).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }
}
