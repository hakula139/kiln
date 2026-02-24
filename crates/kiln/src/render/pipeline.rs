use syntect::parsing::SyntaxSet;

use crate::directive::callout::render_callout;
use crate::directive::div::render_div;
use crate::directive::parser::parse_directives;
use crate::directive::{DirectiveBlock, DirectiveKind};
use crate::render::markdown::render_markdown;
use crate::render::toc::render_toc_html;

/// The fully rendered output of a single page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedPage {
    pub content_html: String,
    pub toc_html: String,
}

/// Renders raw markdown through the full pipeline: directive processing,
/// markdown rendering, and `ToC` generation.
#[must_use]
pub fn render_page(raw_content: &str, syntax_set: &SyntaxSet) -> RenderedPage {
    let processed = render_directives(raw_content, syntax_set);
    let md_output = render_markdown(&processed, syntax_set);
    let toc_html = render_toc_html(&md_output.headings);

    RenderedPage {
        content_html: md_output.html,
        toc_html,
    }
}

/// Recursively processes directive blocks in content, replacing them with
/// rendered HTML.
///
/// Top-level blocks are rendered first (their bodies are recursively processed),
/// then replaced right-to-left so byte offsets stay valid.
///
/// Each directive body is rendered as an isolated markdown document. This means:
/// - Headings inside directives do **not** appear in the page-level `ToC`.
/// - Footnotes and reference-link definitions do not resolve across directive
///   boundaries.
fn render_directives(content: &str, syntax_set: &SyntaxSet) -> String {
    let all_blocks = parse_directives(content);
    if all_blocks.is_empty() {
        return content.to_owned();
    }

    let top_level = top_level_blocks(&all_blocks);
    let mut result = content.to_owned();

    // Replace right-to-left so earlier ranges remain valid.
    for block in top_level.into_iter().rev() {
        let inner = render_directives(&block.body, syntax_set);
        let md_output = render_markdown(&inner, syntax_set);
        let html = render_directive_block(
            &block.kind,
            block.id.as_deref(),
            &block.classes,
            &md_output.html,
        );

        // Blank-line padding: <details> / <div> are CommonMark type 6 HTML
        // blocks which cannot interrupt paragraphs. Safe because the directive
        // parser only matches column-0 fences (never indented contexts).
        let padded = format!("\n{html}\n");
        result.replace_range(block.range.clone(), &padded);
    }

    result
}

/// Filters to only top-level directive blocks (those not nested inside another).
///
/// Assumes `blocks` are sorted by ascending `range.start`.
fn top_level_blocks(blocks: &[DirectiveBlock]) -> Vec<&DirectiveBlock> {
    let mut result = Vec::new();
    let mut outer_end: usize = 0;

    for block in blocks {
        if block.range.start >= outer_end {
            result.push(block);
            outer_end = block.range.end;
        }
    }

    result
}

