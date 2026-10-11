use std::borrow::Borrow;
use std::collections::HashMap;
use std::ops::Range;
use std::vec;

use itertools::Either;
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, LinkType, Parser, Tag, TagEnd};

use super::assets::{Feature, PageAssets};
use super::code_block::{CodeBlockSpec, parse_fence_info};
use super::emoji::match_emoji;
use super::footnote::Notes;
use super::heading::{NUMBERING_START, render_number};
use super::highlight::highlight_code;
use super::icon::match_icon;
use super::image::{render_block_image, render_inline_image};
use super::image_attrs::{ImageAttrs, extract_image_attrs};
use super::lqip::ImageMeta;
use super::mermaid::render_mermaid;
use super::table::stamp_nowrap;
use super::toc::TocEntry;
use super::{PageResources, RenderOptions, Spanned};
use crate::html::escape;
use crate::markdown::markdown_options;
use crate::text::slugify_or;

/// One parsed Markdown fragment, the page body or a directive body.
pub(super) struct MarkdownDocument {
    pub(super) body: Vec<Spanned>,
    pub(super) notes: Notes,
    /// Candidate IDs until the pipeline allocates page-wide IDs.
    pub(super) headings: Vec<TocEntry>,
    /// Keyed by the image's offset in the parsed source.
    pub(super) image_attrs: HashMap<usize, ImageAttrs>,
}

impl MarkdownDocument {
    pub(super) fn parse(content: &str, options: &RenderOptions) -> Self {
        let (content, image_attrs) = extract_image_attrs(content);
        let events = Parser::new_ext(&content, markdown_options())
            .into_offset_iter()
            .map(|(event, range)| (event.into_static(), range));
        let (body, notes) = Notes::extract(expand_shortcodes(events, &content, options));
        let headings = collect_headings(body.iter().chain(notes.events()));
        Self {
            body,
            notes,
            headings,
            image_attrs,
        }
    }

    /// Yields body events followed by reachable footnote bodies in reference order.
    pub(super) fn events(&self) -> impl Iterator<Item = &Spanned> {
        self.body.iter().chain(self.notes.events())
    }
}

/// Renders a document with allocated IDs to HTML, adding auto-detected features (math, mermaid)
/// to `assets`.
#[must_use]
pub(super) fn render_document(
    document: MarkdownDocument,
    resources: &PageResources<'_>,
    options: &RenderOptions,
    assets: &mut PageAssets,
) -> String {
    let MarkdownDocument {
        body,
        notes,
        headings,
        image_attrs,
    } = document;
    let mut renderer = MarkdownRenderer {
        image_attrs: &image_attrs,
        resources,
        options,
        assets,
        headings: headings.into_iter(),
    };
    let mut html = renderer.render_events(body);
    notes.render(&mut html, |events| renderer.render_events(events));
    html
}

/// Consumes the events inside an image whose `Start` was just taken, through its matching `End`.
pub(super) fn image_content<T: Borrow<Spanned>>(
    events: &mut impl Iterator<Item = T>,
) -> impl Iterator<Item = T> {
    let mut depth = 1;
    events.take_while(move |event| {
        match event.borrow().0 {
            Event::Start(Tag::Image { .. }) => depth += 1,
            Event::End(TagEnd::Image) => depth -= 1,
            _ => {}
        }
        depth > 0
    })
}

struct MarkdownRenderer<'a> {
    image_attrs: &'a HashMap<usize, ImageAttrs>,
    resources: &'a PageResources<'a>,
    options: &'a RenderOptions,
    assets: &'a mut PageAssets,
    headings: vec::IntoIter<TocEntry>,
}

