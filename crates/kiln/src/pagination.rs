use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::url::directory_url;

/// Splits listing items into pages of `per_page`, keeping one empty page for an empty listing.
pub(crate) fn paginate<T>(items: &[T], per_page: NonZeroUsize) -> Vec<&[T]> {
    if items.is_empty() {
        vec![items]
    } else {
        items.chunks(per_page.get()).collect()
    }
}

/// Output directory of page `number` (1-indexed) of the listing at `base`.
pub(crate) fn page_dir(base: &Path, number: usize) -> PathBuf {
    if number == 1 {
        base.to_owned()
    } else {
        base.join("page").join(number.to_string())
    }
}

/// URL of page `number` of the listing at `listing_url`, which ends in `/`.
fn numbered_url(listing_url: &str, number: usize) -> String {
    format!(
        "{listing_url}{}",
        directory_url(&page_dir(Path::new(""), number))
    )
}

/// Template-friendly pagination metadata.
#[derive(Debug, Serialize)]
pub struct PaginationVars {
    pub current_page: usize,
    pub total_pages: usize,
    /// Listing URL path without its trailing slash (e.g., `/tags/rust`), for page-jump controls.
    /// Page 1 lives at `{base_path}/` and page N at `{base_path}/page/{n}/`.
    pub base_path: String,
    pub prev_url: Option<String>,
    pub next_url: Option<String>,
    /// The first and last pages and those within two of the current one, with gaps between.
    pub items: Vec<PaginationItem>,
}

/// One entry of the numbered page navigation.
#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum PaginationItem {
    Page {
        number: usize,
        url: String,
        is_current: bool,
    },
    /// Elided pages, serialized as `none`.
    Gap,
}

impl PaginationVars {
    /// Creates metadata for page `current_page` in `1..=total_pages` of the listing at
    /// `listing_url`, a URL path ending in `/`.
    pub(crate) fn new(listing_url: &str, current_page: usize, total_pages: usize) -> Self {
        let url = |number| numbered_url(listing_url, number);
        Self {
            current_page,
            total_pages,
            base_path: listing_url.trim_end_matches('/').to_owned(),
            prev_url: (current_page > 1).then(|| url(current_page - 1)),
            next_url: (current_page < total_pages).then(|| url(current_page + 1)),
            items: pagination_items(current_page, total_pages, url),
        }
    }
}

fn pagination_items(
    current_page: usize,
    total_pages: usize,
    url: impl Fn(usize) -> String,
) -> Vec<PaginationItem> {
    let window = current_page.saturating_sub(2).max(1)..=(current_page + 2).min(total_pages);
    let mut numbers: Vec<_> = [1, total_pages].into_iter().chain(window).collect();
    numbers.sort_unstable();
    numbers.dedup();

    let mut items = Vec::new();
    let mut previous = 0;
    for number in numbers {
        if number > previous + 1 {
            items.push(PaginationItem::Gap);
        }
        items.push(PaginationItem::Page {
            number,
            url: url(number),
            is_current: number == current_page,
        });
        previous = number;
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── paginate ──

    #[test]
    fn paginate_chunks_items_and_keeps_one_empty_page() {
        let per_page = NonZeroUsize::new(10).unwrap();
        let items: Vec<_> = (0..25).collect();
        assert_eq!(
            paginate(&items, per_page)
                .iter()
                .map(|page| (page[0], page.len()))
                .collect::<Vec<_>>(),
            [(0, 10), (10, 10), (20, 5)]
        );
        assert_eq!(paginate::<i32>(&[], per_page), [&[] as &[i32]]);
    }

    // ── page_dir ──

    #[test]
    fn page_dir_nests_later_pages() {
        for (base, number, expected) in [
            ("", 1, ""),
            ("", 2, "page/2"),
            ("tags/rust", 1, "tags/rust"),
            ("tags/rust", 12, "tags/rust/page/12"),
        ] {
            assert_eq!(page_dir(Path::new(base), number), Path::new(expected));
        }
    }

    // ── PaginationVars::new ──

    #[test]
    fn new_links_neighbors_and_strips_base_path() {
        for (listing_url, current, total, base_path, previous, next) in [
            ("/", 1, 1, "", None, None),
            ("/t/", 1, 3, "/t", None, Some("/t/page/2/")),
            ("/t/", 2, 3, "/t", Some("/t/"), Some("/t/page/3/")),
            ("/blog/t/", 3, 3, "/blog/t", Some("/blog/t/page/2/"), None),
        ] {
            let vars = PaginationVars::new(listing_url, current, total);
            assert_eq!(
                (
                    vars.current_page,
                    vars.total_pages,
                    vars.base_path.as_str(),
                    vars.prev_url.as_deref(),
                    vars.next_url.as_deref()
                ),
                (current, total, base_path, previous, next)
            );
        }
    }

    #[test]
    fn new_windows_items_around_current_page() {
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
            let expected: Vec<_> = numbers
                .into_iter()
                .map(|number| match number {
                    Some(number) => PaginationItem::Page {
                        number,
                        url: if number == 1 {
                            "/t/".to_owned()
                        } else {
                            format!("/t/page/{number}/")
                        },
                        is_current: number == current,
                    },
                    None => PaginationItem::Gap,
                })
                .collect();
            assert_eq!(PaginationVars::new("/t/", current, total).items, expected);
        }
    }

    // ── PaginationItem ──

    #[test]
    fn pagination_item_gap_serializes_as_none() {
        assert!(minijinja::Value::from_serialize(PaginationItem::Gap).is_none());
    }
}
