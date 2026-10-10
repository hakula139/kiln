pub mod assets;
mod code_block;
pub mod emoji;
mod footnote;
mod heading;
mod highlight;
pub mod icon;
mod image;
mod image_attrs;
pub mod lqip;
mod markdown;
mod mermaid;
mod page_ids;
pub mod pipeline;
mod table;
mod toc;

use std::ops::Range;
use std::path::Path;

use anyhow::{Context, Result};
use pulldown_cmark::Event;
use serde::Deserialize;

use self::lqip::ImageResolver;
use crate::static_assets::StaticAssetManifest;

type Spanned = (Event<'static>, Range<usize>);

/// Source context and published assets used by a page and its nested directives.
pub struct PageResources<'a> {
    pub source_dir: Option<&'a Path>,
    pub images: &'a ImageResolver,
    pub assets: &'a StaticAssetManifest,
    pub page_url: &'a str,
    pub deployment_prefix: &'a str,
}

impl PageResources<'_> {
    pub(super) fn image_url(&self, src: &str) -> String {
        self.assets
            .resolve(src, self.page_url, self.deployment_prefix)
            .unwrap_or_else(|| src.to_owned())
    }
}

/// Feature flags and settings for the render pipeline.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct RenderOptions {
    pub code_max_lines: Option<usize>,
    pub emojis: bool,
    pub fontawesome: bool,
    #[serde(skip)]
    pub heading_numbering: bool,
    pub table_nowrap_width: Option<usize>,
}

impl RenderOptions {
    /// Extracts render options from the site `[params]` table.
    ///
    /// # Errors
    ///
    /// Returns an error if a known render option has an incompatible type.
    pub fn from_params(params: &toml::Table) -> Result<Self> {
        params
            .clone()
            .try_into()
            .context("failed to parse render options from [params]")
    }
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::*;

    // ── RenderOptions::from_params ──

    #[test]
    fn from_params_defaults() {
        let options = RenderOptions::from_params(&toml::Table::new()).unwrap();
        assert!(options.code_max_lines.is_none());
        assert!(!options.emojis);
        assert!(!options.fontawesome);
        assert!(!options.heading_numbering);
        assert!(options.table_nowrap_width.is_none());
    }

    #[test]
    fn from_params_reads_known_options_and_ignores_other_params() {
        let params: toml::Table = toml::from_str(indoc! {r#"
            code_max_lines = 40
            emojis = true
            fontawesome = true
            table_nowrap_width = 30
            site_title = "Example"
            social = { github = "user" }
        "#})
        .unwrap();
        let options = RenderOptions::from_params(&params).unwrap();
        assert_eq!(options.code_max_lines, Some(40));
        assert!(options.emojis);
        assert!(options.fontawesome);
        assert_eq!(options.table_nowrap_width, Some(30));
    }

    #[test]
    fn from_params_type_mismatch_returns_error() {
        let params: toml::Table = toml::from_str(indoc! {r#"
            emojis = "yes"
        "#})
        .unwrap();
        assert!(RenderOptions::from_params(&params).is_err());
    }
}
