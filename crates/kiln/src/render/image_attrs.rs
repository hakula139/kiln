use std::collections::HashMap;

use pulldown_cmark::{Event, Parser, Tag};

use super::lqip::ImageMeta;
use crate::attrs::{find_attr_block_end, parse_pandoc_attrs};
use crate::markdown::markdown_options;

/// Attributes extracted from Pandoc-style `{...}` blocks after images,
/// merged with auto-detected dimensions and an optional LQIP placeholder.
#[derive(Debug, Clone, Default)]
pub(super) struct ImageAttrs {
    pub id: Option<String>,
    pub classes: Vec<String>,
    pub width: Option<String>,
    pub height: Option<String>,
    pub auto_width: Option<u32>,
    pub auto_height: Option<u32>,
    pub lqip_uri: Option<String>,
}

/// Removes image attribute blocks, keyed by image offset in the cleaned Markdown.
#[must_use]
pub(super) fn extract_image_attrs(input: &str) -> (String, HashMap<usize, ImageAttrs>) {
    let mut output = String::with_capacity(input.len());
    let mut attrs_map = HashMap::new();
    let mut copied = 0;
    for (event, range) in Parser::new_ext(input, markdown_options()).into_offset_iter() {
        if !matches!(event, Event::Start(Tag::Image { .. })) || range.start < copied {
            continue;
        }
        let Some(brace_end) = input[range.end..]
            .starts_with('{')
            .then(|| find_brace_end(input, range.end))
            .flatten()
        else {
            continue;
        };
        let image_offset = output.len() + range.start - copied;
        output.push_str(&input[copied..range.end]);
        let attrs = parse_image_attrs(&input[range.end + 1..brace_end]);
        if !attrs.is_empty() {
            attrs_map.insert(image_offset, attrs);
        }
        copied = brace_end + 1;
    }
    output.push_str(&input[copied..]);
    (output, attrs_map)
}

fn find_brace_end(s: &str, start: usize) -> Option<usize> {
    let inner = &s[start + 1..];
    let line = inner.split_once('\n').map_or(inner, |(line, _)| line);
    find_attr_block_end(line).map(|end| start + 1 + end)
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
        ..ImageAttrs::default()
    }
}

impl ImageAttrs {
    #[must_use]
    fn is_empty(&self) -> bool {
        self.id.is_none()
            && self.classes.is_empty()
            && self.width.is_none()
            && self.height.is_none()
            && self.auto_width.is_none()
            && self.auto_height.is_none()
            && self.lqip_uri.is_none()
    }

    /// Stamps auto-detected dimensions and LQIP onto the attributes from a
    /// resolver lookup. Manual fields are untouched.
    pub(super) fn fill_from_meta(&mut self, meta: &ImageMeta) {
        self.auto_width = Some(meta.width);
        self.auto_height = Some(meta.height);
        self.lqip_uri.clone_from(&meta.lqip_uri);
    }
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::*;

    // ── extract_image_attrs ──

