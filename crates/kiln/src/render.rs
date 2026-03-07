pub mod highlight;
pub mod icon;
pub mod image;
pub mod image_attrs;
pub mod markdown;
pub mod pipeline;
pub mod toc;

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
