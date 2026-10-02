use std::borrow::Cow;
use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;
use pulldown_cmark::{Event, Tag, TagEnd};
use syntect::parsing::SyntaxSet;

use super::RenderOptions;
use super::assets::{AssetsHandle, PageAssets};
use super::emoji::replace_emojis;
use super::heading::HeadingNumbers;
use super::icon::replace_icons;
use super::image_attrs::{ImageAttrs, extract_image_attrs};
use super::lqip::ImageResolver;
use super::markdown::{MarkdownDocument, MarkdownOutput, MarkdownSettings, render_markdown};
use super::page_ids::PageIds;
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
    let mut placeholder_prefix = "<!--kiln-directive-".to_owned();
    while raw_content.contains(&placeholder_prefix) {
        placeholder_prefix.push('-');
    }
    let renderer = PageRenderer {
        syntax_set,
        engine,
        config,
        options,
        source_dir,
        image_resolver,
        assets: AssetsHandle::default(),
        placeholder_prefix: &placeholder_prefix,
    };
    let mut document = renderer.prepare_document(raw_content, &mut 0);
    let mut ids = PageIds::default();
    document.reserve_authored_ids(&mut ids, &placeholder_prefix);
    let mut numbers = options.heading_numbering.then(HeadingNumbers::default);
    document.allocate_headings(&mut ids, &mut numbers, &placeholder_prefix);
    let md_output = renderer.render_document(document, &mut ids)?;
    let toc_html = render_toc_html(&md_output.headings);

    Ok(RenderedPage {
        content_html: md_output.html,
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
    placeholder_prefix: &'a str,
}

struct PreparedDocument {
    scope: usize,
    markdown: MarkdownDocument,
    image_attrs: HashMap<usize, ImageAttrs>,
    directives: Vec<(DirectiveBlock, PreparedDocument)>,
}

impl PreparedDocument {
    fn reserve_authored_ids(&self, ids: &mut PageIds, prefix: &str) {
        let mut raw_html = String::new();
        let mut image_depth = 0;
        for (event, range) in self.markdown.footnotes.events() {
            match event {
                Event::Start(Tag::Image { .. }) => {
                    if image_depth == 0
                        && let Some(id) = self
                            .image_attrs
                            .get(&range.start)
                            .and_then(|attrs| attrs.id.as_deref())
                    {
                        ids.reserve(id);
                    }
                    image_depth += 1;
                }
                Event::End(TagEnd::Image) => image_depth -= 1,
                Event::Html(html) | Event::InlineHtml(html) if image_depth == 0 => {
                    if let Some(index) = placeholder_index(html, prefix) {
                        let (block, document) = &self.directives[index];
                        if let Some(id) = &block.id {
                            ids.reserve(id);
                        }
                        document.reserve_authored_ids(ids, prefix);
                    } else {
                        raw_html.push_str(html);
                    }
                }
                _ => {}
            }
        }
        ids.reserve_html(&raw_html);
    }

    fn allocate_headings(
        &mut self,
        ids: &mut PageIds,
        numbers: &mut Option<HeadingNumbers>,
        prefix: &str,
    ) {
        let mut headings = self.markdown.headings.iter_mut();
        for (event, _) in self.markdown.footnotes.events() {
            match event {
                Event::End(TagEnd::Heading(_)) => {
                    if let Some(heading) = headings.next() {
                        heading.id = ids.allocate(&heading.id);
                        heading.number =
                            numbers.as_mut().map(|numbers| numbers.next(heading.level));
                    }
                }
                Event::Html(html) => {
                    if let Some(index) = placeholder_index(html, prefix) {
                        self.directives[index]
                            .1
                            .allocate_headings(ids, numbers, prefix);
                    }
                }
                _ => {}
            }
        }
    }
}

impl PageRenderer<'_> {
    fn prepare_document(&self, content: &str, next_scope: &mut usize) -> PreparedDocument {
        let scope = *next_scope;
        *next_scope += 1;
        let all_blocks = parse_directives(content);
        let top_level = top_level_blocks(&all_blocks);
        let mut processed = content.to_owned();
        let directives = top_level
            .iter()
            .map(|block| {
                (
                    (*block).clone(),
                    self.prepare_document(&block.body, next_scope),
                )
            })
            .collect();
        for (index, block) in top_level.into_iter().enumerate().rev() {
            // A standalone comment keeps rendered directive HTML out of the surrounding parser.
            let placeholder = format!(
                "\n{}",
                directive_placeholder(self.placeholder_prefix, index)
            );
            processed.replace_range(block.range.clone(), &placeholder);
        }
        let mut content = Cow::Borrowed(processed.as_str());
        if self.options.emojis {
            content = Cow::Owned(replace_emojis(&content));
        }
        if self.options.fontawesome {
            content = Cow::Owned(replace_icons(&content));
        }
        let (cleaned, image_attrs) = extract_image_attrs(&content);
        PreparedDocument {
            scope,
            markdown: MarkdownDocument::parse(&cleaned),
            image_attrs,
            directives,
        }
    }

    fn render_document(
        &self,
        mut document: PreparedDocument,
        ids: &mut PageIds,
    ) -> Result<MarkdownOutput> {
        document
            .markdown
            .footnotes
            .allocate_ids(ids, document.scope);
        let mut fragments = Vec::with_capacity(document.directives.len());
        for (block, inner) in document.directives {
            let body = self.render_document(inner, ids)?;
            let mut html = render_directive_block(
                &block,
                &body.html,
                self.engine,
                self.config,
                self.source_dir,
                &self.assets,
            )?;
            if !html.ends_with('\n') {
                html.push('\n');
            }
            fragments.push(html);
        }
        let mut output = render_markdown(
            document.markdown,
            self.syntax_set,
            &document.image_attrs,
            self.image_resolver,
            self.source_dir,
            MarkdownSettings {
                code_max_lines: if document.scope == 0 {
                    self.options.code_max_lines
                } else {
                    None
                },
                table_nowrap_width: self.options.table_nowrap_width,
            },
            &mut self.assets.lock().features,
        );
        output.html = restore_directives(output.html, self.placeholder_prefix, &fragments);
        Ok(output)
    }
}

