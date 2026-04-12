use std::path::Path;

/// Computes the canonical URL for a page from its output path.
///
/// For `index.html` pages (page bundles), returns the directory path with a
/// trailing slash. For other files, returns the file path as-is.
#[must_use]
pub(crate) fn page_url(base_url: &str, output_path: &Path) -> String {
    let base = base_url.trim_end_matches('/');
    let rel = output_path.to_string_lossy();

    if let Some(dir) = rel.strip_suffix("index.html") {
        format!("{base}/{dir}")
    } else {
        format!("{base}/{rel}")
    }
}

/// Resolves a relative path against a page's output URL.
///
/// Absolute paths (starting with `/`) and external URLs (containing `://`)
/// are returned as-is. Relative paths are resolved against the page's
/// directory URL (must end with `/`) so that co-located assets like
/// `assets/cover.webp` become `/posts/section/slug/assets/cover.webp`.
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
    fn page_url_root_index() {
        assert_eq!(
            page_url("https://example.com", Path::new("index.html")),
            "https://example.com/"
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

    // ── resolve_relative_url ──

    #[test]
    fn resolve_relative_url_absolute_path() {
        assert_eq!(
            resolve_relative_url("/images/cover.webp", "https://example.com/posts/foo/"),
            "/images/cover.webp"
        );
    }

    #[test]
    fn resolve_relative_url_relative_path() {
        assert_eq!(
            resolve_relative_url("assets/cover.webp", "https://example.com/posts/foo/"),
            "/posts/foo/assets/cover.webp"
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

    #[test]
    fn resolve_relative_url_bare_path() {
        assert_eq!(
            resolve_relative_url("style.css", "/posts/my-post/"),
            "/posts/my-post/style.css"
        );
    }
}
