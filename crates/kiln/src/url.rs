use std::path::Path;
use std::str::FromStr;

use anyhow::{Context, ensure};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use serde::{Deserialize, Serialize, Serializer};

const COMPONENT_ENCODE_SET: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'.')
    .remove(b'-')
    .remove(b'_')
    .remove(b'~');

/// Absolute site URL with a host, whose path is the deployment prefix ending in `/`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct BaseUrl(url::Url);

impl BaseUrl {
    /// Path the site is deployed under, such as `/` or `/blog/`.
    #[must_use]
    pub fn prefix(&self) -> &str {
        self.0.path()
    }

    /// Joins a site-relative or root-relative path to the base URL.
    #[must_use]
    pub fn site_url(&self, path: &str) -> String {
        join_site_url(self.0.as_str(), path)
    }
}

impl FromStr for BaseUrl {
    type Err = anyhow::Error;

    fn from_str(input: &str) -> anyhow::Result<Self> {
        let mut url = url::Url::parse(input).context("invalid base_url")?;
        ensure!(
            !url.cannot_be_a_base() && url.has_host(),
            "base_url must be an absolute URL with a host"
        );
        ensure!(
            url.query().is_none() && url.fragment().is_none(),
            "base_url must not contain a query or fragment"
        );
        if !url.path().ends_with('/') {
            url.set_path(&format!("{}/", url.path()));
        }
        Ok(Self(url))
    }
}

impl TryFrom<String> for BaseUrl {
    type Error = anyhow::Error;

    fn try_from(input: String) -> anyhow::Result<Self> {
        input.parse()
    }
}

/// Templates receive the URL without its trailing slash, so `{{ config.base_url }}/path` works.
impl Serialize for BaseUrl {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.0.as_str().trim_end_matches('/'))
    }
}

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

/// Converts a relative directory path to an encoded URL path ending in `/`, or an empty string
/// for the root.
pub(crate) fn directory_url(path: &Path) -> String {
    let mut url = path_url(path);
    if !url.is_empty() {
        url.push('/');
    }
    url
}

/// Computes the canonical URL for a page from its output path.
///
/// For `index.html` pages (page bundles), returns the directory path with a
/// trailing slash. For other files, returns the file path as-is.
#[must_use]
pub(crate) fn page_url(base_url: &BaseUrl, output_path: &Path) -> String {
    match output_path.parent() {
        Some(directory)
            if output_path
                .file_name()
                .is_some_and(|name| name == "index.html") =>
        {
            base_url.site_url(&directory_url(directory))
        }
        _ => base_url.site_url(&path_url(output_path)),
    }
}

/// Joins a site-relative or root-relative path to a base URL or path prefix ending in `/`.
#[must_use]
pub(crate) fn join_site_url(base: &str, path: &str) -> String {
    format!("{base}{}", path.trim_start_matches('/'))
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

    fn base(input: &str) -> BaseUrl {
        input.parse().unwrap()
    }

    // ── BaseUrl ──

    #[test]
    fn base_url_normalizes_prefix_and_serializes_without_trailing_slash() {
        for (input, prefix, serialized) in [
            ("https://example.com", "/", "https://example.com"),
            ("https://example.com/", "/", "https://example.com"),
            (
                "https://example.com/blog",
                "/blog/",
                "https://example.com/blog",
            ),
            (
                "http://localhost:5456/blog/",
                "/blog/",
                "http://localhost:5456/blog",
            ),
        ] {
            let base_url = base(input);
            assert_eq!(base_url.prefix(), prefix, "{input}");
            assert_eq!(
                toml::Value::try_from(&base_url).unwrap(),
                toml::Value::String(serialized.into()),
                "{input}"
            );
        }
    }

    #[test]
    fn base_url_invalid_returns_error() {
        for input in [
            "example.com",
            "mailto:user@example.com",
            "file:///srv/site",
            "https://example.com/?preview=1",
            "https://example.com/#top",
        ] {
            assert!(input.parse::<BaseUrl>().is_err(), "{input}");
        }
    }

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
        for (base_url, path, expected) in [
            (
                "https://example.com",
                "foo/bar/index.html",
                "https://example.com/foo/bar/",
            ),
            (
                "https://example.com/blog",
                "foo/index.html",
                "https://example.com/blog/foo/",
            ),
            ("https://example.com", "index.html", "https://example.com/"),
            (
                "https://example.com",
                "myindex.html",
                "https://example.com/myindex.html",
            ),
            (
                "https://example.com/blog/",
                "中文/hash#query? 100%.html",
                "https://example.com/blog/%E4%B8%AD%E6%96%87/hash%23query%3F%20100%25.html",
            ),
        ] {
            assert_eq!(
                page_url(&base(base_url), Path::new(path)),
                expected,
                "{base_url}, {path}"
            );
        }
    }

    // ── join_site_url ──

    #[test]
    fn join_site_url_accepts_relative_and_root_relative_paths() {
        for (base, path, expected) in [
            (
                "https://example.com/blog/",
                "posts/article/",
                "https://example.com/blog/posts/article/",
            ),
            (
                "https://example.com/blog/",
                "/posts/article/",
                "https://example.com/blog/posts/article/",
            ),
            ("/blog/", "/style.css", "/blog/style.css"),
            ("/", "", "/"),
        ] {
            assert_eq!(join_site_url(base, path), expected, "{base}, {path}");
        }
    }
}
