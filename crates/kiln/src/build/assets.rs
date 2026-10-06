use std::path::Path;

use anyhow::{Context, Result};

use crate::content::page::Page;
use crate::output::copy_file;

/// Stages public bundle assets before stylesheet compilation and fingerprinting.
///
/// # Errors
///
/// Returns an error if an output path cannot be resolved or a bundle asset cannot be copied.
pub(super) fn copy_page_assets(
    pages: &[Page],
    content_dir: &Path,
    output_dir: &Path,
) -> Result<()> {
    for page in pages {
        let bundle = page
            .source_path
            .parent()
            .context("page source has no parent")?;
        let output = page.output_path(content_dir)?;
        let destination = output_dir.join(output.parent().context("page output has no parent")?);
        for asset in &page.assets {
            let relative = asset
                .strip_prefix(bundle)
                .context("asset is outside its bundle")?;
            copy_file(asset, &destination.join(relative))
                .with_context(|| format!("failed to copy asset {}", asset.display()))?;
        }
    }
    Ok(())
}
