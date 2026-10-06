use std::collections::{BTreeSet, HashMap};
use std::path::Path;

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use syntect::parsing::SyntaxSet;

use super::Spanned;
use super::assets::Feature;
use super::code_block::{CodeBlockSpec, parse_fence_info};
use super::footnote::Footnotes;
use super::heading::render_number;
use super::highlight::highlight_code;
use super::image::{render_block_image, render_inline_image};
use super::image_attrs::ImageAttrs;
use super::lqip::ImageResolver;
use super::mermaid::render_mermaid;
use super::table::TableNowrap;
use super::toc::TocEntry;
use crate::html::escape;
use crate::text::slugify;

pub(super) struct MarkdownDocument {
    pub(super) footnotes: Footnotes,
    pub(super) headings: Vec<TocEntry>,
}

impl MarkdownDocument {
    pub(super) fn parse(content: &str) -> Self {
        let events = Parser::new_ext(content, markdown_options())
            .into_offset_iter()
            .map(|(event, range)| (event.into_static(), range))
            .collect();
        let footnotes = Footnotes::collect(events);
        let headings = collect_headings(footnotes.events());
        Self {
            footnotes,
            headings,
        }
    }
}

/// The result of rendering markdown content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownOutput {
    /// The rendered HTML string.
    pub html: String,
    /// Table of contents entries collected from headings.
    pub headings: Vec<TocEntry>,
}

/// Site-level settings applied while rendering markdown.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct MarkdownSettings {
    pub code_max_lines: Option<usize>,
    /// In terminal columns.
    pub table_nowrap_width: Option<usize>,
}

/// Renders markdown content to HTML with GFM extensions, math support, syntax highlighting,
/// and image enhancement. Auto-detected features (math, mermaid) are inserted into `features`.
#[must_use]
pub(super) fn render_markdown(
    document: MarkdownDocument,
    syntax_set: &SyntaxSet,
    image_attrs: &HashMap<usize, ImageAttrs>,
    image_resolver: &ImageResolver,
    base_dir: Option<&Path>,
    settings: MarkdownSettings,
    features: &mut BTreeSet<Feature>,
) -> MarkdownOutput {
    let MarkdownDocument {
        footnotes,
        headings,
    } = document;
    let mut renderer = MarkdownRenderer {
        syntax_set,
        image_attrs,
        image_resolver,
        base_dir,
        settings,
        features,
        headings: &headings,
        heading_index: 0,
    };
    let html = footnotes.render(|events, level| renderer.render_blocks(events, level));
    MarkdownOutput { html, headings }
}

struct MarkdownRenderer<'a> {
    syntax_set: &'a SyntaxSet,
    image_attrs: &'a HashMap<usize, ImageAttrs>,
    image_resolver: &'a ImageResolver,
    base_dir: Option<&'a Path>,
    settings: MarkdownSettings,
    features: &'a mut BTreeSet<Feature>,
    headings: &'a [TocEntry],
    heading_index: usize,
}

