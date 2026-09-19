//! `FileSystemController.kt`: host file-system browsing (admin only).

use crate::auth::RequireAuth;
use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::State;
use axum::{routing, Json, Router};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub fn router() -> Router<AppState> {
    Router::new().route("/api/v1/filesystem", routing::post(get_directory_listing))
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct DirectoryRequestDto {
    pub path: String,
    pub show_files: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryListingDto {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    pub directories: Vec<PathDto>,
    pub files: Vec<PathDto>,
}

#[derive(Debug, Serialize)]
pub struct PathDto {
    #[serde(rename = "type")]
    pub type_: String,
    pub name: String,
    pub path: String,
}

async fn get_directory_listing(
    State(_state): State<AppState>,
    auth: RequireAuth,
    body: Option<Json<DirectoryRequestDto>>,
) -> Result<Json<DirectoryListingDto>, ApiError> {
    auth.0.require_admin()?;
    let request = body.map(|b| b.0).unwrap_or_default();

    if request.path.is_empty() {
        // Java's `FileSystems.getDefault().getRootDirectories()` on Unix: just `/`
        let root = Path::new("/");
        return Ok(Json(DirectoryListingDto {
            parent: None,
            directories: vec![path_dto(root)],
            files: vec![],
        }));
    }

    let path = PathBuf::from(&request.path);
    if !path.is_absolute() {
        return Err(ApiError::bad_request("Path must be absolute"));
    }
    let directory = if path.is_dir() {
        path.clone()
    } else {
        path.parent().map(Path::to_path_buf).unwrap_or(path.clone())
    };

    let read = std::fs::read_dir(&directory);
    let Ok(entries) = read else {
        return Err(ApiError::bad_request("Path does not exist"));
    };
    let mut items: Vec<PathDto> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| !is_hidden(p))
        .filter(|p| request.show_files || p.is_dir())
        .map(|p| path_dto(&p))
        .collect();
    items.sort_by_key(|p| p.path.to_lowercase());
    let (directories, files): (Vec<_>, Vec<_>) =
        items.into_iter().partition(|p| p.type_ == "directory");

    Ok(Json(DirectoryListingDto {
        parent: Some(
            path.parent()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default(),
        ),
        directories,
        files,
    }))
}

fn is_hidden(path: &Path) -> bool {
    path.file_name()
        .map(|n| n.to_string_lossy().starts_with('.'))
        .unwrap_or(false)
}

fn path_dto(path: &Path) -> PathDto {
    PathDto {
        type_: if path.is_dir() { "directory" } else { "file" }.to_string(),
        name: path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string_lossy().into_owned()),
        path: path.to_string_lossy().into_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::collections::tests::{call, insert_user, test_state};
    use axum::body::Body;
    use axum::http::{Request, StatusCode};

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

    async fn post(
        state: &AppState,
        key: &str,
        body: serde_json::Value,
    ) -> (StatusCode, serde_json::Value) {
        let (s, _, b) = call(
            state,
            router(),
            Request::post("/api/v1/filesystem")
                .header("X-API-Key", key)
                .header("Content-Type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await;
        (
            s,
            serde_json::from_slice::<serde_json::Value>(&b).unwrap_or(serde_json::Value::Null),
        )
    }

    #[tokio::test]
    async fn root_directories() {
        let state = test_state();
        let key = seed_admin(&state);
        let (status, json) = post(&state, &key, serde_json::json!({})).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["files"].as_array().unwrap().len(), 0);
        let dirs = json["directories"].as_array().unwrap();
        assert_eq!(dirs.len(), 1);
        assert_eq!(dirs[0]["type"], "directory");
        assert_eq!(dirs[0]["path"], "/");
    }

    #[tokio::test]
    async fn absolute_listing_and_filters() {
        let state = test_state();
        let key = seed_admin(&state);
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("a.txt"), b"x").unwrap();
        std::fs::write(dir.path().join(".hidden"), b"x").unwrap();

        let (status, json) = post(
            &state,
            &key,
            serde_json::json!({"path": dir.path().to_string_lossy()}),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        // default: only directories, hidden excluded
        assert_eq!(json["directories"].as_array().unwrap().len(), 1);
        assert_eq!(json["directories"][0]["name"], "sub");
        assert_eq!(json["files"].as_array().unwrap().len(), 0);
        assert_eq!(
            json["parent"],
            dir.path().parent().unwrap().to_string_lossy().to_string()
        );

        // showFiles=true: files included
        let (status, json) = post(
            &state,
            &key,
            serde_json::json!({"path": dir.path().to_string_lossy(), "showFiles": true}),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["files"].as_array().unwrap().len(), 1);
        assert_eq!(json["files"][0]["name"], "a.txt");
        assert_eq!(json["files"][0]["type"], "file");

        // relative path → 400
        let (status, json) = post(&state, &key, serde_json::json!({"path": "some/relative"})).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(json["message"]
            .as_str()
            .unwrap()
            .contains("Path must be absolute"));

        // nonexistent → 400
        let (status, json) = post(
            &state,
            &key,
            serde_json::json!({"path": "/definitely/not/here"}),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(json["message"]
            .as_str()
            .unwrap()
            .contains("Path does not exist"));

        // non-admin → 403
        insert_user(&state.db, "u@b.c", &[], &[], Default::default(), "u");
        let (status, _) = post(&state, "u", serde_json::json!({})).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }
}
