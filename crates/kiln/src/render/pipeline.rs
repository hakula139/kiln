use std::path::Path;

use anyhow::Result;
use syntect::parsing::SyntaxSet;

use super::RenderOptions;
use super::emoji::replace_emojis;
use super::icon::replace_icons;
use super::image_attrs::extract_image_attrs;
use super::markdown::render_markdown;
use super::toc::render_toc_html;
use crate::directive::callout::render_callout;
use crate::directive::div::render_div;
use crate::directive::parser::parse_directives;
use crate::directive::{DirectiveBlock, DirectiveContext, DirectiveKind};
use crate::template::TemplateEngine;

/// The fully rendered output of a single page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedPage {
    pub content_html: String,
    pub toc_html: String,
}

/// Renders raw markdown through the full pipeline: directive processing,
/// markdown rendering, and `ToC` generation.
///
/// # Errors
///
/// Returns an error if a template-based directive fails to render.
pub fn render_page(
    raw_content: &str,
    syntax_set: &SyntaxSet,
    engine: &TemplateEngine,
    options: &RenderOptions,
    source_dir: Option<&Path>,
) -> Result<RenderedPage> {
    let processed = render_directives(raw_content, syntax_set, engine, source_dir)?;

    // Pre-process: extract image attrs, optionally replace shortcodes.
    let mut preprocessed = processed;
    if options.emojis {
        preprocessed = replace_emojis(&preprocessed);
    }
    if options.fontawesome {
        preprocessed = replace_icons(&preprocessed);
    }
    let (cleaned, image_attrs) = extract_image_attrs(&preprocessed);

    let md_output = render_markdown(&cleaned, syntax_set, &image_attrs, options.code_max_lines);
    let toc_html = render_toc_html(&md_output.headings);

    Ok(RenderedPage {
        content_html: md_output.html,
        toc_html,
    })
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
fn render_directives(
    content: &str,
    syntax_set: &SyntaxSet,
    engine: &TemplateEngine,
    source_dir: Option<&Path>,
) -> Result<String> {
    let all_blocks = parse_directives(content);
    if all_blocks.is_empty() {
        return Ok(content.to_owned());
    }

    let top_level = top_level_blocks(&all_blocks);
    let mut result = content.to_owned();

    // Replace right-to-left so earlier ranges remain valid.
    for block in top_level.into_iter().rev() {
        let inner = render_directives(&block.body, syntax_set, engine, source_dir)?;
        let (cleaned, image_attrs) = extract_image_attrs(&inner);
        let md_output = render_markdown(&cleaned, syntax_set, &image_attrs, None);
        let html = render_directive_block(block, &md_output.html, engine, source_dir)?;

        // Blank-line padding: <details> / <div> are CommonMark type 6 HTML
        // blocks which cannot interrupt paragraphs. Safe because the directive
        // parser only matches column-0 fences (never indented contexts).
        let padded = format!("\n{html}\n");
        result.replace_range(block.range.clone(), &padded);
    }

    Ok(result)
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
///
/// For `Unknown` directives, checks the template engine for a
/// `directives/<name>.html` template. Falls back to `render_div` if no
/// template exists.
fn render_directive_block(
    block: &DirectiveBlock,
    body_html: &str,
    engine: &TemplateEngine,
    source_dir: Option<&Path>,
) -> Result<String> {
    let id = block.id.as_deref();
    let classes = &block.classes;

    match &block.kind {
        DirectiveKind::Callout { kind, title, open } => Ok(render_callout(
            *kind,
            title.as_deref(),
            *open,
            id,
            classes,
            body_html,
        )),
        DirectiveKind::Unknown {
            name,
            positional_args,
            named_args,
        } => {
            let ctx = DirectiveContext {
                name: name.clone(),
                positional_args: positional_args.clone(),
                named_args: named_args.clone(),
                id: block.id.clone(),
                classes: block.classes.clone(),
                body_html: body_html.to_owned(),
                body_raw: block.body.clone(),
                source_dir: source_dir.map(|p| p.to_string_lossy().into_owned()),
            };
            match engine.render_directive(name, ctx) {
                Some(result) => result,
                None => Ok(render_div(name, id, classes, body_html)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::LazyLock;

    use std::fs;

    use indoc::indoc;

    use super::*;
    use crate::test_utils::test_engine;

    static SYNTAX_SET: LazyLock<SyntaxSet> = LazyLock::new(two_face::syntax::extra_newlines);

    fn render(input: &str) -> RenderedPage {
        render_with(input, &test_engine())
    }

    fn render_with(input: &str, engine: &TemplateEngine) -> RenderedPage {
        render_page(input, &SYNTAX_SET, engine, &RenderOptions::default(), None).unwrap()
    }

    // ── render_page ──

    #[test]
    fn render_page_no_directives() {
        let page = render(indoc! {"
            # Hello

            Some text.
        "});
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
    fn render_page_with_emojis_and_fontawesome() {
        let engine = test_engine();
        let options = RenderOptions {
            emojis: true,
            fontawesome: true,
            ..RenderOptions::default()
        };
        let input = "Hello :smile: and :(fas fa-link):";
        let page = render_page(input, &SYNTAX_SET, &engine, &options, None).unwrap();
        assert!(
            page.content_html.contains('\u{1f604}'),
            "emoji should be replaced, html:\n{}",
            page.content_html
        );
        assert!(
            page.content_html.contains(r#"class="fas fa-link""#),
            "icon should be replaced, html:\n{}",
            page.content_html
        );
    }

    // ── render_directives ──

    #[test]
    fn render_directives_sequential() {
        let page = render(indoc! {"
            ::: callout
            First.
            :::

            Some text between.

            ::: callout {type=warning}
            Second.
            :::
        "});
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
    fn render_directives_with_image_attrs() {
        let page = render(indoc! {"
            ::: callout
            ![A photo](img.png){width=240}
            :::
        "});
        assert!(
            page.content_html.contains(r#"width="240""#),
            "image inside directive should have width attribute, html:\n{}",
            page.content_html
        );
        assert!(
            !page.content_html.contains("{width=240}"),
            "raw attr block should be stripped, html:\n{}",
            page.content_html
        );
    }

    #[test]
    fn render_directives_nested() {
        let page = render(indoc! {"
            :::: callout {type=warning}
            Outer text.

            ::: callout {type=tip}
            Inner text.
            :::
            ::::
        "});
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

    // ── top_level_blocks ──

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

    // ── render_directive_block ──

    #[test]
    fn render_directive_callout() {
        let page = render(indoc! {"
            ::: callout
            Hello **world**.
            :::
        "});
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
    fn render_directive_with_id_and_classes() {
        let page = render(indoc! {"
            ::: callout {#my-note .highlight type=tip}
            Body text.
            :::
        "});
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
    fn render_directive_with_code_and_math() {
        let page = render(indoc! {"
            ::: callout
            Inline $x^2$ math.

            ```rust
            fn main() {}
            ```
            :::
        "});
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

    #[test]
    fn render_directive_uses_template() {
        let dir = tempfile::tempdir().unwrap();
        let directives = dir.path().join("directives");
        fs::create_dir_all(&directives).unwrap();
        fs::write(
            directives.join("my-widget.html"),
            "<widget>{{ name }}: {{ body_html | safe }}</widget>",
        )
        .unwrap();

        let engine = TemplateEngine::new(Some(dir.path()), None).unwrap();
        let page = render_with(
            indoc! {"
                ::: my-widget
                Inner **content**.
                :::
            "},
            &engine,
        );
        assert!(
            page.content_html.contains("<widget>my-widget:"),
            "should use directive template, html:\n{}",
            page.content_html
        );
        assert!(
            page.content_html.contains("<strong>content</strong>"),
            "body should be markdown-rendered, html:\n{}",
            page.content_html
        );
    }

    #[test]
    fn render_directive_template_accesses_parsed_args() {
        let dir = tempfile::tempdir().unwrap();
        let directives = dir.path().join("directives");
        fs::create_dir_all(&directives).unwrap();
        fs::write(
            directives.join("widget.html"),
            "my-pos={{ positional_args[0] }} my-key={{ named_args.key }}",
        )
        .unwrap();

        let engine = TemplateEngine::new(Some(dir.path()), None).unwrap();
        let page = render_with(
            indoc! {r#"
                ::: widget {"my-title" key="value"}
                Body
                :::
            "#},
            &engine,
        );
        assert!(
            page.content_html.contains("my-pos=my-title"),
            "template should access positional_args, html:\n{}",
            page.content_html
        );
        assert!(
            page.content_html.contains("my-key=value"),
            "template should access named_args, html:\n{}",
            page.content_html
        );
    }

    #[test]
    fn render_directive_template_accesses_source_dir() {
        let dir = tempfile::tempdir().unwrap();
        let directives = dir.path().join("directives");
        fs::create_dir_all(&directives).unwrap();
        fs::write(
            directives.join("reader.html"),
            "{% set data = read_file(positional_args[0]) %}DATA:{{ data }}",
        )
        .unwrap();

        let source = tempfile::tempdir().unwrap();
        fs::write(source.path().join("data.csv"), "A,B\n1,2").unwrap();

        let engine = TemplateEngine::new(Some(dir.path()), None).unwrap();
        let page = render_page(
            indoc! {r#"
                ::: reader {"data.csv"}
                :::
            "#},
            &SYNTAX_SET,
            &engine,
            &RenderOptions::default(),
            Some(source.path()),
        )
        .unwrap();
        assert!(
            page.content_html.contains("DATA:A,B\n1,2"),
            "template should read file via source_dir, html:\n{}",
            page.content_html
        );
    }

    #[test]
    fn render_directive_fallback_to_div() {
        let page = render(indoc! {"
            ::: custom
            Some body.
            :::
        "});
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
    fn render_directive_anonymous_div() {
        let page = render(indoc! {"
            ::: {.compact-table}
            | A | B |
            |---|---|
            | 1 | 2 |
            :::
        "});
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
}
