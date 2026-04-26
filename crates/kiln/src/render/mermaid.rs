use crate::html::escape;

/// Wraps a Mermaid diagram source in a `<pre class="mermaid">` block ready
/// for client-side rendering by mermaid.js.
///
/// The DSL is emitted twice:
///
/// - As the element's inner text, where mermaid.js reads it on first render.
/// - In `data-source`, so a theme-toggle handler can restore the source
///   after mermaid replaces the inner content with `<svg>`.
///
/// Both copies are HTML-escaped for safety in inner-text and attribute-value
/// contexts.
#[must_use]
pub(crate) fn render_mermaid(source: &str) -> String {
    let escaped = escape(source);
    let mut html = format!(r#"<pre class="mermaid" data-source="{escaped}">{escaped}</pre>"#);
    html.push('\n');
    html
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::*;

    // ── render_mermaid ──

    #[test]
    fn render_mermaid_wraps_source_in_pre_with_class() {
        let html = render_mermaid("graph TD\nA --> B\n");
        assert_eq!(
            html,
            "<pre class=\"mermaid\" data-source=\"graph TD\nA --&gt; B\n\">\
             graph TD\nA --&gt; B\n</pre>\n",
        );
    }

    #[test]
    fn render_mermaid_escapes_html_special_chars() {
        let html = render_mermaid(r#"A["<b>&"]"#);
        // Both inner text and attribute value carry the escaped form so the
        // browser's textContent / dataset.source decode back to the original.
        assert_eq!(
            html,
            "<pre class=\"mermaid\" data-source=\"A[&quot;&lt;b&gt;&amp;&quot;]\">\
             A[&quot;&lt;b&gt;&amp;&quot;]</pre>\n",
        );
    }

    #[test]
    fn render_mermaid_preserves_dsl_whitespace() {
        let source = indoc! {"
            graph TB
                A((36))
                A --> B((8))
        "};
        let html = render_mermaid(source);
        // Indentation and newlines are preserved verbatim — mermaid is
        // whitespace-sensitive in some dialects (flowchart subgraphs).
        assert!(
            html.contains("\n    A((36))\n    A --&gt; B((8))\n"),
            "indentation and newlines preserved, html:\n{html}",
        );
    }

    #[test]
    fn render_mermaid_empty_source() {
        let html = render_mermaid("");
        assert_eq!(html, "<pre class=\"mermaid\" data-source=\"\"></pre>\n");
    }
}
