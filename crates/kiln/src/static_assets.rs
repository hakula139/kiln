pub(crate) mod publication;
pub(crate) mod script;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use percent_encoding::percent_decode_str;
use sha2::{Digest, Sha256};

use crate::url::{join_site_url, path_url};

const FINGERPRINT_LENGTH: usize = 12;

/// Published URLs keyed by the source-named site paths that reference them.
#[derive(Clone, Debug, Default)]
pub struct StaticAssetManifest {
    urls: BTreeMap<String, String>,
}

/// A site URL resolved to its published file.
pub(crate) struct ResolvedAsset<'a> {
    /// Encoded site-root-relative path, without the deployment prefix.
    pub(crate) path: &'a str,
    /// Public URL, including the deployment prefix and the authored query and fragment.
    pub(crate) url: String,
}

impl StaticAssetManifest {
    /// Records source-named output paths and the relative paths their files were published at.
    pub(crate) fn new<'a>(outputs: impl IntoIterator<Item = (&'a PathBuf, &'a PathBuf)>) -> Self {
        let urls = outputs
            .into_iter()
            .map(|(source, published)| (site_path(source), site_path(published)))
            .collect();
        Self { urls }
    }

    /// Publishes every file in `output_dir` as a shared asset would be, for tests that need
    /// content-addressed media without a site root.
    #[cfg(test)]
    pub(crate) fn build(output_dir: &Path) -> Result<Self> {
        let mut outputs = BTreeMap::new();
        for entry in walkdir::WalkDir::new(output_dir).sort_by_file_name() {
            let entry = entry?;
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path().strip_prefix(output_dir)?.to_owned();
            let published = if is_media(&path) {
                rename_fingerprinted(output_dir, &path)?
            } else {
                path.clone()
            };
            outputs.insert(path, published);
        }
        Ok(Self::new(&outputs))
    }

    /// Renames a generated file to its content-addressed name and records its URL.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read or renamed.
    pub(crate) fn fingerprint(&mut self, output_dir: &Path, path: &Path) -> Result<()> {
        let source = site_path(path);
        let published = site_path(&rename_fingerprinted(output_dir, path)?);
        for url in self.urls.values_mut().filter(|url| **url == source) {
            url.clone_from(&published);
        }
        self.urls.insert(source, published);
        Ok(())
    }

    /// Rejects source-named files that host cache rules for content-addressed files would match:
    /// a dotted image, font or script stem, or a dotted `site` / `page` stylesheet under
    /// `assets/css/`.
    ///
    /// # Errors
    ///
    /// Returns an error naming the first such file.
    pub(crate) fn ensure_unaddressed_names(&self) -> Result<()> {
        for (source, published) in &self.urls {
            let path = Path::new(published);
            let reserved = source == published
                && path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .and_then(|stem| stem.split_once('.'))
                    .is_some_and(|(name, _)| {
                        is_media(path)
                            || is_script(path)
                            || (has_extension(path, &["css"])
                                && matches!(name, "page" | "site")
                                && path
                                    .parent()
                                    .is_some_and(|parent| parent.ends_with("assets/css")))
                    });
            ensure!(
                !reserved,
                "{published} keeps its source name, but its dotted name is reserved for \
                 content-addressed assets and would match their immutable cache rules. Rename it, \
                 or move media to assets/"
            );
        }
        Ok(())
    }

    pub(crate) fn asset_url(&self, url: &str) -> Result<&str> {
        self.urls
            .get(url)
            .map(String::as_str)
            .with_context(|| format!("static asset not found: {url}"))
    }

    /// Resolves a URL against a page's output URL, retaining its query and fragment, or returns
    /// `None` for URLs outside the site.
    ///
    /// # Errors
    ///
    /// Returns an error for a site URL without a published file.
    pub(crate) fn resolve_local(
        &self,
        url: &str,
        page_url: &str,
        prefix: &str,
    ) -> Result<Option<ResolvedAsset<'_>>> {
        if is_passthrough(url) {
            return Ok(None);
        }
        let end = url.find(['?', '#']).unwrap_or(url.len());
        let (path, suffix) = url.split_at(end);
        let Some(key) = site_key(path, page_url, prefix) else {
            return Ok(None);
        };
        let published = self
            .urls
            .get(&key)
            .with_context(|| format!("static asset not found: {url}"))?;
        Ok(Some(ResolvedAsset {
            path: published,
            url: format!("{}{suffix}", join_site_url(prefix, published)),
        }))
    }
}

