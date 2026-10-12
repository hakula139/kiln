/// Escapes characters that are special in HTML.
///
/// Escapes `&`, `<`, `>`, `"`, `'`. Safe for use in both element content and attribute values.
pub(crate) fn escape(input: &str) -> String {
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

/// `indent` + `writeln!`, suppressing the infallible `fmt::Result`.
macro_rules! writeln_indented {
    ($html:expr, $level:expr, $($arg:tt)*) => {{
        use ::std::fmt::Write as _;
        $crate::html::indent($html, $level);
        _ = ::std::writeln!($html, $($arg)*);
    }};
}

pub(crate) use writeln_indented;

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::*;

    // ── escape ──

    #[test]
    fn escapes_all_special_chars() {
        assert_eq!(escape("a&b<c>d\"e'f"), "a&amp;b&lt;c&gt;d&quot;e&#39;f");
    }

    // ── writeln_indented ──

    #[test]
    fn indents_two_spaces_per_level() {
        let mut html = String::from("<p>\n");
        writeln_indented!(&mut html, 2, "<div>");
        writeln_indented!(&mut html, 1, "<span>{}</span>", "hi");
        writeln_indented!(&mut html, 0, "<br />");
        assert_eq!(
            html,
            indoc! {"
                <p>
                    <div>
                  <span>hi</span>
                <br />
            "}
        );
    }
}