impl MarkdownRenderer<'_> {
    fn render_blocks(&mut self, events: Vec<Spanned>, level: u8) -> String {
        let mut html = String::new();
        let mut block = Vec::new();
        let mut depth = 0;
        for event in events {
            match &event.0 {
                Event::Start(_) => depth += 1,
                Event::End(_) => depth -= 1,
                _ => {}
            }
            block.push(event);
            if depth == 0 {
                let rendered = self.render_events(std::mem::take(&mut block));
                if !rendered.is_empty() {
                    crate::html::indent(&mut html, level);
                }
                html.push_str(&rendered);
            }
        }
        html
    }

    fn render_events(&mut self, events: Vec<Spanned>) -> String {
        let mut output_events: Vec<Event<'_>> = Vec::new();

        let mut in_code_block = false;
        let mut code_spec = CodeBlockSpec::default();
        let mut code_buf = String::new();
        let mut is_mermaid_block = false;
        let mut para_buf: Vec<Spanned> = Vec::new();
        let mut in_para = false;
        let mut table_nowrap = self.settings.table_nowrap_width.map(TableNowrap::new);

        for (event, range) in events {
            match event {
                // ── Headings ──
                Event::Start(Tag::Heading { .. }) => {
                    let entry = &self.headings[self.heading_index];
                    self.heading_index += 1;
                    output_events.push(Event::Html(
                        format!(r#"<{} id="{}">"#, entry.level, escape(&entry.id)).into(),
                    ));
                    output_events.push(Event::Html(render_number(entry.number.as_deref()).into()));
                }
                Event::End(TagEnd::Heading(level)) => {
                    output_events.push(Event::Html(format!("</{level}>\n").into()));
                }

                // ── Code blocks: buffer content, emit on End ──
                Event::Start(Tag::CodeBlock(kind)) => {
                    in_code_block = true;
                    code_spec = match kind {
                        CodeBlockKind::Fenced(lang) => {
                            parse_fence_info(&lang, self.settings.code_max_lines)
                        }
                        CodeBlockKind::Indented => CodeBlockSpec {
                            max_lines: self.settings.code_max_lines,
                            ..CodeBlockSpec::default()
                        },
                    };
                    is_mermaid_block = code_spec
                        .lang
                        .as_deref()
                        .is_some_and(|l| l.eq_ignore_ascii_case("mermaid"));
                    if is_mermaid_block {
                        self.features.insert(Feature::Mermaid);
                    }
                    code_buf.clear();
                }
                Event::End(TagEnd::CodeBlock) => {
                    in_code_block = false;
                    let html = if is_mermaid_block {
                        render_mermaid(&code_buf)
                    } else {
                        highlight_code(self.syntax_set, &code_buf, &code_spec)
                    };
                    output_events.push(Event::Html(html.into()));
                    code_buf.clear();
                    is_mermaid_block = false;
                }
                Event::Text(ref t) if in_code_block => {
                    code_buf.push_str(t);
                }

                // ── Paragraphs: buffer to detect sole-image blocks ──
                Event::Start(Tag::Paragraph) => {
                    in_para = true;
                    para_buf.clear();
                }
                Event::End(TagEnd::Paragraph) => {
                    in_para = false;
                    if let Some(html) = try_render_block_image(
                        &para_buf,
                        self.image_attrs,
                        self.image_resolver,
                        self.base_dir,
                    ) {
                        output_events.push(Event::Html(html.into()));
                    } else {
                        output_events.push(Event::Html("<p>".into()));
                        flush_paragraph(
                            &para_buf,
                            self.image_attrs,
                            self.image_resolver,
                            self.base_dir,
                            &mut output_events,
                            self.features,
                        );
                        output_events.push(Event::Html("</p>\n".into()));
                    }
                    para_buf.clear();
                }
                _ if in_para => {
                    para_buf.push((event, range));
                }

                // ── Everything else (tables, math, etc.) ──
                other => {
                    if let Some(nowrap) = &mut table_nowrap {
                        nowrap.observe(&other, &mut output_events);
                    }
                    output_events.push(transform_math(other, self.features));
                }
            }
        }

        let mut html = String::new();
        pulldown_cmark::html::push_html(&mut html, output_events.into_iter());

        html
    }
}

/// Checks if a paragraph's buffered events represent a sole image (block image promotion).
fn try_render_block_image(
    events: &[Spanned],
    image_attrs: &HashMap<usize, ImageAttrs>,
    image_resolver: &ImageResolver,
    base_dir: Option<&Path>,
) -> Option<String> {
    let (src, title, byte_offset) = match &events.first()?.0 {
        Event::Start(Tag::Image {
            dest_url, title, ..
        }) => (
            dest_url.to_string(),
            title.to_string(),
            events.first()?.1.start,
        ),
        _ => return None,
    };

    if !matches!(&events.last()?.0, Event::End(TagEnd::Image)) {
        return None;
    }

    let inner = &events[1..events.len() - 1];

    // Reject multiple images in the same paragraph.
    if inner.iter().any(|(ev, _)| {
        matches!(
            ev,
            Event::Start(Tag::Image { .. }) | Event::End(TagEnd::Image)
        )
    }) {
        return None;
    }

    let alt = extract_alt_text(inner);
    let enriched = enrich_image_attrs(
        image_attrs.get(&byte_offset),
        &src,
        image_resolver,
        base_dir,
    );
    Some(render_block_image(&src, &alt, &title, enriched.as_ref()))
}

