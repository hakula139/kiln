use std::collections::HashSet;

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use super::escape_html;
use super::toc::TocEntry;

/// The result of rendering markdown content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownOutput {
    /// The rendered HTML string.
    pub html: String,
    /// Table of contents entries collected from headings.
    pub headings: Vec<TocEntry>,
}

/// Renders markdown content to HTML with GFM extensions and math support.
///
/// Headings receive auto-generated `id` attributes (CJK-aware slugification)
/// and are collected into `headings` for table of contents generation. Explicit
/// heading IDs (`## Foo {#bar}`) are respected when present. Math events are
/// rendered as KaTeX-compatible `<span>` elements with `\(...\)` / `\[...\]`
/// delimiters.
#[must_use]
pub fn render_markdown(content: &str) -> MarkdownOutput {
    let options = markdown_options();

    // Pass 1: collect heading metadata (text, level, IDs).
    let headings = collect_headings(content, options);

    // Pass 2: stream events through push_html, replacing heading tags and math.
    let mut h = 0;
    let events = Parser::new_ext(content, options).map(|event| match event {
        Event::Start(Tag::Heading { .. }) => {
            let entry = &headings[h];
            h += 1;
            Event::Html(format!("<{} id=\"{}\">", entry.level, escape_html(&entry.id)).into())
        }
        Event::End(TagEnd::Heading(level)) => Event::Html(format!("</{level}>\n").into()),
        other => transform_math(other),
    });

    let mut html = String::new();
    pulldown_cmark::html::push_html(&mut html, events);

    MarkdownOutput { html, headings }
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
            _ if in_heading => push_heading_text(&mut text, &event),
            _ => {}
        }
    }

    headings
}

