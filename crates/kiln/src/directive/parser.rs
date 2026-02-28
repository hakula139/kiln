use super::{DirectiveBlock, DirectiveKind};
use crate::markdown::{detect_opening_code_fence, is_closing_code_fence};

struct StackEntry {
    colon_count: usize,
    kind: DirectiveKind,
    id: Option<String>,
    classes: Vec<String>,
    /// Byte offset of the first line after the opening fence.
    body_start: usize,
    /// Byte offset of the opening fence line.
    range_start: usize,
}

/// Parsed result from the text after the opening colon fence.
struct DirectiveHead<'a> {
    name: &'a str,
    args: String,
    id: Option<&'a str>,
    classes: Vec<&'a str>,
}

/// Scans content for `:::`-fenced directive blocks.
///
/// Returns blocks sorted by ascending byte offset.
/// Unclosed directives are silently skipped.
#[must_use]
pub fn parse_directives(content: &str) -> Vec<DirectiveBlock> {
    let mut blocks = Vec::new();
    let mut stack = Vec::new();
    let mut code_fence = None;
    let mut offset = 0;

    for raw_line in content.split('\n') {
        // +1 for the '\n' delimiter, but cap at content length for the final
        // segment which has no trailing newline.
        let next_offset = (offset + raw_line.len() + 1).min(content.len());
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);

        if let Some((fence_char, fence_count)) = code_fence {
            if is_closing_code_fence(line, fence_char, fence_count) {
                code_fence = None;
            }
            offset = next_offset;
            continue;
        }

        if let Some(fence) = detect_opening_code_fence(line) {
            code_fence = Some(fence);
            offset = next_offset;
            continue;
        }

        if let Some(colon_count) = count_leading_colons(line) {
            let after_colons = line[colon_count..].trim();

            if after_colons.is_empty() {
                // Closing fence — only matches the topmost stack entry if its
                // opening colon count ≤ the closing count. This prevents a
                // closing fence from "reaching through" unclosed inner blocks.
                if stack
                    .last()
                    .is_some_and(|e: &StackEntry| e.colon_count <= colon_count)
                    && let Some(entry) = stack.pop()
                {
                    let body = extract_body(content, entry.body_start, offset);
                    blocks.push(DirectiveBlock {
                        kind: entry.kind,
                        id: entry.id,
                        classes: entry.classes,
                        body,
                        range: entry.range_start..next_offset,
                    });
                }
            } else {
                let head = parse_directive_head(after_colons);
                stack.push(StackEntry {
                    colon_count,
                    kind: DirectiveKind::from_name(head.name, &head.args),
                    id: head.id.map(String::from),
                    classes: head.classes.into_iter().map(String::from).collect(),
                    body_start: next_offset,
                    range_start: offset,
                });
            }
        }

        offset = next_offset;
    }

    blocks.sort_by_key(|b| b.range.start);
    blocks
}

/// Returns the number of leading `:` characters if there are at least 3.
///
/// Only matches column-0 directives — indented lines are intentionally ignored
/// since directives are top-level constructs.
fn count_leading_colons(line: &str) -> Option<usize> {
    let count = line.bytes().take_while(|&b| b == b':').count();
    (count >= 3).then_some(count)
}

/// Splits the text after the colons into a directive name and Pandoc
/// attributes (`#id`, `.class`, `key=value`).
///
/// Accepts `name {attrs}`, bare `name`, or `{attrs}` alone. Attributes always
/// require `{…}` braces.
fn parse_directive_head(text: &str) -> DirectiveHead<'_> {
    let text = text.trim();

    // Split off directive name (if not starting with '{').
    let (name, rest) = if text.starts_with('{') {
        ("", text)
    } else {
        let pos = text.find(char::is_whitespace).unwrap_or(text.len());
        (&text[..pos], text[pos..].trim_start())
    };

    // Parse {#id .class key=value} if present.
    if let Some(inner) = rest.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
        let mut head = parse_pandoc_attrs(inner.trim());
        head.name = name;
        return head;
    }

    // Name only — text after the name without braces is ignored.
    DirectiveHead {
        name,
        args: String::new(),
        id: None,
        classes: Vec::new(),
    }
}

