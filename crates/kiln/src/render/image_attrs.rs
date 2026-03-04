use std::collections::HashMap;

use crate::directive::parse_pandoc_attrs;
use crate::markdown::{for_each_non_code_line, scan_code_span};

/// Attributes extracted from Pandoc-style `{...}` blocks after images.
#[derive(Debug, Clone, Default)]
pub struct ImageAttrs {
    pub id: Option<String>,
    pub classes: Vec<String>,
    pub width: Option<String>,
    pub height: Option<String>,
}

/// Extracts `![alt](url){...}` attribute blocks from markdown.
///
/// Returns the cleaned markdown (with `{...}` stripped) and a map from the
/// image's byte position (start of `![`) in the **cleaned** output to its
/// attributes.
///
/// Skips images inside fenced code blocks (` ``` ` / `~~~`) and
/// inline code spans (`` ` ``).
#[must_use]
pub fn extract_image_attrs(input: &str) -> (String, HashMap<usize, ImageAttrs>) {
    let mut output = String::with_capacity(input.len());
    let mut attrs_map: HashMap<usize, ImageAttrs> = HashMap::new();
    for_each_non_code_line(input, &mut output, |line, out| {
        extract_image_attrs_in_line(line, out, &mut attrs_map);
    });
    (output, attrs_map)
}

fn extract_image_attrs_in_line(
    line: &str,
    output: &mut String,
    attrs_map: &mut HashMap<usize, ImageAttrs>,
) {
    let bytes = line.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'`' {
            let (end, span) = scan_code_span(line, i);
            output.push_str(span);
            i = end;
        } else if bytes[i] == b'!' && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
            i = try_extract_image(line, i, output, attrs_map);
        } else {
            let ch = line[i..].chars().next().unwrap();
            output.push(ch);
            i += ch.len_utf8();
        }
    }
}

fn try_extract_image(
    line: &str,
    start: usize,
    output: &mut String,
    attrs_map: &mut HashMap<usize, ImageAttrs>,
) -> usize {
    let bytes = line.as_bytes();
    let Some(paren_end) = find_image_end(bytes, start) else {
        output.push('!');
        return start + 1;
    };

    let img_pos = output.len();
    output.push_str(&line[start..paren_end]);

    if paren_end < bytes.len()
        && bytes[paren_end] == b'{'
        && let Some(brace_end) = find_brace_end(bytes, paren_end)
    {
        let parsed = parse_image_attrs(&line[paren_end + 1..brace_end]);
        if !parsed.is_empty() {
            attrs_map.insert(img_pos, parsed);
        }
        return brace_end + 1;
    }

    paren_end
}

fn find_image_end(bytes: &[u8], start: usize) -> Option<usize> {
    // Skip `![`, find matching `]`.
    let i = find_matching_close(bytes, start + 2, b'[', b']')?;
    // Expect `(` immediately after `]`.
    if i >= bytes.len() || bytes[i] != b'(' {
        return None;
    }
    find_matching_close(bytes, i + 1, b'(', b')')
}

fn find_matching_close(bytes: &[u8], start: usize, open: u8, close: u8) -> Option<usize> {
    let mut depth: usize = 1;
    let mut i = start;
    while i < bytes.len() && depth > 0 {
        match bytes[i] {
            b'\\' => i += 1,
            b if b == open => depth += 1,
            b if b == close => depth -= 1,
            _ => {}
        }
        i += 1;
    }
    (depth == 0).then_some(i)
}

fn find_brace_end(bytes: &[u8], start: usize) -> Option<usize> {
    let mut i = start + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'}' => return Some(i),
            b'\n' => return None,
            _ => i += 1,
        }
    }
    None
}

fn parse_image_attrs(attr_str: &str) -> ImageAttrs {
    let pandoc = parse_pandoc_attrs(attr_str);
    let (mut width, mut height) = (None, None);
    for (key, value) in pandoc.kvs {
        match key {
            "width" => width = Some(value.into_owned()),
            "height" => height = Some(value.into_owned()),
            _ => {}
        }
    }

    ImageAttrs {
        id: pandoc.id.map(str::to_string),
        classes: pandoc.classes.into_iter().map(str::to_string).collect(),
        width,
        height,
    }
}

