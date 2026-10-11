use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::output::write_output;
use crate::pagination::{PaginationVars, Paginator};
use crate::url::{directory_url, join_site_url};

/// Writes paginated output and returns the number of pages written.
///
/// Always generates at least one page (even when empty).
pub(super) fn write_paginated<T, F>(
    items: &[T],
    per_page: usize,
    base_path: &Path,
    deployment_prefix: &str,
    output_dir: &Path,
    mut render: F,
) -> Result<usize>
where
    F: FnMut(&[T], PaginationVars) -> Result<String>,
{
    let paginator = Paginator::new(items, per_page);

    let page_count = paginator.total_pages();
    let url = join_site_url(deployment_prefix, &directory_url(base_path));
    for page_num in 1..=page_count {
        let page_items = paginator.page_items(page_num);
        let pagination = PaginationVars::new(&url, page_num, page_count);

        let html = render(page_items, pagination)?;

        write_output(&output_dir.join(paginated_path(base_path, page_num)), &html)?;
    }

    Ok(page_count)
}

pub(super) fn paginated_path(base: &Path, number: usize) -> PathBuf {
    if number == 1 {
        base.join("index.html")
    } else {
        base.join("page")
            .join(number.to_string())
            .join("index.html")
    }
}