    #[test]
    fn extract_image_attrs_uses_markdown_image_boundaries() {
        for image in [
            r#"![Alt](photo.png "A ) B")"#,
            r#"![Alt](photo.png "A ( B")"#,
            indoc! {"
                ![Alt
                text](photo.png)"},
        ] {
            let (cleaned, attrs) = extract_image_attrs(&format!("{image}{{width=80}}"));
            assert_eq!(cleaned, image);
            assert_eq!(attrs[&0].width.as_deref(), Some("80"));
        }
    }

    #[test]
    fn extract_image_attrs_preserves_code_contexts() {
        for input in [
            indoc! {"
                `first
                ![Alt](photo.png){width=80}`
            "},
            "    ![Alt](photo.png){width=80}",
            indoc! {"
                > ```
                > ![Alt](photo.png){width=80}
                > ```
            "},
        ] {
            let (cleaned, attrs) = extract_image_attrs(input);
            assert_eq!(cleaned, input);
            assert!(attrs.is_empty());
        }
    }

    #[test]
    fn extract_image_attrs_no_attrs_passthrough() {
        let input = "![alt](img.png)";
        let (output, attrs) = extract_image_attrs(input);
        assert_eq!(output, input);
        assert!(attrs.is_empty());
    }

    #[test]
    fn extract_image_attrs_authored_fields() {
        let (output, attrs) =
            extract_image_attrs("![alt](img.png){#photo .hero width=500 height=300}");
        assert_eq!(output, "![alt](img.png)");
        let attrs = &attrs[&0];
        assert_eq!(attrs.id.as_deref(), Some("photo"));
        assert_eq!(attrs.classes, vec!["hero"]);
        assert_eq!(attrs.width.as_deref(), Some("500"));
        assert_eq!(attrs.height.as_deref(), Some("300"));
    }

    #[test]
    fn extract_image_attrs_skips_dot_inside_quoted_value() {
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
    fn extract_image_attrs_with_escaped_quote_in_value() {
        let input = r#"![alt](img.png){.hero width="val\"ue"}"#;
        let (output, attrs) = extract_image_attrs(input);
        assert_eq!(output, "![alt](img.png)");
        assert_eq!(attrs.len(), 1);
        let a = &attrs[&0];
        assert_eq!(a.classes, vec!["hero"]);
        assert_eq!(a.width.as_deref(), Some("val\"ue"));
    }

    #[test]
    fn extract_image_attrs_with_quoted_brace_in_value() {
        for (input, width) in [
            (r#"![alt](img.png){width="a}b" height=300} tail"#, "a}b"),
            (r#"![alt](img.png){width="a\"}b" height=300} tail"#, "a\"}b"),
        ] {
            let (output, attrs) = extract_image_attrs(input);
            assert_eq!(output, "![alt](img.png) tail");
            let a = &attrs[&0];
            assert_eq!(a.width.as_deref(), Some(width));
            assert_eq!(a.height.as_deref(), Some("300"));
        }
    }

    #[test]
    fn extract_image_attrs_nested_brackets() {
        let input = "![alt [nested]](img.png){width=100}";
        let (output, attrs) = extract_image_attrs(input);
        assert_eq!(output, "![alt [nested]](img.png)");
        assert_eq!(attrs.len(), 1);
        let a = &attrs[&0];
        assert_eq!(a.width.as_deref(), Some("100"));
    }

    #[test]
    fn extract_image_attrs_with_title() {
        let input = r#"![alt](img.png "title"){width=100}"#;
        let (output, attrs) = extract_image_attrs(input);
        assert_eq!(output, r#"![alt](img.png "title")"#);
        assert_eq!(attrs.len(), 1);
        let a = &attrs[&0];
        assert_eq!(a.width.as_deref(), Some("100"));
    }

    #[test]
    fn extract_image_attrs_escaped_bracket() {
        let input = r"![alt\]text](img.png){width=100}";
        let (output, attrs) = extract_image_attrs(input);
        assert_eq!(output, r"![alt\]text](img.png)");
        assert_eq!(attrs.len(), 1);
        let a = &attrs[&0];
        assert_eq!(a.width.as_deref(), Some("100"));
    }

    #[test]
    fn extract_image_attrs_multiple_images() {
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
    fn extract_image_attrs_non_ascii() {
        let input = "图片说明 ![描述](图片.png){width=200} 后续文本";
        let (output, attrs) = extract_image_attrs(input);
        assert_eq!(output, "图片说明 ![描述](图片.png) 后续文本");
        assert_eq!(attrs.len(), 1);
        let a = &attrs[&output.find("![描述]").unwrap()];
        assert_eq!(a.width.as_deref(), Some("200"));
    }

    #[test]
    fn extract_image_attrs_unknown_key_ignored() {
        let input = "![alt](img.png){foo=bar width=100}";
        let (output, attrs) = extract_image_attrs(input);
        assert_eq!(output, "![alt](img.png)");
        assert_eq!(attrs.len(), 1);
        let a = &attrs[&0];
        assert_eq!(a.width.as_deref(), Some("100"));
    }

    #[test]
    fn extract_image_attrs_bang_without_bracket_preserved() {
        let input = "!{width=500}";
        let (output, attrs) = extract_image_attrs(input);
        assert_eq!(output, input);
        assert!(attrs.is_empty());
    }

    #[test]
    fn extract_image_attrs_alt_without_paren_preserved() {
        let input = "![alt]{width=500}";
        let (output, attrs) = extract_image_attrs(input);
        assert_eq!(output, input);
        assert!(attrs.is_empty());
    }

    #[test]
    fn extract_image_attrs_no_image_braces_preserved() {
        let input = "text {width=500} more";
        let (output, attrs) = extract_image_attrs(input);
        assert_eq!(output, input);
        assert!(attrs.is_empty());
    }

    #[test]
    fn extract_image_attrs_unclosed_brace_preserved() {
        let input = indoc! {"
            ![alt](img.png){width=500
            next line
        "};
        let (output, attrs) = extract_image_attrs(input);
        assert_eq!(output, input);
        assert!(attrs.is_empty());
    }

    #[test]
    fn extract_image_attrs_unclosed_brace_at_eof_preserved() {
        let input = "![alt](img.png){width=500";
        let (output, attrs) = extract_image_attrs(input);
        assert_eq!(output, input);
        assert!(attrs.is_empty());
    }

    #[test]
    fn extract_image_attrs_unclosed_quoted_value_preserved() {
        let input = r#"![alt](img.png){width="a}b"#;
        let (output, attrs) = extract_image_attrs(input);
        assert_eq!(output, input);
        assert!(attrs.is_empty());
    }

    #[test]
    fn extract_image_attrs_empty_brace_block_drops_attrs_entry() {
        let input = "![alt](img.png){}";
        let (output, attrs) = extract_image_attrs(input);
        assert_eq!(output, "![alt](img.png)");
        assert!(attrs.is_empty());
    }

    #[test]
    fn extract_image_attrs_unmatched_open_bracket_passes_bang_through() {
        let input = "![no close paren or bracket";
        let (output, attrs) = extract_image_attrs(input);
        assert_eq!(output, input);
        assert!(attrs.is_empty());
    }

    // ── extract_image_attrs (code awareness) ──

    #[test]
    fn extract_image_attrs_skips_inline_code() {
        let input = "`![alt](img.png){width=500}` rest";
        let (output, attrs) = extract_image_attrs(input);
        assert_eq!(output, input);
        assert!(attrs.is_empty());
    }

    #[test]
    fn extract_image_attrs_skips_fenced_code() {
        let input = indoc! {"
            ```
            ![alt](img.png){width=500}
            ```
        "};
        let (output, attrs) = extract_image_attrs(input);
        assert_eq!(output, input);
        assert!(attrs.is_empty());

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
    fn extract_image_attrs_after_fenced_code() {
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

    // ── ImageAttrs::is_empty ──

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
        assert!(
            !ImageAttrs {
                auto_width: Some(100),
                ..Default::default()
            }
            .is_empty()
        );
        assert!(
            !ImageAttrs {
                auto_height: Some(100),
                ..Default::default()
            }
            .is_empty()
        );
        assert!(
            !ImageAttrs {
                lqip_uri: Some("data:image/webp;base64,AAA".into()),
                ..Default::default()
            }
            .is_empty()
        );
    }

    // ── ImageAttrs::fill_from_meta ──

    #[test]
    fn fill_from_meta_stamps_auto_dims_and_lqip() {
        let mut attrs = ImageAttrs::default();
        let meta = ImageMeta {
            width: 1600,
            height: 900,
            lqip_uri: Some("data:image/webp;base64,XYZ".into()),
        };
        attrs.fill_from_meta(&meta);
        assert_eq!(attrs.auto_width, Some(1600));
        assert_eq!(attrs.auto_height, Some(900));
        assert_eq!(
            attrs.lqip_uri.as_deref(),
            Some("data:image/webp;base64,XYZ")
        );
    }

    #[test]
    fn fill_from_meta_preserves_manual_fields() {
        let mut attrs = ImageAttrs {
            id: Some("hero".into()),
            classes: vec!["wide".into()],
            width: Some("400".into()),
            height: Some("300".into()),
            ..ImageAttrs::default()
        };
        let meta = ImageMeta {
            width: 1600,
            height: 900,
            lqip_uri: None,
        };
        attrs.fill_from_meta(&meta);
        assert_eq!(attrs.id.as_deref(), Some("hero"));
        assert_eq!(attrs.classes, vec!["wide"]);
        assert_eq!(attrs.width.as_deref(), Some("400"));
        assert_eq!(attrs.height.as_deref(), Some("300"));
        assert_eq!(attrs.auto_width, Some(1600));
        assert_eq!(attrs.auto_height, Some(900));
        assert!(attrs.lqip_uri.is_none());
    }
}
