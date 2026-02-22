use std::collections::HashSet;

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use syntect::parsing::SyntaxSet;

use super::escape_html;
use super::highlight::highlight_code;
use super::image::{render_block_image, render_inline_image};
use super::toc::TocEntry;

/// The result of rendering markdown content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownOutput {
    /// The rendered HTML string.
    pub html: String,
    /// Table of contents entries collected from headings.
    pub headings: Vec<TocEntry>,
}

/// Renders markdown content to HTML with GFM extensions, math support, syntax
/// highlighting, and image enhancement.
///
/// - Headings receive auto-generated `id` attributes (CJK-aware slugification)
///   and are collected into `headings` for table of contents generation.
///   Explicit heading IDs (`## Foo {#bar}`) are respected when present.
/// - Math events are rendered as KaTeX-compatible `<span>` elements.
/// - Fenced code blocks with a language tag receive syntect CSS-class
///   highlighting with line numbers.
/// - Block images (sole image in a paragraph) are wrapped in `<figure>`
///   elements.
#[must_use]
pub fn render_markdown(content: &str, syntax_set: &SyntaxSet) -> MarkdownOutput {
    let options = markdown_options();

    // Pass 1: collect heading metadata (text, level, IDs).
    let headings = collect_headings(content, options);

    // Pass 2: transform events through a manual loop for N:1 buffering.
    let parser = Parser::new_ext(content, options);
    let mut output_events: Vec<Event<'_>> = Vec::new();

    let mut heading_index: usize = 0;
    let mut in_code_block = false;
    let mut code_lang: Option<String> = None;
    let mut code_buf = String::new();
    let mut para_buf: Vec<Event<'_>> = Vec::new();
    let mut in_para = false;

    for event in parser {
        match event {
            // -- Headings --
            Event::Start(Tag::Heading { .. }) => {
                let entry = &headings[heading_index];
                heading_index += 1;
                output_events.push(Event::Html(
                    format!("<{} id=\"{}\">", entry.level, escape_html(&entry.id)).into(),
                ));
            }
            Event::End(TagEnd::Heading(level)) => {
                output_events.push(Event::Html(format!("</{level}>\n").into()));
            }

            // -- Code blocks: buffer content, highlight on End --
            Event::Start(Tag::CodeBlock(kind)) => {
                in_code_block = true;
                code_lang = match kind {
                    // Info strings can contain metadata after the language
                    // token (e.g., "rust no_run"); extract just the first word.
                    CodeBlockKind::Fenced(lang) => lang
                        .split_ascii_whitespace()
                        .next()
                        .filter(|s| !s.is_empty())
                        .map(String::from),
                    CodeBlockKind::Indented => None,
                };
                code_buf.clear();
            }
            Event::End(TagEnd::CodeBlock) => {
                in_code_block = false;
                let lang = code_lang.take().unwrap_or_default();
                let html = highlight_code(syntax_set, &lang, &code_buf);
                output_events.push(Event::Html(html.into()));
                code_buf.clear();
            }
            Event::Text(ref t) if in_code_block => {
                code_buf.push_str(t);
            }

            // -- Paragraphs: buffer to detect sole-image blocks --
            Event::Start(Tag::Paragraph) => {
                in_para = true;
                para_buf.clear();
            }
            Event::End(TagEnd::Paragraph) => {
                in_para = false;
                if let Some(html) = try_render_block_image(&para_buf) {
                    output_events.push(Event::Html(html.into()));
                } else {
                    output_events.push(Event::Html("<p>".into()));
                    flush_paragraph(&para_buf, &mut output_events);
                    output_events.push(Event::Html("</p>\n".into()));
                }
                para_buf.clear();
            }
            _ if in_para => {
                para_buf.push(event);
            }

            // -- Everything else (math, etc.) --
            other => {
                output_events.push(transform_math(other));
            }
        }
    }

    let mut html = String::new();
    pulldown_cmark::html::push_html(&mut html, output_events.into_iter());

    MarkdownOutput { html, headings }
}