impl MarkdownRenderer<'_> {
    fn render_events(&mut self, events: Vec<Spanned>) -> String {
        let mut output = Vec::with_capacity(events.len());
        self.transform(events, &mut output);
        if let Some(max_width) = self.options.table_nowrap_width {
            stamp_nowrap(&mut output, max_width);
        }
        let mut html = String::new();
        pulldown_cmark::html::push_html(
            &mut html,
            output
                .into_iter()
                .map(|event| transform_math(event, self.assets)),
        );
        html
    }

    /// Replaces headings, code blocks, and images with their final markup. Math stays as events
    /// until table widths are measured.
    fn transform(&mut self, events: Vec<Spanned>, output: &mut Vec<Event<'static>>) {
        let mut events = events.into_iter();
        while let Some((event, range)) = events.next() {
            match event {
                Event::Start(Tag::Paragraph) => {
                    let body: Vec<_> = events
                        .by_ref()
                        .take_while(|(event, _)| !matches!(event, Event::End(TagEnd::Paragraph)))
                        .collect();
                    if let Some(html) = self.block_image(&body) {
                        output.push(Event::Html(html.into()));
                    } else {
                        output.push(Event::Start(Tag::Paragraph));
                        self.transform(body, output);
                        output.push(Event::End(TagEnd::Paragraph));
                    }
                }

                Event::Start(Tag::Heading {
                    level,
                    classes,
                    mut attrs,
                    ..
                }) => {
                    let entry = self
                        .headings
                        .next()
                        .expect("collect_headings yields one entry per heading");
                    attrs.retain(|(key, _)| key.as_ref() != NUMBERING_START);
                    output.push(Event::Start(Tag::Heading {
                        level,
                        id: Some(entry.id.into()),
                        classes,
                        attrs,
                    }));
                    if let Some(number) = entry.number {
                        output.push(Event::Html(render_number(&number).into()));
                    }
                }

                Event::Start(Tag::CodeBlock(kind)) => {
                    output.push(Event::Html(
                        self.render_code_block(kind, &mut events).into(),
                    ));
                }

                Event::Start(Tag::Image {
                    dest_url, title, ..
                }) => {
                    let inner: Vec<_> = image_content(&mut events).collect();
                    let (url, alt, attrs) = self.image_parts(&dest_url, range.start, &inner);
                    output.push(Event::Html(
                        render_inline_image(&url, &alt, &title, attrs.as_ref()).into(),
                    ));
                }

                other => output.push(other),
            }
        }
    }

    fn render_code_block(
        &mut self,
        kind: CodeBlockKind<'_>,
        events: &mut impl Iterator<Item = Spanned>,
    ) -> String {
        let spec = match kind {
            CodeBlockKind::Fenced(info) => parse_fence_info(&info),
            CodeBlockKind::Indented => CodeBlockSpec::default(),
        };
        let mut code = String::new();
        while let Some((Event::Text(text), _)) = events.next() {
            code.push_str(&text);
        }

        if spec.is_mermaid() {
            self.assets.add_feature(Feature::Mermaid);
            render_mermaid(&code)
        } else {
            highlight_code(&code, &spec, self.options.code_max_lines)
        }
    }

    /// Renders a paragraph holding exactly one image as a figure.
    fn block_image(&self, body: &[Spanned]) -> Option<String> {
        let [
            (
                Event::Start(Tag::Image {
                    dest_url, title, ..
                }),
                range,
            ),
            inner @ ..,
            (Event::End(TagEnd::Image), _),
        ] = body
        else {
            return None;
        };
        if inner.iter().any(|(event, _)| {
            matches!(
                event,
                Event::Start(Tag::Image { .. }) | Event::End(TagEnd::Image)
            )
        }) {
            return None;
        }
        let (url, alt, attrs) = self.image_parts(dest_url, range.start, inner);
        Some(render_block_image(&url, &alt, title, attrs.as_ref()))
    }

    /// Resolves an image's URL, plain-text alt, and authored attributes merged with on-disk
    /// metadata.
    fn image_parts(
        &self,
        src: &str,
        offset: usize,
        inner: &[Spanned],
    ) -> (String, String, Option<ImageAttrs>) {
        let mut alt = String::new();
        for (event, _) in inner {
            push_plain_text(&mut alt, event);
        }
        let (url, meta) = self.resources.resolve_image(src);
        let attrs = enrich_image_attrs(self.image_attrs.get(&offset), meta.as_deref());
        (url, alt, attrs)
    }
}

/// Merges authored `{...}` attrs with resolver-supplied on-disk metadata.
/// Returns `None` only when neither side has anything to contribute.
fn enrich_image_attrs(base: Option<&ImageAttrs>, meta: Option<&ImageMeta>) -> Option<ImageAttrs> {
    if base.is_none() && meta.is_none() {
        return None;
    }
    let mut attrs = base.cloned().unwrap_or_default();
    if let Some(meta) = meta {
        attrs.fill_from_meta(meta);
    }
    Some(attrs)
}

