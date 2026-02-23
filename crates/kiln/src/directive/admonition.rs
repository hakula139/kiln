use super::{AdmonitionKind, parse_attrs};
use crate::render::escape_html;

/// Renders an admonition to HTML as a collapsible `<details>` element.
///
/// - `title`: when `None`, the kind's display name is used.
/// - `open`: maps to the HTML `open` attribute on `<details>`.
/// - `body_html` must be pre-rendered — the caller handles markdown recursion.
#[must_use]
pub fn render_admonition(
    kind: AdmonitionKind,
    title: Option<&str>,
    open: bool,
    body_html: &str,
) -> String {
    let default_title = kind.to_string();
    let display_title = escape_html(title.unwrap_or(&default_title));
    let css_class: &str = kind.as_ref();
    let open_attr = if open { " open" } else { "" };

    format!(
        "<details class=\"admonition {css_class}\"{open_attr}>\n\
         <summary class=\"admonition-title\">{display_title}</summary>\n\
         <div class=\"admonition-body\">{body_html}</div>\n\
         </details>\n"
    )
}

/// Parses admonition parameters from Pandoc-style key-value attributes.
///
/// Recognized keys: `title`, `open`.
pub(super) fn parse_args(args: &str) -> (Option<String>, bool) {
    let mut title = None;
    let mut open = true;

    for (key, value) in parse_attrs(args) {
        match key {
            "title" => title = (!value.is_empty()).then(|| value.to_string()),
            "open" => open = !value.eq_ignore_ascii_case("false"),
            _ => {}
        }
    }

    (title, open)
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::*;

    // -- render_admonition --

    #[test]
    fn render_default_title_and_empty_body() {
        let html = render_admonition(AdmonitionKind::Info, None, true, "");
        assert_eq!(
            html,
            indoc! {r#"
                <details class="admonition info" open>
                <summary class="admonition-title">Info</summary>
                <div class="admonition-body"></div>
                </details>
            "#}
        );
    }

    #[test]
    fn render_open_with_title() {
        let html = render_admonition(
            AdmonitionKind::Note,
            Some("Read This"),
            true,
            "<p>Hello</p>\n",
        );
        assert_eq!(
            html,
            indoc! {r#"
                <details class="admonition note" open>
                <summary class="admonition-title">Read This</summary>
                <div class="admonition-body"><p>Hello</p>
                </div>
                </details>
            "#}
        );
    }

    #[test]
    fn render_collapsed() {
        let html = render_admonition(
            AdmonitionKind::Tip,
            Some("Hint"),
            false,
            "<p>Hidden content</p>\n",
        );
        assert_eq!(
            html,
            indoc! {r#"
                <details class="admonition tip">
                <summary class="admonition-title">Hint</summary>
                <div class="admonition-body"><p>Hidden content</p>
                </div>
                </details>
            "#}
        );
    }

    #[test]
    fn render_escapes_title() {
        let html = render_admonition(
            AdmonitionKind::Tip,
            Some("<script>alert(1)</script>"),
            true,
            "",
        );
        assert!(
            html.contains("&lt;script&gt;alert(1)&lt;/script&gt;"),
            "title should be escaped, html:\n{html}"
        );
        assert!(
            !html.contains("<script>"),
            "raw script tag must not appear, html:\n{html}"
        );
    }

    #[test]
    fn render_body_html_passed_through() {
        let body = indoc! {"
            <ul>
              <li>Item <strong>one</strong></li>
              <li>Item two</li>
            </ul>
        "};
        let html = render_admonition(AdmonitionKind::Example, Some("Steps"), true, body);
        assert!(
            html.contains(body),
            "body HTML should be passed through unchanged, html:\n{html}"
        );
    }

    #[test]
    fn render_all_kinds_css_class() {
        use strum::IntoEnumIterator;
        for kind in AdmonitionKind::iter() {
            let html = render_admonition(kind, None, true, "");
            let expected = format!(r#"<details class="admonition {}"#, kind.as_ref());
            assert!(
                html.contains(&expected),
                "kind {kind:?} should produce class {:?}, html:\n{html}",
                kind.as_ref()
            );
        }
    }

    // -- parse_args --

    #[test]
    fn parse_args_defaults() {
        assert_eq!(parse_args(""), (None, true));
    }

    #[test]
    fn parse_args_title_only() {
        assert_eq!(
            parse_args(r#"title="Custom""#),
            (Some("Custom".into()), true)
        );
    }

    #[test]
    fn parse_args_open() {
        assert_eq!(parse_args("open=false"), (None, false));
        assert_eq!(parse_args("open=true"), (None, true));
        // Case-insensitive
        assert_eq!(parse_args("open=False"), (None, false));
        assert_eq!(parse_args("open=FALSE"), (None, false));
    }

    #[test]
    fn parse_args_title_and_open() {
        assert_eq!(
            parse_args(r#"title="My Title" open=false"#),
            (Some("My Title".into()), false)
        );
    }

    #[test]
    fn parse_args_empty_title_treated_as_none() {
        assert_eq!(parse_args(r#"title="""#), (None, true));
    }
}
