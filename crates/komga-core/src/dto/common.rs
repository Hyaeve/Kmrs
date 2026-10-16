//! Small shared DTOs: `AuthorDto.kt`, `WebLinkDto.kt`, `AlternateTitleDto.kt`, `GroupCountDto.kt`.

use crate::model::common::{Author, WebLink};
use crate::model::series::AlternateTitle;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorDto {
    pub name: String,
    pub role: String,
}

impl From<&Author> for AuthorDto {
    fn from(a: &Author) -> Self {
        Self {
            name: a.name.clone(),
            role: a.role.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebLinkDto {
    pub label: String,
    pub url: String,
}

impl From<&WebLink> for WebLinkDto {
    fn from(l: &WebLink) -> Self {
        Self {
            label: l.label.clone(),
            url: l.url.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlternateTitleDto {
    pub label: String,
    pub title: String,
}

impl From<&AlternateTitle> for AlternateTitleDto {
    fn from(t: &AlternateTitle) -> Self {
        Self {
            label: t.label.clone(),
            title: t.title.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupCountDto {
    pub group: String,
    pub count: i32,
}