/// Extracts `#id`, `.class`, and remaining key=value args from the interior
/// of a `{…}` Pandoc attribute block.
///
/// `#id` and `.class` tokens are extracted regardless of position — they can
/// be interleaved with key=value pairs. The first `#id` wins; duplicates are
/// silently ignored. Everything else is collected into the args string.
fn parse_pandoc_attrs(input: &str) -> DirectiveHead<'_> {
    let mut id: Option<&str> = None;
    let mut classes = Vec::new();
    let mut args = String::new();
    let mut scan = input;

    while !scan.is_empty() {
        if let Some(after_hash) = scan.strip_prefix('#') {
            let token = take_token(after_hash);
            if id.is_none() && !token.is_empty() {
                id = Some(token);
            }
            scan = after_hash[token.len()..].trim_start();
            continue;
        }

        if let Some(after_dot) = scan.strip_prefix('.') {
            let token = take_token(after_dot);
            if !token.is_empty() {
                classes.push(token);
            }
            scan = after_dot[token.len()..].trim_start();
            continue;
        }

        // key=value or bare word — collect into args.
        let (token, rest) = take_attr_token(scan);
        if !args.is_empty() {
            args.push(' ');
        }
        args.push_str(token);

        scan = rest;
    }

    DirectiveHead {
        name: "",
        args,
        id,
        classes,
    }
}

/// Returns the leading non-whitespace slice of `s`.
fn take_token(s: &str) -> &str {
    let end = s.find(char::is_whitespace).unwrap_or(s.len());
    &s[..end]
}

/// Consumes the next attribute token (bare word or `key=value`) from the
/// input, respecting quoted values. Returns `(token, remainder)`.
fn take_attr_token(s: &str) -> (&str, &str) {
    let ws = s.find(char::is_whitespace).unwrap_or(s.len());

    // key="quoted value" — find the closing quote.
    if let Some(eq) = s.find('=').filter(|&p| p < ws) {
        let after_eq = &s[eq + 1..];
        if let Some(quoted) = after_eq.strip_prefix('"') {
            let (end, _) = super::scan_quoted_value(quoted);
            let consumed = eq + 1 + 1 + end + usize::from(end < quoted.len());
            return (&s[..consumed], s[consumed..].trim_start());
        }
    }

    (&s[..ws], s[ws..].trim_start())
}

