use crate::render::escape_html;

/// Renders an unknown directive as a `<div>` wrapper.
///
/// - The directive name (if non-empty) becomes the first CSS class.
/// - Extra `.class` tokens from Pandoc attributes are appended.
/// - When no name, id, or classes are present, the body is passed through as-is.
#[must_use]
pub fn render_div(name: &str, id: Option<&str>, classes: &[String], body_html: &str) -> String {
    let has_attrs = id.is_some() || !name.is_empty() || !classes.is_empty();
    if !has_attrs {
        return body_html.to_owned();
    }

    let id_attr = id
        .map(|v| format!(" id=\"{}\"", escape_html(v)))
        .unwrap_or_default();

    let mut class_parts = Vec::new();
    if !name.is_empty() {
        class_parts.push(escape_html(name));
    }
    for class in classes {
        class_parts.push(escape_html(class));
    }
    let class_attr = if class_parts.is_empty() {
        String::new()
    } else {
        format!(" class=\"{}\"", class_parts.join(" "))
    };

    format!("<div{id_attr}{class_attr}>{body_html}</div>\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- render_div --

    #[test]
    fn render_with_name() {
        let html = render_div("compact-table", None, &[], "<p>Content</p>\n");
        assert_eq!(
            html,
            "<div class=\"compact-table\"><p>Content</p>\n</div>\n"
        );
    }

    #[test]
    fn render_with_id() {
        let html = render_div("", Some("section-1"), &[], "<p>Content</p>\n");
        assert_eq!(html, "<div id=\"section-1\"><p>Content</p>\n</div>\n");
    }

    #[test]
    fn render_with_extra_classes() {
        let classes = vec!["compact".into(), "wide".into()];
        let html = render_div("", None, &classes, "<p>Content</p>\n");
        assert_eq!(html, "<div class=\"compact wide\"><p>Content</p>\n</div>\n");
    }

    #[test]
    fn render_with_name_id_and_classes() {
        let classes = vec!["extra".into(), "wide".into()];
        let html = render_div("wrapper", Some("main"), &classes, "<p>Body</p>\n");
        assert_eq!(
            html,
            "<div id=\"main\" class=\"wrapper extra wide\"><p>Body</p>\n</div>\n"
        );
    }

    #[test]
    fn render_without_attrs() {
        let html = render_div("", None, &[], "<p>Content</p>\n");
        assert_eq!(html, "<p>Content</p>\n");
    }

    #[test]
    fn render_escapes_name() {
        let html = render_div("<script>", None, &[], "");
        assert!(
            html.contains("class=\"&lt;script&gt;\""),
            "name should be escaped, html:\n{html}"
        );
        assert!(
            !html.contains("<script>"),
            "raw script tag must not appear, html:\n{html}"
        );
    }

    #[test]
    fn render_escapes_id_and_classes() {
        let classes = vec![r#"a"b"#.into()];
        let html = render_div("", Some(r#"x"y"#), &classes, "");
        assert!(
            html.contains(r#"id="x&quot;y""#),
            "id should be escaped, html:\n{html}"
        );
        assert!(
            html.contains(r"a&quot;b"),
            "class should be escaped, html:\n{html}"
        );
    }
}