/// Returns the manifest key of a URL path resolved against `page_url`, or `None` when it falls
/// outside the deployment prefix.
fn site_key(path: &str, page_url: &str, prefix: &str) -> Option<String> {
    let base = url::Url::parse(page_url)
        .or_else(|_| url::Url::parse("https://kiln.invalid/")?.join(page_url))
        .ok()?;
    let resolved = base.join(path).ok()?;
    let path = resolved
        .path()
        .strip_prefix(prefix.trim_end_matches('/'))
        .filter(|path| path.starts_with('/'))?;
    let decoded = percent_decode_str(path).decode_utf8().ok()?;
    Some(site_path(Path::new(decoded.trim_start_matches('/'))))
}

fn site_path(path: &Path) -> String {
    format!("/{}", path_url(path))
}

/// Whether a reference leaves the site or stays within the current document.
pub(crate) fn is_passthrough(url: &str) -> bool {
    url.starts_with("//") || url.starts_with('#') || url::Url::parse(url).is_ok()
}

/// Whether a file is a raster image or font, which references no other files.
pub(crate) fn is_media(path: &Path) -> bool {
    has_extension(
        path,
        &[
            "avif", "bmp", "gif", "ico", "jpeg", "jpg", "png", "webp", "otf", "ttf", "woff",
            "woff2",
        ],
    )
}

/// Whether a file is JavaScript.
pub(crate) fn is_script(path: &Path) -> bool {
    has_extension(path, &["js", "mjs"])
}

fn has_extension(path: &Path, extensions: &[&str]) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extensions
                .iter()
                .any(|candidate| extension.eq_ignore_ascii_case(candidate))
        })
}

/// Renames a published file to `<stem>.<hash>.<ext>` in place and returns its new relative path.
pub(crate) fn rename_fingerprinted(output_dir: &Path, path: &Path) -> Result<PathBuf> {
    let source = output_dir.join(path);
    let bytes = fs::read(&source)
        .with_context(|| format!("failed to read static asset {}", source.display()))?;
    let file_stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .with_context(|| format!("static asset has no UTF-8 file stem: {}", path.display()))?;
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .with_context(|| format!("static asset has no UTF-8 extension: {}", path.display()))?;
    let published = path.with_file_name(format!(
        "{file_stem}.{}.{extension}",
        content_fingerprint(&bytes)
    ));
    let target = output_dir.join(&published);
    if target.exists() {
        bail!(
            "fingerprinted static asset conflicts with existing file {}",
            target.display()
        );
    }
    fs::rename(&source, &target).with_context(|| {
        format!(
            "failed to rename {} to {}",
            source.display(),
            target.display()
        )
    })?;
    Ok(published)
}

