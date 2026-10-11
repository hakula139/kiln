pub(crate) mod publication;
pub(crate) mod script;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use percent_encoding::percent_decode_str;
use sha2::{Digest, Sha256};

use crate::url::{join_site_url, path_url};

const FINGERPRINT_LENGTH: usize = 12;

/// Published files keyed by the source-named site paths that reference them.
#[derive(Clone, Debug)]
pub struct StaticAssetManifest {
    /// Output-relative published paths keyed by encoded source-named site paths.
    paths: BTreeMap<String, PathBuf>,
    /// Path of the configured `base_url`, which authored root-relative URLs start with.
    authored_prefix: String,
    /// Path of the effective base URL, which published URLs start with.
    deployment_prefix: String,
}

/// A site URL resolved to its published file.
pub(crate) struct ResolvedAsset<'a> {
    /// Output-relative path of the published file.
    pub(crate) path: &'a Path,
    /// Public URL, including the deployment prefix and the authored query and fragment.
    pub(crate) url: String,
}

impl Default for StaticAssetManifest {
    fn default() -> Self {
        Self {
            paths: BTreeMap::new(),
            authored_prefix: "/".to_owned(),
            deployment_prefix: "/".to_owned(),
        }
    }
}

impl StaticAssetManifest {
    /// Records source-named output paths and the relative paths their files were published at.
    pub(crate) fn new<'a>(outputs: impl IntoIterator<Item = (&'a PathBuf, &'a PathBuf)>) -> Self {
        let paths = outputs
            .into_iter()
            .map(|(source, published)| (site_path(source), published.clone()))
            .collect();
        Self {
            paths,
            ..Self::default()
        }
    }

    /// Sets the base URL paths that authored and published root-relative URLs start with.
    pub(crate) fn with_prefixes(mut self, authored: &str, deployment: &str) -> Self {
        authored.clone_into(&mut self.authored_prefix);
        deployment.clone_into(&mut self.deployment_prefix);
        self
    }

    /// Renames a generated file to its content-addressed name and records its URL.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read or renamed.
    pub(crate) fn fingerprint(&mut self, output_dir: &Path, path: &Path) -> Result<()> {
        let published = rename_fingerprinted(output_dir, path)?;
        self.paths.insert(site_path(path), published);
        Ok(())
    }

    /// Returns the public URL of the file published for an encoded source-named site path.
    ///
    /// # Errors
    ///
    /// Returns an error if no file was published for `path`.
    pub(crate) fn asset_url(&self, path: &str) -> Result<String> {
        self.paths
            .get(path)
            .map(|published| join_site_url(&self.deployment_prefix, &site_path(published)))
            .with_context(|| format!("static asset not found: {path}"))
    }

    /// Resolves an authored URL against a page's output URL, retaining its query and fragment, or
    /// returns `None` for URLs outside the site.
    ///
    /// Root-relative URLs start with the configured `base_url` path, whichever base URL the build
    /// publishes under.
    ///
    /// # Errors
    ///
    /// Returns an error for a site URL without a published file.
    pub(crate) fn resolve_local(
        &self,
        url: &str,
        page_url: &str,
    ) -> Result<Option<ResolvedAsset<'_>>> {
        self.resolve(url, page_url, &self.authored_prefix)
    }

    /// Resolves a template asset URL, where root-relative paths omit the base URL path.
    ///
    /// # Errors
    ///
    /// Returns an error for a site URL without a published file.
    pub(crate) fn resolve_template_url(
        &self,
        url: &str,
        page_url: &str,
    ) -> Result<Option<ResolvedAsset<'_>>> {
        self.resolve(url, page_url, "/")
    }

    /// Returns the published path of an authored root-relative URL path, or `None` when it is
    /// outside the configured `base_url` path.
    ///
    /// # Errors
    ///
    /// Returns an error for a site path without a published file.
    pub(crate) fn published_path(&self, path: &str) -> Result<Option<&Path>> {
        self.lookup(path, "/", &self.authored_prefix)
    }

    fn resolve(
        &self,
        url: &str,
        page_url: &str,
        root_prefix: &str,
    ) -> Result<Option<ResolvedAsset<'_>>> {
        if is_passthrough(url) {
            return Ok(None);
        }
        let end = url.find(['?', '#']).unwrap_or(url.len());
        let (path, suffix) = url.split_at(end);
        let prefix = if path.starts_with('/') {
            root_prefix
        } else {
            &self.deployment_prefix
        };
        let Some(published) = self.lookup(path, page_url, prefix)? else {
            return Ok(None);
        };
        Ok(Some(ResolvedAsset {
            path: published,
            url: format!(
                "{}{suffix}",
                join_site_url(&self.deployment_prefix, &site_path(published))
            ),
        }))
    }

    fn lookup(&self, path: &str, page_url: &str, prefix: &str) -> Result<Option<&Path>> {
        let Some(key) = site_key(path, page_url, prefix) else {
            return Ok(None);
        };
        self.paths
            .get(&key)
            .map(|published| Some(published.as_path()))
            .with_context(|| format!("static asset not found: {path}"))
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
fn is_media(path: &Path) -> bool {
    has_extension(
        path,
        &[
            "avif", "bmp", "gif", "ico", "jpeg", "jpg", "png", "webp", "otf", "ttf", "woff",
            "woff2",
        ],
    )
}

/// Whether a file is JavaScript.
fn is_script(path: &Path) -> bool {
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
fn rename_fingerprinted(output_dir: &Path, path: &Path) -> Result<PathBuf> {
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

    fn photo_manifest(authored: &str, deployment: &str) -> StaticAssetManifest {
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
        StaticAssetManifest::new(&outputs).with_prefixes(authored, deployment)
    }

    #[test]
    fn resolve_local_publishes_authored_urls_under_the_deployment_prefix() {
        for (deployment, page_url) in [
            ("/blog/", "https://example.com/blog/posts/a/"),
            ("/", "http://localhost:5456/posts/a/"),
            ("/preview/", "/preview/posts/a/"),
        ] {
            let manifest = photo_manifest("/blog/", deployment);
            for src in [
                "photo%20%25.avif?x=1#view",
                "../a/photo%20%25.avif?x=1#view",
                "/blog/posts/a/photo%20%25.avif?x=1#view",
            ] {
                let asset = manifest.resolve_local(src, page_url).unwrap().unwrap();
                assert_eq!(asset.path, Path::new("posts/a/photo %.d9298a10d1b0.avif"));
                assert_eq!(
                    asset.url,
                    format!("{deployment}posts/a/photo%20%25.d9298a10d1b0.avif?x=1#view"),
                    "{deployment} {src}"
                );
            }
        }
    }

    #[test]
    fn resolve_local_external_and_outside_prefix_urls_return_none() {
        let manifest = photo_manifest("/blog/", "/");
        for src in [
            "https://cdn.example.com/a.avif",
            "//cdn.example.com/a.avif",
            "data:image/png;base64,abc",
            "#figure",
            "/posts/a/photo%20%25.avif",
        ] {
            assert!(
                manifest.resolve_local(src, "/posts/a/").unwrap().is_none(),
                "{src}"
            );
        }
    }

    #[test]
    fn resolve_local_missing_site_asset_returns_error() {
        let err = StaticAssetManifest::default()
            .with_prefixes("/blog/", "/blog/")
            .resolve_local("missing.png?q=1", "/blog/posts/a/")
            .err()
            .unwrap()
            .to_string();

        assert!(
            err.contains("static asset not found: missing.png"),
            "got: {err}"
        );
    }

    // ── StaticAssetManifest::resolve_template_url ──

    #[test]
    fn resolve_template_url_treats_root_paths_as_site_paths() {
        let manifest = photo_manifest("/blog/", "/preview/");

        for (src, expected) in [
            (
                "/posts/a/photo%20%25.avif",
                "/preview/posts/a/photo%20%25.d9298a10d1b0.avif",
            ),
            (
                "/blog/posts/a/photo%20%25.avif",
                "/preview/blog/posts/a/photo%20%25.ba7816bf8f01.avif",
            ),
            (
                "photo%20%25.avif",
                "/preview/posts/a/photo%20%25.d9298a10d1b0.avif",
            ),
        ] {
            let asset = manifest
                .resolve_template_url(src, "/preview/posts/a/")
                .unwrap()
                .unwrap();
            assert_eq!(asset.url, expected, "{src}");
        }
    }

    // ── StaticAssetManifest::published_path ──

    #[test]
    fn published_path_strips_the_authored_prefix() {
        let manifest = photo_manifest("/blog/", "/");

        assert_eq!(
            manifest
                .published_path("/blog/posts/a/photo%20%25.avif")
                .unwrap(),
            Some(Path::new("posts/a/photo %.d9298a10d1b0.avif"))
        );
        assert_eq!(
            manifest
                .published_path("/posts/a/photo%20%25.avif")
                .unwrap(),
            None
        );
    }
}
