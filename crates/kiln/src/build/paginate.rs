use std::path::Path;

use anyhow::{Context, Result};

use crate::output::write_output;
use crate::pagination::{PaginationVars, Paginator};
use crate::url::{join_site_url, path_url};

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
    let url = join_site_url(deployment_prefix, &path_url(base_path));
    for page_num in 1..=page_count {
        let page_items = paginator.page_items(page_num);
        let pagination = PaginationVars::new(&url, page_num, page_count);

        let html = render(page_items, pagination)?;

        let dest = output_dir.join(paginated_path(base_path, page_num));
        write_output(&dest, &html)
            .with_context(|| format!("failed to write {}", dest.display()))?;
    }

    Ok(page_count)
}

pub(super) fn paginated_path(base: &Path, number: usize) -> std::path::PathBuf {
    if number == 1 {
        base.join("index.html")
    } else {
        base.join("page")
            .join(number.to_string())
            .join("index.html")
    }
}

/// Resolves a pagination count from `params`, trying each TOML path in order and falling back to
/// `default` when none matches.
///
/// Each path is a sequence of keys to traverse (e.g., `&["home", "paginate"]` reads
/// `params.home.paginate`). Non-positive integers are treated as missing so `paginate = 0` falls
/// through to the next path or `default`.
#[must_use]
pub(super) fn paginate_config(
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
                paginate = 16

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
