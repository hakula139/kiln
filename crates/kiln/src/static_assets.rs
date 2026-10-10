pub(crate) mod publication;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use percent_encoding::percent_decode_str;
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

use crate::output::copy_file;
use crate::url::{join_site_url, path_url};

const FINGERPRINT_LENGTH: usize = 12;
pub(crate) const MEDIA_DIRECTORY: &str = "_assets";

/// Content-addressed URLs for static assets and canonical page stylesheets.
#[derive(Clone, Debug, Default)]
pub struct StaticAssetManifest {
    urls: BTreeMap<String, String>,
    fingerprinted_paths: BTreeSet<PathBuf>,
}

impl StaticAssetManifest {
    /// Publishes content-hashed copies of supported assets.
    ///
    /// # Errors
    ///
    /// Returns an error for unreadable files or conflicting generated paths.
    pub fn build(output_dir: &Path) -> Result<Self> {
        let mut manifest = Self::build_media(output_dir)?;
        manifest.fingerprint_code(output_dir)?;
        Ok(manifest)
    }

    pub(crate) fn build_media(output_dir: &Path) -> Result<Self> {
        for entry in fs::read_dir(output_dir)? {
            if entry?
                .file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case(MEDIA_DIRECTORY)
            {
                bail!("reserved asset directory conflicts with published files: {MEDIA_DIRECTORY}");
            }
        }
        let mut manifest = Self::default();
        manifest.collect(output_dir, false)?;
        Ok(manifest)
    }

    pub(crate) fn fingerprint_code(&mut self, output_dir: &Path) -> Result<()> {
        self.collect(output_dir, true)
    }

    fn collect(&mut self, output_dir: &Path, code: bool) -> Result<()> {
        let paths = WalkDir::new(output_dir)
            .follow_links(false)
            .sort_by_file_name()
            .into_iter()
            .filter_entry(|entry| entry.path() != output_dir.join(MEDIA_DIRECTORY))
            .filter_map(|entry| match entry {
                Ok(entry) if entry.file_type().is_file() => Some(Ok(entry.into_path())),
                Ok(_) => None,
                Err(error) => Some(Err(error)),
            })
            .collect::<std::result::Result<Vec<_>, _>>()
            .context("failed to collect published assets")?;
        for path in paths {
            let relative = path.strip_prefix(output_dir)?;
            if code && !is_code(relative) {
                continue;
            }
            let url = format!("/{}", path_url(relative));
            if !code && !is_media(relative) {
                self.urls.insert(url.clone(), url);
                continue;
            }
            let bytes = fs::read(&path)
                .with_context(|| format!("failed to read static asset {}", path.display()))?;
            let mut fingerprinted = fingerprinted_path(relative, &bytes)?;
            if !code {
                fingerprinted = Path::new(MEDIA_DIRECTORY).join(fingerprinted);
            }
            let target = output_dir.join(&fingerprinted);
            if target.exists() {
                bail!(
                    "fingerprinted static asset conflicts with existing file {}",
                    target.display()
                );
            }
            copy_file(&path, &target)?;
            self.urls
                .insert(url, format!("/{}", path_url(&fingerprinted)));
            self.fingerprinted_paths.insert(relative.to_owned());
            self.fingerprinted_paths.insert(fingerprinted);
        }
        Ok(())
    }

    pub(crate) fn asset_url(&self, url: &str) -> Result<String> {
        self.resolve(url, "/", "")
            .with_context(|| format!("asset_url: static asset not found: {url}"))
    }

    /// Resolves a published URL against a page's output URL, retaining its query and fragment.
    pub(crate) fn resolve(&self, url: &str, page_url: &str, prefix: &str) -> Option<String> {
        if is_external(url) || url.starts_with('#') {
            return Some(url.to_owned());
        }
        let end = url.find(['?', '#']).unwrap_or(url.len());
        let (path, suffix) = url.split_at(end);
        let base = url::Url::parse(page_url)
            .or_else(|_| url::Url::parse("https://kiln.invalid/")?.join(page_url))
            .ok()?;
        let resolved = base.join(path).ok()?;
        let path = resolved
            .path()
            .strip_prefix(prefix.trim_end_matches('/'))
            .filter(|path| path.starts_with('/'))?;
        let decoded = percent_decode_str(path).decode_utf8().ok()?;
        let key = format!("/{}", path_url(Path::new(decoded.trim_start_matches('/'))));
        self.urls
            .get(&key)
            .map(|published| format!("{}{suffix}", join_site_url(prefix, published)))
    }

