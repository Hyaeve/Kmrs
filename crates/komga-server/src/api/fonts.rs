//! `FontsController.kt`: font families and downloads (embedded OpenDyslexic + fonts dir).

use crate::auth::MaybeAuth;
use crate::error::ApiError;
use crate::http::headers::content_disposition;
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::response::{IntoResponse, Response};
use axum::{routing, Json, Router};
use std::collections::BTreeMap;
use std::path::{Path as FsPath, PathBuf};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/fonts/families", routing::get(get_fonts))
        .route(
            "/api/v1/fonts/resource/{fontFamily}/{fontFile}",
            routing::get(get_font_file),
        )
        .route(
            "/api/v1/fonts/resource/{fontFamily}/css",
            routing::get(get_font_family_as_css),
        )
}

const SUPPORTED_EXTENSIONS: [&str; 4] = ["woff", "woff2", "ttf", "otf"];

const EMBEDDED_FONTS: [(&str, &[u8]); 8] = [
    (
        "OpenDyslexic-Bold-Italic.woff",
        include_bytes!("../../resources/embeddedFonts/OpenDyslexic/OpenDyslexic-Bold-Italic.woff"),
    ),
    (
        "OpenDyslexic-Bold-Italic.woff2",
        include_bytes!("../../resources/embeddedFonts/OpenDyslexic/OpenDyslexic-Bold-Italic.woff2"),
    ),
    (
        "OpenDyslexic-Bold.woff",
        include_bytes!("../../resources/embeddedFonts/OpenDyslexic/OpenDyslexic-Bold.woff"),
    ),
    (
        "OpenDyslexic-Bold.woff2",
        include_bytes!("../../resources/embeddedFonts/OpenDyslexic/OpenDyslexic-Bold.woff2"),
    ),
    (
        "OpenDyslexic-Italic.woff",
        include_bytes!("../../resources/embeddedFonts/OpenDyslexic/OpenDyslexic-Italic.woff"),
    ),
    (
        "OpenDyslexic-Italic.woff2",
        include_bytes!("../../resources/embeddedFonts/OpenDyslexic/OpenDyslexic-Italic.woff2"),
    ),
    (
        "OpenDyslexic-Regular.woff",
        include_bytes!("../../resources/embeddedFonts/OpenDyslexic/OpenDyslexic-Regular.woff"),
    ),
    (
        "OpenDyslexic-Regular.woff2",
        include_bytes!("../../resources/embeddedFonts/OpenDyslexic/OpenDyslexic-Regular.woff2"),
    ),
];

enum FontSource {
    Embedded(&'static [u8]),
    File(PathBuf),
}

struct FontRegistry {
    families: BTreeMap<String, Vec<(String, FontSource)>>,
}

impl FontRegistry {
    fn load(fonts_dir: &FsPath) -> Self {
        let mut families: BTreeMap<String, Vec<(String, FontSource)>> = BTreeMap::new();
        families.insert(
            "OpenDyslexic".to_string(),
            EMBEDDED_FONTS
                .iter()
                .map(|(name, bytes)| (name.to_string(), FontSource::Embedded(bytes)))
                .collect(),
        );
        if fonts_dir.is_dir() {
            if let Ok(entries) = std::fs::read_dir(fonts_dir) {
                for entry in entries.filter_map(|e| e.ok()) {
                    let dir = entry.path();
                    if !dir.is_dir() {
                        continue;
                    }
                    let family = entry.file_name().to_string_lossy().into_owned();
                    if let Ok(files) = std::fs::read_dir(&dir) {
                        let fonts: Vec<(String, FontSource)> = files
                            .filter_map(|e| e.ok())
                            .map(|e| e.path())
                            .filter(|p| p.is_file())
                            .filter(|p| {
                                extension_of(p).is_some_and(|ext| {
                                    SUPPORTED_EXTENSIONS
                                        .iter()
                                        .any(|e| e.eq_ignore_ascii_case(&ext))
                                })
                            })
                            .map(|p| {
                                (
                                    p.file_name()
                                        .map(|n| n.to_string_lossy().into_owned())
                                        .unwrap_or_default(),
                                    FontSource::File(p),
                                )
                            })
                            .collect();
                        if !fonts.is_empty() {
                            families.insert(family, fonts);
                        }
                    }
                }
            }
        }
        Self { families }
    }

