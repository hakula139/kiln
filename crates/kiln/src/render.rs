pub mod assets;
pub mod emoji;
pub mod highlight;
pub mod icon;
pub mod image;
pub mod image_attrs;
pub mod markdown;
pub mod mermaid;
pub mod pipeline;
pub mod toc;

/// Feature flags and settings for the render pipeline.
#[derive(Debug, Clone, Default)]
pub struct RenderOptions {
    pub code_max_lines: Option<usize>,
    pub emojis: bool,
    pub fontawesome: bool,
}

impl RenderOptions {
    /// Extracts render options from the site `[params]` table.
    #[must_use]
    pub fn from_params(params: &toml::Table) -> Self {
        Self {
            code_max_lines: params
                .get("code_max_lines")
                .and_then(toml::Value::as_integer)
                .and_then(|n| usize::try_from(n).ok()),
            emojis: params
                .get("emojis")
                .and_then(toml::Value::as_bool)
                .unwrap_or(false),
            fontawesome: params
                .get("fontawesome")
                .and_then(toml::Value::as_bool)
                .unwrap_or(false),
        }
    }
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::*;

    // ── RenderOptions::from_params ──

    #[test]
    fn render_options_defaults() {
        let options = RenderOptions::from_params(&toml::Table::new());
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
        let options = RenderOptions::from_params(&params);
        assert_eq!(options.code_max_lines, Some(40));
        assert!(options.emojis);
        assert!(options.fontawesome);
    }
}
