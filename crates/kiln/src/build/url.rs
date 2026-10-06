use std::path::Path;

/// Computes the canonical URL for a page from its output path.
///
/// For `index.html` pages (page bundles), returns the directory path with a
/// trailing slash. For other files, returns the file path as-is.
#[must_use]
pub(crate) fn page_url(base_url: &str, output_path: &Path) -> String {
    let rel = output_path.to_string_lossy();
    let path = rel.strip_suffix("index.html").unwrap_or(&rel);
    join_site_url(base_url, path)
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

/// Resolves a relative path against a page's output URL.
///
/// Absolute paths (starting with `/`) and external URLs (containing `://`) are returned as-is.
/// Relative paths are resolved against the page's directory URL (must end with `/`) so that
/// co-located assets like `assets/cover.webp` become `/posts/section/slug/assets/cover.webp`.
#[must_use]
pub(crate) fn resolve_relative_url(src: &str, page_url: &str) -> String {
    if src.starts_with('/') || src.contains("://") {
        return src.to_owned();
    }
    let path = if let Some(scheme_end) = page_url.find("://") {
        let after_scheme = scheme_end + 3;
        page_url[after_scheme..]
            .find('/')
            .map_or(page_url, |i| &page_url[after_scheme + i..])
    } else {
        page_url
    };
    format!("{path}{src}")
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── page_url ──

    #[test]
    fn page_url_index_html() {
        assert_eq!(
            page_url("https://example.com", Path::new("foo/bar/index.html")),
            "https://example.com/foo/bar/"
        );
    }

    #[test]
    fn page_url_non_index() {
        assert_eq!(
            page_url("https://example.com", Path::new("standalone.html")),
            "https://example.com/standalone.html"
        );
    }

    #[test]
    fn page_url_trailing_slash_base() {
        assert_eq!(
            page_url("https://example.com/", Path::new("foo/index.html")),
            "https://example.com/foo/"
        );
    }

    #[test]
    fn page_url_preserves_base_path() {
        for (output_path, expected) in [
            ("foo/index.html", "https://example.com/blog/foo/"),
            (
                "standalone.html",
                "https://example.com/blog/standalone.html",
            ),
        ] {
            assert_eq!(
                page_url("https://example.com/blog/", Path::new(output_path)),
                expected
            );
        }
    }

    #[test]
    fn page_url_root_index() {
        assert_eq!(
            page_url("https://example.com", Path::new("index.html")),
            "https://example.com/"
        );
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

    // ── resolve_relative_url ──

    #[test]
    fn resolve_relative_url_relative_path() {
        assert_eq!(
            resolve_relative_url("assets/cover.webp", "https://example.com/posts/foo/"),
            "/posts/foo/assets/cover.webp"
        );
    }

    #[test]
    fn resolve_relative_url_bare_path() {
        assert_eq!(
            resolve_relative_url("style.css", "/posts/my-post/"),
            "/posts/my-post/style.css"
        );
    }

    #[test]
    fn resolve_relative_url_absolute_path() {
        assert_eq!(
            resolve_relative_url("/images/cover.webp", "https://example.com/posts/foo/"),
            "/images/cover.webp"
        );
    }

    #[test]
    fn resolve_relative_url_external_url() {
        assert_eq!(
            resolve_relative_url(
                "https://cdn.example.com/img.jpg",
                "https://example.com/posts/foo/"
            ),
            "https://cdn.example.com/img.jpg"
        );
    }
}
