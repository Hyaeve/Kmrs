//! Query parameter parsing and `Pageable` extraction.
//! `foo[]` and `foo` are merged as equivalents (aligned with `BracketParamsRequestWrapper`).

use crate::dto::common::{Pageable, SortOrder};
use std::collections::HashMap;

/// Parses the query string into a multi-value map, merging `foo[]` into `foo`.
pub fn parse_query_multi(query: &str) -> HashMap<String, Vec<String>> {
    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    for (key, value) in form_urlencoded::parse(query.as_bytes()) {
        let key = key.strip_suffix("[]").unwrap_or(&key).to_string();
        map.entry(key).or_default().push(value.into_owned());
    }
    map
}

pub trait QueryExt {
    fn first(&self, key: &str) -> Option<&str>;
    fn all(&self, key: &str) -> &[String];
    fn first_bool(&self, key: &str) -> Option<bool>;
    fn first_u32(&self, key: &str) -> Option<u32>;
}

impl QueryExt for HashMap<String, Vec<String>> {
    fn first(&self, key: &str) -> Option<&str> {
        self.get(key)?.first().map(String::as_str)
    }

    fn all(&self, key: &str) -> &[String] {
        self.get(key).map(Vec::as_slice).unwrap_or(&[])
    }

    fn first_bool(&self, key: &str) -> Option<bool> {
        self.first(key).map(|v| v.eq_ignore_ascii_case("true"))
    }

    fn first_u32(&self, key: &str) -> Option<u32> {
        self.first(key).and_then(|v| v.parse().ok())
    }
}

/// Parses Pageable from the query: `page` (0-based, default 0), `size` (default 20),
/// `sort=property(,asc|desc)` (repeatable, direction defaults to asc), `unpaged=true`.
pub fn pageable_from_query(params: &HashMap<String, Vec<String>>) -> Pageable {
    let sort = params
        .all("sort")
        .iter()
        .filter_map(|s| {
            let mut parts = s.splitn(2, ',');
            let property = parts.next()?.trim();
            if property.is_empty() {
                return None;
            }
            let descending = parts
                .next()
                .map(|d| d.trim().eq_ignore_ascii_case("desc"))
                .unwrap_or(false);
            Some(SortOrder {
                property: property.to_string(),
                descending,
            })
        })
        .collect();
    Pageable {
        page: params.first_u32("page").unwrap_or(0),
        size: params.first_u32("size").unwrap_or(20),
        sort,
        unpaged: params.first_bool("unpaged").unwrap_or(false),
    }
}

/// axum extractor: injects the parsed (query map, pageable) directly into the handler.
pub struct QueryPageable {
    pub params: HashMap<String, Vec<String>>,
    pub pageable: Pageable,
}

impl<S> axum::extract::FromRequestParts<S> for QueryPageable
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        let params = parse_query_multi(parts.uri.query().unwrap_or(""));
        let pageable = pageable_from_query(&params);
        Ok(Self { params, pageable })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bracket_params_merge() {
        let map = parse_query_multi("tag[]=a&tag=b&sort=name,desc&sort=created");
        assert_eq!(map.all("tag"), &["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn pageable_parsing() {
        let map = parse_query_multi("page=2&size=50&sort=name,desc&sort=created&unpaged=true");
        let p = pageable_from_query(&map);
        assert_eq!(p.page, 2);
        assert_eq!(p.size, 50);
        assert_eq!(p.sort.len(), 2);
        assert_eq!(p.sort[0].property, "name");
        assert!(p.sort[0].descending);
        assert!(!p.sort[1].descending);
        assert!(p.unpaged);
    }

    #[test]
    fn defaults() {
        let p = pageable_from_query(&HashMap::new());
        assert_eq!(p.page, 0);
        assert_eq!(p.size, 20);
        assert!(p.sort.is_empty());
        assert!(!p.unpaged);
    }
}
