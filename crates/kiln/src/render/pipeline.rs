use std::path::Path;

use anyhow::Result;
use pulldown_cmark::{CodeBlockKind, Event, Tag};

use super::assets::{AssetsHandle, PageAssets};
use super::code_block::parse_fence_info;
use super::document::{MarkdownDocument, image_content, render_document};
use super::heading::{HeadingNumbers, NUMBERING_START};
use super::page_ids::PageIds;
use super::toc::render_toc_html;
use super::{PageResources, RenderOptions};
use crate::config::Config;
use crate::directive::callout::Callout;
use crate::directive::div::render_div;
use crate::directive::parser::{DirectiveNode, parse_directives};
use crate::directive::{DirectiveBlock, DirectiveContext};
use crate::template::TemplateEngine;

/// The fully rendered output of a single page.
#[derive(Debug)]
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
/// Returns an error if a directive template fails or heading numbers are invalid, duplicate,
/// or overflowing.
pub fn render_page(
    raw_content: &str,
    engine: &TemplateEngine,
    config: &Config,
    options: &RenderOptions,
    resources: &PageResources<'_>,
    heading_numbering: bool,
) -> Result<RenderedPage> {
    let mut placeholder_prefix = "<!--kiln-directive-".to_owned();
    while raw_content.contains(&placeholder_prefix) {
        placeholder_prefix.push('-');
    }
    let renderer = PageRenderer {
        engine,
        config,
        options,
        resources,
        assets: AssetsHandle::default(),
        placeholder_prefix: &placeholder_prefix,
    };
    let mut document =
        renderer.prepare_fragment(raw_content, parse_directives(raw_content), 0, 0, &mut 1);
    let mut ids = PageIds::default();
    document.reserve_authored_ids(&mut ids, &placeholder_prefix);
    let mut numbers = heading_numbering.then(HeadingNumbers::default);
    document.allocate_headings(&mut ids, &mut numbers, &placeholder_prefix)?;
    let toc_html = render_toc_html(&document.markdown.headings);
    let content_html = renderer.render_fragment(document, &mut ids)?;

    Ok(RenderedPage {
        content_html,
        toc_html,
        assets: renderer.assets.snapshot(),
    })
}

/// Inputs shared by the page body and every directive body rendered for one page.
struct PageRenderer<'a> {
    engine: &'a TemplateEngine,
    config: &'a Config,
    options: &'a RenderOptions,
    resources: &'a PageResources<'a>,
    assets: AssetsHandle,
    placeholder_prefix: &'a str,
}

struct PreparedDocument {
    scope: usize,
    markdown: MarkdownDocument,
    directives: Vec<(DirectiveBlock, PreparedDocument)>,
}

