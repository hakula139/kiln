use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

use crate::output::copy_file;

const FINGERPRINT_LENGTH: usize = 12;

/// Content-addressed URLs for files in a merged static output tree.
#[derive(Clone, Debug, Default)]
pub struct StaticAssetManifest {
    urls: BTreeMap<String, String>,
    fingerprinted_paths: BTreeSet<PathBuf>,
}

impl StaticAssetManifest {
    /// Builds the manifest and writes fingerprinted CSS / JS copies into `output_dir`.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be read, an asset cannot be copied, or a generated
    /// fingerprinted path conflicts with an existing file.
    pub fn build(output_dir: &Path) -> Result<Self> {
        let mut manifest = Self::default();
        let entries = WalkDir::new(output_dir)
            .follow_links(false)
            .sort_by_file_name()
            .into_iter()
            .filter_map(|entry| match entry {
                Ok(entry) if entry.file_type().is_file() => Some(Ok(entry.into_path())),
                Ok(_) => None,
                Err(error) => Some(Err(error)),
            })
            .collect::<std::result::Result<Vec<_>, _>>()
            .with_context(|| {
                format!(
                    "failed to collect static assets in {}",
                    output_dir.display()
                )
            })?;

        for path in entries {
            let relative = path.strip_prefix(output_dir).with_context(|| {
                format!(
                    "static asset {} is not under {}",
                    path.display(),
                    output_dir.display()
                )
            })?;
            let Some(url) = path_to_url(relative) else {
                continue;
            };
            if !is_fingerprintable(relative) {
                manifest.urls.insert(url.clone(), url);
                continue;
            }
            manifest.fingerprinted_paths.insert(relative.to_owned());

            let bytes = fs::read(&path)
                .with_context(|| format!("failed to read static asset {}", path.display()))?;
            let fingerprinted = fingerprinted_path(relative, &bytes)?;
            let target = output_dir.join(&fingerprinted);
            if target.exists() {
                bail!(
                    "fingerprinted static asset conflicts with existing file {}",
                    target.display()
                );
            }
            copy_file(&path, &target)?;

            let fingerprinted_url = path_to_url(&fingerprinted).with_context(|| {
                format!(
                    "fingerprinted static asset path is not valid UTF-8: {}",
                    fingerprinted.display()
                )
            })?;
            manifest.urls.insert(url, fingerprinted_url);
            manifest.fingerprinted_paths.insert(fingerprinted);
        }

        Ok(manifest)
    }

    pub(crate) fn asset_url(&self, url: &str) -> std::result::Result<String, minijinja::Error> {
        validate_url(url)?;
        self.urls.get(url).cloned().ok_or_else(|| {
            minijinja::Error::new(
                minijinja::ErrorKind::InvalidOperation,
                format!("asset_url: static asset not found: {url}"),
            )
        })
    }

    pub(crate) fn fingerprinted_paths(&self) -> &BTreeSet<PathBuf> {
        &self.fingerprinted_paths
    }
}

fn validate_url(url: &str) -> std::result::Result<(), minijinja::Error> {
    let invalid = || {
        minijinja::Error::new(
            minijinja::ErrorKind::InvalidOperation,
            format!(
                "asset_url requires a root-relative static path without a query or fragment: {url}"
            ),
        )
    };

    let Some(relative) = url.strip_prefix('/') else {
        return Err(invalid());
    };
    if relative.is_empty() || url.contains('?') || url.contains('#') {
        return Err(invalid());
    }
    if Path::new(relative)
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(invalid());
    }

    Ok(())
}

fn path_to_url(path: &Path) -> Option<String> {
    let components = path
        .components()
        .map(|component| match component {
            Component::Normal(value) => value.to_str(),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    Some(format!("/{}", components.join("/")))
}

fn is_fingerprintable(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("css")
                || extension.eq_ignore_ascii_case("js")
                || extension.eq_ignore_ascii_case("mjs")
        })
}

pub(crate) fn is_fingerprinted_copy(path: &Path) -> Result<bool> {
    let Some(extension) = path.extension().and_then(|extension| extension.to_str()) else {
        return Ok(false);
    };
    if !is_fingerprintable(path) {
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
    let digest = format!("{:x}", Sha256::digest(bytes));
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
    fn build_keeps_other_static_urls_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("images")).unwrap();
        fs::write(dir.path().join("images/logo.webp"), "image").unwrap();

        let manifest = StaticAssetManifest::build(dir.path()).unwrap();

        assert_eq!(
            manifest.asset_url("/images/logo.webp").unwrap(),
            "/images/logo.webp"
        );
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
    fn build_returns_error_on_fingerprinted_path_collision() {
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
    fn asset_url_returns_error_for_missing_asset() {
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

    #[test]
    fn asset_url_rejects_paths_outside_static_root() {
        let manifest = StaticAssetManifest::default();

        for url in [
            "js/app.js",
            "//example.com/app.js",
            "/../app.js",
            "/app.js?v=1",
            "/app.js#module",
        ] {
            let err = manifest.asset_url(url).unwrap_err().to_string();
            assert!(
                err.contains("requires a root-relative static path"),
                "url {url:?} produced: {err}"
            );
        }
    }

    // ── is_fingerprinted_copy ──

    #[test]
    fn fingerprinted_copy_requires_matching_original() {
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