/// Flushes buffered paragraph events, replacing inline image sequences with `render_inline_image`
/// output while passing other events through.
fn flush_paragraph(
    events: &[Spanned],
    image_attrs: &HashMap<usize, ImageAttrs>,
    image_resolver: &ImageResolver,
    base_dir: Option<&Path>,
    output: &mut Vec<Event<'static>>,
    features: &mut BTreeSet<Feature>,
) {
    let mut i = 0;
    while i < events.len() {
        if let Event::Start(Tag::Image {
            dest_url, title, ..
        }) = &events[i].0
        {
            let src = dest_url.to_string();
            let title = title.to_string();
            let byte_offset = events[i].1.start;

            // Collect inner events up to End(Image) for alt text extraction.
            let inner_start = i + 1;
            i = inner_start;
            while i < events.len() && !matches!(events[i].0, Event::End(TagEnd::Image)) {
                i += 1;
            }
            let alt = extract_alt_text(&events[inner_start..i]);
            if i < events.len() {
                i += 1; // skip End(Image)
            }

            let enriched = enrich_image_attrs(
                image_attrs.get(&byte_offset),
                &src,
                image_resolver,
                base_dir,
            );
            output.push(Event::Html(
                render_inline_image(&src, &alt, &title, enriched.as_ref()).into(),
            ));
        } else {
            output.push(transform_math(events[i].0.clone(), features));
            i += 1;
        }
    }
}

/// Merges authored `{...}` attrs with resolver-supplied on-disk metadata.
/// Returns `None` only when neither side has anything to contribute.
fn enrich_image_attrs(
    base: Option<&ImageAttrs>,
    src: &str,
    image_resolver: &ImageResolver,
    base_dir: Option<&Path>,
) -> Option<ImageAttrs> {
    let meta = image_resolver.resolve(src, base_dir);
    if base.is_none() && meta.is_none() {
        return None;
    }
    let mut attrs = base.cloned().unwrap_or_default();
    if let Some(meta) = meta {
        attrs.fill_from_meta(&meta);
    }
    Some(attrs)
}

/// Extracts plain text from image inner events for use as alt text.
fn extract_alt_text(events: &[Spanned]) -> String {
    let mut alt = String::new();
    for (ev, _) in events {
        push_plain_text(&mut alt, ev);
    }
    alt
}