    /// Returns relative paths of original fingerprinted assets and their generated copies.
    pub(crate) fn fingerprinted_paths(&self) -> &BTreeSet<PathBuf> {
        &self.fingerprinted_paths
    }
}

pub(crate) fn is_external(url: &str) -> bool {
    url.starts_with("//") || url::Url::parse(url).is_ok()
}

fn is_code(path: &Path) -> bool {
    matches!(extension(path).as_str(), "css" | "js" | "mjs")
}

fn is_media(path: &Path) -> bool {
    matches!(
        extension(path).as_str(),
        "avif"
            | "bmp"
            | "gif"
            | "ico"
            | "jpeg"
            | "jpg"
            | "png"
            | "webp"
            | "otf"
            | "ttf"
            | "woff"
            | "woff2"
    )
}

fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

pub(crate) fn is_fingerprinted_copy(path: &Path) -> Result<bool> {
    let Some(extension) = path.extension().and_then(|extension| extension.to_str()) else {
        return Ok(false);
    };
    if !is_code(path) {
        return Ok(false);
    }
    let Some((original_stem, fingerprint)) = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .and_then(|stem| stem.rsplit_once('.'))
    else {
        return Ok(false);
    };
    if original_stem.is_empty()
        || fingerprint.len() != FINGERPRINT_LENGTH
        || !fingerprint.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Ok(false);
    }

    if !path
        .with_file_name(format!("{original_stem}.{extension}"))
        .is_file()
    {
        return Ok(false);
    }

    let bytes = fs::read(path)
        .with_context(|| format!("failed to read static asset {}", path.display()))?;
    Ok(fingerprint == content_fingerprint(&bytes))
}

fn fingerprinted_path(path: &Path, bytes: &[u8]) -> Result<PathBuf> {
    let file_stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .with_context(|| format!("static asset has no UTF-8 file stem: {}", path.display()))?;
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .with_context(|| format!("static asset has no UTF-8 extension: {}", path.display()))?;
    let file_name = format!("{file_stem}.{}.{extension}", content_fingerprint(bytes));
    Ok(path.with_file_name(file_name))
}