/// Checks if a paragraph's buffered events represent a sole image (block image).
///
/// Pattern: `Start(Image)`, any inner events (alt text, formatting), `End(Image)`,
/// with no other images in the paragraph.
fn try_render_block_image(events: &[Event<'_>]) -> Option<String> {
    let (src, title) = match events.first()? {
        Event::Start(Tag::Image {
            dest_url, title, ..
        }) => (dest_url.to_string(), title.to_string()),
        _ => return None,
    };

    if !matches!(events.last()?, Event::End(TagEnd::Image)) {
        return None;
    }

    let inner = &events[1..events.len() - 1];

    // Reject multiple images in the same paragraph.
    if inner.iter().any(|ev| {
        matches!(
            ev,
            Event::Start(Tag::Image { .. }) | Event::End(TagEnd::Image)
        )
    }) {
        return None;
    }

    let alt = extract_alt_text(inner);
    Some(render_block_image(&src, &alt, &title))
}

/// Flushes buffered paragraph events, replacing inline image sequences with
/// `render_inline_image()` output while passing other events through.
fn flush_paragraph<'a>(events: &[Event<'a>], output: &mut Vec<Event<'a>>) {
    let mut i = 0;
    while i < events.len() {
        if let Event::Start(Tag::Image {
            dest_url, title, ..
        }) = &events[i]
        {
            let src = dest_url.to_string();
            let title = title.to_string();

            // Collect inner events up to End(Image) for alt text extraction.
            let inner_start = i + 1;
            i = inner_start;
            while i < events.len() && !matches!(events[i], Event::End(TagEnd::Image)) {
                i += 1;
            }
            let alt = extract_alt_text(&events[inner_start..i]);
            if i < events.len() {
                i += 1; // skip End(Image)
            }

            output.push(Event::Html(render_inline_image(&src, &alt, &title).into()));
        } else {
            output.push(transform_math(events[i].clone()));
            i += 1;
        }
    }
}