fn markdown_options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_HEADING_ATTRIBUTES
        | Options::ENABLE_MATH
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
            Event::End(TagEnd::Heading(_)) if in_heading => {
                in_heading = false;
                let raw_id = explicit_id.take().unwrap_or_else(|| slugify(&text));
                let raw_id = if raw_id.is_empty() {
                    "section".to_owned()
                } else {
                    raw_id
                };
                headings.push(TocEntry {
                    level,
                    number: None,
                    id: raw_id,
                    title: std::mem::take(&mut text),
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

/// Converts math events into KaTeX-compatible HTML; passes other events through.
fn transform_math<'a>(event: Event<'a>, features: &mut BTreeSet<Feature>) -> Event<'a> {
    match event {
        Event::InlineMath(content) => {
            features.insert(Feature::Math);
            let html = format!(
                r#"<span class="math math-inline">\({}\)</span>"#,
                escape(&content)
            );
            Event::InlineHtml(html.into())
        }
        Event::DisplayMath(content) => {
            features.insert(Feature::Math);
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
    use std::sync::LazyLock;

    use indoc::indoc;
    use syntect::parsing::SyntaxSet;

    use super::*;

    static SYNTAX_SET: LazyLock<SyntaxSet> = LazyLock::new(two_face::syntax::extra_newlines);

    // Stub resolver for tests with no local images — `resolve` returns `None`.
    static EMPTY_RESOLVER: LazyLock<ImageResolver> = LazyLock::new(|| {
        ImageResolver::new(Path::new(""), crate::render::lqip::ImageConfig::default())
    });

    fn render(content: &str) -> MarkdownOutput {
        let mut features = BTreeSet::new();
        let document = prepare(content);
        render_markdown(
            document,
            &SYNTAX_SET,
            &HashMap::new(),
            &EMPTY_RESOLVER,
            None,
            MarkdownSettings::default(),
            &mut features,
        )
    }

    fn render_with_resolver(
        content: &str,
        resolver: &ImageResolver,
        base_dir: &Path,
    ) -> MarkdownOutput {
        let (cleaned, attrs) = crate::render::image_attrs::extract_image_attrs(content);
        let mut features = BTreeSet::new();
        let document = prepare(&cleaned);
        render_markdown(
            document,
            &SYNTAX_SET,
            &attrs,
            resolver,
            Some(base_dir),
            MarkdownSettings::default(),
            &mut features,
        )
    }

    fn prepare(content: &str) -> MarkdownDocument {
        let mut document = MarkdownDocument::parse(content);
        let mut ids = super::super::page_ids::PageIds::default();
        for heading in &mut document.headings {
            heading.id = ids.allocate(&heading.id);
        }
        document.footnotes.allocate_ids(&mut ids, 0);
        document
    }

    fn write_tiny_png(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let img = image::RgbaImage::from_pixel(8, 4, image::Rgba([200, 100, 50, 255]));
        img.save_with_format(path, image::ImageFormat::Png).unwrap();
    }

    // ── render_markdown: basic ──

    #[test]
    fn render_markdown_paragraph() {
        let out = render("Hello, world!");
        assert_eq!(out.html.trim(), "<p>Hello, world!</p>");
        assert_eq!(out.headings, Vec::<TocEntry>::new());
    }

    // ── render_markdown: headings ──

    #[test]
    fn render_markdown_heading_with_id() {
        let out = render("## Introduction");
        assert!(
            out.html.contains(r#"<h2 id="introduction">"#),
            "html:\n{}",
            out.html
        );
        assert_eq!(out.headings.len(), 1);
        assert_eq!(out.headings[0].level, HeadingLevel::H2);
        assert_eq!(out.headings[0].id, "introduction");
        assert_eq!(out.headings[0].title, "Introduction");
    }

    #[test]
    fn render_markdown_multiple_headings_toc() {
        let md = indoc! {"
            ## First

            ### Second

            ## Third
        "};
        let out = render(md);
        assert_eq!(out.headings.len(), 3);
        assert_eq!(out.headings[0].level, HeadingLevel::H2);
        assert_eq!(out.headings[0].title, "First");
        assert_eq!(out.headings[1].level, HeadingLevel::H3);
        assert_eq!(out.headings[1].title, "Second");
        assert_eq!(out.headings[2].level, HeadingLevel::H2);
        assert_eq!(out.headings[2].title, "Third");
    }

    #[test]
    fn render_markdown_heading_with_explicit_id() {
        let out = render("## Introduction {#custom-id}");
        assert!(
            out.html.contains(r#"id="custom-id""#),
            "should use explicit ID, html:\n{}",
            out.html
        );
        assert_eq!(out.headings[0].id, "custom-id");
    }

    #[test]
    fn render_markdown_heading_with_inline_code() {
        let out = render("## The `foo` function");
        assert!(
            out.html.contains("<code>foo</code>"),
            "should preserve inline formatting, html:\n{}",
            out.html
        );
        assert_eq!(out.headings[0].id, "the-foo-function");
    }

    #[test]
    fn render_markdown_heading_with_inline_math() {
        let out = render("## The $x^2$ equation");
        assert!(
            out.html
                .contains(r#"<span class="math math-inline">\(x^2\)</span>"#),
            "should contain KaTeX HTML in heading, html:\n{}",
            out.html
        );
        assert_eq!(out.headings[0].id, "the-x-2-equation");
        assert_eq!(out.headings[0].title, "The x^2 equation");
    }

    #[test]
    fn render_markdown_heading_with_display_math() {
        let out = render(r"## Sum $$\sum_{i=1}^n$$");
        assert!(
            out.html
                .contains(r#"<span class="math math-display">\[\sum_{i=1}^n\]</span>"#),
            "should contain KaTeX HTML in heading, html:\n{}",
            out.html
        );
        assert_eq!(out.headings[0].id, "sum-sum_-i-1-n");
        assert_eq!(out.headings[0].title, r"Sum \sum_{i=1}^n");
    }

    #[test]
    fn render_markdown_heading_with_link() {
        let out = render("## See [example](https://example.com)");
        assert_eq!(out.headings[0].id, "see-example");
        assert_eq!(out.headings[0].title, "See example");
        assert!(
            out.html.contains(r#"href="https://example.com""#),
            "link should be preserved in HTML, html:\n{}",
            out.html
        );
    }

    #[test]
    fn render_markdown_cjk_heading() {
        let out = render("## 测试文本");
        assert_eq!(out.headings[0].id, "测试文本");
        assert!(out.html.contains(r#"id="测试文本""#), "html:\n{}", out.html);
    }

    #[test]
    fn render_markdown_empty_heading_gets_fallback_id() {
        let out = render("##  \n");
        assert_eq!(out.headings[0].id, "section");
    }

    // ── render_markdown: GFM extensions ──

    #[test]
    fn render_markdown_gfm_table() {
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
            out.html.contains(expected),
            "should preserve table nesting and cell order, html:\n{}",
            out.html
        );
    }

    #[test]
    fn render_markdown_strikethrough() {
        let out = render("~~deleted~~");
        assert!(
            out.html.contains("<del>deleted</del>"),
            "html:\n{}",
            out.html
        );
    }

    #[test]
    fn render_markdown_tasklist() {
        let md = indoc! {"
            - [x] Done
            - [ ] Todo
        "};
        let out = render(md);

        let input_before = |label: &str| -> String {
            let pos = out
                .html
                .find(label)
                .unwrap_or_else(|| panic!("missing {label}"));
            let start = out.html[..pos]
                .rfind("<input")
                .unwrap_or_else(|| panic!("no input before {label}"));
            out.html[start..pos].to_owned()
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

    // ── render_markdown: footnotes ──

    #[test]
    fn render_markdown_footnotes_relocated_after_body() {
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
        let closing = out.html.find("Closing paragraph.").unwrap();
        let section = out
            .html
            .find(r#"<section class="footnotes""#)
            .expect("footnote section emitted");
        assert!(
            closing < section,
            "definition should move after the body, html:\n{}",
            out.html
        );
        assert!(
            out.html
                .contains(r##"<a href="#fn-1" role="doc-noteref">1</a>"##),
            "html:\n{}",
            out.html
        );
        let ids: Vec<_> = out.headings.iter().map(|h| h.id.as_str()).collect();
        assert_eq!(ids, ["before", "after", "note-heading"]);
    }

    #[test]
    fn render_markdown_footnotes_inline_markup_in_reference_paragraph() {
        let md = indoc! {"
            Math $x$[^1] and ![icon](a.png)[^1].

            [^1]: Note.
        "};
        let out = render(md);
        assert!(out.html.contains(r#"src="a.png""#));
        assert!(
            out.html.contains(r#"<span class="math math-inline">"#)
                && out.html.contains(r#"id="fnref-1-2""#),
            "html:\n{}",
            out.html
        );
    }

    #[test]
    fn render_markdown_footnotes_preserves_images_math_and_code_in_notes() {
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
        assert!(out.html.contains(r#"class="math math-inline""#));
        assert!(out.html.contains(r#"src="inline.png""#));
        assert!(out.html.contains(r#"src="block.png""#));
        assert!(out.html.contains("<figure"));
        let plain = render(indoc! {"
            ```text
            first
              second
            ```
        "});
        assert!(out.html.contains(plain.html.trim()), "{}", out.html);
        assert!(
            out.html
                .contains("</figure>\n      <p><a class=\"footnote-backref\"")
        );
    }

    #[test]
    fn render_markdown_footnote_syntax_interrupts_image_parsing() {
        let out = render(indoc! {"
            ![Alt[^a]](image.png)

            [^a]: Visible.
        "});
        assert!(out.html.starts_with("<p>![Alt<sup"));
        assert!(out.html.contains(r#"id="fnref-a-1""#));
        assert!(out.html.contains(r##"href="#fnref-a-1""##));
        assert!(!out.html.contains("<img"));
    }

    // ── render_markdown: math ──

    #[test]
    fn render_markdown_inline_math() {
        let out = render("$x^2$");
        assert!(
            out.html
                .contains(r#"<span class="math math-inline">\(x^2\)</span>"#),
            "html:\n{}",
            out.html
        );
    }

    #[test]
    fn render_markdown_inline_math_with_underscores() {
        let out = render("The matrix $a_{ij}$ is symmetric.");
        assert!(
            out.html.contains("a_{ij}"),
            "underscores in inline math preserved, html:\n{}",
            out.html
        );
    }

    #[test]
    fn render_markdown_display_math() {
        let out = render("$$E=mc^2$$");
        assert!(
            out.html
                .contains(r#"<span class="math math-display">\[E=mc^2\]</span>"#),
            "html:\n{}",
            out.html
        );
    }

    #[test]
    fn render_markdown_display_math_with_underscores() {
        let out = render("$$a_{ij} + b_{ij}$$");
        assert!(
            out.html.contains("a_{ij} + b_{ij}"),
            "underscores in math should not become emphasis, html:\n{}",
            out.html
        );
        assert!(
            !out.html.contains("<em>"),
            "no emphasis tags inside math, html:\n{}",
            out.html
        );
    }

    #[test]
    fn render_markdown_math_with_html_chars() {
        let out = render("$x < y$");
        assert!(
            out.html.contains(r"\(x &lt; y\)"),
            "math content should be HTML-escaped, html:\n{}",
            out.html
        );
    }

    // ── render_markdown: code blocks ──

    #[test]
    fn render_markdown_code_block() {
        let md = indoc! {"
            ```
            fn main() {}
            ```
        "};
        let out = render(md);
        assert!(
            out.html.contains(r#"class="highlight""#),
            "no-lang code block should still have highlight wrapper, html:\n{}",
            out.html
        );
        assert!(
            out.html.contains(r#"data-lang="plaintext""#),
            "no-lang code block should normalize to plaintext, html:\n{}",
            out.html
        );
    }

    #[test]
    fn render_markdown_indented_code_block() {
        let md = "    fn main() {}\n";
        let out = render(md);
        assert!(
            out.html.contains(r#"class="highlight""#),
            "indented code block should have highlight wrapper, html:\n{}",
            out.html
        );
        assert!(
            out.html.contains(r#"data-lang="plaintext""#),
            "indented code block should normalize to plaintext, html:\n{}",
            out.html
        );
    }

    #[test]
    fn render_markdown_code_block_with_language() {
        let md = indoc! {"
            ```rust
            fn main() {}
            ```
        "};
        let out = render(md);
        assert!(
            out.html.contains(r#"class="highlight""#),
            "should have highlight wrapper, html:\n{}",
            out.html
        );
        assert!(
            out.html.contains(r#"data-lang="rust""#),
            "should have data-lang attribute, html:\n{}",
            out.html
        );
        assert!(
            out.html.contains("<span class="),
            "should contain highlighted spans, html:\n{}",
            out.html
        );
    }

    #[test]
    fn render_markdown_code_block_info_string_metadata() {
        let md = indoc! {"
            ```rust no_run
            fn main() {}
            ```
        "};
        let out = render(md);
        assert!(
            out.html.contains(r#"data-lang="rust""#),
            "should extract language from info string, html:\n{}",
            out.html
        );
        assert!(
            !out.html.contains("no_run"),
            "metadata after language should be stripped, html:\n{}",
            out.html
        );
        assert!(
            out.html.contains("<span class="),
            "should contain highlighted spans, html:\n{}",
            out.html
        );
    }

    #[test]
    fn render_markdown_code_block_mermaid_emits_bare_pre() {
        let md = indoc! {"
            ```mermaid
            graph TD
            A --> B
            ```
        "};
        let out = render(md);
        assert!(
            out.html.contains(r#"<pre class="mermaid""#),
            "should emit bare pre.mermaid, html:\n{}",
            out.html
        );
        assert!(
            !out.html.contains(r#"class="code-block""#),
            "mermaid block should bypass the code-block chrome, html:\n{}",
            out.html
        );
        assert!(
            out.html.contains(indoc! {r#"
                data-source="graph TD
                A --&gt; B
                ""#
            }),
            "should preserve the DSL verbatim in data-source for theme-toggle re-render, html:\n{}",
            out.html
        );
    }

    #[test]
    fn render_markdown_code_block_mermaid_case_insensitive() {
        let md = indoc! {"
            ```Mermaid
            graph TD
            ```
        "};
        let out = render(md);
        assert!(
            out.html.contains(r#"<pre class="mermaid""#),
            "case-insensitive language tag should still emit pre.mermaid, html:\n{}",
            out.html
        );
    }

    // ── render_markdown: images ──

    #[test]
    fn render_markdown_block_image() {
        let md = "![A photo](img.png)\n";
        let out = render(md);
        assert!(
            out.html.contains("<figure>"),
            "should become a figure, html:\n{}",
            out.html
        );
        assert!(
            out.html.contains(r#"alt="A photo""#),
            "should have alt attribute, html:\n{}",
            out.html
        );
        assert!(
            out.html.contains("<figcaption>A photo</figcaption>"),
            "should have figcaption with alt text, html:\n{}",
            out.html
        );
    }

    #[test]
    fn render_markdown_block_image_with_title() {
        let md = "![alt text](img.png \"My Title\")\n";
        let out = render(md);
        assert!(
            out.html.contains("<figure>"),
            "should become a figure, html:\n{}",
            out.html
        );
        assert!(
            out.html.contains(r#"alt="alt text""#),
            "should have alt attribute, html:\n{}",
            out.html
        );
        assert!(
            out.html.contains(r#"title="My Title""#),
            "should have title attribute, html:\n{}",
            out.html
        );
        assert!(
            out.html.contains("<figcaption>alt text</figcaption>"),
            "should have figcaption with alt text, html:\n{}",
            out.html
        );
    }

    #[test]
    fn render_markdown_block_image_with_formatted_alt() {
        let md = "![*bold* alt](img.png)\n";
        let out = render(md);
        assert!(
            out.html.contains("<figure>"),
            "should become a figure, html:\n{}",
            out.html
        );
        assert!(
            out.html.contains(r#"alt="bold alt""#),
            "should have plain-text alt attribute, html:\n{}",
            out.html
        );
        assert!(
            out.html.contains("<figcaption>bold alt</figcaption>"),
            "should have figcaption with plain text from formatted alt, html:\n{}",
            out.html
        );
    }

    #[test]
    fn render_markdown_block_image_with_soft_break_in_alt() {
        let md = indoc! {"
            ![line1
            line2](img.png)
        "};
        let out = render(md);
        assert!(
            out.html.contains("<figure>"),
            "should become a figure, html:\n{}",
            out.html
        );
        assert!(
            out.html.contains(r#"alt="line1 line2""#),
            "soft break in alt attribute should become a space, html:\n{}",
            out.html
        );
        assert!(
            out.html.contains("<figcaption>line1 line2</figcaption>"),
            "soft break in figcaption should become a space, html:\n{}",
            out.html
        );
    }

    #[test]
    fn render_markdown_inline_image() {
        let md = "Text ![icon](icon.png) more text\n";
        let out = render(md);
        assert!(
            !out.html.contains("<figure>"),
            "should not become a figure, html:\n{}",
            out.html
        );
        assert!(
            out.html.contains("<img "),
            "should have img tag, html:\n{}",
            out.html
        );
        assert!(
            out.html.contains(r#"alt="icon""#),
            "should have alt attribute, html:\n{}",
            out.html
        );
    }

    #[test]
    fn render_markdown_image_with_trailing_text_stays_inline() {
        let md = "![icon](icon.png) followed by text\n";
        let out = render(md);
        assert!(
            !out.html.contains("<figure>"),
            "image with trailing text should not become a figure, html:\n{}",
            out.html
        );
        assert!(
            out.html.contains("<img "),
            "should have img tag, html:\n{}",
            out.html
        );
    }

    #[test]
    fn render_markdown_multiple_images_stay_inline() {
        let md = "![a](a.png) ![b](b.png)\n";
        let out = render(md);
        assert!(
            !out.html.contains("<figure>"),
            "multiple images should not become figures, html:\n{}",
            out.html
        );
        assert!(
            out.html.contains(r#"alt="a""#),
            "first image should be present, html:\n{}",
            out.html
        );
        assert!(
            out.html.contains(r#"alt="b""#),
            "second image should be present, html:\n{}",
            out.html
        );
    }

    // ── render_markdown: image resolution ──

    #[test]
    fn render_markdown_resolver_stamps_dimensions_and_lqip_on_block_image() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = dir.path().join("bundle");
        write_tiny_png(&bundle.join("img.png"));

        let resolver = ImageResolver::new(dir.path(), crate::render::lqip::ImageConfig::default());
        let out = render_with_resolver("![alt](img.png)\n", &resolver, &bundle);

        assert!(out.html.contains(r#"width="8""#), "html:\n{}", out.html);
        assert!(out.html.contains(r#"height="4""#), "html:\n{}", out.html);

        // Pin the nesting order so a transposition (e.g. wrapper outside the
        // figure, or img outside the wrapper) trips the test.
        let figure = out.html.find("<figure>").expect("figure opens");
        let wrapper = out
            .html
            .find(r#"<span class="lqip" style="--lqip-uri:url('data:image/webp;base64,"#)
            .expect("wrapper opens");
        let img = out.html.find("<img ").expect("img tag");
        let wrapper_close = out.html.find("</span>").expect("wrapper closes");
        let figure_close = out.html.find("</figure>").expect("figure closes");
        assert!(
            figure < wrapper
                && wrapper < img
                && img < wrapper_close
                && wrapper_close < figure_close,
            "expected <figure> > <span.lqip> > <img> > </span> > </figure>, html:\n{}",
            out.html,
        );
    }

    #[test]
    fn render_markdown_resolver_merges_with_authored_attrs_on_inline_image() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = dir.path().join("bundle");
        write_tiny_png(&bundle.join("img.png"));

        let resolver = ImageResolver::new(dir.path(), crate::render::lqip::ImageConfig::default());
        let out =
            render_with_resolver("![a](img.png){width=4} ![b](img.png)\n", &resolver, &bundle);

        let fragment = scraper::Html::parse_fragment(&out.html);
        let images = scraper::Selector::parse("img").unwrap();
        let attrs: Vec<_> = fragment
            .select(&images)
            .map(|image| {
                (
                    image.value().attr("src"),
                    image.value().attr("alt"),
                    image.value().attr("width"),
                    image.value().attr("height"),
                )
            })
            .collect();

        assert_eq!(
            attrs,
            vec![
                (Some("img.png"), Some("a"), Some("4"), Some("2")),
                (Some("img.png"), Some("b"), Some("8"), Some("4")),
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
    fn render_markdown_resolver_miss_emits_remote_image_without_dimensions() {
        let dir = tempfile::tempdir().unwrap();
        let resolver = ImageResolver::new(dir.path(), crate::render::lqip::ImageConfig::default());
        let out = render_with_resolver(
            "![remote](https://cdn.example.com/x.png)\n",
            &resolver,
            dir.path(),
        );

        assert!(out.html.contains(r#"src="https://cdn.example.com/x.png""#));
        assert!(!out.html.contains("width="), "html:\n{}", out.html);
        assert!(!out.html.contains(r#"class="lqip""#), "html:\n{}", out.html);
    }
}
