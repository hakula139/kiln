use serde::Serialize;

/// A paginator that provides windowed views over a slice of items.
///
/// Page numbers are 1-indexed. Borrows items and performs zero allocation for pagination math.
#[derive(Debug)]
pub struct Paginator<'a, T> {
    items: &'a [T],
    per_page: usize,
}

impl<'a, T> Paginator<'a, T> {
    /// Creates a new paginator.
    ///
    /// # Panics
    ///
    /// Panics if `per_page` is zero.
    #[must_use]
    pub fn new(items: &'a [T], per_page: usize) -> Self {
        assert!(per_page > 0, "per_page must be positive");
        Self { items, per_page }
    }

    #[must_use]
    pub fn total_pages(&self) -> usize {
        self.items.len().div_ceil(self.per_page).max(1)
    }

    /// Returns the items on the given page (1-indexed).
    ///
    /// Returns an empty slice for out-of-range page numbers.
    #[must_use]
    pub fn page_items(&self, page_num: usize) -> &'a [T] {
        if page_num == 0 || page_num > self.total_pages() {
            return &[];
        }
        let start = (page_num - 1) * self.per_page;
        let end = start.saturating_add(self.per_page).min(self.items.len());
        &self.items[start..end]
    }
}

/// Computes the URL for a paginated page.
///
/// Page 1 is the canonical URL (just `base_path`). Page N>1 appends `page/{n}/`.
#[must_use]
pub fn paginated_url(base_path: &str, page_num: usize) -> String {
    let base = base_path.trim_end_matches('/');
    if page_num <= 1 {
        format!("{base}/")
    } else {
        format!("{base}/page/{page_num}/")
    }
}

/// Template-friendly pagination metadata.
#[derive(Debug, Clone, Serialize)]
pub struct PaginationVars {
    pub current_page: usize,
    pub total_pages: usize,
    /// Base URL for constructing page URLs (e.g., `/tags/rust`).
    ///
    /// Page 1 URL is `{base_url}/`, page N URL is `{base_url}/page/{n}/`.
    /// Useful for page-jump controls that need to navigate to arbitrary pages.
    pub base_url: String,
    pub prev_url: Option<String>,
    pub next_url: Option<String>,
    /// Numbered page entries with ellipsis markers for display.
    ///
    /// Shows first, last, and pages within ±2 of the current page.
    /// Gaps are represented by items with `number: None`.
    pub items: Vec<PaginationItem>,
}

/// A single entry in the pagination display.
///
/// When `number` is `None`, this represents an ellipsis ("...") marker.
#[derive(Debug, Clone, Serialize)]
pub struct PaginationItem {
    /// Page number, or `None` for an ellipsis marker.
    pub number: Option<usize>,
    /// Page URL, or `None` for an ellipsis marker.
    pub url: Option<String>,
    pub is_current: bool,
}

impl PaginationVars {
    /// Creates metadata for a page in `1..=total_pages`.
    ///
    /// # Panics
    ///
    /// Panics if the page is outside that range.
    #[must_use]
    pub fn new(base_path: &str, current_page: usize, total_pages: usize) -> Self {
        assert!(
            (1..=total_pages).contains(&current_page),
            "invalid page number"
        );
        let base_url = base_path.trim_end_matches('/').to_owned();
        let prev_url = (current_page > 1).then(|| paginated_url(base_path, current_page - 1));
        let next_url =
            (current_page < total_pages).then(|| paginated_url(base_path, current_page + 1));
        let items = build_pagination_items(base_path, current_page, total_pages);

        Self {
            current_page,
            total_pages,
            base_url,
            prev_url,
            next_url,
            items,
        }
    }
}