fn content_fingerprint(bytes: &[u8]) -> String {
    let digest = hex::encode(Sha256::digest(bytes));
    digest[..FINGERPRINT_LENGTH].to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── StaticAssetManifest::build ──

    #[test]
    fn build_fingerprints_css_and_js() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("css")).unwrap();
        fs::create_dir_all(dir.path().join("js")).unwrap();
        fs::write(dir.path().join("css/style.css"), "abc").unwrap();
        fs::write(dir.path().join("js/app.js"), "console.log('ok')").unwrap();
        fs::write(dir.path().join("js/module.mjs"), "export const value = 1;").unwrap();

        let manifest = StaticAssetManifest::build(dir.path()).unwrap();

        assert_eq!(
            manifest.asset_url("/css/style.css").unwrap(),
            "/css/style.ba7816bf8f01.css"
        );
        assert_eq!(
            manifest.asset_url("/js/app.js").unwrap(),
            "/js/app.cf8e73474dc9.js"
        );
        assert_eq!(
            manifest.asset_url("/js/module.mjs").unwrap(),
            "/js/module.fcbcb7aece71.mjs"
        );
        assert_eq!(
            fs::read(dir.path().join("css/style.ba7816bf8f01.css")).unwrap(),
            b"abc"
        );
        assert!(dir.path().join("js/app.cf8e73474dc9.js").is_file());
        assert!(dir.path().join("js/module.fcbcb7aece71.mjs").is_file());
    }

    #[test]
    fn build_keeps_control_file_urls_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("_headers"), "image").unwrap();

        let manifest = StaticAssetManifest::build(dir.path()).unwrap();

        assert_eq!(manifest.asset_url("/_headers").unwrap(), "/_headers");
    }

    #[test]
    fn build_encodes_filename_components() {
        for (name, encoded) in [
            (
                "space % # 世界.js",
                "space%20%25%20%23%20%E4%B8%96%E7%95%8C",
            ),
            ("literal%20name.js", "literal%2520name"),
            #[cfg(unix)]
            ("question?.js", "question%3F"),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let output = dir.path();
            fs::write(output.join(name), "abc").unwrap();
            let manifest = StaticAssetManifest::build(output).unwrap();
            assert_eq!(
                manifest.asset_url(&format!("/{encoded}.js")).unwrap(),
                format!("/{encoded}.ba7816bf8f01.js"),
            );
            assert_eq!(
                fs::read(output.join(fingerprinted_path(Path::new(name), b"abc").unwrap()))
                    .unwrap(),
                b"abc",
            );
        }
    }

    #[test]
    fn build_changes_url_when_content_changes() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("app.js"), "first").unwrap();
        let first = StaticAssetManifest::build(dir.path())
            .unwrap()
            .asset_url("/app.js")
            .unwrap();

        fs::remove_file(dir.path().join(first.trim_start_matches('/'))).unwrap();
        fs::write(dir.path().join("app.js"), "second").unwrap();
        let second = StaticAssetManifest::build(dir.path())
            .unwrap()
            .asset_url("/app.js")
            .unwrap();

        assert_ne!(first, second);
    }

    #[test]
    fn build_fingerprinted_path_collision_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("style.css"), "abc").unwrap();
        fs::write(dir.path().join("style.ba7816bf8f01.css"), "occupied").unwrap();

        let err = StaticAssetManifest::build(dir.path())
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
    }

    // ── StaticAssetManifest::asset_url ──

    #[test]
    fn asset_url_missing_asset_returns_error() {
        let manifest = StaticAssetManifest::default();
        let err = manifest
            .asset_url("/js/missing.js")
            .unwrap_err()
            .to_string();

        assert!(
            err.contains("asset_url: static asset not found: /js/missing.js"),
            "got: {err}"
        );
    }

    // ── StaticAssetManifest::resolve ──

    #[test]
    fn resolve_published_resources_with_page_paths_and_suffixes() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("posts/a")).unwrap();
        fs::write(dir.path().join("posts/a/photo %.avif"), "abc").unwrap();
        fs::write(dir.path().join("font.woff2"), "abc").unwrap();
        fs::create_dir_all(dir.path().join("blog/posts/a")).unwrap();
        fs::write(dir.path().join("blog/posts/a/photo %.avif"), "different").unwrap();
        let manifest = StaticAssetManifest::build(dir.path()).unwrap();
        let expected = "/blog/_assets/posts/a/photo%20%25.ba7816bf8f01.avif?x=1#view";
        for src in [
            "photo%20%25.avif?x=1#view",
            "../a/photo%20%25.avif?x=1#view",
            "/blog/posts/a/photo%20%25.avif?x=1#view",
        ] {
            assert_eq!(
                manifest.resolve(src, "https://example.com/blog/posts/a/", "/blog/"),
                Some(expected.into())
            );
        }
        assert_eq!(
            manifest.asset_url("/font.woff2?v=2").unwrap(),
            "/_assets/font.ba7816bf8f01.woff2?v=2"
        );
        assert_eq!(
            fs::read(dir.path().join("_assets/posts/a/photo %.ba7816bf8f01.avif")).unwrap(),
            b"abc"
        );
        for src in [
            "https://cdn.example.com/a.avif",
            "//cdn.example.com/a.avif",
            "data:image/png;base64,abc",
        ] {
            assert_eq!(manifest.asset_url(src).unwrap(), src);
        }
        assert_eq!(manifest.resolve("missing.png", "/posts/a/", ""), None);
        assert_eq!(
            manifest.resolve("/posts/a/photo%20%25.avif", "/blog/posts/a/", "/blog"),
            None
        );
    }

    // ── is_fingerprinted_copy ──

    #[test]
    fn is_fingerprinted_copy_requires_matching_original() {
        let dir = tempfile::tempdir().unwrap();
        let fingerprinted = dir.path().join("app.ba7816bf8f01.js");
        fs::write(&fingerprinted, "abc").unwrap();

        assert!(!is_fingerprinted_copy(&fingerprinted).unwrap());

        fs::write(dir.path().join("app.js"), "original").unwrap();
        assert!(is_fingerprinted_copy(&fingerprinted).unwrap());

        let unrelated = dir.path().join("app.abcdef123456.js");
        fs::write(&unrelated, "abc").unwrap();
        assert!(!is_fingerprinted_copy(&unrelated).unwrap());
    }
}