/// Accumulates plain-text content from a heading's inner events for slug generation.
fn push_heading_text(buf: &mut String, event: &Event) {
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
    use indoc::indoc;

    use super::*;

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
        let out = render_markdown("Hello, world!");
        assert_eq!(out.html.trim(), "<p>Hello, world!</p>");
        assert!(out.headings.is_empty());
    }

    // -- render_markdown: headings --

    #[test]
    fn render_heading_with_id() {
        let out = render_markdown("## Introduction");
        assert!(
            out.html.contains("<h2 id=\"introduction\">"),
            "html: {}",
            out.html
        );
        assert_eq!(out.headings.len(), 1);
        assert_eq!(out.headings[0].id, "introduction");
        assert_eq!(out.headings[0].level, HeadingLevel::H2);
        assert_eq!(out.headings[0].title, "Introduction");
    }

    #[test]
    fn render_heading_with_explicit_id() {
        let out = render_markdown("## Introduction {#custom-id}");
        assert!(
            out.html.contains("id=\"custom-id\""),
            "should use explicit ID, html: {}",
            out.html
        );
        assert_eq!(out.headings[0].id, "custom-id");
    }

    #[test]
    fn render_heading_with_inline_code() {
        let out = render_markdown("## The `foo` function");
        assert!(
            out.html.contains("<code>foo</code>"),
            "should preserve inline formatting, html: {}",
            out.html
        );
        assert_eq!(out.headings[0].id, "the-foo-function");
    }

    #[test]
    fn render_heading_with_math() {
        let out = render_markdown("## The $x^2$ equation");
        assert!(
            out.html
                .contains("<span class=\"math math-inline\">\\(x^2\\)</span>"),
            "should contain KaTeX HTML in heading, html: {}",
            out.html
        );
        assert_eq!(out.headings[0].id, "the-x-2-equation");
        assert_eq!(out.headings[0].title, "The x^2 equation");
    }

    #[test]
    fn render_heading_with_link() {
        let out = render_markdown("## See [example](https://example.com)");
        assert_eq!(out.headings[0].title, "See example");
        assert_eq!(out.headings[0].id, "see-example");
        assert!(
            out.html.contains("href=\"https://example.com\""),
            "link should be preserved in HTML, html: {}",
            out.html
        );
    }

    #[test]
    fn render_cjk_heading() {
        let out = render_markdown("## 测试文本");
        assert_eq!(out.headings[0].id, "测试文本");
        assert!(out.html.contains("id=\"测试文本\""), "html: {}", out.html);
    }

    #[test]
    fn render_empty_heading_gets_fallback_id() {
        // A heading with only whitespace produces an empty slug → fallback "section".
        let out = render_markdown("##  \n");
        assert_eq!(out.headings[0].id, "section");
    }

    #[test]
    fn render_multiple_headings_toc() {
        let md = indoc! {"
            ## First

            ### Second

            ## Third
        "};
        let out = render_markdown(md);
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
        let out = render_markdown(md);
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
        let out = render_markdown(md);
        assert!(
            out.html.contains("<table>"),
            "should have table, html: {}",
            out.html
        );
        assert!(
            out.html.contains("<thead>"),
            "should have thead, html: {}",
            out.html
        );
        assert!(
            out.html.contains("<th>A</th>"),
            "should have header cells, html: {}",
            out.html
        );
        assert!(
            out.html.contains("<td>1</td>"),
            "should have data cells, html: {}",
            out.html
        );
    }

    #[test]
    fn render_strikethrough() {
        let out = render_markdown("~~deleted~~");
        assert!(
            out.html.contains("<del>deleted</del>"),
            "html: {}",
            out.html
        );
    }

    #[test]
    fn render_tasklist() {
        let md = indoc! {"
            - [x] Done
            - [ ] Todo
        "};
        let out = render_markdown(md);

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
        let out = render_markdown(md);
        assert!(
            out.html.contains("<a href=\"#1\">"),
            "should link to footnote definition, html: {}",
            out.html
        );
        assert!(
            out.html
                .contains("<div class=\"footnote-definition\" id=\"1\">"),
            "should have footnote definition with matching id, html: {}",
            out.html
        );
        assert!(
            out.html.contains("Footnote content."),
            "should include footnote body, html: {}",
            out.html
        );
    }

    // -- render_markdown: math --

    #[test]
    fn render_inline_math() {
        let out = render_markdown("$x^2$");
        assert!(
            out.html
                .contains("<span class=\"math math-inline\">\\(x^2\\)</span>"),
            "html: {}",
            out.html
        );
    }

    #[test]
    fn render_display_math() {
        let out = render_markdown("$$E=mc^2$$");
        assert!(
            out.html
                .contains("<span class=\"math math-display\">\\[E=mc^2\\]</span>"),
            "html: {}",
            out.html
        );
    }

    #[test]
    fn render_math_with_html_chars() {
        let out = render_markdown("$x < y$");
        assert!(
            out.html.contains("\\(x &lt; y\\)"),
            "math content should be HTML-escaped, html: {}",
            out.html
        );
    }

    #[test]
    fn render_inline_math_with_underscores() {
        let out = render_markdown("The matrix $a_{ij}$ is symmetric.");
        assert!(
            out.html.contains("a_{ij}"),
            "underscores in inline math preserved, html: {}",
            out.html
        );
    }

    #[test]
    fn render_display_math_with_underscores() {
        let out = render_markdown("$$a_{ij} + b_{ij}$$");
        assert!(
            out.html.contains("a_{ij} + b_{ij}"),
            "underscores in math should not become emphasis, html: {}",
            out.html
        );
        assert!(
            !out.html.contains("<em>"),
            "no emphasis tags inside math, html: {}",
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
        let out = render_markdown(md);
        assert!(out.html.contains("<pre>"), "html: {}", out.html);
        assert!(out.html.contains("<code>"), "html: {}", out.html);
    }

    #[test]
    fn render_code_block_with_language() {
        let md = indoc! {"
            ```rust
            fn main() {}
            ```
        "};
        let out = render_markdown(md);
        assert!(
            out.html.contains("<code class=\"language-rust\">"),
            "html: {}",
            out.html
        );
    }
}
