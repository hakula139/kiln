pub mod emoji;
pub mod highlight;
pub mod icon;
pub mod image;
pub mod image_attrs;
pub mod markdown;
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

/// Escapes characters that are special in HTML.
///
/// Escapes `&`, `<`, `>`, `"`, `'`.
/// Safe for use in both element content and attribute values.
#[must_use]
pub(crate) fn escape_html(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '"' => output.push_str("&quot;"),
            '\'' => output.push_str("&#39;"),
            _ => output.push(ch),
        }
    }
    output
}

/// Appends `level` × 2 spaces of indentation to an HTML string.
pub(crate) fn indent(html: &mut String, level: u8) {
    for _ in 0..level {
        html.push_str("  ");
    }
}

/// Appends indentation, writes formatted content, and adds a newline.
///
/// Equivalent to `indent` + `writeln!`, suppressing the infallible `fmt::Result`.
macro_rules! writeln_indented {
    ($html:expr, $level:expr, $($arg:tt)*) => {{
        use ::std::fmt::Write as _;
        $crate::render::indent($html, $level);
        _ = ::std::writeln!($html, $($arg)*);
    }};
}

pub(crate) use writeln_indented;

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::*;

    // -- RenderOptions::from_params --

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

    // -- escape_html --

    #[test]
    fn escape_html_special_chars() {
        assert_eq!(
            escape_html("a&b<c>d\"e'f"),
            "a&amp;b&lt;c&gt;d&quot;e&#39;f"
        );
    }

    #[test]
    fn escape_html_no_op_for_plain_text() {
        assert_eq!(escape_html("hello world"), "hello world");
    }

    #[test]
    fn escape_html_empty() {
        assert_eq!(escape_html(""), "");
    }

    // -- indent --

    #[test]
    fn indent_zero_level() {
        let mut html = String::from("<p>");
        indent(&mut html, 0);
        assert_eq!(html, "<p>");
    }

    #[test]
    fn indent_multiple_levels() {
        let mut html = String::from("<p>");
        indent(&mut html, 3);
        assert_eq!(html, "<p>      ");
    }

    // -- writeln_indented --

    #[test]
    fn writeln_indented_static() {
        let mut html = String::from("<p>\n");
        writeln_indented!(&mut html, 2, "<div>");
        assert_eq!(
            html,
            indoc! {"
                <p>
                    <div>
            "}
        );
    }

    #[test]
    fn writeln_indented_formatted() {
        let mut html = String::from("<p>\n");
        writeln_indented!(&mut html, 1, "<span>{}</span>", "hi");
        assert_eq!(
            html,
            indoc! {"
                <p>
                  <span>hi</span>
            "}
        );
    }

    #[test]
    fn writeln_indented_zero_level() {
        let mut html = String::from("<p>\n");
        writeln_indented!(&mut html, 0, "<br />");
        assert_eq!(
            html,
            indoc! {"
                <p>
                <br />
            "}
        );
    }
}