/// Dispatches a directive block to its renderer.
fn render_directive_block(
    kind: &DirectiveKind,
    id: Option<&str>,
    classes: &[String],
    body_html: &str,
) -> String {
    match kind {
        DirectiveKind::Callout { kind, title, open } => {
            render_callout(*kind, title.as_deref(), *open, id, classes, body_html)
        }
        // args intentionally unused until directive-specific renderers exist.
        DirectiveKind::Unknown { name, .. } => render_div(name, id, classes, body_html),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::LazyLock;

    use indoc::indoc;

    use super::*;

    static SYNTAX_SET: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_newlines);

    // -- render_page --

    #[test]
    fn render_no_directives() {
        let input = indoc! {"
            # Hello

            Some text.
        "};
        let page = render_page(input, &SYNTAX_SET);
        assert!(
            page.content_html.contains("<p>Some text.</p>"),
            "html:\n{}",
            page.content_html
        );
        assert!(
            !page.toc_html.is_empty(),
            "should generate ToC from heading"
        );
    }

    #[test]
    fn render_single_callout() {
        let input = indoc! {"
            ::: callout
            Hello **world**.
            :::
        "};
        let page = render_page(input, &SYNTAX_SET);
        assert!(
            page.content_html.contains(r#"class="callout note""#),
            "should have callout wrapper, html:\n{}",
            page.content_html
        );
        assert!(
            page.content_html.contains("<strong>world</strong>"),
            "body markdown should be rendered, html:\n{}",
            page.content_html
        );
    }

    #[test]
    fn render_unknown_directive() {
        let input = indoc! {"
            ::: custom
            Some body.
            :::
        "};
        let page = render_page(input, &SYNTAX_SET);
        assert!(
            page.content_html.contains(r#"class="custom""#),
            "unknown directive should be wrapped in div with name as class, html:\n{}",
            page.content_html
        );
        assert!(
            page.content_html.contains("<p>Some body.</p>"),
            "unknown directive body rendered as markdown, html:\n{}",
            page.content_html
        );
    }

    #[test]
    fn render_anonymous_div() {
        let input = indoc! {"
            ::: {.compact-table}
            | A | B |
            |---|---|
            | 1 | 2 |
            :::
        "};
        let page = render_page(input, &SYNTAX_SET);
        assert!(
            page.content_html.contains(r#"class="compact-table""#),
            "anonymous div should have class, html:\n{}",
            page.content_html
        );
        assert!(
            page.content_html.contains("<table>"),
            "table should be rendered inside div, html:\n{}",
            page.content_html
        );
    }

    #[test]
    fn render_callout_with_id_and_classes() {
        let input = indoc! {"
            ::: callout {#my-note .highlight type=tip}
            Body text.
            :::
        "};
        let page = render_page(input, &SYNTAX_SET);
        assert!(
            page.content_html.contains(r#"id="my-note""#),
            "id should be propagated, html:\n{}",
            page.content_html
        );
        assert!(
            page.content_html
                .contains(r#"class="callout tip highlight""#),
            "classes should be propagated, html:\n{}",
            page.content_html
        );
    }

    #[test]
    fn render_multiple_sequential_directives() {
        let input = indoc! {"
            ::: callout
            First.
            :::

            Some text between.

            ::: callout {type=warning}
            Second.
            :::
        "};
        let page = render_page(input, &SYNTAX_SET);
        assert!(
            page.content_html.contains(r#"class="callout note""#),
            "first callout, html:\n{}",
            page.content_html
        );
        assert!(
            page.content_html.contains(r#"class="callout warning""#),
            "second callout, html:\n{}",
            page.content_html
        );
        assert!(
            page.content_html.contains("<p>Some text between.</p>"),
            "text between directives preserved, html:\n{}",
            page.content_html
        );
    }

    #[test]
    fn render_nested_callouts() {
        let input = indoc! {"
            :::: callout {type=warning}
            Outer text.

            ::: callout {type=tip}
            Inner text.
            :::
            ::::
        "};
        let page = render_page(input, &SYNTAX_SET);
        assert!(
            page.content_html.contains(r#"class="callout warning""#),
            "outer callout, html:\n{}",
            page.content_html
        );
        assert!(
            page.content_html.contains("<p>Outer text.</p>"),
            "outer body rendered, html:\n{}",
            page.content_html
        );
        assert!(
            page.content_html.contains(r#"class="callout tip""#),
            "inner callout, html:\n{}",
            page.content_html
        );
        assert!(
            page.content_html.contains("<p>Inner text.</p>"),
            "inner body rendered, html:\n{}",
            page.content_html
        );
    }

    #[test]
    fn render_directive_with_code_and_math() {
        let input = indoc! {"
            ::: callout
            Inline $x^2$ math.

            ```rust
            fn main() {}
            ```
            :::
        "};
        let page = render_page(input, &SYNTAX_SET);
        assert!(
            page.content_html.contains("math-inline"),
            "math should be rendered, html:\n{}",
            page.content_html
        );
        assert!(
            page.content_html.contains(r#"class="highlight""#),
            "code should be highlighted, html:\n{}",
            page.content_html
        );
    }

    // -- top_level_blocks --

    #[test]
    fn top_level_blocks_filters_nested() {
        let input = indoc! {"
            :::: outer
            ::: inner
            Body
            :::
            ::::
        "};
        let all = parse_directives(input);
        assert_eq!(all.len(), 2, "parser should find both blocks");

        let top = top_level_blocks(&all);
        assert_eq!(top.len(), 1, "only outer block is top-level");
        assert_eq!(top[0].range.start, 0);
    }
}
