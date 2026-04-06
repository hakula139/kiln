use serde::Serialize;

/// A paginator that provides windowed views over a slice of items.
///
/// Page numbers are 1-indexed. The paginator borrows the items and
/// performs zero allocation for pagination math.
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

    /// Returns the total number of pages.
    #[must_use]
    pub fn total_pages(&self) -> usize {
        self.items.len().div_ceil(self.per_page)
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
        let end = (start + self.per_page).min(self.items.len());
        &self.items[start..end]
    }
}

/// Computes the URL for a paginated page.
///
/// Page 1 is the canonical URL (just `base_path`).
/// Page N>1 appends `page/{n}/`.
#[must_use]
pub fn page_url(base_path: &str, page_num: usize) -> String {
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
    /// Creates pagination variables for the given page number.
    #[must_use]
    pub fn new(base_path: &str, current_page: usize, total_pages: usize) -> Self {
        let base_url = base_path.trim_end_matches('/').to_owned();
        let prev_url = (current_page > 1).then(|| page_url(base_path, current_page - 1));
        let next_url = (current_page < total_pages).then(|| page_url(base_path, current_page + 1));
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
    let mut items = Vec::new();
    let mut ellipsed = false;

    for n in 1..=total_pages {
        let right = total_pages - n;
        let show = n <= 1 || right == 0 || (n + 2 >= current_page && n <= current_page + 2);

        if show {
            ellipsed = false;
            items.push(PaginationItem {
                number: Some(n),
                url: Some(page_url(base_path, n)),
                is_current: n == current_page,
            });
        } else if !ellipsed {
            ellipsed = true;
            items.push(PaginationItem {
                number: None,
                url: None,
                is_current: false,
            });
        }
    }

    items
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Paginator ──

    /// Exercises `total_pages`, `page_items` (full, partial, out-of-range),
    /// and the page-0 guard in a single paginator instance.
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
        assert_eq!(p.page_items(2).len(), 10);
    }

    #[test]
    fn paginator_empty() {
        let items: Vec<i32> = Vec::new();
        let p = Paginator::new(&items, 10);
        assert_eq!(p.total_pages(), 0);
    }

    // ── page_url ──

    #[test]
    fn page_url_canonical_vs_subsequent() {
        assert_eq!(page_url("/tags/rust", 1), "/tags/rust/");
        assert_eq!(page_url("/tags/rust", 2), "/tags/rust/page/2/");
    }

    #[test]
    fn page_url_strips_trailing_slash() {
        assert_eq!(page_url("/tags/rust/", 2), "/tags/rust/page/2/");
    }

    // ── PaginationVars ──

    #[test]
    fn pagination_vars_boundaries() {
        // First page: no prev, has next.
        let first = PaginationVars::new("/t", 1, 3);
        assert!(first.prev_url.is_none());
        assert_eq!(first.next_url.as_deref(), Some("/t/page/2/"));

        // Middle page: has both.
        let mid = PaginationVars::new("/t", 2, 3);
        assert_eq!(mid.prev_url.as_deref(), Some("/t/"));
        assert_eq!(mid.next_url.as_deref(), Some("/t/page/3/"));

        // Last page: has prev, no next.
        let last = PaginationVars::new("/t", 3, 3);
        assert_eq!(last.prev_url.as_deref(), Some("/t/page/2/"));
        assert!(last.next_url.is_none());

        // Single page: neither.
        let single = PaginationVars::new("/t", 1, 1);
        assert!(single.prev_url.is_none());
        assert!(single.next_url.is_none());
    }

    #[test]
    fn pagination_vars_items_all_shown_when_few() {
        let vars = PaginationVars::new("/t", 2, 4);
        assert_eq!(vars.items.len(), 4);

        assert_eq!(vars.items[0].number, Some(1));
        assert_eq!(vars.items[0].url.as_deref(), Some("/t/"));
        assert!(!vars.items[0].is_current);

        assert_eq!(vars.items[1].number, Some(2));
        assert_eq!(vars.items[1].url.as_deref(), Some("/t/page/2/"));
        assert!(vars.items[1].is_current);

        assert_eq!(vars.items[2].number, Some(3));
        assert_eq!(vars.items[2].url.as_deref(), Some("/t/page/3/"));
        assert!(!vars.items[2].is_current);

        assert_eq!(vars.items[3].number, Some(4));
        assert_eq!(vars.items[3].url.as_deref(), Some("/t/page/4/"));
        assert!(!vars.items[3].is_current);
    }

    #[test]
    fn pagination_vars_items_with_ellipsis() {
        // 10 pages, current = 1 → show: 1 2 3 ... 10
        let vars = PaginationVars::new("/t", 1, 10);
        let numbers: Vec<Option<usize>> = vars.items.iter().map(|i| i.number).collect();
        assert_eq!(
            numbers,
            vec![Some(1), Some(2), Some(3), None, Some(10)],
            "should show first 3, ellipsis, last"
        );
        assert!(vars.items[0].is_current);
        assert!(vars.items[3].url.is_none(), "ellipsis should have no URL");
    }

    #[test]
    fn pagination_vars_items_middle_page() {
        // 10 pages, current = 5 → show: 1 ... 3 4 5 6 7 ... 10
        let vars = PaginationVars::new("/t", 5, 10);
        let numbers: Vec<Option<usize>> = vars.items.iter().map(|i| i.number).collect();
        assert_eq!(
            numbers,
            vec![
                Some(1),
                None,
                Some(3),
                Some(4),
                Some(5),
                Some(6),
                Some(7),
                None,
                Some(10)
            ]
        );
        assert!(vars.items[4].is_current);
    }

    #[test]
    fn pagination_vars_items_last_page() {
        // 10 pages, current = 10 → show: 1 ... 8 9 10
        let vars = PaginationVars::new("/t", 10, 10);
        let numbers: Vec<Option<usize>> = vars.items.iter().map(|i| i.number).collect();
        assert_eq!(numbers, vec![Some(1), None, Some(8), Some(9), Some(10)]);
        assert!(vars.items[4].is_current);
    }

    #[test]
    fn pagination_vars_single_page_has_one_item() {
        let vars = PaginationVars::new("/t", 1, 1);
        assert_eq!(vars.items.len(), 1);
        assert!(vars.items[0].is_current);
    }
}
