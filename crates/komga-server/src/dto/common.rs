//! Common DTO types: the default Jackson serialization shape of Spring Data `PageImpl`
//! (v1/v2), and komga 1.29's flat `PageResponse` (v3).

use serde::Serialize;

/// `Pageable` request parameter parsing result.
#[derive(Debug, Clone)]
pub struct Pageable {
    /// 0-based page number
    pub page: u32,
    pub size: u32,
    pub sort: Vec<SortOrder>,
    pub unpaged: bool,
}

impl Default for Pageable {
    fn default() -> Self {
        Self {
            page: 0,
            size: 20,
            sort: Vec::new(),
            unpaged: false,
        }
    }
}

impl Pageable {
    pub fn offset(&self) -> u64 {
        self.page as u64 * self.size as u64
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)] // used when building SQL ORDER BY in M3
pub struct SortOrder {
    pub property: String,
    pub descending: bool,
}

/// JSON shape of `Sort`: `{"empty":..,"sorted":..,"unsorted":..}`.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct SortDto {
    pub empty: bool,
    pub sorted: bool,
    pub unsorted: bool,
}

impl SortDto {
    pub fn of(orders: &[SortOrder]) -> Self {
        Self {
            empty: orders.is_empty(),
            sorted: !orders.is_empty(),
            unsorted: orders.is_empty(),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageableDto {
    pub page_number: u32,
    pub page_size: u32,
    pub sort: SortDto,
    pub offset: u64,
    pub paged: bool,
    pub unpaged: bool,
}

/// Spring `PageImpl` JSON.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page<T: Serialize> {
    pub content: Vec<T>,
    pub pageable: PageableDto,
    pub total_elements: u64,
    pub total_pages: u32,
    pub number: u32,
    pub size: u32,
    pub sort: SortDto,
    pub first: bool,
    pub last: bool,
    pub number_of_elements: usize,
    pub empty: bool,
}

impl<T: Serialize> Page<T> {
    /// Page JSON from a DTO query result: the sort flags reflect whether the query actually
    /// applied an ORDER BY (komga's `buildPage` puts the effective sort into the pageable,
    /// so default-sorted endpoints render `sorted: true` even without a `sort` query param).
    pub fn of_dto(dto: komga_db::dto_dao::DtoPage<T>, pageable: &Pageable) -> Self {
        let mut page = Self::of(dto.items, dto.total.max(0) as u64, pageable);
        let sort = if dto.sorted {
            SortDto {
                empty: false,
                sorted: true,
                unsorted: false,
            }
        } else {
            SortDto::of(&[])
        };
        page.sort = sort;
        page.pageable.sort = sort;
        page
    }

    /// Aligned with komga `SeriesDtoDao` construction: even when unpaged, returns PageRequest(page=0, size=max(total,20)).
    pub fn of(content: Vec<T>, total: u64, pageable: &Pageable) -> Self {
        let number_of_elements = content.len();
        let (page_number, page_size, offset) = if pageable.unpaged {
            (0, total.max(20) as u32, 0)
        } else {
            (pageable.page, pageable.size.max(1), pageable.offset())
        };
        // Spring `PageImpl` shrinks an over-reported total down to offset + content size on a
        // non-full page (e.g. referential counts include NULL rows the items list drops)
        let total = if number_of_elements > 0 && offset + page_size as u64 > total {
            offset + number_of_elements as u64
        } else {
            total
        };
        let total_pages = total.div_ceil(page_size as u64) as u32;
        Self {
            empty: number_of_elements == 0,
            first: page_number == 0,
            last: page_number + 1 >= total_pages,
            number: page_number,
            size: page_size,
            number_of_elements,
            total_elements: total,
            total_pages,
            content,
            sort: SortDto::of(&pageable.sort),
            pageable: PageableDto {
                page_number,
                page_size,
                sort: SortDto::of(&pageable.sort),
                offset,
                paged: true,
                unpaged: false,
            },
        }
    }
}

/// komga 1.29 v3 `PageResponse` JSON (`application/query/PageResponse.kt`): a flat shape
/// without the `PageImpl` pageable/sort baggage.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageResponse<T: Serialize> {
    pub content: Vec<T>,
    pub page: u32,
    pub size: u32,
    pub total_elements: u64,
    pub total_pages: u32,
    pub first: bool,
    pub last: bool,
}

impl<T: Serialize> PageResponse<T> {
    /// `PageResult.toPageResponse`: unpaged reports page=0, size=fetched count, and a single
    /// page (0 when empty) — the total is the fetched size since unpaged queries skip COUNT.
    pub fn of_dto(dto: komga_db::dto_dao::DtoPage<T>, pageable: &Pageable) -> Self {
        let fetched = dto.items.len();
        let total = dto.total.max(0) as u64;
        let (page, size, total_pages) = if pageable.unpaged {
            (0, fetched as u32, if total == 0 { 0 } else { 1 })
        } else {
            let size = pageable.size.max(1);
            (pageable.page, size, total.div_ceil(size as u64) as u32)
        };
        Self {
            content: dto.items,
            page,
            size,
            total_elements: total,
            total_pages,
            first: page == 0,
            last: page >= total_pages.saturating_sub(1) || total_pages == 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_json_shape() {
        let page = Page::of(
            vec![1, 2],
            8,
            &Pageable {
                page: 2,
                size: 3,
                sort: vec![],
                unpaged: false,
            },
        );
        let json = serde_json::to_value(&page).unwrap();
        assert_eq!(json["pageable"]["pageNumber"], 2);
        assert_eq!(json["pageable"]["pageSize"], 3);
        assert_eq!(json["pageable"]["offset"], 6);
        assert_eq!(json["pageable"]["paged"], true);
        assert_eq!(json["pageable"]["unpaged"], false);
        assert_eq!(
            json["pageable"]["sort"],
            serde_json::json!({"empty":true,"sorted":false,"unsorted":true})
        );
        assert_eq!(json["totalElements"], 8);
        assert_eq!(json["totalPages"], 3);
        assert_eq!(json["number"], 2);
        assert_eq!(json["size"], 3);
        assert_eq!(json["first"], false);
        assert_eq!(json["last"], true);
        assert_eq!(json["numberOfElements"], 2);
        assert_eq!(json["empty"], false);

        // middle page has last=false
        let page = Page::of(
            vec![1, 2, 3],
            8,
            &Pageable {
                page: 1,
                size: 3,
                sort: vec![],
                unpaged: false,
            },
        );
        let json = serde_json::to_value(&page).unwrap();
        assert_eq!(json["first"], false);
        assert_eq!(json["last"], false);
    }

    #[test]
    fn unpaged_shape() {
        // komga returns PageRequest(0, max(total,20)) for unpaged: paged=true, unpaged=false
        let page = Page::of(
            vec![1, 2],
            2,
            &Pageable {
                page: 0,
                size: 20,
                sort: vec![],
                unpaged: true,
            },
        );
        let json = serde_json::to_value(&page).unwrap();
        assert_eq!(json["pageable"]["paged"], true);
        assert_eq!(json["pageable"]["unpaged"], false);
        assert_eq!(json["pageable"]["pageSize"], 20);
        assert_eq!(json["totalPages"], 1);

        // when total > 20, pageSize = total
        let page = Page::of(
            vec![0; 30],
            30,
            &Pageable {
                page: 0,
                size: 20,
                sort: vec![],
                unpaged: true,
            },
        );
        let json = serde_json::to_value(&page).unwrap();
        assert_eq!(json["pageable"]["pageSize"], 30);
        assert_eq!(json["size"], 30);
    }

    #[test]
    fn page_response_shape() {
        let dto = |items: Vec<i32>, total| komga_db::dto_dao::DtoPage {
            items,
            total,
            sorted: false,
        };

        // unpaged: page=0, size=fetched count, single page
        let json = serde_json::to_value(PageResponse::of_dto(
            dto(vec![1, 2], 2),
            &Pageable {
                unpaged: true,
                ..Default::default()
            },
        ))
        .unwrap();
        assert_eq!(json["page"], 0);
        assert_eq!(json["size"], 2);
        assert_eq!(json["totalPages"], 1);
        assert_eq!(json["first"], true);
        assert_eq!(json["last"], true);
        assert!(json.get("pageable").is_none());

        // unpaged empty: totalPages=0, last=true
        let json = serde_json::to_value(PageResponse::of_dto(
            dto(vec![], 0),
            &Pageable {
                unpaged: true,
                ..Default::default()
            },
        ))
        .unwrap();
        assert_eq!(json["size"], 0);
        assert_eq!(json["totalPages"], 0);
        assert_eq!(json["last"], true);

        // paged middle page: neither first nor last
        let json = serde_json::to_value(PageResponse::of_dto(
            dto(vec![4, 5, 6], 8),
            &Pageable {
                page: 1,
                size: 3,
                sort: vec![],
                unpaged: false,
            },
        ))
        .unwrap();
        assert_eq!(json["page"], 1);
        assert_eq!(json["totalPages"], 3);
        assert_eq!(json["first"], false);
        assert_eq!(json["last"], false);
    }
}
