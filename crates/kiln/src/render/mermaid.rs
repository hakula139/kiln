use indoc::formatdoc;

use crate::html::escape;

/// Wraps a Mermaid diagram source in a `<pre class="mermaid">` block for mermaid.js.
///
/// The escaped source appears both as inner text, where mermaid.js reads it, and in `data-source`,
/// so a theme toggle can restore it after mermaid replaces the content with `<svg>`.
#[must_use]
pub(super) fn render_mermaid(source: &str) -> String {
    let escaped = escape(source);
    formatdoc! {r#"
        <pre class="mermaid" data-source="{escaped}">{escaped}</pre>
    "#}
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::*;

    // ── render_mermaid ──

    #[test]
    fn render_mermaid_wraps_source_in_pre_with_class() {
        let source = indoc! {"
            graph TD
                A --> B
        "};
        let html = render_mermaid(source);
        let expected = indoc! {r#"
            <pre class="mermaid" data-source="graph TD
                A --&gt; B
            ">graph TD
                A --&gt; B
            </pre>
        "#};
        assert_eq!(html, expected);
    }

    #[test]
    fn render_mermaid_escapes_html_special_chars() {
        let html = render_mermaid(r#"A["<b>&"]"#);
        // Both inner text and attribute value carry the escaped form so the
        // browser's textContent / dataset.source decode back to the original.
        assert_eq!(
            html,
            indoc! {r#"
                <pre class="mermaid" data-source="A[&quot;&lt;b&gt;&amp;&quot;]">A[&quot;&lt;b&gt;&amp;&quot;]</pre>
            "#},
        );
    }

    #[test]
    fn render_mermaid_empty_source() {
        let html = render_mermaid("");
        assert_eq!(
            html,
            indoc! {r#"
                <pre class="mermaid" data-source=""></pre>
            "#},
        );
    }
}
