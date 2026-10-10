use std::path::Path;

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};

const COMPONENT_ENCODE_SET: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'.')
    .remove(b'-')
    .remove(b'_')
    .remove(b'~');

/// Encodes a filesystem name as one URL path component.
pub(crate) fn encode_component(component: &str) -> String {
    utf8_percent_encode(component, COMPONENT_ENCODE_SET).to_string()
}

/// Converts a relative filesystem path to an encoded URL path using forward slashes.
pub(crate) fn path_url(path: &Path) -> String {
    path.components()
        .map(|component| encode_component(&component.as_os_str().to_string_lossy()))
        .collect::<Vec<_>>()
        .join("/")
}

/// Computes the canonical URL for a page from its output path.
///
/// For `index.html` pages (page bundles), returns the directory path with a
/// trailing slash. For other files, returns the file path as-is.
#[must_use]
pub(crate) fn page_url(base_url: &str, output_path: &Path) -> String {
    if output_path
        .file_name()
        .is_some_and(|name| name == "index.html")
    {
        let directory = path_url(output_path.parent().unwrap_or(Path::new("")));
        let url = join_site_url(base_url, &directory);
        format!("{}/", url.trim_end_matches('/'))
    } else {
        join_site_url(base_url, &path_url(output_path))
    }
}

/// Joins a site-relative path to a base URL, preserving any configured base path.
///
/// Trims slashes at the join boundary. An empty path returns the site root with a trailing slash.
#[must_use]
pub(crate) fn join_site_url(base_url: &str, path: &str) -> String {
    format!(
        "{}/{}",
        base_url.trim_end_matches('/'),
        path.trim_start_matches('/'),
    )
}

/// Adds the deployment prefix to site-root-relative asset URLs.
pub(crate) fn site_asset_url(src: &str, prefix: &str) -> String {
    if src.starts_with('/') && !src.starts_with("//") {
        join_site_url(prefix, src)
    } else {
        src.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── path_url ──

    #[test]
    fn path_url_encodes_components_and_preserves_separators() {
        let path = Path::new("中文").join("hash#query? 100%.html");
        assert_eq!(
            path_url(&path),
            "%E4%B8%AD%E6%96%87/hash%23query%3F%20100%25.html"
        );
        assert_eq!(path_url(Path::new("../image.webp")), "../image.webp");
    }

    // ── page_url ──

    #[test]
    fn page_url_encodes_paths_and_preserves_base_url() {
        for (base, path, expected) in [
            (
                "https://example.com",
                "foo/bar/index.html",
                "https://example.com/foo/bar/",
            ),
            (
                "https://example.com/",
                "foo/index.html",
                "https://example.com/foo/",
            ),
            (
                "https://example.com/blog/",
                "foo/index.html",
                "https://example.com/blog/foo/",
            ),
            ("https://example.com", "index.html", "https://example.com/"),
            (
                "https://example.com",
                "standalone.html",
                "https://example.com/standalone.html",
            ),
            (
                "https://example.com",
                "myindex.html",
                "https://example.com/myindex.html",
            ),
            (
                "https://example.com/blog/",
                "standalone.html",
                "https://example.com/blog/standalone.html",
            ),
            (
                "https://example.com/blog/",
                "中文/hash#query? 100%.html",
                "https://example.com/blog/%E4%B8%AD%E6%96%87/hash%23query%3F%20100%25.html",
            ),
        ] {
            assert_eq!(page_url(base, Path::new(path)), expected, "{base}, {path}");
        }
    }

    // ── join_site_url ──

    #[test]
    fn join_site_url_preserves_base_path() {
        for path in ["posts/article/", "/posts/article/"] {
            assert_eq!(
                join_site_url("https://example.com/blog/", path),
                "https://example.com/blog/posts/article/",
            );
        }
    }

    #[test]
    fn join_site_url_normalizes_slash_boundary() {
        assert_eq!(
            join_site_url("https://example.com///", "///style.css"),
            "https://example.com/style.css",
        );
    }

    #[test]
    fn join_site_url_empty_path_returns_site_root() {
        for (base_url, expected) in [
            ("https://example.com", "https://example.com/"),
            ("https://example.com/blog/", "https://example.com/blog/"),
            ("", "/"),
        ] {
            assert_eq!(join_site_url(base_url, ""), expected);
        }
    }
}
