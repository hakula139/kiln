use super::{CalloutKind, parse_attrs};
use crate::render::escape_html;

/// Renders a callout to HTML as a collapsible `<details>` element.
///
/// - `title`: when `None`, the kind's display name is used.
/// - `open`: maps to the HTML `open` attribute on `<details>`.
/// - `id` / `classes`: optional Pandoc attributes rendered on the outer element.
/// - `body_html` must be pre-rendered — the caller handles markdown recursion.
#[must_use]
pub fn render_callout(
    kind: CalloutKind,
    title: Option<&str>,
    open: bool,
    id: Option<&str>,
    classes: &[String],
    body_html: &str,
) -> String {
    let default_title = kind.to_string();
    let display_title = escape_html(title.unwrap_or(&default_title));
    let open_attr = if open { " open" } else { "" };

    let id_attr = id
        .map(|v| format!(" id=\"{}\"", escape_html(v)))
        .unwrap_or_default();

    let mut class_val = format!("callout {}", kind.as_ref());
    for class in classes {
        class_val.push(' ');
        class_val.push_str(&escape_html(class));
    }

    format!(
        "<details{id_attr} class=\"{class_val}\"{open_attr}>\n\
         <summary class=\"callout-title\">{display_title}</summary>\n\
         <div class=\"callout-body\">{body_html}</div>\n\
         </details>\n"
    )
}

/// Parses callout parameters from Pandoc-style key-value attributes.
///
/// Recognized keys: `type` (defaults to `note`), `title`, `open`.
#[must_use]
pub(super) fn parse_args(args: &str) -> (CalloutKind, Option<String>, bool) {
    let mut kind = CalloutKind::Note;
    let mut title = None;
    let mut open = true;

    for (key, value) in parse_attrs(args) {
        match key {
            "type" => {
                if let Ok(k) = value.parse::<CalloutKind>() {
                    kind = k;
                }
            }
            "title" => title = (!value.is_empty()).then(|| value.into_owned()),
            "open" => open = !value.eq_ignore_ascii_case("false"),
            _ => {}
        }
    }

    (kind, title, open)
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::*;

    // -- render_callout --

    #[test]
    fn render_default_title_and_empty_body() {
        let html = render_callout(CalloutKind::Info, None, true, None, &[], "");
        assert_eq!(
            html,
            indoc! {r#"
                <details class="callout info" open>
                <summary class="callout-title">Info</summary>
                <div class="callout-body"></div>
                </details>
            "#}
        );
    }

    #[test]
    fn render_all_kinds_css_class() {
        use strum::IntoEnumIterator;
        for kind in CalloutKind::iter() {
            let html = render_callout(kind, None, true, None, &[], "");
            let expected = format!(r#"<details class="callout {}"#, kind.as_ref());
            assert!(
                html.contains(&expected),
                "kind {kind:?} should produce class {:?}, html:\n{html}",
                kind.as_ref()
            );
        }
    }

    #[test]
    fn render_with_title_and_body() {
        let html = render_callout(
            CalloutKind::Note,
            Some("Read This"),
            true,
            None,
            &[],
            "<p>Hello</p>\n",
        );
        assert_eq!(
            html,
            indoc! {r#"
                <details class="callout note" open>
                <summary class="callout-title">Read This</summary>
                <div class="callout-body"><p>Hello</p>
                </div>
                </details>
            "#}
        );
    }

    #[test]
    fn render_collapsed() {
        let html = render_callout(
            CalloutKind::Tip,
            Some("Hint"),
            false,
            None,
            &[],
            "<p>Hidden content</p>\n",
        );
        assert_eq!(
            html,
            indoc! {r#"
                <details class="callout tip">
                <summary class="callout-title">Hint</summary>
                <div class="callout-body"><p>Hidden content</p>
                </div>
                </details>
            "#}
        );
    }

    #[test]
    fn render_with_id() {
        let html = render_callout(CalloutKind::Note, None, true, Some("my-note"), &[], "");
        assert!(
            html.contains(r#"<details id="my-note" class="callout note" open>"#),
            "id attribute should be rendered, html:\n{html}"
        );
    }

    #[test]
    fn render_with_extra_classes() {
        let classes = vec!["compact".into(), "wide".into()];
        let html = render_callout(CalloutKind::Tip, None, true, None, &classes, "");
        assert!(
            html.contains(r#"class="callout tip compact wide""#),
            "extra classes should be appended, html:\n{html}"
        );
    }

    #[test]
    fn render_with_id_and_classes() {
        let classes = vec!["highlight".into()];
        let html = render_callout(
            CalloutKind::Warning,
            None,
            true,
            Some("warn-1"),
            &classes,
            "",
        );
        assert!(
            html.contains(r#"<details id="warn-1" class="callout warning highlight" open>"#),
            "id and extra classes should be rendered, html:\n{html}"
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
        let html = render_callout(CalloutKind::Example, Some("Steps"), true, None, &[], body);
        assert!(
            html.contains(body),
            "body HTML should be passed through unchanged, html:\n{html}"
        );
    }

    #[test]
    fn render_escapes_title() {
        let html = render_callout(
            CalloutKind::Tip,
            Some("<script>alert(1)</script>"),
            true,
            None,
            &[],
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
    fn render_escapes_id_and_classes() {
        let classes = vec![r#"a"b"#.into()];
        let html = render_callout(CalloutKind::Note, None, true, Some(r#"x"y"#), &classes, "");
        assert!(
            html.contains(r#"id="x&quot;y""#),
            "id should be escaped, html:\n{html}"
        );
        assert!(
            html.contains(r"a&quot;b"),
            "class should be escaped, html:\n{html}"
        );
    }

    // -- parse_args --

    #[test]
    fn parse_args_defaults() {
        assert_eq!(parse_args(""), (CalloutKind::Note, None, true));
    }

    #[test]
    fn parse_args_type() {
        assert_eq!(parse_args("type=tip"), (CalloutKind::Tip, None, true));
        // Case-insensitive.
        assert_eq!(parse_args("type=TIP"), (CalloutKind::Tip, None, true));
    }

    #[test]
    fn parse_args_unknown_type_defaults_to_note() {
        assert_eq!(parse_args("type=invalid"), (CalloutKind::Note, None, true));
    }

    #[test]
    fn parse_args_title_only() {
        assert_eq!(
            parse_args(r#"title="Custom""#),
            (CalloutKind::Note, Some("Custom".into()), true)
        );
    }

    #[test]
    fn parse_args_open() {
        assert_eq!(parse_args("open=false"), (CalloutKind::Note, None, false));
        assert_eq!(parse_args("open=true"), (CalloutKind::Note, None, true));
        // Case-insensitive.
        assert_eq!(parse_args("open=FALSE"), (CalloutKind::Note, None, false));
    }

    #[test]
    fn parse_args_all_keys() {
        assert_eq!(
            parse_args(r#"type=warning title="Careful" open=false"#),
            (CalloutKind::Warning, Some("Careful".into()), false)
        );
    }

    #[test]
    fn parse_args_empty_title_treated_as_none() {
        assert_eq!(parse_args(r#"title="""#), (CalloutKind::Note, None, true));
    }

    #[test]
    fn parse_args_ignores_unknown_keys() {
        assert_eq!(
            parse_args(r#"title="Hello" unknown="x" open=false"#),
            (CalloutKind::Note, Some("Hello".into()), false)
        );
    }
}
