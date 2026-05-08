pub mod assets;
pub(crate) mod code_block;
pub mod emoji;
pub mod highlight;
pub mod icon;
pub mod image;
pub mod image_attrs;
pub mod lqip;
pub mod markdown;
pub mod mermaid;
pub mod pipeline;
pub mod toc;

use anyhow::{Context, Result};
use serde::Deserialize;

/// Feature flags and settings for the render pipeline.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct RenderOptions {
    pub code_max_lines: Option<usize>,
    pub emojis: bool,
    pub fontawesome: bool,
}

impl RenderOptions {
    /// Extracts render options from the site `[params]` table. Unknown keys are
    /// ignored (the table holds unrelated theme / site params); type mismatches
    /// surface as errors.
    ///
    /// # Errors
    ///
    /// Returns an error if a known render option key has an incompatible type
    /// (e.g., `emojis = "true"` instead of a boolean).
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
    fn render_options_defaults() {
        let options = RenderOptions::from_params(&toml::Table::new()).unwrap();
        assert!(!options.emojis);
        assert!(!options.fontawesome);
        assert!(options.code_max_lines.is_none());
    }

    #[test]
    fn render_options_all_set() {
        let params: toml::Table = toml::from_str(indoc! {r"
            code_max_lines = 40
            emojis = true
            fontawesome = true
        "})
        .unwrap();
        let options = RenderOptions::from_params(&params).unwrap();
        assert_eq!(options.code_max_lines, Some(40));
        assert!(options.emojis);
        assert!(options.fontawesome);
    }

    #[test]
    fn render_options_ignores_unknown_keys() {
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
    fn render_options_type_mismatch_returns_error() {
        let params: toml::Table = toml::from_str(indoc! {r#"
            emojis = "yes"
        "#})
        .unwrap();
        assert!(RenderOptions::from_params(&params).is_err());
    }
}
