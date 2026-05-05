use std::path::Path;

use anyhow::{Context, Result};

use crate::output::write_output;
use crate::pagination::{PaginationVars, Paginator, paginated_url};

/// Paginates items and writes rendered pages to the output directory.
///
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

        let rel_path = paginated_url(base_path, page_num);
        let dest = output_dir
            .join(rel_path.trim_start_matches('/'))
            .join("index.html");
        write_output(&dest, &html)
            .with_context(|| format!("failed to write {}", dest.display()))?;
    }

    Ok(())
}

/// Resolves a pagination count from `params`, trying each TOML path in order and falling back to
/// `default` when none matches.
///
/// Each path is a sequence of keys to traverse (e.g., `&["home", "paginate"]` reads
/// `params.home.paginate`). Non-positive integers are treated as missing so `paginate = 0` falls
/// through to the next path or `default`.
#[must_use]
pub(crate) fn paginate_config(
    params: &toml::value::Table,
    paths: &[&[&str]],
    default: usize,
) -> usize {
    paths
        .iter()
        .find_map(|path| paginate_at(params, path))
        .unwrap_or(default)
}

/// Reads a single nested integer at `path` from `params`. Returns `None` for missing keys,
/// non-integer values, and non-positive integers.
fn paginate_at(params: &toml::value::Table, path: &[&str]) -> Option<usize> {
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
    fn paginate_config_returns_first_matching_path() {
        let params: toml::value::Table = toml::from_str(indoc! {r"
                [home]
                paginate = 8
            "})
        .unwrap();
        let per_page = paginate_config(&params, &[&["home", "paginate"], &["paginate"]], 10);
        assert_eq!(per_page, 8);
    }

    #[test]
    fn paginate_config_falls_back_to_next_path() {
        let params: toml::value::Table = toml::from_str("paginate = 16").unwrap();
        let per_page = paginate_config(&params, &[&["home", "paginate"], &["paginate"]], 10);
        assert_eq!(per_page, 16);
    }

    #[test]
    fn paginate_config_falls_back_to_default_when_missing() {
        let params: toml::value::Table = toml::from_str("").unwrap();
        assert_eq!(paginate_config(&params, &[&["paginate"]], 10), 10);
    }

    #[test]
    fn paginate_config_skips_non_positive_values() {
        // `0` is rejected so it falls through to the next path or the default.
        let params: toml::value::Table = toml::from_str(indoc! {r"
                paginate = 0

                [home]
                paginate = -1
            "})
        .unwrap();
        let per_page = paginate_config(&params, &[&["home", "paginate"], &["paginate"]], 10);
        assert_eq!(per_page, 10);
    }

    #[test]
    fn paginate_config_falls_back_to_default_with_no_paths() {
        let params: toml::value::Table = toml::from_str("paginate = 8").unwrap();
        assert_eq!(paginate_config(&params, &[], 10), 10);
    }
}
