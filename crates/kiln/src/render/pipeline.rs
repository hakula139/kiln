use std::borrow::Cow;
use std::path::Path;

use anyhow::Result;
use syntect::parsing::SyntaxSet;

use super::RenderOptions;
use super::assets::{AssetsHandle, PageAssets};
use super::emoji::replace_emojis;
use super::icon::replace_icons;
use super::image_attrs::extract_image_attrs;
use super::lqip::ImageResolver;
use super::markdown::{MarkdownOutput, render_markdown};
use super::toc::render_toc_html;
use crate::config::Config;
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
    /// Page-level asset declarations rolled up from the markdown body and nested directive bodies.
    /// Templates iterate this to load conditional runtime dependencies (`KaTeX`, `mermaid.js`).
    pub assets: PageAssets,
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
    config: &Config,
    options: &RenderOptions,
    source_dir: Option<&Path>,
    image_resolver: &ImageResolver,
) -> Result<RenderedPage> {
    let renderer = PageRenderer {
        syntax_set,
        engine,
        config,
        options,
        source_dir,
        image_resolver,
        assets: AssetsHandle::default(),
    };
    let (processed, fragments) = renderer.render_directives(raw_content)?;
    let md_output = renderer.render_markdown(&processed, options.code_max_lines);
    let toc_html = render_toc_html(&md_output.headings);

    Ok(RenderedPage {
        content_html: restore_directives(md_output.html, &fragments),
        toc_html,
        assets: renderer.assets.snapshot(),
    })
}

/// Inputs shared by the page body and every directive body rendered for one page.
struct PageRenderer<'a> {
    syntax_set: &'a SyntaxSet,
    engine: &'a TemplateEngine,
    config: &'a Config,
    options: &'a RenderOptions,
    source_dir: Option<&'a Path>,
    image_resolver: &'a ImageResolver,
    assets: AssetsHandle,
}

impl PageRenderer<'_> {
    /// Recursively renders directive blocks, replacing each with a placeholder line and returning
    /// the rendered HTML fragments for [`restore_directives`].
    ///
    /// The placeholder keeps rendered HTML out of the surrounding markdown, which would otherwise
    /// reparse it and could break a code block containing a blank line followed by indented code.
    /// Replacement is right-to-left so byte offsets stay valid. Each directive body is rendered as
    /// an isolated markdown document, so its headings stay out of the page-level `ToC` and its
    /// footnotes stay within the directive.
    fn render_directives(&self, content: &str) -> Result<(String, Vec<String>)> {
        let all_blocks = parse_directives(content);
        let top_level = top_level_blocks(&all_blocks);
        let mut result = content.to_owned();
        let mut fragments = Vec::with_capacity(top_level.len());

        for block in top_level.into_iter().rev() {
            let (inner, inner_fragments) = self.render_directives(&block.body)?;
            let md_output = self.render_markdown(&inner, None);
            let body_html = restore_directives(md_output.html, &inner_fragments);
            let mut html = render_directive_block(
                block,
                &body_html,
                self.engine,
                self.config,
                self.source_dir,
                &self.assets,
            )?;
            if !html.ends_with('\n') {
                html.push('\n');
            }

            // The directive parser only matches column-0 fences, so the placeholder always starts
            // a line and parses as a standalone HTML comment block.
            let padded = format!("\n{}", directive_placeholder(fragments.len()));
            fragments.push(html);
            result.replace_range(block.range.clone(), &padded);
        }

        Ok((result, fragments))
    }

    /// Applies the enabled shortcode replacements and image attribute blocks, then renders the
    /// markdown.
    fn render_markdown(&self, content: &str, code_max_lines: Option<usize>) -> MarkdownOutput {
        let mut content = Cow::Borrowed(content);
        if self.options.emojis {
            content = Cow::Owned(replace_emojis(&content));
        }
        if self.options.fontawesome {
            content = Cow::Owned(replace_icons(&content));
        }
        let (cleaned, image_attrs) = extract_image_attrs(&content);

        render_markdown(
            &cleaned,
            self.syntax_set,
            &image_attrs,
            self.image_resolver,
            self.source_dir,
            code_max_lines,
            &mut self.assets.lock().features,
        )
    }
}

/// Substitutes the fragments returned by [`PageRenderer::render_directives`] back into the
/// rendered HTML.
fn restore_directives(mut html: String, fragments: &[String]) -> String {
    for (index, fragment) in fragments.iter().enumerate() {
        html = html.replacen(&directive_placeholder(index), fragment, 1);
    }
    html
}