/// Builds the pagination display items with ellipsis gaps.
///
/// Always shows the first and last pages. Shows pages within ±2 of the
/// current page. Gaps between shown ranges get a single ellipsis marker.
fn build_pagination_items(
    base_path: &str,
    current_page: usize,
    total_pages: usize,
) -> Vec<PaginationItem> {
    let start = current_page.saturating_sub(2).max(1);
    let end = current_page.saturating_add(2).min(total_pages);
    let mut numbers = vec![1];
    numbers.extend(start..=end);
    numbers.push(total_pages);
    numbers.sort_unstable();
    numbers.dedup();

    let mut items = Vec::with_capacity(9);
    let mut previous = 0;
    for number in numbers {
        if number > previous + 1 {
            items.push(PaginationItem {
                number: None,
                url: None,
                is_current: false,
            });
        }
        items.push(PaginationItem {
            number: Some(number),
            url: Some(paginated_url(base_path, number)),
            is_current: number == current_page,
        });
        previous = number;
    }

    items
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Paginator ──

    #[test]
    fn paginator_basic() {
        let items: Vec<i32> = (0..25).collect();
        let p = Paginator::new(&items, 10);

        assert_eq!(p.total_pages(), 3);
        assert_eq!(p.page_items(1), &(0..10).collect::<Vec<_>>());
        assert_eq!(p.page_items(2), &(10..20).collect::<Vec<_>>());
        assert_eq!(p.page_items(3), &(20..25).collect::<Vec<_>>());
        assert!(p.page_items(0).is_empty(), "page 0 should return empty");
        assert!(
            p.page_items(4).is_empty(),
            "past last page should return empty"
        );
    }

    #[test]
    fn paginator_exact_fit() {
        let items: Vec<i32> = (0..20).collect();
        let p = Paginator::new(&items, 10);
        assert_eq!(p.total_pages(), 2);
        assert_eq!(p.page_items(2), &(10..20).collect::<Vec<_>>());
    }

    #[test]
    fn paginator_empty() {
        let items: Vec<i32> = Vec::new();
        let p = Paginator::new(&items, 10);
        assert_eq!(p.total_pages(), 1);
        assert_eq!(p.page_items(1), items.as_slice());
    }

    // ── paginated_url ──

    #[test]
    fn paginated_url_canonical_vs_subsequent() {
        assert_eq!(paginated_url("/tags/rust", 1), "/tags/rust/");
        assert_eq!(paginated_url("/tags/rust", 2), "/tags/rust/page/2/");
    }

    #[test]
    fn paginated_url_strips_trailing_slash() {
        assert_eq!(paginated_url("/tags/rust/", 2), "/tags/rust/page/2/");
    }

    // ── PaginationVars ──

    #[test]
    fn pagination_vars_boundaries() {
        for (current, total, previous, next) in [
            (1, 1, None, None),
            (1, 3, None, Some("/t/page/2/")),
            (2, 3, Some("/t/"), Some("/t/page/3/")),
            (3, 3, Some("/t/page/2/"), None),
        ] {
            let vars = PaginationVars::new("/t/", current, total);
            assert_eq!(vars.current_page, current);
            assert_eq!(vars.total_pages, total);
            assert_eq!(vars.base_url, "/t");
            assert_eq!(vars.prev_url.as_deref(), previous);
            assert_eq!(vars.next_url.as_deref(), next);
        }
    }

    #[test]
    fn pagination_vars_windowed_items() {
        for (current, total, numbers) in [
            (1, 1, vec![Some(1)]),
            (2, 4, vec![Some(1), Some(2), Some(3), Some(4)]),
            (1, 10, vec![Some(1), Some(2), Some(3), None, Some(10)]),
            (
                5,
                10,
                vec![
                    Some(1),
                    None,
                    Some(3),
                    Some(4),
                    Some(5),
                    Some(6),
                    Some(7),
                    None,
                    Some(10),
                ],
            ),
            (10, 10, vec![Some(1), None, Some(8), Some(9), Some(10)]),
        ] {
            let vars = PaginationVars::new("/t", current, total);
            let expected: Vec<_> = numbers
                .into_iter()
                .map(|number| {
                    let url = number.map(|number| {
                        if number == 1 {
                            "/t/".to_owned()
                        } else {
                            format!("/t/page/{number}/")
                        }
                    });
                    (number, url, number == Some(current))
                })
                .collect();
            assert_eq!(
                vars.items
                    .into_iter()
                    .map(|item| (item.number, item.url, item.is_current))
                    .collect::<Vec<_>>(),
                expected
            );
        }
    }
}