fn content_fingerprint(bytes: &[u8]) -> String {
    let digest = hex::encode(Sha256::digest(bytes));
    digest[..FINGERPRINT_LENGTH].to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── StaticAssetManifest::fingerprint ──

    #[test]
    fn fingerprint_renames_in_place_and_records_encoded_urls() {
        let dir = tempfile::tempdir().unwrap();
        let mut manifest = StaticAssetManifest::default();
        for (name, encoded) in [
            ("css/site.css", "css/site"),
            (
                "space % # 世界.css",
                "space%20%25%20%23%20%E4%B8%96%E7%95%8C",
            ),
            ("literal%20name.css", "literal%2520name"),
            #[cfg(unix)]
            ("question?.css", "question%3F"),
        ] {
            let path = Path::new(name);
            fs::create_dir_all(dir.path().join(path.parent().unwrap())).unwrap();
            fs::write(dir.path().join(path), "abc").unwrap();

            manifest.fingerprint(dir.path(), path).unwrap();

            assert_eq!(
                manifest.asset_url(&format!("/{encoded}.css")).unwrap(),
                format!("/{encoded}.ba7816bf8f01.css"),
            );
            assert!(!dir.path().join(path).exists());
            assert_eq!(
                fs::read(dir.path().join(name.replace(".css", ".ba7816bf8f01.css"))).unwrap(),
                b"abc",
            );
        }
    }

    #[test]
    fn fingerprint_existing_target_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("style.css"), "abc").unwrap();
        fs::write(dir.path().join("style.ba7816bf8f01.css"), "occupied").unwrap();

        let err = StaticAssetManifest::default()
            .fingerprint(dir.path(), Path::new("style.css"))
            .unwrap_err()
            .to_string();

        assert!(
            err.contains("fingerprinted static asset conflicts with existing file"),
            "got: {err}"
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("style.ba7816bf8f01.css")).unwrap(),
            "occupied"
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("style.css")).unwrap(),
            "abc"
        );
    }

    #[test]
    fn fingerprint_updates_aliases_of_the_renamed_file() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("Theme.js"), "abc").unwrap();
        let outputs = BTreeMap::from([
            (PathBuf::from("Theme.js"), PathBuf::from("Theme.js")),
            (PathBuf::from("theme.js"), PathBuf::from("Theme.js")),
        ]);
        let mut manifest = StaticAssetManifest::new(&outputs);

        manifest
            .fingerprint(dir.path(), Path::new("Theme.js"))
            .unwrap();

        for url in ["/Theme.js", "/theme.js"] {
            assert_eq!(manifest.asset_url(url).unwrap(), "/Theme.ba7816bf8f01.js");
        }
    }

    // ── StaticAssetManifest::ensure_unaddressed_names ──

    #[test]
    fn ensure_unaddressed_names_accepts_hashed_and_unreserved_names() {
        let outputs = BTreeMap::from([
            (
                PathBuf::from("assets/logo.dark.png"),
                PathBuf::from("assets/logo.dark.ba7816bf8f01.png"),
            ),
            (PathBuf::from("favicon.ico"), PathBuf::from("favicon.ico")),
            (PathBuf::from("app.js"), PathBuf::from("app.js")),
            (
                PathBuf::from("assets/css/theme.min.css"),
                PathBuf::from("assets/css/theme.min.css"),
            ),
            (PathBuf::from("site.min.css"), PathBuf::from("site.min.css")),
            (PathBuf::from("data.v1.json"), PathBuf::from("data.v1.json")),
        ]);

        StaticAssetManifest::new(&outputs)
            .ensure_unaddressed_names()
            .unwrap();
    }

    #[test]
    fn ensure_unaddressed_names_reserved_source_names_returns_error() {
        for path in [
            "logo.dark.png",
            "fonts/Example.Regular.WOFF2",
            "vendor.min.js",
            "assets/css/site.min.css",
            "posts/a/assets/css/page.print.css",
        ] {
            let outputs = BTreeMap::from([(PathBuf::from(path), PathBuf::from(path))]);

            let err = StaticAssetManifest::new(&outputs)
                .ensure_unaddressed_names()
                .unwrap_err()
                .to_string();

            assert!(
                err.contains(&format!("/{path} keeps its source name")),
                "got: {err}"
            );
        }
    }

    // ── StaticAssetManifest::asset_url ──

    #[test]
    fn asset_url_missing_asset_returns_error() {
        let err = StaticAssetManifest::default()
            .asset_url("/js/missing.js")
            .unwrap_err()
            .to_string();

        assert!(
            err.contains("static asset not found: /js/missing.js"),
            "got: {err}"
        );
    }

    // ── StaticAssetManifest::resolve_local ──

    #[test]
    fn resolve_local_published_resources_with_page_paths_and_suffixes() {
        let outputs = BTreeMap::from([
            (
                PathBuf::from("blog/posts/a/photo %.avif"),
                PathBuf::from("blog/posts/a/photo %.ba7816bf8f01.avif"),
            ),
            (
                PathBuf::from("posts/a/photo %.avif"),
                PathBuf::from("posts/a/photo %.d9298a10d1b0.avif"),
            ),
        ]);
        let manifest = StaticAssetManifest::new(&outputs);

        for src in [
            "photo%20%25.avif?x=1#view",
            "../a/photo%20%25.avif?x=1#view",
            "/blog/posts/a/photo%20%25.avif?x=1#view",
        ] {
            let asset = manifest
                .resolve_local(src, "https://example.com/blog/posts/a/", "/blog/")
                .unwrap()
                .unwrap();
            assert_eq!(asset.path, "/posts/a/photo%20%25.d9298a10d1b0.avif");
            assert_eq!(
                asset.url,
                "/blog/posts/a/photo%20%25.d9298a10d1b0.avif?x=1#view"
            );
        }

        for (src, page_url, prefix) in [
            ("https://cdn.example.com/a.avif", "/", ""),
            ("//cdn.example.com/a.avif", "/", ""),
            ("data:image/png;base64,abc", "/", ""),
            ("#figure", "/", ""),
            ("/posts/a/photo%20%25.avif", "/blog/posts/a/", "/blog"),
        ] {
            assert!(
                manifest
                    .resolve_local(src, page_url, prefix)
                    .unwrap()
                    .is_none(),
                "{src}"
            );
        }
    }

    #[test]
    fn resolve_local_missing_site_asset_returns_error() {
        let err = StaticAssetManifest::default()
            .resolve_local("missing.png?q=1", "/blog/posts/a/", "/blog")
            .err()
            .unwrap()
            .to_string();

        assert!(
            err.contains("static asset not found: missing.png?q=1"),
            "got: {err}"
        );
    }
}