/// Expands enabled emoji and icon shortcodes in text outside code blocks.
/// Expands shortcodes in text outside code blocks and autolinks.
fn expand_shortcodes(
    events: impl Iterator<Item = Spanned>,
    source: &str,
    options: &RenderOptions,
) -> impl Iterator<Item = Spanned> {
    let (emojis, icons) = (options.emojis, options.fontawesome);
    let mut literal = false;
    events.flat_map(move |(event, range)| {
        match &event {
            Event::Start(
                Tag::CodeBlock(_)
                | Tag::Link {
                    link_type: LinkType::Autolink | LinkType::Email,
                    ..
                },
            ) => literal = true,
            Event::End(TagEnd::CodeBlock | TagEnd::Link) => literal = false,
            Event::Text(text) if !literal && (emojis || icons) && text.contains(':') => {
                // A backslash escape leaves its character at the start of the next text event.
                let escaped = source[..range.start]
                    .bytes()
                    .rev()
                    .take_while(|&byte| byte == b'\\')
                    .count()
                    % 2
                    == 1;
                let expanded = expand_text(text, &range, escaped, emojis, icons);
                return Either::Right(expanded.into_iter());
            }
            _ => {}
        }
        Either::Left(std::iter::once((event, range)))
    })
}

/// Splits a text event around shortcodes, keeping an `escaped` first character literal. Emoji stay
/// text, while icons become inline HTML.
fn expand_text(
    text: &str,
    range: &Range<usize>,
    escaped: bool,
    emojis: bool,
    icons: bool,
) -> Vec<Spanned> {
    let mut events = Vec::new();
    let (literal, mut rest) = text.split_at(usize::from(escaped));
    let mut plain = String::with_capacity(text.len());
    plain.push_str(literal);
    while let Some(colon) = rest.find(':') {
        plain.push_str(&rest[..colon]);
        rest = &rest[colon..];
        if emojis && let Some((emoji, len)) = match_emoji(rest) {
            plain.push_str(emoji);
            rest = &rest[len..];
        } else if icons && let Some((html, len)) = match_icon(rest) {
            if !plain.is_empty() {
                events.push((
                    Event::Text(std::mem::take(&mut plain).into()),
                    range.clone(),
                ));
            }
            events.push((Event::InlineHtml(html.into()), range.clone()));
            rest = &rest[len..];
        } else {
            plain.push(':');
            rest = &rest[1..];
        }
    }
    plain.push_str(rest);
    if !plain.is_empty() {
        events.push((Event::Text(plain.into()), range.clone()));
    }
    events
}

/// Collects heading metadata with authored or slugified candidate IDs.
fn collect_headings<'a>(events: impl Iterator<Item = &'a Spanned>) -> Vec<TocEntry> {
    let mut headings = Vec::new();

    let mut level = HeadingLevel::H1;
    let mut explicit_id: Option<String> = None;
    let mut text = String::new();
    let mut in_heading = false;

    for (event, _) in events {
        match event {
            Event::Start(Tag::Heading {
                level: l, id: eid, ..
            }) => {
                level = *l;
                explicit_id = eid.as_deref().map(str::to_owned);
                text.clear();
                in_heading = true;
            }
            Event::End(TagEnd::Heading(_)) => {
                in_heading = false;
                headings.push(TocEntry {
                    level,
                    number: None,
                    id: explicit_id
                        .take()
                        .unwrap_or_else(|| slugify_or(&text, "section")),
                    title: text.trim().to_owned(),
                });
            }
            _ if in_heading => push_plain_text(&mut text, event),
            _ => {}
        }
    }

    headings
}

/// Accumulates plain-text content from an event into `buf`.
fn push_plain_text(buf: &mut String, event: &Event) {
    match event {
        Event::Text(t) | Event::Code(t) | Event::InlineMath(t) | Event::DisplayMath(t) => {
            buf.push_str(t);
        }
        Event::SoftBreak | Event::HardBreak => buf.push(' '),
        _ => {}
    }
}

