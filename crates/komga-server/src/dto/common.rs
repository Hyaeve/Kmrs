//! Common DTO types: the default Jackson serialization shape of Spring Data `PageImpl`.

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
    /// Aligned with komga `SeriesDtoDao` construction: even when unpaged, returns PageRequest(page=0, size=max(total,20)).
    pub fn of(content: Vec<T>, total: u64, pageable: &Pageable) -> Self {
        let number_of_elements = content.len();
        let (page_number, page_size, offset) = if pageable.unpaged {
            (0, total.max(20) as u32, 0)
        } else {
            (pageable.page, pageable.size.max(1), pageable.offset())
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
}
