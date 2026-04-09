use std::path::Path;

use anyhow::{Context, Result};

use crate::output::write_output;
use crate::pagination::{PaginationVars, Paginator, page_url as pagination_url};

/// Paginates items and writes rendered pages to the output directory.
///
/// For each page of the paginator, collects the items, creates pagination
/// vars, calls the render closure to produce HTML, and writes the result.
/// Always generates at least one page (even when empty).
pub(crate) fn write_paginated<T, F>(
    items: &[T],
    per_page: usize,
    base_path: &str,
    output_dir: &Path,
    mut render: F,
) -> Result<()>
where
    T: Clone,
    F: FnMut(Vec<T>, PaginationVars) -> Result<String>,
{
    let paginator = Paginator::new(items, per_page);

    for page_num in 1..=paginator.total_pages().max(1) {
        let page_items = paginator.page_items(page_num).to_vec();
        let pagination = PaginationVars::new(base_path, page_num, paginator.total_pages());

        let html = render(page_items, pagination)?;

        let rel_path = pagination_url(base_path, page_num);
        let dest = output_dir
            .join(rel_path.trim_start_matches('/'))
            .join("index.html");
        write_output(&dest, &html)
            .with_context(|| format!("failed to write {}", dest.display()))?;
    }

    Ok(())
}

/// Reads a pagination count from a nested TOML params path.
///
/// `path` specifies the keys to traverse (e.g., `["home", "paginate"]` reads
/// `params.home.paginate`).
pub(crate) fn paginate_config(params: &toml::value::Table, path: &[&str]) -> Option<usize> {
    let (&first, rest) = path.split_first()?;
    let mut current: &toml::Value = params.get(first)?;
    for key in rest {
        current = current.get(key)?;
    }
    current
        .as_integer()
        .and_then(|n| usize::try_from(n).ok())
        .filter(|&n| n > 0)
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::*;

    // ── paginate_config ──

    #[test]
    fn paginate_config_nested() {
        let params: toml::value::Table = toml::from_str(indoc! {r"
                [home]
                paginate = 8
            "})
        .unwrap();
        assert_eq!(paginate_config(&params, &["home", "paginate"]), Some(8));
    }

    #[test]
    fn paginate_config_flat() {
        let params: toml::value::Table = toml::from_str("paginate = 16").unwrap();
        assert_eq!(paginate_config(&params, &["paginate"]), Some(16));
    }

    #[test]
    fn paginate_config_missing_returns_none() {
        let params: toml::value::Table = toml::from_str("").unwrap();
        assert_eq!(paginate_config(&params, &["paginate"]), None);
    }

    #[test]
    fn paginate_config_negative_returns_none() {
        let params: toml::value::Table = toml::from_str("paginate = -1").unwrap();
        assert_eq!(paginate_config(&params, &["paginate"]), None);
    }

    #[test]
    fn paginate_config_zero_returns_none() {
        let params: toml::value::Table = toml::from_str("paginate = 0").unwrap();
        assert_eq!(paginate_config(&params, &["paginate"]), None);
    }

    #[test]
    fn paginate_config_empty_path_returns_none() {
        let params: toml::value::Table = toml::from_str("paginate = 10").unwrap();
        assert_eq!(paginate_config(&params, &[]), None);
    }
}