/// Converts math events into KaTeX-compatible HTML and passes other events through.
fn transform_math<'a>(event: Event<'a>, assets: &mut PageAssets) -> Event<'a> {
    match event {
        Event::InlineMath(content) => {
            assets.add_feature(Feature::Math);
            let html = format!(
                r#"<span class="math math-inline">\({}\)</span>"#,
                escape(&content)
            );
            Event::InlineHtml(html.into())
        }
        Event::DisplayMath(content) => {
            assets.add_feature(Feature::Math);
            let html = format!(
                r#"<span class="math math-display">\[{}\]</span>"#,
                escape(&content)
            );
            Event::Html(format!("{html}\n").into())
        }
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use indoc::indoc;

    use crate::config::ImageConfig;
    use crate::render::lqip::ImageResolver;
    use crate::render::pipeline::{RenderedPage, render_page};
    use crate::render::{PageResources, RenderOptions};
    use crate::static_assets::StaticAssetManifest;
    use crate::test_utils::{render_test_page, test_engine, test_resources};

    fn render(content: &str) -> RenderedPage {
        render_test_page(content, &RenderOptions::default())
    }

    fn render_with_resolver(
        content: &str,
        resolver: &ImageResolver,
        output_dir: &Path,
    ) -> RenderedPage {
        render_page(
            content,
            &test_engine(),
            &RenderOptions::default(),
            &PageResources {
                images: resolver,
                assets: &StaticAssetManifest::build(output_dir).unwrap(),
                ..test_resources()
            },
            false,
        )
        .unwrap()
    }

    fn write_tiny_png(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let img = image::RgbaImage::from_pixel(8, 4, image::Rgba([200, 100, 50, 255]));
        img.save_with_format(path, image::ImageFormat::Png).unwrap();
    }

    // ── render_document: basic ──

    #[test]
    fn render_document_paragraph() {
        let out = render("Hello, world!");
        assert_eq!(out.content_html, "<p>Hello, world!</p>\n");
        assert_eq!(out.toc_html, "");
    }

    // ── render_document: headings ──

    #[test]
    fn render_document_headings_build_toc() {
        let out = render(indoc! {"
            ## First

            ### Second

            ## Third
        "});
        assert!(
            out.content_html.contains(r#"<h2 id="first">"#),
            "html:\n{}",
            out.content_html
        );
        assert_eq!(
            out.toc_html,
            indoc! {r##"
                <nav class="toc">
                  <ul>
                    <li><a href="#first">First</a>
                      <ul>
                        <li><a href="#second">Second</a>
                        </li>
                      </ul>
                    </li>
                    <li><a href="#third">Third</a>
                    </li>
                  </ul>
                </nav>
            "##}
        );
    }

    #[test]
    fn render_document_heading_ids_and_titles() {
        for (input, id, title) in [
            ("## Introduction {#custom-id}", "custom-id", "Introduction"),
            (
                "## The `foo` function",
                "the-foo-function",
                "The foo function",
            ),
            (
                "## The $x^2$ equation",
                "the-x-2-equation",
                "The x^2 equation",
            ),
            (
                r"## Sum $$\sum_{i=1}^n$$",
                "sum-sum_-i-1-n",
                r"Sum \sum_{i=1}^n",
            ),
            (
                "## See [example](https://example.com)",
                "see-example",
                "See example",
            ),
            ("## 测试文本", "测试文本", "测试文本"),
            ("##  \n", "section", ""),
        ] {
            let out = render(input);
            assert!(
                out.content_html.contains(&format!(r#"<h2 id="{id}">"#)),
                "{input}: {}",
                out.content_html
            );
            assert!(
                out.toc_html
                    .contains(&format!(r##"<a href="#{id}">{title}</a>"##)),
                "{input}: {}",
                out.toc_html
            );
        }
    }

    #[test]
    fn render_document_heading_keeps_inline_markup() {
        let out = render("## The `foo` function and $x^2$ [link](https://example.com)");
        for expected in [
            "<code>foo</code>",
            r#"<span class="math math-inline">\(x^2\)</span>"#,
            r#"href="https://example.com""#,
        ] {
            assert!(
                out.content_html.contains(expected),
                "{expected}: {}",
                out.content_html
            );
        }
        let out = render(r"## Sum $$\sum_{i=1}^n$$");
        assert!(
            out.content_html
                .contains(r#"<span class="math math-display">\[\sum_{i=1}^n\]</span>"#),
            "html:\n{}",
            out.content_html
        );
    }

    // ── render_document: shortcodes ──

    #[test]
    fn render_document_expands_shortcodes_only_in_text() {
        let out = render_test_page(
            indoc! {r#"
                ## :(fas fa-star): Hi :smile:

                文 :unknown: :x::smile: 尾 :(fas fa-link): `:smile:` $:smile:$ [a](/b/:smile:) <span title=":smile:"></span>

                :white_check_mark: :+1: \:smile: &#58;smile: \\:smile: <https://example.com/:smile:>

                <style>a:link:hover{}</style>

                    :smile:
            "#},
            &RenderOptions {
                emojis: true,
                fontawesome: true,
                ..RenderOptions::default()
            },
        );
        let html = &out.content_html;
        assert!(
            html.contains(concat!(
                r#"<p>文 :unknown: ❌😄 尾 <i class="fas fa-link" aria-hidden="true"></i> "#,
                r#"<code>:smile:</code> <span class="math math-inline">\(:smile:\)</span> "#,
                r#"<a href="/b/:smile:">a</a> <span title=":smile:"></span></p>"#,
            )),
            "{html}"
        );
        assert!(
            html.contains(concat!(
                r"<p>✅ 👍 :smile: :smile: \😄 ",
                r#"<a href="https://example.com/:smile:">https://example.com/:smile:</a></p>"#,
            )),
            "{html}"
        );
        assert!(html.contains("<style>a:link:hover{}</style>"), "{html}");
        assert_eq!(html.matches('😄').count(), 3, "{html}");
        assert!(out.toc_html.contains(r##"<a href="#hi">Hi 😄</a>"##));
    }

    // ── render_document: GFM extensions ──

    #[test]
    fn render_document_gfm_table() {
        let md = indoc! {"
            | Name | City |
            |------|------|
            | Alice | Paris |
            | Bob | Tokyo |
        "};
        let out = render(md);

        // Each cell carries distinct content so a column-swap or row-swap bug breaks the test.
        let expected = indoc! {"
            <table><thead><tr><th>Name</th><th>City</th></tr></thead><tbody>
            <tr><td>Alice</td><td>Paris</td></tr>
            <tr><td>Bob</td><td>Tokyo</td></tr>
            </tbody></table>"
        };
        assert!(
            out.content_html.contains(expected),
            "should preserve table nesting and cell order, html:\n{}",
            out.content_html
        );
    }

    #[test]
    fn render_document_strikethrough() {
        let out = render("~~deleted~~");
        assert!(
            out.content_html.contains("<del>deleted</del>"),
            "html:\n{}",
            out.content_html
        );
    }

    #[test]
    fn render_document_tasklist() {
        let md = indoc! {"
            - [x] Done
            - [ ] Todo
        "};
        let out = render(md);

        let input_before = |label: &str| -> String {
            let pos = out
                .content_html
                .find(label)
                .unwrap_or_else(|| panic!("missing {label}"));
            let start = out.content_html[..pos]
                .rfind("<input")
                .unwrap_or_else(|| panic!("no input before {label}"));
            out.content_html[start..pos].to_owned()
        };

        let done_input = input_before("Done");
        assert!(
            done_input.contains("checked"),
            "checked item should have checked attribute, input: {done_input}",
        );
        let todo_input = input_before("Todo");
        assert!(
            !todo_input.contains("checked"),
            "unchecked item should not have checked attribute, input: {todo_input}",
        );
    }

    // ── render_document: footnotes ──

    #[test]
    fn render_document_footnotes_relocated_after_body() {
        let md = indoc! {"
            ## Before

            Text[^1].

            [^1]:
                ## Note heading

                Footnote content.

            ## After

            Closing paragraph.
        "};
        let out = render(md);
        let closing = out.content_html.find("Closing paragraph.").unwrap();
        let section = out
            .content_html
            .find(r#"<section class="footnotes""#)
            .expect("footnote section emitted");
        assert!(
            closing < section,
            "definition should move after the body, html:\n{}",
            out.content_html
        );
        assert!(
            out.content_html
                .contains(r##"<a href="#fn-1" role="doc-noteref">1</a>"##),
            "html:\n{}",
            out.content_html
        );
        let positions = ["before", "after", "note-heading"]
            .map(|id| out.toc_html.find(&format!(r##"href="#{id}""##)).unwrap());
        assert!(positions.is_sorted(), "toc:\n{}", out.toc_html);
    }

    #[test]
    fn render_document_footnotes_inline_markup_in_reference_paragraph() {
        let md = indoc! {"
            Math $x$[^1] and ![icon](a.png)[^1].

            [^1]: Note.
        "};
        let out = render(md);
        assert!(out.content_html.contains(r#"src="a.png""#));
        assert!(
            out.content_html
                .contains(r#"<span class="math math-inline">"#)
                && out.content_html.contains(r#"id="fnref-1-2""#),
            "html:\n{}",
            out.content_html
        );
    }

    #[test]
    fn render_document_footnotes_preserves_images_math_and_code_in_notes() {
        let out = render(indoc! {r"
            Body[^a].

            [^a]:
                Math $x$ and ![inline](inline.png).

                ```text
                first
                  second
                ```

                ![block](block.png)
        "});
        assert!(out.content_html.contains(r#"class="math math-inline""#));
        assert!(out.content_html.contains(r#"src="inline.png""#));
        assert!(out.content_html.contains(r#"src="block.png""#));
        assert!(out.content_html.contains("<figure"));
        let plain = render(indoc! {"
            ```text
            first
              second
            ```
        "});
        assert!(
            out.content_html.contains(plain.content_html.trim()),
            "{}",
            out.content_html
        );
        assert!(
            out.content_html
                .contains("</figure>\n<p><a class=\"footnote-backref\"")
        );
    }

    #[test]
    fn render_document_footnote_syntax_interrupts_image_parsing() {
        let out = render(indoc! {"
            ![Alt[^a]](image.png)

            [^a]: Visible.
        "});
        assert!(out.content_html.starts_with("<p>![Alt<sup"));
        assert!(out.content_html.contains(r#"id="fnref-a-1""#));
        assert!(out.content_html.contains(r##"href="#fnref-a-1""##));
        assert!(!out.content_html.contains("<img"));
    }

    // ── render_document: math ──

    #[test]
    fn render_document_inline_math() {
        for (input, expected) in [
            (
                "$x^2$",
                r#"<p><span class="math math-inline">\(x^2\)</span></p>"#,
            ),
            (
                "The matrix $a_{ij}$ is symmetric.",
                r#"<p>The matrix <span class="math math-inline">\(a_{ij}\)</span> is symmetric.</p>"#,
            ),
        ] {
            let out = render(input);
            assert!(out.content_html.contains(expected), "{}", out.content_html);
        }
    }

    #[test]
    fn render_document_display_math() {
        for (input, expected) in [
            (
                "$$E=mc^2$$",
                r#"<span class="math math-display">\[E=mc^2\]</span>"#,
            ),
            (
                "$$a_{ij} + b_{ij}$$",
                r#"<span class="math math-display">\[a_{ij} + b_{ij}\]</span>"#,
            ),
        ] {
            let out = render(input);
            assert!(out.content_html.contains(expected), "{}", out.content_html);
        }
    }

    #[test]
    fn render_document_math_with_html_chars() {
        let out = render("$x < y$");
        assert!(
            out.content_html.contains(r"\(x &lt; y\)"),
            "math content should be HTML-escaped, html:\n{}",
            out.content_html
        );
    }

    // ── render_document: code blocks ──

    #[test]
    fn render_document_plaintext_code_blocks() {
        for md in [
            indoc! {"
                ```
                fn main() {}
                ```
            "},
            "    fn main() {}",
        ] {
            let out = render(md);
            assert!(
                out.content_html.contains(r#"class="highlight""#),
                "{}",
                out.content_html
            );
            assert!(
                out.content_html.contains(indoc! {r#"
                    <details class="code-block" open>
                      <summary class="code-header">
                        <span class="code-lang">Plain Text</span>
                "#}),
                "{}",
                out.content_html
            );
            assert!(
                out.content_html.contains("fn main() {}"),
                "{}",
                out.content_html
            );
        }
    }

    #[test]
    fn render_document_code_block_extracts_language_and_highlights_source() {
        for md in [
            indoc! {"
                ```rust
                fn main() {}
                ```
            "},
            indoc! {"
                ```rust no_run
                fn main() {}
                ```
            "},
        ] {
            let out = render(md);
            assert!(
                out.content_html.contains(r#"class="highlight""#),
                "{}",
                out.content_html
            );
            assert!(
                out.content_html.contains(r#"data-lang="rust""#),
                "{}",
                out.content_html
            );
            assert!(!out.content_html.contains("no_run"), "{}", out.content_html);
            assert!(
                out.content_html
                    .contains(r#"<span class="source rust"><span class="meta function rust">"#),
                "{}",
                out.content_html
            );
        }
    }

    #[test]
    fn render_document_code_block_mermaid_emits_bare_pre() {
        let md = indoc! {"
            ```mermaid
            graph TD
            A --> B
            ```
        "};
        let out = render(md);
        assert!(
            out.content_html.contains(r#"<pre class="mermaid""#),
            "should emit bare pre.mermaid, html:\n{}",
            out.content_html
        );
        assert!(
            !out.content_html.contains(r#"class="code-block""#),
            "mermaid block should bypass the code-block chrome, html:\n{}",
            out.content_html
        );
        assert!(
            out.content_html.contains(indoc! {r#"
                data-source="graph TD
                A --&gt; B
                ""#
            }),
            "should preserve the DSL verbatim in data-source for theme-toggle re-render, html:\n{}",
            out.content_html
        );
    }

    #[test]
    fn render_document_code_block_mermaid_case_insensitive() {
        let md = indoc! {"
            ```Mermaid
            graph TD
            ```
        "};
        let out = render(md);
        assert!(
            out.content_html.contains(r#"<pre class="mermaid""#),
            "case-insensitive language tag should still emit pre.mermaid, html:\n{}",
            out.content_html
        );
    }

    // ── render_document: images ──

    #[test]
    fn render_document_block_image() {
        let md = "![A photo](img.png)\n";
        let out = render(md);
        assert!(
            out.content_html.contains("<figure>"),
            "should become a figure, html:\n{}",
            out.content_html
        );
        assert!(
            out.content_html.contains(r#"alt="A photo""#),
            "should have alt attribute, html:\n{}",
            out.content_html
        );
        assert!(
            out.content_html
                .contains("<figcaption>A photo</figcaption>"),
            "should have figcaption with alt text, html:\n{}",
            out.content_html
        );
    }

    #[test]
    fn render_document_block_image_with_title() {
        let md = "![alt text](img.png \"My Title\")\n";
        let out = render(md);
        assert!(
            out.content_html.contains("<figure>"),
            "should become a figure, html:\n{}",
            out.content_html
        );
        assert!(
            out.content_html.contains(r#"alt="alt text""#),
            "should have alt attribute, html:\n{}",
            out.content_html
        );
        assert!(
            out.content_html.contains(r#"title="My Title""#),
            "should have title attribute, html:\n{}",
            out.content_html
        );
        assert!(
            out.content_html
                .contains("<figcaption>alt text</figcaption>"),
            "should have figcaption with alt text, html:\n{}",
            out.content_html
        );
    }

    #[test]
    fn render_document_block_image_with_formatted_alt() {
        let md = "![*bold* alt](img.png)\n";
        let out = render(md);
        assert!(
            out.content_html.contains("<figure>"),
            "should become a figure, html:\n{}",
            out.content_html
        );
        assert!(
            out.content_html.contains(r#"alt="bold alt""#),
            "should have plain-text alt attribute, html:\n{}",
            out.content_html
        );
        assert!(
            out.content_html
                .contains("<figcaption>bold alt</figcaption>"),
            "should have figcaption with plain text from formatted alt, html:\n{}",
            out.content_html
        );
    }

    #[test]
    fn render_document_block_image_with_soft_break_in_alt() {
        let md = indoc! {"
            ![line1
            line2](img.png)
        "};
        let out = render(md);
        assert!(
            out.content_html.contains("<figure>"),
            "should become a figure, html:\n{}",
            out.content_html
        );
        assert!(
            out.content_html.contains(r#"alt="line1 line2""#),
            "soft break in alt attribute should become a space, html:\n{}",
            out.content_html
        );
        assert!(
            out.content_html
                .contains("<figcaption>line1 line2</figcaption>"),
            "soft break in figcaption should become a space, html:\n{}",
            out.content_html
        );
    }

    #[test]
    fn render_document_inline_image() {
        let md = "Text ![icon](icon.png) more text\n";
        let out = render(md);
        assert!(
            !out.content_html.contains("<figure>"),
            "should not become a figure, html:\n{}",
            out.content_html
        );
        assert!(
            out.content_html.contains("<img "),
            "should have img tag, html:\n{}",
            out.content_html
        );
        assert!(
            out.content_html.contains(r#"alt="icon""#),
            "should have alt attribute, html:\n{}",
            out.content_html
        );
    }

    #[test]
    fn render_document_inline_image_with_nested_alt_preserves_surrounding_text() {
        let out = render("Before ![outer ![inner](inner.png)](outer.png) after.");

        assert_eq!(
            out.content_html,
            indoc! {r#"
                <p>Before <img src="outer.png" alt="outer inner" loading="lazy" decoding="async" /> after.</p>
            "#},
        );
    }

    #[test]
    fn render_document_image_with_trailing_text_stays_inline() {
        let md = "![icon](icon.png) followed by text\n";
        let out = render(md);
        assert!(
            !out.content_html.contains("<figure>"),
            "image with trailing text should not become a figure, html:\n{}",
            out.content_html
        );
        assert!(
            out.content_html.contains("<img "),
            "should have img tag, html:\n{}",
            out.content_html
        );
    }

    #[test]
    fn render_document_multiple_images_stay_inline() {
        let md = "![a](a.png) ![b](b.png)\n";
        let out = render(md);
        assert!(
            !out.content_html.contains("<figure>"),
            "multiple images should not become figures, html:\n{}",
            out.content_html
        );
        assert!(
            out.content_html.contains(r#"alt="a""#),
            "first image should be present, html:\n{}",
            out.content_html
        );
        assert!(
            out.content_html.contains(r#"alt="b""#),
            "second image should be present, html:\n{}",
            out.content_html
        );
    }

    // ── render_document: image resolution ──

    #[test]
    fn render_document_resolver_stamps_dimensions_and_lqip_on_block_image() {
        let dir = tempfile::tempdir().unwrap();
        write_tiny_png(&dir.path().join("img.png"));

        let resolver = ImageResolver::new(dir.path(), ImageConfig::default());
        let out = render_with_resolver("![alt](img.png)\n", &resolver, dir.path());

        assert!(
            out.content_html.contains(r#"width="8""#),
            "html:\n{}",
            out.content_html
        );
        assert!(
            out.content_html.contains(r#"height="4""#),
            "html:\n{}",
            out.content_html
        );

        // Pin the nesting order so a transposition (e.g. wrapper outside the
        // figure, or img outside the wrapper) trips the test.
        let figure = out.content_html.find("<figure>").expect("figure opens");
        let wrapper = out
            .content_html
            .find(r#"<span class="lqip" style="--lqip-uri:url('data:image/webp;base64,"#)
            .expect("wrapper opens");
        let img = out.content_html.find("<img ").expect("img tag");
        let wrapper_close = out.content_html.find("</span>").expect("wrapper closes");
        let figure_close = out.content_html.find("</figure>").expect("figure closes");
        assert!(
            figure < wrapper
                && wrapper < img
                && img < wrapper_close
                && wrapper_close < figure_close,
            "expected <figure> > <span.lqip> > <img> > </span> > </figure>, html:\n{}",
            out.content_html,
        );
    }

    #[test]
    fn render_document_resolver_merges_with_authored_attrs_on_inline_image() {
        let dir = tempfile::tempdir().unwrap();
        write_tiny_png(&dir.path().join("img.png"));

        let resolver = ImageResolver::new(dir.path(), ImageConfig::default());
        let out = render_with_resolver(
            "![a](img.png){width=4} ![b](img.png)\n",
            &resolver,
            dir.path(),
        );

        let fragment = scraper::Html::parse_fragment(&out.content_html);
        let images = scraper::Selector::parse("img").unwrap();
        let attrs: Vec<_> = fragment
            .select(&images)
            .map(|image| {
                (
                    image.value().attr("alt"),
                    image.value().attr("width"),
                    image.value().attr("height"),
                )
            })
            .collect();

        assert_eq!(
            attrs,
            vec![
                (Some("a"), Some("4"), Some("2")),
                (Some("b"), Some("8"), Some("4")),
            ]
        );
        assert!(
            fragment
                .select(&scraper::Selector::parse("figure").unwrap())
                .next()
                .is_none()
        );
    }

    #[test]
    fn render_document_resolver_miss_emits_remote_image_without_dimensions() {
        let dir = tempfile::tempdir().unwrap();
        let resolver = ImageResolver::new(dir.path(), ImageConfig::default());
        let out = render_with_resolver(
            "![remote](https://cdn.example.com/x.png)\n",
            &resolver,
            dir.path(),
        );

        assert!(
            out.content_html
                .contains(r#"src="https://cdn.example.com/x.png""#)
        );
        assert!(
            !out.content_html.contains("width="),
            "html:\n{}",
            out.content_html
        );
        assert!(
            !out.content_html.contains(r#"class="lqip""#),
            "html:\n{}",
            out.content_html
        );
    }
}
