use std::collections::BTreeMap;

use super::CalloutKind;
use crate::html::{escape, writeln_indented};

/// Renders a callout to HTML as a collapsible `<details>` element.
///
/// Output structure:
///
/// ```html
/// <details class="callout note" open>
///   <summary class="callout-title">Title</summary>
///   <div class="callout-body"><div class="callout-body-inner">...</div></div>
/// </details>
/// ```
///
/// - `title`: when `None`, the kind's display name is used.
/// - `open`: maps to the HTML `open` attribute on `<details>`.
/// - `id` / `classes`: optional Pandoc attributes rendered on the outer element.
/// - `body_html` must be pre-rendered, since the caller handles markdown recursion.
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
    let display_title = escape(title.unwrap_or(&default_title));
    let open_attr = if open { " open" } else { "" };

    let id_attr = id
        .map(|v| format!(r#" id="{}""#, escape(v)))
        .unwrap_or_default();

    let mut class_val = format!("callout {}", kind.as_ref());
    for class in classes {
        class_val.push(' ');
        class_val.push_str(&escape(class));
    }

    let mut html = String::new();
    writeln_indented!(
        &mut html,
        0,
        r#"<details{id_attr} class="{class_val}"{open_attr}>"#
    );
    writeln_indented!(
        &mut html,
        1,
        r#"<summary class="callout-title">{display_title}</summary>"#
    );
    writeln_indented!(
        &mut html,
        1,
        r#"<div class="callout-body"><div class="callout-body-inner">{body_html}</div></div>"#
    );
    writeln_indented!(&mut html, 0, "</details>");
    html
}

/// Extracts callout parameters from pre-parsed named arguments.
///
/// Recognized keys: `type` (defaults to `note`), `title`, `open`.
#[must_use]
pub(super) fn parse_named_args(
    named: &BTreeMap<String, String>,
) -> (CalloutKind, Option<String>, bool) {
    let kind = named
        .get("type")
        .and_then(|v| v.parse::<CalloutKind>().ok())
        .unwrap_or(CalloutKind::Note);

    let title = named.get("title").filter(|v| !v.is_empty()).cloned();

    let open = named
        .get("open")
        .is_none_or(|v| !v.eq_ignore_ascii_case("false"));

    (kind, title, open)
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::*;

    // ── render_callout ──

    #[test]
    fn render_callout_default_title_and_empty_body() {
        let html = render_callout(CalloutKind::Info, None, true, None, &[], "");
        assert_eq!(
            html,
            indoc! {r#"
                <details class="callout info" open>
                  <summary class="callout-title">Info</summary>
                  <div class="callout-body"><div class="callout-body-inner"></div></div>
                </details>
            "#}
        );
    }

    #[test]
    fn render_callout_all_kinds_css_class() {
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
    fn render_callout_with_title_and_body() {
        let html = render_callout(
            CalloutKind::Note,
            Some("Read This"),
            true,
            None,
            &[],
            "<p>Hello</p>\n<p>World</p>\n",
        );
        assert_eq!(
            html,
            indoc! {r#"
                <details class="callout note" open>
                  <summary class="callout-title">Read This</summary>
                  <div class="callout-body"><div class="callout-body-inner"><p>Hello</p>
                <p>World</p>
                </div></div>
                </details>
            "#}
        );
    }

    #[test]
    fn render_callout_collapsed() {
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
                  <div class="callout-body"><div class="callout-body-inner"><p>Hidden content</p>
                </div></div>
                </details>
            "#}
        );
    }

    #[test]
    fn render_callout_with_id_and_classes() {
        let classes = vec!["highlight".into(), "wide".into()];
        let html = render_callout(
            CalloutKind::Warning,
            None,
            true,
            Some("warn-1"),
            &classes,
            "",
        );
        assert!(
            html.contains(r#"<details id="warn-1" class="callout warning highlight wide" open>"#),
            "id and extra classes should be rendered, html:\n{html}"
        );
    }

    #[test]
    fn render_callout_escapes_title() {
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
    fn render_callout_escapes_id_and_classes() {
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

    // ── parse_named_args ──

    fn named(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    #[test]
    fn parse_named_args_defaults() {
        assert_eq!(
            parse_named_args(&BTreeMap::new()),
            (CalloutKind::Note, None, true)
        );
    }

    #[test]
    fn parse_named_args_type() {
        assert_eq!(
            parse_named_args(&named(&[("type", "tip")])),
            (CalloutKind::Tip, None, true)
        );
        assert_eq!(
            parse_named_args(&named(&[("type", "TIP")])),
            (CalloutKind::Tip, None, true)
        );
    }

    #[test]
    fn parse_named_args_title_only() {
        assert_eq!(
            parse_named_args(&named(&[("title", "Custom")])),
            (CalloutKind::Note, Some("Custom".into()), true)
        );
    }

    #[test]
    fn parse_named_args_open() {
        assert_eq!(
            parse_named_args(&named(&[("open", "false")])),
            (CalloutKind::Note, None, false)
        );
        assert_eq!(
            parse_named_args(&named(&[("open", "true")])),
            (CalloutKind::Note, None, true)
        );
        assert_eq!(
            parse_named_args(&named(&[("open", "FALSE")])),
            (CalloutKind::Note, None, false)
        );
    }

    #[test]
    fn parse_named_args_all_keys() {
        assert_eq!(
            parse_named_args(&named(&[
                ("open", "false"),
                ("title", "Careful"),
                ("type", "warning"),
            ])),
            (CalloutKind::Warning, Some("Careful".into()), false)
        );
    }

    #[test]
    fn parse_named_args_unknown_type_defaults_to_note() {
        assert_eq!(
            parse_named_args(&named(&[("type", "invalid")])),
            (CalloutKind::Note, None, true)
        );
    }

    #[test]
    fn parse_named_args_empty_title_treated_as_none() {
        assert_eq!(
            parse_named_args(&named(&[("title", "")])),
            (CalloutKind::Note, None, true)
        );
    }

    #[test]
    fn parse_named_args_ignores_unknown_keys() {
        assert_eq!(
            parse_named_args(&named(&[
                ("open", "false"),
                ("title", "Hello"),
                ("unknown", "x"),
            ])),
            (CalloutKind::Note, Some("Hello".into()), false)
        );
    }
}
