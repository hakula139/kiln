pub mod assets;
mod code_block;
mod document;
mod emoji;
mod footnote;
mod heading;
mod highlight;
mod icon;
mod image;
mod image_attrs;
pub mod lqip;
mod mermaid;
mod page_ids;
pub mod pipeline;
mod table;
mod toc;

use std::num::NonZeroUsize;
use std::ops::Range;
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use pulldown_cmark::Event;
use serde::{Deserialize, Deserializer};

use self::lqip::{ImageMeta, ImageResolver};
use crate::i18n::I18n;
use crate::static_assets::{ResolvedAsset, StaticAssetManifest};

type Spanned = (Event<'static>, Range<usize>);

/// Source context and published assets used by a page and its nested directives.
pub struct PageResources<'a> {
    pub source_dir: Option<&'a Path>,
    pub images: &'a ImageResolver,
    pub assets: &'a StaticAssetManifest,
    pub page_url: &'a str,
}

impl PageResources<'_> {
    /// Resolves an authored image to its public URL and the published file's metadata.
    ///
    /// # Errors
    ///
    /// Returns an error for a site image without a published file.
    pub(crate) fn resolve_image(&self, src: &str) -> Result<(String, Option<Arc<ImageMeta>>)> {
        let asset = self.assets.resolve_local(src, self.page_url)?;
        Ok(self.image(src, asset))
    }

    /// Resolves an image whose root-relative paths omit the base URL path.
    ///
    /// # Errors
    ///
    /// Returns an error for a site image without a published file.
    pub(crate) fn resolve_site_image(&self, src: &str) -> Result<(String, Option<Arc<ImageMeta>>)> {
        let asset = self.assets.resolve_template_url(src, self.page_url)?;
        Ok(self.image(src, asset))
    }

    fn image(
        &self,
        src: &str,
        asset: Option<ResolvedAsset<'_>>,
    ) -> (String, Option<Arc<ImageMeta>>) {
        match asset {
            Some(asset) => {
                let meta = self.images.resolve_published(asset.path);
                (asset.url, meta)
            }
            None => (src.to_owned(), None),
        }
    }
}

/// Site-wide render settings read from the `[params]` table, plus the localized strings the
/// renderer emits.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct RenderOptions {
    /// Zero disables the limit, which lets a site override a theme default.
    #[serde(deserialize_with = "deserialize_line_limit")]
    pub code_max_lines: Option<NonZeroUsize>,
    pub emojis: bool,
    pub fontawesome: bool,
    /// In terminal columns.
    pub table_nowrap_width: Option<usize>,
    #[serde(skip)]
    pub copy_button: CopyButton,
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

fn deserialize_line_limit<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<NonZeroUsize>, D::Error> {
    Ok(Option::<usize>::deserialize(deserializer)?.and_then(NonZeroUsize::new))
}

/// Localized labels of the code-block copy button.
#[derive(Debug, Default)]
pub struct CopyButton {
    pub text: String,
    pub aria_label: String,
}

impl CopyButton {
    /// Resolves the `copy` and `copy_code` translation keys.
    #[must_use]
    pub(crate) fn new(i18n: &I18n) -> Self {
        Self {
            text: i18n.t("copy").into_owned(),
            aria_label: i18n.t("copy_code").into_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::*;

    // ── RenderOptions::from_params ──

    #[test]
    fn defaults_disable_every_option() {
        let options = RenderOptions::from_params(&toml::Table::new()).unwrap();
        assert!(options.code_max_lines.is_none());
        assert!(!options.emojis);
        assert!(!options.fontawesome);
        assert!(options.table_nowrap_width.is_none());
    }

    #[test]
    fn reads_known_options_and_ignores_other_params() {
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
        assert_eq!(options.code_max_lines, NonZeroUsize::new(40));
        assert!(options.emojis);
        assert!(options.fontawesome);
        assert_eq!(options.table_nowrap_width, Some(30));
    }

    #[test]
    fn zero_code_max_lines_disables_limit() {
        let params: toml::Table = toml::from_str("code_max_lines = 0").unwrap();
        let options = RenderOptions::from_params(&params).unwrap();
        assert!(options.code_max_lines.is_none());
    }

    #[test]
    fn type_mismatch_returns_error() {
        for (input, key, expected) in [
            (
                r#"emojis = "yes""#,
                "emojis",
                r#"invalid type: string "yes", expected a boolean"#,
            ),
            (
                r#"code_max_lines = "x""#,
                "code_max_lines",
                r#"invalid type: string "x", expected usize"#,
            ),
            (
                "code_max_lines = -1",
                "code_max_lines",
                "invalid value: integer `-1`, expected usize",
            ),
        ] {
            let params: toml::Table = toml::from_str(input).unwrap();
            let error = RenderOptions::from_params(&params).unwrap_err();
            assert_eq!(
                format!("{error:#}"),
                format!("failed to parse render options from [params]: {expected}\nin `{key}`\n")
            );
        }
    }
}