fn placeholder_index(html: &str, prefix: &str) -> Option<usize> {
    html.strip_prefix(prefix)?
        .strip_suffix("-->\n")?
        .parse()
        .ok()
}

/// Substitutes the fragments returned by [`PageRenderer::prepare_document`] back into the
/// rendered HTML.
fn restore_directives(mut html: String, prefix: &str, fragments: &[String]) -> String {
    for (index, fragment) in fragments.iter().enumerate() {
        html = html.replacen(&directive_placeholder(prefix, index), fragment, 1);
    }
    html
}

/// Returns the placeholder line for the `index`-th directive fragment. Fragments are
/// newline-terminated, so each one replaces a whole line and keeps its own lines in the output.
fn directive_placeholder(prefix: &str, index: usize) -> String {
    format!("{prefix}{index}-->\n")
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
    fn render_page_heading_numbering_preserves_titles_and_ids() {
        let input = indoc! {"
            ## First *title* {#custom}
            #### Deep
            ### Sibling
            ## Last
        "};
        let render_numbered = || {
            render_page(
                input,
                &SYNTAX_SET,
                &test_engine(),
                &test_config(),
                &RenderOptions {
                    heading_numbering: true,
                    ..RenderOptions::default()
                },
                None,
                &EMPTY_RESOLVER,
            )
            .unwrap()
        };
        let page = render_numbered();
        assert!(page.content_html.contains(
            r#"<h2 id="custom"><span class="heading-number">1</span> First <em>title</em></h2>"#
        ));
        for (id, number, title) in [
            ("custom", "1", "First title"),
            ("deep", "1.1", "Deep"),
            ("sibling", "1.2", "Sibling"),
            ("last", "2", "Last"),
        ] {
            assert!(page.toc_html.contains(&format!(
                r##"href="#{id}"><span class="heading-number">{number}</span> {title}</a>"##
            )));
            assert!(page.content_html.contains(&format!(
                r#"id="{id}"><span class="heading-number">{number}</span> "#
            )));
        }
        assert_eq!(page, render_numbered());
        let default = render(input);
        assert!(!default.content_html.contains("heading-number"));
        assert!(!default.toc_html.contains("heading-number"));
        assert!(
            default
                .content_html
                .contains(r#"id="custom">First <em>title</em>"#)
        );
    }

    #[test]
    fn render_page_heading_numbering_follows_document_order_across_scopes() {
        let page = render_page(
            indoc! {"
                #### First

                [^a]:
                    ### Note

                ::: callout {type=note}
                ##### Inner

                ::: callout {type=tip}
                #### Sibling
                :::
                :::

                ## Last

                Text[^a].
            "},
            &SYNTAX_SET,
            &test_engine(),
            &test_config(),
            &RenderOptions {
                heading_numbering: true,
                ..RenderOptions::default()
            },
            None,
            &EMPTY_RESOLVER,
        )
        .unwrap();
        let mut previous = 0;
        for (id, number) in [
            ("first", "1"),
            ("inner", "1.1"),
            ("sibling", "2"),
            ("last", "3"),
            ("note", "3.1"),
        ] {
            let position = page
                .content_html
                .find(&format!(
                    r#"id="{id}"><span class="heading-number">{number}</span> "#
                ))
                .unwrap();
            assert!(position >= previous, "{}", page.content_html);
            previous = position;
        }
        assert!(!page.toc_html.contains("#inner"));
        assert!(!page.toc_html.contains("#sibling"));
        assert!(
            page.toc_html
                .contains(r##"href="#note"><span class="heading-number">3.1</span> Note</a>"##)
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

    #[test]
    fn render_page_table_nowrap_reaches_directive_bodies() {
        let options = RenderOptions {
            table_nowrap_width: Some(8),
            ..RenderOptions::default()
        };
        let input = indoc! {"
            | Top |
            | --- |
            | a |

            ::: {.compact-table}
            | Nested |
            | ------ |
            | b |
            :::
        "};
        let page = render_page(
            input,
            &SYNTAX_SET,
            &test_engine(),
            &test_config(),
            &options,
            None,
            &EMPTY_RESOLVER,
        )
        .unwrap();
        assert!(
            page.content_html.contains(r#"<th class="nowrap">Top</th>"#),
            "top-level table should get nowrap cells, html:\n{}",
            page.content_html
        );
        assert!(
            page.content_html
                .contains(r#"<th class="nowrap">Nested</th>"#),
            "directive-body table should get nowrap cells, html:\n{}",
            page.content_html
        );
    }

    #[test]
    fn render_page_heading_ids_follow_document_order_across_scopes() {
        let page = render(indoc! {"
            ## Shared

            ::: callout {type=note}
            ## Shared

            ::: callout {type=tip}
            ## Shared
            :::
            :::

            ## Shared

            ::: callout {type=note}
            ## Shared {#shared}
            :::

            ## Shared
        "});
        let mut previous = 0;
        for id in [
            "shared", "shared-1", "shared-2", "shared-3", "shared-4", "shared-5",
        ] {
            let position = page.content_html.find(&format!(r#"id="{id}""#)).unwrap();
            assert!(position >= previous, "{id}: {}", page.content_html);
            previous = position;
        }
        for id in ["shared", "shared-3", "shared-5"] {
            assert!(page.toc_html.contains(&format!(r##"href="#{id}""##)));
        }
        assert!(!page.toc_html.contains(r##"href="#shared-1""##));
    }

    #[test]
    fn render_page_footnote_ids_are_scoped_and_avoid_authored_ids() {
        let page = render(indoc! {"
            ## Heading {#fn-a}

            Text[^a].

            [^a]: Outer.

            ::: callout {#fnref-a-1 type=note}
            Inner[^a].

            [^a]: Inner note.

            ::: callout {type=tip}
            Nested[^a].

            [^a]: Nested note.
            :::
            :::
        "});
        for id in [
            "fn-a-1",
            "fn-1-a",
            "fn-2-a",
            "fnref-a-1-1",
            "fnref-1-a-1",
            "fnref-2-a-1",
        ] {
            assert_eq!(
                page.content_html.matches(&format!(r#"id="{id}""#)).count(),
                1,
                "{id}: {}",
                page.content_html
            );
            assert!(page.content_html.contains(&format!(r##"href="#{id}""##)));
        }
        assert!(page.toc_html.contains(r##"href="#fn-a""##));
    }

    #[test]
    fn render_page_reserves_visible_authored_ids() {
        let page = render(indoc! {r#"
            ![<script>](script.png)

            <div id=shared></div>

            ## Shared

            Inline <span ID='fn&#45;a'></span> reference[^a].

            [^a]: Note.

            [^unused]:
                ![Hidden](hidden.png){#visible}

                <span id="unused"></span>

            ## Visible

            ![<span id="visible">Alt</span>](image.png){#image}

            ## Image

            ## Unused

            ## Code

            `<span id="code"></span>`

            ```html
            <span id="code"></span>
            ```

            ::: callout
            <span id="fn-1-a"></span>

            Scoped[^a].

            [^a]: Inner.
            :::
        "#});
        for id in ["shared-1", "visible", "image-1", "unused", "code"] {
            assert!(
                page.content_html.contains(&format!(r#"<h2 id="{id}">"#)),
                "{id}: {}",
                page.content_html
            );
            assert!(page.toc_html.contains(&format!(r##"href="#{id}""##)));
        }
        for id in ["fn-a-1", "fn-1-a-1"] {
            assert!(page.content_html.contains(&format!(r#"<li id="{id}">"#)));
            assert!(page.content_html.contains(&format!(r##"href="#{id}""##)));
        }
    }

    #[test]
    fn render_page_preserves_authored_placeholder_comments() {
        let page = render(indoc! {"
            <!--kiln-directive-99-->

            <!--kiln-directive-0-->

            ::: callout {type=note}
            ## Inside
            :::

            ## Inside
        "});
        assert!(page.content_html.contains("<!--kiln-directive-99-->"));
        assert!(page.content_html.contains("<!--kiln-directive-0-->"));
        assert_eq!(page.content_html.matches("<details").count(), 1);
        assert!(page.toc_html.contains(r##"href="#inside-1""##));
    }

    // ── render_page: directives ──

    #[test]
    fn render_page_directives_sequential() {
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
    fn render_page_directives_with_image_attrs() {
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
    fn render_page_directives_nested() {
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
    fn render_page_directives_body_html_survives_outer_render() {
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
}