impl ImageAttrs {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.id.is_none()
            && self.classes.is_empty()
            && self.width.is_none()
            && self.height.is_none()
    }
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::*;

    // -- extract_image_attrs --

    #[test]
    fn extract_no_attrs_passthrough() {
        let input = "![alt](img.png)";
        let (output, attrs) = extract_image_attrs(input);
        assert_eq!(output, input);
        assert!(attrs.is_empty());
    }

    #[test]
    fn extract_id_and_class() {
        let input = "![alt](img.png){#photo .hero}";
        let (output, attrs) = extract_image_attrs(input);
        assert_eq!(output, "![alt](img.png)");
        assert_eq!(attrs.len(), 1);
        let a = &attrs[&0];
        assert_eq!(a.id.as_deref(), Some("photo"));
        assert_eq!(a.classes, vec!["hero"]);
    }

    #[test]
    fn extract_width_and_height() {
        let input = "![alt](img.png){width=500 height=300}";
        let (output, attrs) = extract_image_attrs(input);
        assert_eq!(output, "![alt](img.png)");
        assert_eq!(attrs.len(), 1);
        let a = &attrs[&0];
        assert_eq!(a.width.as_deref(), Some("500"));
        assert_eq!(a.height.as_deref(), Some("300"));
    }

    #[test]
    fn extract_class_and_width() {
        let input = "![alt](img.png){.hero width=800}";
        let (output, attrs) = extract_image_attrs(input);
        assert_eq!(output, "![alt](img.png)");
        assert_eq!(attrs.len(), 1);
        let a = &attrs[&0];
        assert_eq!(a.classes, vec!["hero"]);
        assert_eq!(a.width.as_deref(), Some("800"));
    }

    #[test]
    fn extract_skips_dot_inside_quoted_value() {
        // Dots inside quoted values must not be misidentified as classes.
        let input = r#"![alt](img.png){.real width="my .fake class"}"#;
        let (output, attrs) = extract_image_attrs(input);
        assert_eq!(output, "![alt](img.png)");
        assert_eq!(attrs.len(), 1);
        let a = &attrs[&0];
        assert_eq!(a.classes, vec!["real"]);
        assert_eq!(a.width.as_deref(), Some("my .fake class"));
    }

    #[test]
    fn extract_with_escaped_quote_in_value() {
        let input = r#"![alt](img.png){.hero width="val\"ue"}"#;
        let (output, attrs) = extract_image_attrs(input);
        assert_eq!(output, "![alt](img.png)");
        assert_eq!(attrs.len(), 1);
        let a = &attrs[&0];
        assert_eq!(a.classes, vec!["hero"]);
        assert_eq!(a.width.as_deref(), Some("val\"ue"));
    }

    #[test]
    fn extract_nested_brackets() {
        let input = "![alt [nested]](img.png){width=100}";
        let (output, attrs) = extract_image_attrs(input);
        assert_eq!(output, "![alt [nested]](img.png)");
        assert_eq!(attrs.len(), 1);
        let a = &attrs[&0];
        assert_eq!(a.width.as_deref(), Some("100"));
    }

    #[test]
    fn extract_with_title() {
        let input = r#"![alt](img.png "title"){width=100}"#;
        let (output, attrs) = extract_image_attrs(input);
        assert_eq!(output, r#"![alt](img.png "title")"#);
        assert_eq!(attrs.len(), 1);
        let a = &attrs[&0];
        assert_eq!(a.width.as_deref(), Some("100"));
    }

    #[test]
    fn extract_escaped_bracket() {
        let input = r"![alt\]text](img.png){width=100}";
        let (output, attrs) = extract_image_attrs(input);
        assert_eq!(output, r"![alt\]text](img.png)");
        assert_eq!(attrs.len(), 1);
        let a = &attrs[&0];
        assert_eq!(a.width.as_deref(), Some("100"));
    }

    #[test]
    fn extract_multiple_images() {
        let input = "![a](a.png){width=100} text ![b](b.png){width=200}";
        let (output, attrs) = extract_image_attrs(input);
        assert_eq!(output, "![a](a.png) text ![b](b.png)");
        assert_eq!(attrs.len(), 2);
        let a = &attrs[&0];
        assert_eq!(a.width.as_deref(), Some("100"));
        let b = &attrs[&output.find("![b]").unwrap()];
        assert_eq!(b.width.as_deref(), Some("200"));
    }

    #[test]
    fn extract_non_ascii() {
        let input = "图片说明 ![描述](图片.png){width=200} 后续文本";
        let (output, attrs) = extract_image_attrs(input);
        assert_eq!(output, "图片说明 ![描述](图片.png) 后续文本");
        assert_eq!(attrs.len(), 1);
        let a = &attrs[&output.find("![描述]").unwrap()];
        assert_eq!(a.width.as_deref(), Some("200"));
    }

    #[test]
    fn extract_unknown_key_ignored() {
        let input = "![alt](img.png){foo=bar width=100}";
        let (output, attrs) = extract_image_attrs(input);
        assert_eq!(output, "![alt](img.png)");
        assert_eq!(attrs.len(), 1);
        let a = &attrs[&0];
        assert_eq!(a.width.as_deref(), Some("100"));
    }

    #[test]
    fn extract_bang_without_bracket_preserved() {
        let input = "!{width=500}";
        let (output, attrs) = extract_image_attrs(input);
        assert_eq!(output, input);
        assert!(attrs.is_empty());
    }

    #[test]
    fn extract_alt_without_paren_preserved() {
        let input = "![alt]{width=500}";
        let (output, attrs) = extract_image_attrs(input);
        assert_eq!(output, input);
        assert!(attrs.is_empty());
    }

    #[test]
    fn extract_no_image_braces_preserved() {
        let input = "text {width=500} more";
        let (output, attrs) = extract_image_attrs(input);
        assert_eq!(output, input);
        assert!(attrs.is_empty());
    }

    #[test]
    fn extract_unclosed_brace_preserved() {
        let input = "![alt](img.png){width=500\nnext line\n";
        let (output, attrs) = extract_image_attrs(input);
        assert_eq!(output, input);
        assert!(attrs.is_empty());
    }

    #[test]
    fn extract_unclosed_brace_at_eof_preserved() {
        let input = "![alt](img.png){width=500";
        let (output, attrs) = extract_image_attrs(input);
        assert_eq!(output, input);
        assert!(attrs.is_empty());
    }

    // -- extract_image_attrs (code awareness) --

    #[test]
    fn extract_skips_inline_code() {
        let input = "`![alt](img.png){width=500}` rest";
        let (output, attrs) = extract_image_attrs(input);
        assert_eq!(output, input);
        assert!(attrs.is_empty());
    }

    #[test]
    fn extract_skips_fenced_code() {
        // Backtick fences.
        let input = indoc! {"
            ```
            ![alt](img.png){width=500}
            ```
        "};
        let (output, attrs) = extract_image_attrs(input);
        assert_eq!(output, input);
        assert!(attrs.is_empty());

        // Tilde fences.
        let input = indoc! {"
            ~~~
            ![alt](img.png){width=500}
            ~~~
        "};
        let (output, attrs) = extract_image_attrs(input);
        assert_eq!(output, input);
        assert!(attrs.is_empty());
    }

    #[test]
    fn extract_after_fenced_code() {
        let input = indoc! {"
            ```
            code
            ```
            ![alt](img.png){width=500}
        "};
        let (output, attrs) = extract_image_attrs(input);
        let expected = indoc! {"
            ```
            code
            ```
            ![alt](img.png)
        "};
        assert_eq!(output, expected);
        assert_eq!(attrs.len(), 1);
        let a = &attrs[&output.find("![alt]").unwrap()];
        assert_eq!(a.width.as_deref(), Some("500"));
    }

    // -- ImageAttrs::is_empty --

    #[test]
    fn is_empty_default() {
        assert!(ImageAttrs::default().is_empty());
    }

    #[test]
    fn is_empty_with_any_field_returns_false() {
        assert!(
            !ImageAttrs {
                id: Some("x".into()),
                ..Default::default()
            }
            .is_empty()
        );
        assert!(
            !ImageAttrs {
                classes: vec!["x".into()],
                ..Default::default()
            }
            .is_empty()
        );
        assert!(
            !ImageAttrs {
                width: Some("100".into()),
                ..Default::default()
            }
            .is_empty()
        );
        assert!(
            !ImageAttrs {
                height: Some("100".into()),
                ..Default::default()
            }
            .is_empty()
        );
    }
}