/// Returns the placeholder line for the `index`-th directive fragment. Fragments are
/// newline-terminated, so each one replaces a whole line and keeps its own lines in the output.
fn directive_placeholder(index: usize) -> String {
    format!("<!--kiln-directive-{index}-->\n")
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
/// For `Unknown` directives, checks the template engine for a `directives/<name>.html` template.
/// Falls back to `render_div` if no template exists.
fn render_directive_block(
    block: &DirectiveBlock,
    body_html: &str,
    engine: &TemplateEngine,
    config: &Config,
    source_dir: Option<&Path>,
    assets: &AssetsHandle,
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
            match engine.render_directive(name, ctx, assets, config) {
                Some(result) => result,
                None => Ok(render_div(name, id, classes, body_html)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::LazyLock;

    use indoc::indoc;

    use super::*;
    use crate::render::assets::Feature;
    use crate::render::lqip::ImageConfig;
    use crate::test_utils::{test_config, test_engine, test_i18n};

    static SYNTAX_SET: LazyLock<SyntaxSet> = LazyLock::new(two_face::syntax::extra_newlines);

    // Empty static-root resolver: `resolve` returns `None` for any path these tests reference.
    static EMPTY_RESOLVER: LazyLock<ImageResolver> =
        LazyLock::new(|| ImageResolver::new(Path::new(""), ImageConfig::default()));

    fn render(input: &str) -> RenderedPage {
        render_with(input, &test_engine())
    }

    fn render_with(input: &str, engine: &TemplateEngine) -> RenderedPage {
        render_page(
            input,
            &SYNTAX_SET,
            engine,
            &test_config(),
            &RenderOptions::default(),
            None,
            &EMPTY_RESOLVER,
        )
        .unwrap()
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
        let page = render_page(
            input,
            &SYNTAX_SET,
            &engine,
            &test_config(),
            &options,
            None,
            &EMPTY_RESOLVER,
        )
        .unwrap();
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

    #[test]
    fn render_page_replaces_shortcodes_in_directive_bodies() {
        let options = RenderOptions {
            emojis: true,
            fontawesome: true,
            ..RenderOptions::default()
        };
        let page = render_page(
            indoc! {"
                ::: callout
                Hello :smile: and :(fas fa-link):

                `:smile:`
                :::
            "},
            &SYNTAX_SET,
            &test_engine(),
            &test_config(),
            &options,
            None,
            &EMPTY_RESOLVER,
        )
        .unwrap();
        assert!(
            page.content_html.contains("Hello \u{1f604} and"),
            "emoji should be replaced, html:\n{}",
            page.content_html
        );
        assert!(
            page.content_html.contains(r#"class="fas fa-link""#),
            "icon should be replaced, html:\n{}",
            page.content_html
        );
        assert!(
            page.content_html.contains("<code>:smile:</code>"),
            "inline code should keep the shortcode, html:\n{}",
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

    #[test]
    fn render_directives_body_html_survives_outer_render() {
        let page = render(indoc! {"
            ::: callout
            ```text
            first

                indented
            ```
            :::
        "});
        assert!(
            !page.content_html.contains("<pre><code>"),
            "rendered body must not be reparsed as markdown, html:\n{}",
            page.content_html
        );
        assert!(
            page.content_html.contains("    indented"),
            "code line should keep its indentation, html:\n{}",
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
        assert!(
            page.assets.features.contains(&Feature::Math),
            "math inside a directive body should bubble up to page assets, features: {:?}",
            page.assets.features,
        );
    }

    // ── render_page: asset auto-detection ──

    #[test]
    fn render_page_detects_math_feature_at_body_level() {
        let page = render(indoc! {r"
            Plain text with $a + b$ inline math.
        "});
        assert!(
            page.assets.features.contains(&Feature::Math),
            "math expression should set Feature::Math, features: {:?}",
            page.assets.features,
        );
        assert!(
            !page.assets.features.contains(&Feature::Mermaid),
            "no mermaid fence should leave Feature::Mermaid unset, features: {:?}",
            page.assets.features,
        );
    }

    #[test]
    fn render_page_detects_mermaid_feature_from_fence() {
        let page = render(indoc! {"
            ```mermaid
            graph TD
              A --> B
            ```
        "});
        assert!(
            page.assets.features.contains(&Feature::Mermaid),
            "```mermaid fence should set Feature::Mermaid, features: {:?}",
            page.assets.features,
        );
    }

    #[test]
    fn render_page_detects_mermaid_feature_case_insensitively() {
        let page = render(indoc! {"
            ```Mermaid
            graph TD
              A --> B
            ```
        "});
        assert!(
            page.assets.features.contains(&Feature::Mermaid),
            "uppercase fence should still set Feature::Mermaid, features: {:?}",
            page.assets.features,
        );
    }

    #[test]
    fn render_page_detects_mermaid_feature_with_info_string_metadata() {
        let page = render(indoc! {"
            ```mermaid no_run
            graph TD
              A --> B
            ```
        "});
        assert!(
            page.assets.features.contains(&Feature::Mermaid),
            "fence with trailing metadata should still set Feature::Mermaid, features: {:?}",
            page.assets.features,
        );
    }

    #[test]
    fn render_page_no_features_for_plain_content() {
        let page = render(indoc! {"
            # Heading

            Just text. No math, no diagrams.
        "});
        assert!(
            page.assets.features.is_empty(),
            "plain content should detect no features, got: {:?}",
            page.assets.features,
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

        let engine = TemplateEngine::new(Some(dir.path()), None, &test_i18n()).unwrap();
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
    fn render_directive_template_output_keeps_own_line() {
        let dir = tempfile::tempdir().unwrap();
        let directives = dir.path().join("directives");
        fs::create_dir_all(&directives).unwrap();
        fs::write(directives.join("widget.html"), "<widget></widget>\n").unwrap();

        let engine = TemplateEngine::new(Some(dir.path()), None, &test_i18n()).unwrap();
        let page = render_with(
            indoc! {"
                ::: widget
                Body
                :::
                After.
            "},
            &engine,
        );
        assert_eq!(page.content_html, "<widget></widget>\n<p>After.</p>\n");
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

        let engine = TemplateEngine::new(Some(dir.path()), None, &test_i18n()).unwrap();
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

        let engine = TemplateEngine::new(Some(dir.path()), None, &test_i18n()).unwrap();
        let page = render_page(
            indoc! {r#"
                ::: reader {"data.csv"}
                :::
            "#},
            &SYNTAX_SET,
            &engine,
            &test_config(),
            &RenderOptions::default(),
            Some(source.path()),
            &EMPTY_RESOLVER,
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
