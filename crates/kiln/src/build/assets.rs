use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::url::{page_url, resolve_relative_url};
use crate::content::page::Page;
use crate::output::copy_file;
use crate::static_assets::{StaticAssetManifest, path_to_url};

const PAGE_STYLE: &str = "assets/css/style.generated.css";

/// Stages canonical page stylesheets before minification and fingerprinting.
///
/// # Errors
///
/// Returns an error if a page output path cannot be resolved or a stylesheet cannot be copied.
pub(super) fn copy_page_styles(
    pages: &[Page],
    content_dir: &Path,
    output_dir: &Path,
) -> Result<()> {
    for page in pages {
        if let Some(style) = page_style(page) {
            let output = page.output_path(content_dir)?;
            let relative = style_output_path(&output)?;
            copy_file(style, &output_dir.join(relative))?;
        }
    }
    Ok(())
}

/// Returns the root-relative fingerprinted stylesheet URL, including the site's base path.
/// Returns `None` when the page has no canonical stylesheet.
///
/// # Errors
///
/// Returns an error for an invalid output path or a stylesheet missing from the manifest.
pub(super) fn page_style_url(
    page: &Page,
    content_dir: &Path,
    base_url: &str,
    manifest: &StaticAssetManifest,
) -> Result<Option<String>> {
    if page_style(page).is_none() {
        return Ok(None);
    }
    let output = page.output_path(content_dir)?;
    let relative = style_output_path(&output)?;
    let url = path_to_url(&relative).context("page stylesheet path is not valid UTF-8")?;
    let fingerprinted = manifest.asset_url(&url)?;
    let site_url = page_url(base_url, Path::new("index.html"));
    Ok(Some(resolve_relative_url(
        fingerprinted.trim_start_matches('/'),
        &site_url,
    )))
}

/// Copies public bundle assets without replacing assets already published by the manifest.
///
/// # Errors
///
/// Returns an error if an asset is outside its bundle or output tree, or copying fails.
pub(super) fn copy_page_assets(
    page: &Page,
    output: &Path,
    site_output: &Path,
    manifest: &StaticAssetManifest,
) -> Result<()> {
    let Some(bundle) = page.source_path.parent() else {
        return Ok(());
    };
    let output_dir = output.parent().context("page output has no parent")?;
    for asset in &page.assets {
        let relative = asset.strip_prefix(bundle).with_context(|| {
            format!(
                "asset {} is not under {}",
                asset.display(),
                bundle.display()
            )
        })?;
        let destination = output_dir.join(relative);
        let published = destination
            .strip_prefix(site_output)
            .context("bundle asset destination is outside the site output")?;
        // Parent bundles can include a child's already minified and fingerprinted stylesheet.
        if !manifest.fingerprinted_paths().contains(published) {
            copy_file(asset, &destination)
                .with_context(|| format!("failed to copy asset {}", asset.display()))?;
        }
    }
    Ok(())
}

fn page_style(page: &Page) -> Option<&Path> {
    let canonical = page.source_path.parent()?.join(PAGE_STYLE);
    page.assets
        .iter()
        .find(|asset| **asset == canonical)
        .map(PathBuf::as_path)
}

fn style_output_path(output: &Path) -> Result<PathBuf> {
    Ok(output
        .parent()
        .context("page output has no parent")?
        .join(PAGE_STYLE))
}
