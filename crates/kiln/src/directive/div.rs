use crate::attrs::Identity;

/// Renders a directive without a template as a `<div>`, with a non-empty name as its first class.
///
/// A block without a name, id or classes passes its body through unchanged.
pub(crate) fn render_div(name: &str, identity: &Identity, body_html: &str) -> String {
    let base: &[&str] = if name.is_empty() { &[] } else { &[name] };
    let attrs = identity.html_attrs(base);
    if attrs.is_empty() {
        return body_html.to_owned();
    }
    format!("<div{attrs}>{body_html}</div>\n")
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::*;

    // ── render_div ──

    #[test]
    fn puts_name_before_authored_classes() {
        let identity = Identity {
            id: Some("main".into()),
            classes: vec!["extra".into(), "wide".into()],
        };
        let html = render_div("wrapper", &identity, "<p>Body</p>\n");
        assert_eq!(
            html,
            indoc! {r#"
                <div id="main" class="wrapper extra wide"><p>Body</p>
                </div>
            "#},
        );
    }

    #[test]
    fn anonymous_block_keeps_authored_classes() {
        let identity = Identity {
            id: None,
            classes: vec!["compact".into()],
        };
        assert_eq!(
            render_div("", &identity, "<p>Content</p>\n"),
            indoc! {r#"
                <div class="compact"><p>Content</p>
                </div>
            "#},
        );
    }

    #[test]
    fn passes_bare_body_through() {
        let html = render_div("", &Identity::default(), "<p>Content</p>\n");
        assert_eq!(html, "<p>Content</p>\n");
    }
}
