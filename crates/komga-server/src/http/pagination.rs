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
/// `sort` (repeatable), `unpaged=true`.
///
/// Sort parsing mirrors Spring's `SortOrderParser`: each `sort` value is a
/// comma-separated list of properties, optionally followed by one direction
/// (`asc|desc`, default asc) that applies to every property in the list, and a
/// trailing `ignorecase` token (consumed but dropped — komga's `toSortField`
/// never applies it). So `sort=series,metadata.numberSort,asc` is two orders.
pub fn pageable_from_query(params: &HashMap<String, Vec<String>>) -> Pageable {
    let mut sort = vec![];
    for part in params.all("sort") {
        // Spring drops segments that are blank or only dots
        let mut elements: Vec<&str> = part
            .split(',')
            .filter(|s| s.chars().any(|c| c != '.' && !c.is_whitespace()))
            .collect();
        if elements
            .last()
            .is_some_and(|s| s.eq_ignore_ascii_case("ignorecase"))
        {
            elements.pop();
        }
        let descending = match elements.last() {
            Some(d) if d.eq_ignore_ascii_case("asc") || d.eq_ignore_ascii_case("desc") => {
                let desc = d.eq_ignore_ascii_case("desc");
                elements.pop();
                desc
            }
            _ => false,
        };
        for property in elements {
            sort.push(SortOrder {
                property: property.to_string(),
                descending,
            });
        }
    }
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
    fn sort_multiple_properties_one_direction() {
        // Spring: a trailing direction applies to every property in the same sort value
        let map = parse_query_multi("sort=series,metadata.numberSort,asc");
        let p = pageable_from_query(&map);
        assert_eq!(p.sort.len(), 2);
        assert_eq!(p.sort[0].property, "series");
        assert!(!p.sort[0].descending);
        assert_eq!(p.sort[1].property, "metadata.numberSort");
        assert!(!p.sort[1].descending);

        let map = parse_query_multi("sort=series,metadata.numberSort,desc");
        let p = pageable_from_query(&map);
        assert_eq!(p.sort.len(), 2);
        assert!(p.sort.iter().all(|o| o.descending));
    }

    #[test]
    fn sort_spring_edge_cases() {
        // direction with no property yields no order (Spring consumes "asc" as direction)
        let p = pageable_from_query(&parse_query_multi("sort=asc"));
        assert!(p.sort.is_empty());

        // a trailing ignorecase token is consumed, not treated as a property
        let p = pageable_from_query(&parse_query_multi("sort=name,ignorecase"));
        assert_eq!(p.sort.len(), 1);
        assert_eq!(p.sort[0].property, "name");
        assert!(!p.sort[0].descending);

        let p = pageable_from_query(&parse_query_multi("sort=name,desc,ignorecase"));
        assert_eq!(p.sort.len(), 1);
        assert!(p.sort[0].descending);

        // blank and dots-only segments are dropped
        let p = pageable_from_query(&parse_query_multi("sort=name,,...&sort="));
        assert_eq!(p.sort.len(), 1);
        assert_eq!(p.sort[0].property, "name");
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