    fn get(&self) -> &BTreeMap<String, Vec<(String, FontSource)>> {
        &self.families
    }
}

async fn get_fonts(State(state): State<AppState>, _auth: MaybeAuth) -> Json<Vec<String>> {
    Json(
        FontRegistry::load(&state.config.fonts_dir)
            .get()
            .keys()
            .cloned()
            .collect(),
    )
}

async fn get_font_file(
    State(state): State<AppState>,
    _auth: MaybeAuth,
    Path((font_family, font_file)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    let registry = FontRegistry::load(&state.config.fonts_dir);
    let Some(files) = registry.get().get(&font_family) else {
        return Err(ApiError::not_found(""));
    };
    let Some((_, source)) = files.iter().find(|(name, _)| name == &font_file) else {
        return Err(ApiError::not_found(""));
    };
    let bytes = match source {
        FontSource::Embedded(b) => b.to_vec(),
        FontSource::File(p) => std::fs::read(p).map_err(|e| ApiError::Internal(e.to_string()))?,
    };
    let extension = extension_of(FsPath::new(&font_file))
        .map(|e| e.to_lowercase())
        .unwrap_or_default();
    Ok((
        [
            (
                axum::http::header::CONTENT_TYPE,
                format!("font/{extension}"),
            ),
            (
                axum::http::header::CONTENT_DISPOSITION,
                content_disposition("attachment", &font_file),
            ),
        ],
        bytes,
    )
        .into_response())
}

async fn get_font_family_as_css(
    State(state): State<AppState>,
    _auth: MaybeAuth,
    Path(font_family): Path<String>,
) -> Result<Response, ApiError> {
    let registry = FontRegistry::load(&state.config.fonts_dir);
    let Some(files) = registry.get().get(&font_family) else {
        return Err(ApiError::not_found(""));
    };
    let mut groups: BTreeMap<(&'static str, &'static str), Vec<&str>> = BTreeMap::new();
    for (name, _) in files {
        let style = if name.to_lowercase().contains("italic") {
            "italic"
        } else {
            "normal"
        };
        let weight = if name.to_lowercase().contains("bold") {
            "bold"
        } else {
            "normal"
        };
        groups.entry((style, weight)).or_default().push(name);
    }
    let css = groups
        .iter()
        .map(|((style, weight), names)| {
            let src = names
                .iter()
                .map(|name| {
                    let extension = extension_of(FsPath::new(name))
                        .map(|e| e.to_lowercase())
                        .unwrap_or_default();
                    let format = match extension.as_str() {
                        "ttf" => "truetype",
                        "otf" => "opentype",
                        other => other,
                    };
                    format!("url('{name}') format('{format}')")
                })
                .collect::<Vec<_>>()
                .join(",");
            format!(
                "@font-face {{\n    font-family: '{font_family}';\n    src: {src};\n    font-weight: {weight};\n    font-style: {style};\n}}\n"
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    Ok((
        [
            (axum::http::header::CONTENT_TYPE, "text/css".to_string()),
            (
                axum::http::header::CONTENT_DISPOSITION,
                content_disposition("attachment", &format!("{font_family}.css")),
            ),
        ],
        css,
    )
        .into_response())
}

fn extension_of(path: &FsPath) -> Option<String> {
    path.extension().map(|e| e.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::collections::tests::{call, get, insert_user, test_state};
    use axum::http::StatusCode;

    #[tokio::test]
    async fn families_and_download() {
        let state = test_state();
        insert_user(&state.db, "u@b.c", &[], &[], Default::default(), "k");

        let (status, json) = {
            let (s, _, b) = call(&state, router(), get("/api/v1/fonts/families", "k")).await;
            (s, serde_json::from_slice::<serde_json::Value>(&b).unwrap())
        };
        assert_eq!(status, StatusCode::OK);
        let families = json.as_array().unwrap();
        assert_eq!(families.len(), 1);
        assert_eq!(families[0], "OpenDyslexic");

        // download a woff2 file
        let (status, headers, bytes) = call(
            &state,
            router(),
            get(
                "/api/v1/fonts/resource/OpenDyslexic/OpenDyslexic-Regular.woff2",
                "k",
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers["content-type"], "font/woff2");
        assert!(headers["content-disposition"]
            .to_str()
            .unwrap()
            .contains("OpenDyslexic-Regular.woff2"));
        assert_eq!(&bytes[0..4], b"wOF2");

        // unknown family/file → 404
        let (status, _, _) = call(
            &state,
            router(),
            get(
                "/api/v1/fonts/resource/Nope/OpenDyslexic-Regular.woff2",
                "k",
            ),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _, _) = call(
            &state,
            router(),
            get("/api/v1/fonts/resource/OpenDyslexic/nope.woff2", "k"),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn css_generation() {
        let state = test_state();
        insert_user(&state.db, "u@b.c", &[], &[], Default::default(), "k");

        let (status, headers, bytes) = call(
            &state,
            router(),
            get("/api/v1/fonts/resource/OpenDyslexic/css", "k"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers["content-type"], "text/css");
        let css = String::from_utf8(bytes).unwrap();
        assert!(css.contains("font-family: 'OpenDyslexic';"));
        assert!(css.contains("font-weight: bold;"));
        assert!(css.contains("font-weight: normal;"));
        assert!(css.contains("font-style: italic;"));
        assert!(css.contains("url('OpenDyslexic-Bold.woff') format('woff')"));
        assert!(css.contains("url('OpenDyslexic-Regular.woff2') format('woff2')"));

        // unknown family → 404
        let (status, _, _) = call(
            &state,
            router(),
            get("/api/v1/fonts/resource/Nope/css", "k"),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn additional_fonts_from_dir() {
        let mut state = test_state();
        let fonts_dir = tempfile::tempdir().unwrap();
        let mut config = (*state.config).clone();
        config.fonts_dir = fonts_dir.path().to_path_buf();
        state.config = std::sync::Arc::new(config);

        let family_dir = fonts_dir.path().join("Custom");
        std::fs::create_dir_all(&family_dir).unwrap();
        std::fs::write(
            family_dir.join("Custom-Regular.ttf"),
            b"\x00\x01\x00\x00fake",
        )
        .unwrap();
        std::fs::write(family_dir.join("notes.txt"), b"not a font").unwrap();

        insert_user(&state.db, "u@b.c", &[], &[], Default::default(), "k");
        let (status, json) = {
            let (s, _, b) = call(&state, router(), get("/api/v1/fonts/families", "k")).await;
            (s, serde_json::from_slice::<serde_json::Value>(&b).unwrap())
        };
        assert_eq!(status, StatusCode::OK);
        let families = json.as_array().unwrap();
        assert_eq!(families.len(), 2);

        let (status, headers, bytes) = call(
            &state,
            router(),
            get("/api/v1/fonts/resource/Custom/Custom-Regular.ttf", "k"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers["content-type"], "font/ttf");
        assert_eq!(bytes, b"\x00\x01\x00\x00fake");

        let (status, _, _) = call(
            &state,
            router(),
            get("/api/v1/fonts/resource/Custom/notes.txt", "k"),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }
}
