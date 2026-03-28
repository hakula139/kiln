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
    pub prev_url: Option<String>,
    pub next_url: Option<String>,
}

impl PaginationVars {
    /// Creates pagination variables for the given page number.
    #[must_use]
    pub fn new(base_path: &str, current_page: usize, total_pages: usize) -> Self {
        let prev_url = (current_page > 1).then(|| page_url(base_path, current_page - 1));
        let next_url = (current_page < total_pages).then(|| page_url(base_path, current_page + 1));

        Self {
            current_page,
            total_pages,
            prev_url,
            next_url,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- Paginator::total_pages --

    #[test]
    fn total_pages_exact_fit() {
        let items: Vec<i32> = (0..20).collect();
        let p = Paginator::new(&items, 10);
        assert_eq!(p.total_pages(), 2);
    }

    #[test]
    fn total_pages_remainder() {
        let items: Vec<i32> = (0..21).collect();
        let p = Paginator::new(&items, 10);
        assert_eq!(p.total_pages(), 3);
    }

    #[test]
    fn total_pages_empty() {
        let items: Vec<i32> = Vec::new();
        let p = Paginator::new(&items, 10);
        assert_eq!(p.total_pages(), 0);
    }

    #[test]
    fn total_pages_single_item() {
        let items = [1];
        let p = Paginator::new(&items, 10);
        assert_eq!(p.total_pages(), 1);
    }

    #[test]
    fn total_pages_per_page_equals_len() {
        let items: Vec<i32> = (0..5).collect();
        let p = Paginator::new(&items, 5);
        assert_eq!(p.total_pages(), 1);
    }

    // -- Paginator::page_items --

    #[test]
    fn page_items_first() {
        let items: Vec<i32> = (0..25).collect();
        let p = Paginator::new(&items, 10);
        assert_eq!(p.page_items(1), &(0..10).collect::<Vec<_>>());
    }

    #[test]
    fn page_items_middle() {
        let items: Vec<i32> = (0..25).collect();
        let p = Paginator::new(&items, 10);
        assert_eq!(p.page_items(2), &(10..20).collect::<Vec<_>>());
    }

    #[test]
    fn page_items_last_partial() {
        let items: Vec<i32> = (0..25).collect();
        let p = Paginator::new(&items, 10);
        assert_eq!(p.page_items(3), &(20..25).collect::<Vec<_>>());
    }

    #[test]
    fn page_items_out_of_range() {
        let items: Vec<i32> = (0..5).collect();
        let p = Paginator::new(&items, 10);
        assert!(p.page_items(2).is_empty());
    }

    #[test]
    fn page_items_zero_returns_empty() {
        let items: Vec<i32> = (0..5).collect();
        let p = Paginator::new(&items, 10);
        assert!(p.page_items(0).is_empty());
    }

    // -- page_url --

    #[test]
    fn page_url_first_page() {
        assert_eq!(page_url("/tags/rust", 1), "/tags/rust/");
    }

    #[test]
    fn page_url_subsequent() {
        assert_eq!(page_url("/tags/rust", 2), "/tags/rust/page/2/");
        assert_eq!(page_url("/tags/rust", 3), "/tags/rust/page/3/");
    }

    #[test]
    fn page_url_strips_trailing_slash() {
        assert_eq!(page_url("/tags/rust/", 2), "/tags/rust/page/2/");
    }

    #[test]
    fn page_url_zero_treated_as_first() {
        assert_eq!(page_url("/tags/rust", 0), "/tags/rust/");
    }

    // -- PaginationVars --

    #[test]
    fn pagination_vars_first_page() {
        let vars = PaginationVars::new("/tags/rust", 1, 3);
        assert_eq!(vars.current_page, 1);
        assert_eq!(vars.total_pages, 3);
        assert!(vars.prev_url.is_none());
        assert_eq!(vars.next_url.as_deref(), Some("/tags/rust/page/2/"));
    }

    #[test]
    fn pagination_vars_middle_page() {
        let vars = PaginationVars::new("/tags/rust", 2, 3);
        assert_eq!(vars.prev_url.as_deref(), Some("/tags/rust/"));
        assert_eq!(vars.next_url.as_deref(), Some("/tags/rust/page/3/"));
    }

    #[test]
    fn pagination_vars_last_page() {
        let vars = PaginationVars::new("/tags/rust", 3, 3);
        assert_eq!(vars.prev_url.as_deref(), Some("/tags/rust/page/2/"));
        assert!(vars.next_url.is_none());
    }

    #[test]
    fn pagination_vars_single_page() {
        let vars = PaginationVars::new("/tags/rust", 1, 1);
        assert!(vars.prev_url.is_none());
        assert!(vars.next_url.is_none());
    }
}
