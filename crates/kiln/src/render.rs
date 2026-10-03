pub mod assets;
pub(crate) mod code_block;
pub mod emoji;
pub(crate) mod footnote;
mod heading;
pub mod highlight;
pub mod icon;
pub mod image;
pub mod image_attrs;
pub mod lqip;
pub mod markdown;
pub mod mermaid;
mod page_ids;
pub mod pipeline;
mod table;
pub mod toc;

use std::ops::Range;

use anyhow::{Context, Result};
use pulldown_cmark::Event;
use serde::Deserialize;

type Spanned = (Event<'static>, Range<usize>);

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
    fn from_params_known_options() {
        let params: toml::Table = toml::from_str(indoc! {r"
            code_max_lines = 40
            emojis = true
            fontawesome = true
            table_nowrap_width = 30
        "})
        .unwrap();
        let options = RenderOptions::from_params(&params).unwrap();
        assert_eq!(options.code_max_lines, Some(40));
        assert!(options.emojis);
        assert!(options.fontawesome);
        assert_eq!(options.table_nowrap_width, Some(30));
    }

    #[test]
    fn from_params_ignores_unknown_keys() {
        let params: toml::Table = toml::from_str(indoc! {r#"
            emojis = true
            site_title = "Example"
            social = { github = "user" }
        "#})
        .unwrap();
        let options = RenderOptions::from_params(&params).unwrap();
        assert!(options.emojis);
        assert!(!options.fontawesome);
        assert!(options.code_max_lines.is_none());
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