/// Extracts plain text from image inner events for use as alt text.
///
/// Collects text content while skipping inline formatting containers
/// (emphasis, strong, etc.), since alt text is plain text.
fn extract_alt_text(events: &[Event<'_>]) -> String {
    let mut alt = String::new();
    for ev in events {
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

/// Scans the markdown for headings, collecting their level, plain text, and
/// generating unique slugified IDs.
fn collect_headings(content: &str, options: Options) -> Vec<TocEntry> {
    let parser = Parser::new_ext(content, options);
    let mut headings = Vec::new();
    let mut used_ids = HashSet::new();

    let mut level = HeadingLevel::H1;
    let mut explicit_id: Option<String> = None;
    let mut text = String::new();
    let mut in_heading = false;

    for event in parser {
        match event {
            Event::Start(Tag::Heading {
                level: l, id: eid, ..
            }) => {
                level = l;
                explicit_id = eid.map(|s| s.to_string());
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
                let id = deduplicate_id(&mut used_ids, &raw_id);
                headings.push(TocEntry {
                    level,
                    id,
                    title: std::mem::take(&mut text),
                });
            }
            _ if in_heading => push_plain_text(&mut text, &event),
            _ => {}
        }
    }

    headings
}

/// Accumulates plain-text content from an event into `buf`.
///
/// Handles text-bearing variants (`Text`, `Code`, `InlineMath`, `DisplayMath`)
/// and converts soft / hard breaks to spaces.
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
fn transform_math(event: Event<'_>) -> Event<'_> {
    match event {
        Event::InlineMath(content) => Event::InlineHtml(
            format!(
                "<span class=\"math math-inline\">\\({}\\)</span>",
                escape_html(&content)
            )
            .into(),
        ),
        Event::DisplayMath(content) => Event::Html(
            format!(
                "<span class=\"math math-display\">\\[{}\\]</span>\n",
                escape_html(&content)
            )
            .into(),
        ),
        other => other,
    }
}

/// Generates a URL-safe slug from heading text.
///
/// - Lowercases ASCII characters
/// - Preserves non-ASCII alphanumeric characters (CJK, accented letters)
/// - Replaces non-alphanumeric characters with `-`
/// - Collapses consecutive `-` and strips leading / trailing `-`
pub(crate) fn slugify(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut prev_dash = true; // strip leading dashes

    for ch in text.chars() {
        if ch.is_alphanumeric() {
            result.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash {
            result.push('-');
            prev_dash = true;
        }
    }

    if result.ends_with('-') {
        result.pop();
    }

    result
}

/// Appends a numeric suffix to make `id` unique within the set of already-used IDs.
///
/// First occurrence → unchanged. Second → `-1`. Third → `-2`.
///
/// Uses `used` to detect collisions between suffixed and natural IDs
/// (e.g., heading "Foo", then "Foo-1", then "Foo" again → "Foo", "Foo-1", "Foo-2").
fn deduplicate_id(used: &mut HashSet<String>, id: &str) -> String {
    if used.insert(id.to_owned()) {
        return id.to_owned();
    }
    let mut n = 1;
    loop {
        let candidate = format!("{id}-{n}");
        n += 1;
        if used.insert(candidate.clone()) {
            return candidate;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::LazyLock;

    use indoc::indoc;
    use syntect::parsing::SyntaxSet;

    use super::*;

    static SYNTAX_SET: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_newlines);

    // -- slugify --

    #[test]
    fn slugify_ascii() {
        assert_eq!(slugify("Hello World"), "hello-world");
    }

    #[test]
    fn slugify_numbers() {
        assert_eq!(slugify("123"), "123");
    }

    #[test]
    fn slugify_cjk() {
        assert_eq!(slugify("你好世界"), "你好世界");
    }

    #[test]
    fn slugify_accented_latin() {
        assert_eq!(slugify("Café Résumé"), "café-résumé");
    }

    #[test]
    fn slugify_mixed() {
        assert_eq!(slugify("1.1 Foobar - 测试文本"), "1-1-foobar-测试文本");
    }

    #[test]
    fn slugify_collapses_dashes() {
        assert_eq!(slugify("a - - b"), "a-b");
    }

    #[test]
    fn slugify_strips_leading_trailing() {
        assert_eq!(slugify(" hello "), "hello");
    }

    #[test]
    fn slugify_empty() {
        assert_eq!(slugify(""), "");
    }

    #[test]
    fn slugify_only_punctuation() {
        assert_eq!(slugify("..."), "");
    }

    // -- deduplicate_id --

    #[test]
    fn dedup_first_use_unchanged() {
        let mut used = HashSet::new();
        assert_eq!(deduplicate_id(&mut used, "foo"), "foo");
    }

    #[test]
    fn dedup_second_use_gets_suffix_1() {
        let mut used = HashSet::new();
        deduplicate_id(&mut used, "foo");
        assert_eq!(deduplicate_id(&mut used, "foo"), "foo-1");
    }

    #[test]
    fn dedup_third_use_gets_suffix_2() {
        let mut used = HashSet::new();
        deduplicate_id(&mut used, "foo");
        deduplicate_id(&mut used, "foo");
        assert_eq!(deduplicate_id(&mut used, "foo"), "foo-2");
    }

    #[test]
    fn dedup_avoids_collision() {
        let mut used = HashSet::new();
        assert_eq!(deduplicate_id(&mut used, "foo"), "foo");
        assert_eq!(deduplicate_id(&mut used, "foo-1"), "foo-1");
        assert_eq!(deduplicate_id(&mut used, "foo"), "foo-2");
        assert_eq!(deduplicate_id(&mut used, "foo-2"), "foo-2-1");
    }

    // -- render_markdown: basic --

    #[test]
    fn render_paragraph() {
        let out = render_markdown("Hello, world!", &SYNTAX_SET);
        assert_eq!(out.html.trim(), "<p>Hello, world!</p>");
        assert!(out.headings.is_empty());
    }

    // -- render_markdown: headings --

    #[test]
    fn render_heading_with_id() {
        let out = render_markdown("## Introduction", &SYNTAX_SET);
        assert!(
            out.html.contains(r#"<h2 id="introduction">"#),
            "html:\n{}",
            out.html
        );
        assert_eq!(out.headings.len(), 1);
        assert_eq!(out.headings[0].id, "introduction");
        assert_eq!(out.headings[0].level, HeadingLevel::H2);
        assert_eq!(out.headings[0].title, "Introduction");
    }

    #[test]
    fn render_heading_with_explicit_id() {
        let out = render_markdown("## Introduction {#custom-id}", &SYNTAX_SET);
        assert!(
            out.html.contains(r#"id="custom-id""#),
            "should use explicit ID, html:\n{}",
            out.html
        );
        assert_eq!(out.headings[0].id, "custom-id");
    }

    #[test]
    fn render_heading_with_inline_code() {
        let out = render_markdown("## The `foo` function", &SYNTAX_SET);
        assert!(
            out.html.contains("<code>foo</code>"),
            "should preserve inline formatting, html:\n{}",
            out.html
        );
        assert_eq!(out.headings[0].id, "the-foo-function");
    }

    #[test]
    fn render_heading_with_math() {
        let out = render_markdown("## The $x^2$ equation", &SYNTAX_SET);
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
    fn render_heading_with_link() {
        let out = render_markdown("## See [example](https://example.com)", &SYNTAX_SET);
        assert_eq!(out.headings[0].title, "See example");
        assert_eq!(out.headings[0].id, "see-example");
        assert!(
            out.html.contains(r#"href="https://example.com""#),
            "link should be preserved in HTML, html:\n{}",
            out.html
        );
    }

    #[test]
    fn render_cjk_heading() {
        let out = render_markdown("## 测试文本", &SYNTAX_SET);
        assert_eq!(out.headings[0].id, "测试文本");
        assert!(out.html.contains(r#"id="测试文本""#), "html:\n{}", out.html);
    }

    #[test]
    fn render_empty_heading_gets_fallback_id() {
        let out = render_markdown("##  \n", &SYNTAX_SET);
        assert_eq!(out.headings[0].id, "section");
    }

    #[test]
    fn render_multiple_headings_toc() {
        let md = indoc! {"
            ## First

            ### Second

            ## Third
        "};
        let out = render_markdown(md, &SYNTAX_SET);
        assert_eq!(out.headings.len(), 3);
        assert_eq!(out.headings[0].title, "First");
        assert_eq!(out.headings[0].level, HeadingLevel::H2);
        assert_eq!(out.headings[1].title, "Second");
        assert_eq!(out.headings[1].level, HeadingLevel::H3);
        assert_eq!(out.headings[2].title, "Third");
        assert_eq!(out.headings[2].level, HeadingLevel::H2);
    }

    #[test]
    fn render_duplicate_headings_dedup() {
        let md = indoc! {"
            ## Foo

            ## Foo

            ## Foo
        "};
        let out = render_markdown(md, &SYNTAX_SET);
        assert_eq!(out.headings[0].id, "foo");
        assert_eq!(out.headings[1].id, "foo-1");
        assert_eq!(out.headings[2].id, "foo-2");
    }

    // -- render_markdown: GFM extensions --

    #[test]
    fn render_gfm_table() {
        let md = indoc! {"
            | A | B |
            |---|---|
            | 1 | 2 |
        "};
        let out = render_markdown(md, &SYNTAX_SET);
        assert!(
            out.html.contains("<table>"),
            "should have table, html:\n{}",
            out.html
        );
        assert!(
            out.html.contains("<thead>"),
            "should have thead, html:\n{}",
            out.html
        );
        assert!(
            out.html.contains("<th>A</th>"),
            "should have header cells, html:\n{}",
            out.html
        );
        assert!(
            out.html.contains("<td>1</td>"),
            "should have data cells, html:\n{}",
            out.html
        );
    }

    #[test]
    fn render_strikethrough() {
        let out = render_markdown("~~deleted~~", &SYNTAX_SET);
        assert!(
            out.html.contains("<del>deleted</del>"),
            "html:\n{}",
            out.html
        );
    }

    #[test]
    fn render_tasklist() {
        let md = indoc! {"
            - [x] Done
            - [ ] Todo
        "};
        let out = render_markdown(md, &SYNTAX_SET);

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

    #[test]
    fn render_footnotes() {
        let md = indoc! {"
            Text[^1].

            [^1]: Footnote content.
        "};
        let out = render_markdown(md, &SYNTAX_SET);
        assert!(
            out.html.contains(r##"<a href="#1">"##),
            "should link to footnote definition, html:\n{}",
            out.html
        );
        assert!(
            out.html
                .contains(r#"<div class="footnote-definition" id="1">"#),
            "should have footnote definition with matching id, html:\n{}",
            out.html
        );
        assert!(
            out.html.contains("Footnote content."),
            "should include footnote body, html:\n{}",
            out.html
        );
    }

    // -- render_markdown: math --

    #[test]
    fn render_inline_math() {
        let out = render_markdown("$x^2$", &SYNTAX_SET);
        assert!(
            out.html
                .contains(r#"<span class="math math-inline">\(x^2\)</span>"#),
            "html:\n{}",
            out.html
        );
    }

    #[test]
    fn render_display_math() {
        let out = render_markdown("$$E=mc^2$$", &SYNTAX_SET);
        assert!(
            out.html
                .contains(r#"<span class="math math-display">\[E=mc^2\]</span>"#),
            "html:\n{}",
            out.html
        );
    }

    #[test]
    fn render_math_with_html_chars() {
        let out = render_markdown("$x < y$", &SYNTAX_SET);
        assert!(
            out.html.contains("\\(x &lt; y\\)"),
            "math content should be HTML-escaped, html:\n{}",
            out.html
        );
    }

    #[test]
    fn render_inline_math_with_underscores() {
        let out = render_markdown("The matrix $a_{ij}$ is symmetric.", &SYNTAX_SET);
        assert!(
            out.html.contains("a_{ij}"),
            "underscores in inline math preserved, html:\n{}",
            out.html
        );
    }

    #[test]
    fn render_display_math_with_underscores() {
        let out = render_markdown("$$a_{ij} + b_{ij}$$", &SYNTAX_SET);
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

    // -- render_markdown: code blocks --

    #[test]
    fn render_code_block() {
        let md = indoc! {"
            ```
            fn main() {}
            ```
        "};
        let out = render_markdown(md, &SYNTAX_SET);
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
    fn render_code_block_with_language() {
        let md = indoc! {"
            ```rust
            fn main() {}
            ```
        "};
        let out = render_markdown(md, &SYNTAX_SET);
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
    fn render_code_block_info_string_metadata() {
        let md = indoc! {"
            ```rust no_run
            fn main() {}
            ```
        "};
        let out = render_markdown(md, &SYNTAX_SET);
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
    fn render_indented_code_block() {
        let md = "    fn main() {}\n";
        let out = render_markdown(md, &SYNTAX_SET);
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

    // -- render_markdown: images --

    #[test]
    fn render_block_image() {
        let md = "![A photo](img.png)\n";
        let out = render_markdown(md, &SYNTAX_SET);
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
    fn render_block_image_with_title() {
        let md = "![alt text](img.png \"My Title\")\n";
        let out = render_markdown(md, &SYNTAX_SET);
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
    fn render_block_image_with_formatted_alt() {
        let md = "![*bold* alt](img.png)\n";
        let out = render_markdown(md, &SYNTAX_SET);
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
    fn render_block_image_with_soft_break_in_alt() {
        let md = "![line1\nline2](img.png)\n";
        let out = render_markdown(md, &SYNTAX_SET);
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
    fn render_inline_image() {
        let md = "Text ![icon](icon.png) more text\n";
        let out = render_markdown(md, &SYNTAX_SET);
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
    fn render_multiple_images_stay_inline() {
        let md = "![a](a.png) ![b](b.png)\n";
        let out = render_markdown(md, &SYNTAX_SET);
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
}
