pub mod config;

use std::path::Path;

use anyhow::{Context, Result};

use crate::config::Config;

/// Build the site from the given project root directory.
///
/// # Errors
///
/// Returns an error if configuration loading or page rendering fails.
pub fn build(root: &Path) -> Result<()> {
    let _config = Config::load(root).context("failed to load config")?;

    println!("Build complete.");
    Ok(())
}
