pub mod highlight;
pub mod image;
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

#[cfg(test)]
mod tests {
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
}