impl PreparedDocument {
    fn reserve_authored_ids(&self, ids: &mut PageIds, prefix: &str) {
        let mut raw_html = String::new();
        let mut events = self.markdown.events();
        while let Some((event, range)) = events.next() {
            match event {
                Event::Start(Tag::Heading { id: Some(id), .. }) => ids.reserve(id),
                Event::Start(Tag::Image { .. }) => {
                    if let Some(id) = self
                        .markdown
                        .image_attrs
                        .get(&range.start)
                        .and_then(|attrs| attrs.id.as_deref())
                    {
                        ids.reserve(id);
                    }
                    image_content(&mut events).for_each(drop);
                }
                Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info))) => {
                    let spec = parse_fence_info(info, None);
                    if !spec
                        .lang
                        .as_deref()
                        .is_some_and(|lang| lang.eq_ignore_ascii_case("mermaid"))
                        && let Some(id) = spec.id
                    {
                        ids.reserve(&id);
                    }
                }
                Event::Html(html) | Event::InlineHtml(html) => {
                    if let Some(index) = placeholder_index(html, prefix) {
                        let (block, document) = &self.directives[index];
                        if let Some(id) = &block.args.identity.id {
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

    /// Allocates heading IDs and numbers in document order, descending into directives in place.
    fn allocate_headings(
        &mut self,
        ids: &mut PageIds,
        numbers: &mut Option<HeadingNumbers>,
        prefix: &str,
    ) -> Result<()> {
        let MarkdownDocument {
            body,
            notes,
            headings,
            ..
        } = &mut self.markdown;
        let mut headings = headings.iter_mut();
        for (event, _) in body.iter().chain(notes.events()) {
            match event {
                Event::Start(Tag::Heading { id, attrs, .. }) => {
                    let heading = headings
                        .next()
                        .expect("collect_headings yields one entry per heading");
                    heading.id = if id.is_some() {
                        ids.claim(&heading.id)
                    } else {
                        ids.allocate(&heading.id)
                    };
                    heading.number = numbers
                        .as_mut()
                        .map(|numbers| {
                            let start = attrs
                                .iter()
                                .find(|(key, _)| key.as_ref() == NUMBERING_START)
                                .map(|(_, value)| value.as_deref().unwrap_or(""));
                            numbers.next(heading.level, start, &heading.id)
                        })
                        .transpose()?;
                }
                Event::Html(html) => {
                    if let Some(index) = placeholder_index(html, prefix) {
                        self.directives[index]
                            .1
                            .allocate_headings(ids, numbers, prefix)?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
}

impl PageRenderer<'_> {
    fn prepare_fragment(
        &self,
        content: &str,
        nodes: Vec<DirectiveNode>,
        source_offset: usize,
        scope: usize,
        next_scope: &mut usize,
    ) -> PreparedDocument {
        let mut processed = content.to_owned();
        for (index, node) in nodes.iter().enumerate().rev() {
            // A standalone comment keeps rendered directive HTML out of the surrounding parser.
            let placeholder = format!(
                "\n{}",
                directive_placeholder(self.placeholder_prefix, index)
            );
            // The raw directive body omits its final line ending.
            let end = (node.block.range.end - source_offset).min(content.len());
            let range = node.block.range.start - source_offset..end;
            processed.replace_range(range, &placeholder);
        }
        let directives = nodes
            .into_iter()
            .map(|node| {
                let child_scope = *next_scope;
                *next_scope += 1;
                let body = &node.block.body;
                let inner = self.prepare_fragment(
                    &content[body.start - source_offset..body.end - source_offset],
                    node.children,
                    body.start,
                    child_scope,
                    next_scope,
                );
                (node.block, inner)
            })
            .collect();
        PreparedDocument {
            scope,
            markdown: MarkdownDocument::parse(&processed, self.options),
            directives,
        }
    }

    fn render_fragment(&self, document: PreparedDocument, ids: &mut PageIds) -> Result<String> {
        let PreparedDocument {
            scope,
            mut markdown,
            directives,
        } = document;
        markdown.notes.allocate_ids(&mut markdown.body, ids, scope);
        let mut fragments = Vec::with_capacity(directives.len());
        for (block, inner) in directives {
            let body = self.render_fragment(inner, ids)?;
            let mut html = self.render_directive_block(&block, &body)?;
            if !html.ends_with('\n') {
                html.push('\n');
            }
            fragments.push(html);
        }
        let html = render_document(
            markdown,
            self.resources,
            self.options,
            &mut self.assets.lock().features,
        );
        Ok(restore_directives(
            &html,
            self.placeholder_prefix,
            &fragments,
        ))
    }

    /// Renders a directive through its `directives/<name>.html` template, or as a `<div>` when
    /// the block is anonymous or the template is missing.
    fn render_directive_block(&self, block: &DirectiveBlock, body_html: &str) -> Result<String> {
        let args = &block.args;
        if !block.name.is_empty() {
            let callout = (block.name == "callout")
                .then(|| Callout::from_args(&args.named))
                .transpose()?;
            let ctx = DirectiveContext {
                name: &block.name,
                positional_args: &args.positional,
                named_args: &args.named,
                identity: &args.identity,
                body_html,
                callout,
                source_dir: self.resources.source_dir.map(Path::to_string_lossy),
                page_url: self.resources.page_url,
            };
            if let Some(result) = self
                .engine
                .render_directive(&ctx, &self.assets, self.config)
            {
                return result;
            }
        }
        Ok(render_div(&block.name, &args.identity, body_html))
    }
}

/// Returns the placeholder line for the `index`-th directive fragment. Fragments are
/// newline-terminated, so each one replaces a whole line and keeps its own lines in the output.
fn directive_placeholder(prefix: &str, index: usize) -> String {
    format!("{prefix}{index}-->\n")
}

fn placeholder_index(html: &str, prefix: &str) -> Option<usize> {
    split_placeholder(html.strip_prefix(prefix)?).map(|(index, _)| index)
}

/// Splits the `<index>-->\n` that follows a placeholder prefix from the remaining text.
fn split_placeholder(after_prefix: &str) -> Option<(usize, &str)> {
    let (index, rest) = after_prefix.split_once("-->\n")?;
    Some((index.parse().ok()?, rest))
}

/// Substitutes the fragments returned by [`PageRenderer::render_fragment`] for their placeholder
/// lines in one pass.
fn restore_directives(html: &str, prefix: &str, fragments: &[String]) -> String {
    let mut output =
        String::with_capacity(html.len() + fragments.iter().map(String::len).sum::<usize>());
    let mut rest = html;
    while let Some(start) = rest.find(prefix) {
        output.push_str(&rest[..start]);
        let after_prefix = &rest[start + prefix.len()..];
        if let Some((index, after)) = split_placeholder(after_prefix)
            && let Some(fragment) = fragments.get(index)
        {
            output.push_str(fragment);
            rest = after;
        } else {
            output.push_str(prefix);
            rest = after_prefix;
        }
    }
    output.push_str(rest);
    output
}

#[cfg(test)]
mod tests {
    use std::fs;

    use indoc::{formatdoc, indoc};
    use tempfile::TempDir;

    use super::*;
    use crate::render::assets::Feature;
    use crate::test_utils::{render_test_page, test_engine, test_i18n, test_resources};

    fn render(input: &str) -> RenderedPage {
        render_test_page(input, &RenderOptions::default())
    }

    fn render_with(input: &str, engine: &TemplateEngine) -> RenderedPage {
        render_page(
            input,
            engine,
            &Config::default(),
            &RenderOptions::default(),
            &test_resources(),
            false,
        )
        .unwrap()
    }

    fn render_numbered(input: &str) -> Result<RenderedPage> {
        render_page(
            input,
            &test_engine(),
            &Config::default(),
            &RenderOptions::default(),
            &test_resources(),
            true,
        )
    }

    /// Creates an engine whose only template is `directives/<name>.html`.
    fn directive_engine(name: &str, template: &str) -> (TempDir, TemplateEngine) {
        let dir = tempfile::tempdir().unwrap();
        let directives = dir.path().join("directives");
        fs::create_dir_all(&directives).unwrap();
        fs::write(directives.join(format!("{name}.html")), template).unwrap();
        let engine = TemplateEngine::new(Some(dir.path()), None, &test_i18n()).unwrap();
        (dir, engine)
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
        assert!(page.toc_html.contains(r##"href="#hello">Hello</a>"##));
        assert!(page.assets.features.is_empty());
    }

    #[test]
    fn render_page_code_max_lines_reaches_code_blocks() {
        let page = render_test_page(
            indoc! {"
                ```
                fenced code
                ```

                    indented code

                ``` text {expand}
                expanded code
                ```

                ::: callout
                ```
                directive code
                ```
                :::
            "},
            &RenderOptions {
                code_max_lines: Some(3),
                ..RenderOptions::default()
            },
        );
        let fragment = scraper::Html::parse_fragment(&page.content_html);
        let selector = scraper::Selector::parse(".code-body").unwrap();
        let max_lines: Vec<_> = fragment
            .select(&selector)
            .map(|body| body.value().attr("data-max-lines"))
            .collect();

        assert_eq!(max_lines, vec![Some("3"), Some("3"), None, Some("3")]);
    }

    #[test]
    fn render_page_enriches_images_in_inline_containers() {
        for input in [
            "- ![Alt](photo.png){width=80 #photo}",
            "## ![Alt](photo.png){width=80 #photo}",
            indoc! {"
                | Image |
                | --- |
                | ![Alt](photo.png){width=80 #photo} |
            "},
        ] {
            let html = render(input).content_html;
            assert!(html.contains(r#"id="photo""#), "{html}");
            assert!(html.contains(r#"width="80""#), "{html}");
            assert!(html.contains(r#"loading="lazy""#), "{html}");
            assert!(!html.contains("<figure"), "{html}");
        }
    }

    #[test]
    fn render_page_replaces_shortcodes_in_body_and_directive_bodies() {
        let page = render_test_page(
            indoc! {"
                Hello :smile: and :(fas fa-link):

                ::: callout
                Inside :smile: and :(fas fa-link):

                `:smile:`
                :::
            "},
            &RenderOptions {
                emojis: true,
                fontawesome: true,
                ..RenderOptions::default()
            },
        );
        let icon = r#"<i class="fas fa-link" aria-hidden="true"></i>"#;
        for expected in [
            format!("<p>Hello \u{1f604} and {icon}</p>"),
            format!("<p>Inside \u{1f604} and {icon}</p>"),
            "<code>:smile:</code>".to_owned(),
        ] {
            assert!(
                page.content_html.contains(&expected),
                "{expected}: {}",
                page.content_html
            );
        }
    }

    #[test]
    fn render_page_table_nowrap_reaches_directive_bodies() {
        // The image's alt text and URL contribute nothing to the measured width.
        let page = render_test_page(
            indoc! {"
                | Top |
                | --- |
                | a ![long alt text](a.png) |

                ::: {.compact-table}
                | Nested |
                | ------ |
                | b |
                :::
            "},
            &RenderOptions {
                table_nowrap_width: Some(8),
                ..RenderOptions::default()
            },
        );
        assert!(
            page.content_html.contains(r#"<th class="nowrap">Top</th>"#),
            "top-level table should get nowrap cells, html:\n{}",
            page.content_html
        );
        assert!(
            page.content_html.contains(r#"<td class="nowrap">a <img"#),
            "image markup should not widen its cell, html:\n{}",
            page.content_html
        );
        assert!(
            page.content_html
                .contains(r#"<th class="nowrap">Nested</th>"#),
            "directive-body table should get nowrap cells, html:\n{}",
            page.content_html
        );
    }

    // ── render_page: headings and IDs ──

    #[test]
    fn render_page_preserves_heading_attributes() {
        let html = render("## Title {.wide data-x=yes numbering-start=3}").content_html;
        assert!(html.contains(r#"class="wide""#), "{html}");
        assert!(html.contains(r#"data-x="yes""#), "{html}");
        assert!(!html.contains("numbering-start"), "{html}");
    }

    #[test]
    fn render_page_reserves_fence_ids_across_directive_scopes() {
        let fence = indoc! {"
            ::: wrapper
            ```rust {#sample}
            let value = 1;
            ```
            ```text {#fn-a}
            footnote id
            ```
            :::
        "};
        let prose = indoc! {"
            ## Sample

            Text[^a]

            [^a]: Note
        "};
        for input in [format!("{fence}\n{prose}"), format!("{prose}\n{fence}")] {
            let html = render(&input).content_html;
            assert_eq!(html.matches(r#"id="sample""#).count(), 1, "{html}");
            assert!(html.contains(r#"id="sample-1""#), "{html}");
            assert_eq!(html.matches(r#"id="fn-a""#).count(), 1, "{html}");
            assert!(html.contains(r#"id="fn-a-1""#), "{html}");
        }
    }

    #[test]
    fn render_page_heading_ids_follow_document_order_around_authored_ids() {
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
            "shared-1", "shared-2", "shared-3", "shared-4", "shared", "shared-5",
        ] {
            let position = page.content_html.find(&format!(r#"id="{id}""#)).unwrap();
            assert!(position >= previous, "{id}: {}", page.content_html);
            previous = position;
        }
        for id in ["shared-1", "shared-4", "shared-5"] {
            assert!(page.toc_html.contains(&format!(r##"href="#{id}""##)));
        }
        for id in ["shared", "shared-2"] {
            assert!(!page.toc_html.contains(&format!(r##"href="#{id}""##)));
        }
    }

    #[test]
    fn render_page_duplicate_explicit_heading_ids_keep_first() {
        let page = render(indoc! {"
            ## First {#dup}

            ## Dup

            ::: callout
            ## Second {#dup}
            :::

            ## Third {#dup}
        "});
        let mut previous = 0;
        for heading in [
            r#"<h2 id="dup">First</h2>"#,
            r#"<h2 id="dup-1">Dup</h2>"#,
            r#"<h2 id="dup-2">Second</h2>"#,
            r#"<h2 id="dup-3">Third</h2>"#,
        ] {
            let position = page.content_html.find(heading).unwrap();
            assert!(position >= previous, "{heading}: {}", page.content_html);
            previous = position;
        }
        assert_eq!(
            page.content_html.matches(r#"id="dup""#).count(),
            1,
            "{}",
            page.content_html
        );
        for id in ["dup", "dup-1", "dup-3"] {
            assert!(page.toc_html.contains(&format!(r##"href="#{id}""##)));
        }
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

    // ── render_page: heading numbering ──

    #[test]
    fn render_page_heading_numbering_preserves_titles_and_ids() {
        for (attribute, expected) in [
            ("", ["1", "1.1", "1.2", "2", "2.1"]),
            (" numbering-start=2", ["2", "2.1", "2.2", "3", "3.1"]),
            (" numbering-start=0", ["0", "0.1", "0.2", "1", "1.1"]),
        ] {
            let input = formatdoc! {"
                ## First *title* {{#custom{attribute}}}
                #### Deep
                ### Sibling
                ## Last
                ### Child
            "};
            let page = render_numbered(&input).unwrap();
            let first = expected[0];
            assert!(page.content_html.contains(&format!(
                r#"<h2 id="custom"><span class="heading-number">{first}</span> First <em>title</em></h2>"#
            )));
            for ((id, title), number) in [
                ("custom", "First title"),
                ("deep", "Deep"),
                ("sibling", "Sibling"),
                ("last", "Last"),
                ("child", "Child"),
            ]
            .into_iter()
            .zip(expected)
            {
                assert!(page.toc_html.contains(&format!(
                    r##"href="#{id}"><span class="heading-number">{number}</span> {title}</a>"##
                )));
                assert!(page.content_html.contains(&format!(
                    r#"id="{id}"><span class="heading-number">{number}</span> "#
                )));
            }
            let again = render_numbered(&input).unwrap();
            assert_eq!(
                (page.content_html, page.toc_html),
                (again.content_html, again.toc_html)
            );
        }
    }

    #[test]
    fn render_page_heading_numbering_attributes_reset_local_counters() {
        let page = render_numbered(indoc! {"
            ## First {#custom numbering-start=2}
            ### Overview
            ### Overview {numbering-start=0}
            ## Last
            ### Overview {numbering-start=0}
            ### Sibling
        "})
        .unwrap();
        for (id, number) in [
            ("custom", "2"),
            ("overview", "2.1"),
            ("overview-1", "2.0"),
            ("last", "3"),
            ("overview-2", "3.0"),
            ("sibling", "3.1"),
        ] {
            assert!(page.content_html.contains(&format!(
                r#"id="{id}"><span class="heading-number">{number}</span> "#
            )));
            assert!(page.toc_html.contains(&format!(
                r##"href="#{id}"><span class="heading-number">{number}</span> "##
            )));
        }
    }

    #[test]
    fn render_page_heading_numbering_follows_document_order_across_scopes() {
        let page = render_numbered(indoc! {"
            #### First {numbering-start=2}

            [^a]:
                ### Note {numbering-start=0}

            ::: callout {type=note}
            ##### Inner {numbering-start=0}

            ::: callout {type=tip}
            #### Sibling
            :::
            :::

            ## Last

            Text[^a].
        "})
        .unwrap();
        let mut previous = 0;
        for (id, number) in [
            ("first", "2"),
            ("inner", "2.0"),
            ("sibling", "3"),
            ("last", "4"),
            ("note", "4.0"),
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
                .contains(r##"href="#note"><span class="heading-number">4.0</span> Note</a>"##)
        );
    }

    #[test]
    fn render_page_heading_numbering_disabled_ignores_attributes() {
        let input = indoc! {"
            ## First {numbering-start=bad}
            ### Child {numbering-start}
            ## Last {numbering-start=-1}
        "};
        let page = render(input);
        assert!(page.content_html.contains(r#"<h2 id="first">First</h2>"#));
        assert!(page.content_html.contains(r#"<h3 id="child">Child</h3>"#));
        assert!(page.content_html.contains(r#"<h2 id="last">Last</h2>"#));
        assert!(!page.content_html.contains("numbering-start"));
        assert!(!page.content_html.contains("heading-number"));
        for (id, title) in [("first", "First"), ("child", "Child"), ("last", "Last")] {
            assert!(
                page.toc_html
                    .contains(&format!(r##"href="#{id}">{title}</a>"##))
            );
        }
        assert!(!page.toc_html.contains("heading-number"));
    }

    #[test]
    fn render_page_heading_numbering_maximum_can_be_reset_or_discarded() {
        let maximum = usize::MAX;
        let input = formatdoc! {"
            ## First {{numbering-start=2}}
            ### Child {{numbering-start={maximum}}}
            ## Next
            ## Maximum {{numbering-start={maximum}}}
            ## Reset {{numbering-start=0}}
            ## Last
        "};
        let page = render_numbered(&input).unwrap();
        for (id, number) in [
            ("first", "2".to_owned()),
            ("child", format!("2.{maximum}")),
            ("next", "3".to_owned()),
            ("maximum", maximum.to_string()),
            ("reset", "0".to_owned()),
            ("last", "1".to_owned()),
        ] {
            assert!(page.content_html.contains(&format!(
                r#"id="{id}"><span class="heading-number">{number}</span> "#
            )));
        }
    }

    #[test]
    fn render_page_invalid_heading_numbering_start_returns_error() {
        for attribute in [
            "numbering-start=-1",
            "numbering-start=1.5",
            "numbering-start=bad",
            "numbering-start=18446744073709551616",
            "numbering-start=",
            "numbering-start",
        ] {
            let input = format!("## Section {{{attribute}}}");
            let error = render_numbered(&input).unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("heading ID section: numbering-start must be a non-negative integer"),
                "{attribute}: {error}"
            );
        }
    }

    #[test]
    fn render_page_heading_numbering_overflow_returns_error() {
        let maximum = usize::MAX;
        let input = formatdoc! {"
            ## First {{numbering-start={maximum}}}
            ## Last
        "};
        let error = render_numbered(&input).unwrap_err();
        assert_eq!(
            error.to_string(),
            "heading ID last: heading numbering exceeds the maximum supported integer"
        );
    }

    #[test]
    fn render_page_duplicate_heading_number_returns_error() {
        let error = render_numbered(indoc! {"
            ## Section
            ### First
            ### Second {numbering-start=1}
        "})
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "heading numbering produces duplicate number 1.1 at heading ID second"
        );
    }

    // ── render_page: directives ──

    #[test]
    fn render_page_directives_sequential() {
        let page = render(indoc! {"
            ::: first
            First.
            :::

            Some text between.

            ::: second
            Second.
            :::
        "});
        assert_eq!(
            page.content_html,
            indoc! {r#"
                <div class="first"><p>First.</p>
                </div>
                <p>Some text between.</p>
                <div class="second"><p>Second.</p>
                </div>
            "#}
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
            :::: outer
            Outer text.

            ::: inner
            Inner text.
            :::
            ::::
        "});
        assert_eq!(
            page.content_html,
            indoc! {r#"
                <div class="outer"><p>Outer text.</p>
                <div class="inner"><p>Inner text.</p>
                </div>
                </div>
            "#}
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

    #[test]
    fn render_page_directive_template_output_keeps_own_line() {
        let (_dir, engine) = directive_engine("widget", "<widget></widget>\n");
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
        assert_eq!(
            page.content_html
                .matches(r#"<div class="callout">"#)
                .count(),
            1
        );
        assert!(page.toc_html.contains(r##"href="#inside-1""##));
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
        for info in ["mermaid", "Mermaid", "mermaid no_run"] {
            let input = formatdoc! {"
                ```{info}
                graph TD
                  A --> B
                ```
            "};
            let page = render(&input);
            assert!(
                page.assets.features.contains(&Feature::Mermaid),
                "fence {info:?} should set Feature::Mermaid, features: {:?}",
                page.assets.features,
            );
        }
    }

    #[test]
    fn render_page_directive_with_code_and_math() {
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

    // ── render_directive_block ──

    #[test]
    fn render_directive_block_callout_template_receives_validated_options() {
        let dir = tempfile::tempdir().unwrap();
        let directives = dir.path().join("directives");
        fs::create_dir_all(&directives).unwrap();
        fs::write(
            directives.join("callout.html"),
            "{{ type }}|{{ title }}|{{ open }}|{{ id }}|{{ classes | join(' ') }}|{{ body_html | safe }}",
        )
        .unwrap();

        let engine = TemplateEngine::new(Some(dir.path()), None, &test_i18n()).unwrap();
        let page = render_with(
            indoc! {"
                ::: callout {#my-note .wide type=Tip open=false}
                Hello **world**.
                :::
            "},
            &engine,
        );
        assert_eq!(
            page.content_html,
            "tip|None|False|my-note|wide|<p>Hello <strong>world</strong>.</p>\n"
        );
    }

    #[test]
    fn render_directive_block_invalid_callout_returns_error() {
        for name in ["callout", "Callout"] {
            let error = render_page(
                &formatdoc! {"
                    ::: {name} {{type=warn}}
                    Body
                    :::
                "},
                &test_engine(),
                &Config::default(),
                &RenderOptions::default(),
                &test_resources(),
                false,
            )
            .unwrap_err();
            assert!(
                error
                    .to_string()
                    .starts_with(r#"unknown callout type "warn""#),
                "{name}: {error}"
            );
        }
    }

    #[test]
    fn render_directive_block_uses_template() {
        let (_dir, engine) = directive_engine(
            "my-widget",
            "<widget>{{ name }}: {{ body_html | safe }}</widget>",
        );
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
    fn render_directive_block_template_accesses_parsed_args() {
        let (_dir, engine) = directive_engine(
            "widget",
            "my-pos={{ positional_args[0] }} my-key={{ named_args.key }}",
        );
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
    fn render_directive_block_template_accesses_source_dir() {
        let (_dir, engine) = directive_engine(
            "reader",
            "{% set data = read_file(positional_args[0]) %}DATA:{{ data }}",
        );
        let source = tempfile::tempdir().unwrap();
        fs::write(source.path().join("data.csv"), "A,B\n1,2").unwrap();

        let page = render_page(
            indoc! {r#"
                ::: reader {"data.csv"}
                :::
            "#},
            &engine,
            &Config::default(),
            &RenderOptions::default(),
            &PageResources {
                source_dir: Some(source.path()),
                ..test_resources()
            },
            false,
        )
        .unwrap();
        assert!(
            page.content_html.contains("DATA:A,B\n1,2"),
            "template should read file via source_dir, html:\n{}",
            page.content_html
        );
    }

    #[test]
    fn render_directive_block_fallback_to_div() {
        let page = render(indoc! {"
            ::: callout {#my-note .highlight type=tip}
            Some body.
            :::
        "});
        assert_eq!(
            page.content_html,
            indoc! {r#"
                <div id="my-note" class="callout highlight"><p>Some body.</p>
                </div>
            "#}
        );
    }

    #[test]
    fn render_directive_block_anonymous_div() {
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

    // ── restore_directives ──

    #[test]
    fn restore_directives_keeps_unmatched_placeholder_text() {
        let html = indoc! {"
            a <!--p-x-->
            <!--p-1-->
            <!--p-0-->
            b"};
        assert_eq!(
            restore_directives(html, "<!--p-", &["F\n".to_owned()]),
            indoc! {"
                a <!--p-x-->
                <!--p-1-->
                F
                b"}
        );
    }
}