/// Extracts the body text between byte offsets `start` and `end`, stripping
/// exactly one trailing line ending.
fn extract_body(content: &str, start: usize, end: usize) -> String {
    if start >= end {
        return String::new();
    }
    let body = &content[start..end];
    let body = body.strip_suffix('\n').unwrap_or(body);
    let body = body.strip_suffix('\r').unwrap_or(body);
    body.to_string()
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::*;
    use crate::directive::CalloutKind;

    // -- Callout --

    #[test]
    fn callout_default_type() {
        let input = indoc! {"
            ::: callout
            Hello world
            :::
        "};
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(
            blocks[0].kind,
            DirectiveKind::Callout {
                kind: CalloutKind::Note,
                title: None,
                open: true,
            }
        );
        assert_eq!(blocks[0].id, None);
        assert!(blocks[0].classes.is_empty());
        assert_eq!(blocks[0].body, "Hello world");
        assert_eq!(blocks[0].range, 0..input.len());
    }

    #[test]
    fn callout_name_case_insensitive() {
        let input = indoc! {"
            ::: Callout
            Body
            :::
        "};
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(
            blocks[0].kind,
            DirectiveKind::Callout {
                kind: CalloutKind::Note,
                title: None,
                open: true,
            }
        );
    }

    #[test]
    fn callout_empty_body() {
        let input = indoc! {"
            ::: callout
            :::
        "};
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].body, "");
    }

    #[test]
    fn callout_with_type_and_attrs() {
        let input = indoc! {r#"
            ::: callout {type=warning title="Careful" open=false}
            Body
            :::
        "#};
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(
            blocks[0].kind,
            DirectiveKind::Callout {
                kind: CalloutKind::Warning,
                title: Some("Careful".into()),
                open: false,
            }
        );
    }

    #[test]
    fn callout_multiple_sequential() {
        let input = indoc! {"
            ::: callout
            First
            :::

            ::: callout {type=warning}
            Second
            :::
        "};
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].body, "First");
        assert_eq!(blocks[1].body, "Second");
    }

    #[test]
    fn callout_multiline_body() {
        let input = indoc! {"
            ::: callout
            First paragraph.

            Second paragraph.
            :::
        "};
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].body, "First paragraph.\n\nSecond paragraph.");
    }

    // -- Unknown directives --

    #[test]
    fn unknown_name_only() {
        let input = indoc! {"
            ::: custom
            Body
            :::
        "};
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(
            blocks[0].kind,
            DirectiveKind::Unknown {
                name: "custom".into(),
                args: String::new(),
            }
        );
    }

    #[test]
    fn unknown_name_and_args() {
        let input = indoc! {"
            ::: table {cols=3}
            Body
            :::
        "};
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(
            blocks[0].kind,
            DirectiveKind::Unknown {
                name: "table".into(),
                args: "cols=3".into(),
            }
        );
        assert_eq!(blocks[0].body, "Body");
    }

    // -- Pandoc attributes --

    #[test]
    fn pandoc_id_extracted() {
        let input = indoc! {"
            ::: callout {#my-id}
            Body
            :::
        "};
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(
            blocks[0].kind,
            DirectiveKind::Callout {
                kind: CalloutKind::Note,
                title: None,
                open: true,
            }
        );
        assert_eq!(blocks[0].id.as_deref(), Some("my-id"));
        assert!(blocks[0].classes.is_empty());
    }

    #[test]
    fn pandoc_extra_classes_collected() {
        let input = indoc! {"
            ::: callout {.highlight .compact}
            Body
            :::
        "};
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].id, None);
        assert_eq!(blocks[0].classes, ["highlight", "compact"]);
    }

    #[test]
    fn pandoc_id_and_classes_with_args() {
        let input = indoc! {r#"
            ::: callout {#box .wide type=warning title="Careful"}
            Body
            :::
        "#};
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(
            blocks[0].kind,
            DirectiveKind::Callout {
                kind: CalloutKind::Warning,
                title: Some("Careful".into()),
                open: true,
            }
        );
        assert_eq!(blocks[0].id.as_deref(), Some("box"));
        assert_eq!(blocks[0].classes, ["wide"]);
    }

    #[test]
    fn pandoc_class_only_no_name() {
        // {.note} without a name word is a generic div, not a callout.
        let input = indoc! {"
            ::: {.note}
            Body
            :::
        "};
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(
            blocks[0].kind,
            DirectiveKind::Unknown {
                name: String::new(),
                args: String::new(),
            }
        );
        assert_eq!(blocks[0].classes, ["note"]);
    }

    #[test]
    fn pandoc_id_only_no_name() {
        // {#section} without a name word is a generic div, not a callout.
        let input = indoc! {"
            ::: {#section}
            Body
            :::
        "};
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(
            blocks[0].kind,
            DirectiveKind::Unknown {
                name: String::new(),
                args: String::new(),
            }
        );
        assert_eq!(blocks[0].id.as_deref(), Some("section"));
        assert!(blocks[0].classes.is_empty());
    }

    #[test]
    fn pandoc_attrs_bare_words_become_args() {
        let input = indoc! {r#"
            ::: {note title="Custom"}
            Body
            :::
        "#};
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(
            blocks[0].kind,
            DirectiveKind::Unknown {
                name: String::new(),
                args: r#"note title="Custom""#.into(),
            }
        );
    }

    #[test]
    fn pandoc_id_after_class() {
        let input = indoc! {"
            ::: callout {.extra #late-id}
            Body
            :::
        "};
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].id.as_deref(), Some("late-id"));
        assert_eq!(blocks[0].classes, ["extra"]);
    }

    #[test]
    fn pandoc_interleaved_attrs() {
        let input = indoc! {"
            ::: callout {.highlight type=tip #my-id .wide}
            Body
            :::
        "};
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(
            blocks[0].kind,
            DirectiveKind::Callout {
                kind: CalloutKind::Tip,
                title: None,
                open: true,
            }
        );
        assert_eq!(blocks[0].id.as_deref(), Some("my-id"));
        assert_eq!(blocks[0].classes, ["highlight", "wide"]);
    }

    #[test]
    fn pandoc_multiple_ids_first_wins() {
        let input = indoc! {"
            ::: callout {#first #second .extra}
            Body
            :::
        "};
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].id.as_deref(), Some("first"));
        assert_eq!(blocks[0].classes, ["extra"]);
    }

    #[test]
    fn pandoc_empty_hash_and_dot_ignored() {
        let input = indoc! {"
            ::: callout {# . .real}
            Body
            :::
        "};
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].id, None);
        assert_eq!(blocks[0].classes, ["real"]);
    }

    #[test]
    fn pandoc_quoted_value_shields_hash_and_dot() {
        let input = indoc! {r#"
            ::: callout {title="Hello #world .bold" #real-id .real-class}
            Body
            :::
        "#};
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(
            blocks[0].kind,
            DirectiveKind::Callout {
                kind: CalloutKind::Note,
                title: Some("Hello #world .bold".into()),
                open: true,
            }
        );
        assert_eq!(blocks[0].id.as_deref(), Some("real-id"));
        assert_eq!(blocks[0].classes, ["real-class"]);
    }

    // -- Nesting --

    #[test]
    fn nested_directives() {
        let input = indoc! {"
            :::: callout {type=warning}
            ::: callout
            Inner
            :::
            Outer
            ::::
        "};
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 2, "should find two blocks");

        assert_eq!(
            blocks[0].kind,
            DirectiveKind::Callout {
                kind: CalloutKind::Warning,
                title: None,
                open: true,
            }
        );
        assert!(
            blocks[0].body.contains("::: callout"),
            "outer body should contain inner raw text"
        );
        assert!(
            blocks[0].body.contains("Outer"),
            "outer body should contain text after inner block"
        );

        assert_eq!(
            blocks[1].kind,
            DirectiveKind::Callout {
                kind: CalloutKind::Note,
                title: None,
                open: true,
            }
        );
        assert_eq!(blocks[1].body, "Inner");
    }

    #[test]
    fn nested_directive_siblings() {
        let input = indoc! {"
            ::::: wrapper
            ::: callout
            First
            :::
            ::: callout {type=warning}
            Second
            :::
            :::::
        "};
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 3, "should find outer + two inner blocks");

        assert_eq!(blocks[0].body.matches(":::").count(), 4);
        assert_eq!(blocks[1].body, "First");
        assert_eq!(blocks[2].body, "Second");
    }

    // -- Closing fence --

    #[test]
    fn unclosed_directive_skipped() {
        let input = indoc! {"
            ::: callout
            No closing fence
        "};
        let blocks = parse_directives(input);
        assert!(blocks.is_empty(), "unclosed directive should be skipped");
    }

    #[test]
    fn closing_fence_colon_count() {
        let input = indoc! {"
            ::: callout
            Body
            ::::
        "};
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 1, ":::: should close ::: (4 >= 3)");
        assert_eq!(blocks[0].body, "Body");

        let input = indoc! {"
            :::: callout
            Body
            :::
        "};
        let blocks = parse_directives(input);
        assert!(blocks.is_empty(), "::: should NOT close :::: (3 < 4)");
    }

    #[test]
    fn closing_fence_cannot_skip_unclosed_inner() {
        let input = indoc! {"
            :::: outer
            ::: inner-a
            ::: inner-b
            :::
            ::::
        "};
        let blocks = parse_directives(input);
        assert_eq!(
            blocks.len(),
            2,
            "should find two closed blocks, blocks:\n{blocks:?}"
        );

        assert!(
            blocks.iter().any(|b| b.body.is_empty()),
            "inner-b should have empty body"
        );
        assert!(
            blocks.iter().any(|b| b.body.contains("::: inner-b")),
            "inner-a body should contain the inner-b fence, blocks:\n{blocks:?}"
        );
    }

    // -- Code fence interaction --

    #[test]
    fn directives_inside_code_fences_ignored() {
        // Backtick fences.
        let input = indoc! {"
            ```
            ::: callout
            Body
            :::
            ```
        "};
        assert!(parse_directives(input).is_empty());

        // Tilde fences.
        let input = indoc! {"
            ~~~
            ::: callout
            Body
            :::
            ~~~
        "};
        assert!(parse_directives(input).is_empty());
    }

    #[test]
    fn code_fence_inside_directive() {
        let input = indoc! {"
            ::: callout
            ```
            ::: callout {type=warning}
            not a directive
            :::
            ```
            :::
        "};
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(
            blocks[0].kind,
            DirectiveKind::Callout {
                kind: CalloutKind::Note,
                title: None,
                open: true,
            }
        );
        assert!(
            blocks[0].body.contains("```"),
            "body should contain the code fence"
        );
    }

    #[test]
    fn indented_code_fence_ignores_directives() {
        let input = "   ```\n::: callout\nBody\n:::\n   ```\n";
        assert!(
            parse_directives(input).is_empty(),
            "directives inside indented code fences should be ignored"
        );
    }

    #[test]
    fn over_indented_code_fence_not_recognized() {
        // Opening fence.
        let input = "    ```\n::: callout\nBody\n:::\n";
        assert_eq!(
            parse_directives(input).len(),
            1,
            "over-indented opening fence should not suppress directives"
        );

        // Closing fence.
        let input = "```\n::: callout\nBody\n:::\n    ```\n";
        assert!(
            parse_directives(input).is_empty(),
            "over-indented closing fence should not close the code block"
        );
    }

    #[test]
    fn short_backtick_run_not_a_code_fence() {
        let input = indoc! {"
            ``
            ::: callout
            Body
            :::
        "};
        assert_eq!(
            parse_directives(input).len(),
            1,
            "two backticks should not suppress directives"
        );
    }

    #[test]
    fn mismatched_code_fence_chars_not_closed() {
        let input = indoc! {"
            ```
            ::: callout
            Body
            :::
            ~~~
        "};
        assert!(
            parse_directives(input).is_empty(),
            "~~~ should not close ``` fence"
        );
    }

    #[test]
    fn backtick_fence_with_backtick_in_info_not_a_fence() {
        let input = indoc! {"
            ```foo`bar
            ::: callout
            Body
            :::
        "};
        assert_eq!(
            parse_directives(input).len(),
            1,
            "invalid backtick fence should not suppress directives"
        );
    }

    // -- Edge cases --

    #[test]
    fn indented_directive_fence_ignored() {
        let input = "  ::: callout\n  Body\n  :::\n";
        assert!(
            parse_directives(input).is_empty(),
            "indented directive fences should not be recognized"
        );
    }

    #[test]
    fn directive_trailing_whitespace_on_fences() {
        let input = "::: callout   \nBody\n:::   \n";
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].body, "Body");
    }

    #[test]
    fn utf8_body_and_range() {
        let prefix = "前言：世界\n";
        let directive = "::: callout\n你好世界\n:::\n";
        let input = format!("{prefix}{directive}");
        let blocks = parse_directives(&input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].body, "你好世界");
        assert_eq!(
            blocks[0].range,
            prefix.len()..input.len(),
            "range should account for multi-byte prefix"
        );
    }

    #[test]
    fn no_directives_returns_empty() {
        let input = indoc! {"
            Just some regular markdown.

            No directives here.
        "};
        assert!(parse_directives(input).is_empty());
    }

    #[test]
    fn eof_without_trailing_newline() {
        let input = "::: callout\nBody\n:::";
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].body, "Body");
        assert_eq!(
            blocks[0].range,
            0..input.len(),
            "range should span entire input"
        );
    }

    #[test]
    fn crlf_line_endings() {
        let input = "::: callout\r\nHello\r\n:::\r\n";
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].body, "Hello");
        assert_eq!(
            blocks[0].range,
            0..input.len(),
            "range should span entire input"
        );
    }
}
