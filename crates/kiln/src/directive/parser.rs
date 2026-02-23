use super::{DirectiveBlock, DirectiveKind};

struct StackEntry {
    colon_count: usize,
    kind: DirectiveKind,
    /// Byte offset of the first line after the opening fence.
    body_start: usize,
    /// Byte offset of the opening fence line.
    range_start: usize,
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
                        body,
                        range: entry.range_start..next_offset,
                    });
                }
            } else {
                let (name, args) = parse_directive_head(after_colons);
                stack.push(StackEntry {
                    colon_count,
                    kind: DirectiveKind::from_name(name, args),
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

/// Detects an opening code fence (three or more `` ` `` or `~` characters).
/// Handles up to 3 spaces of leading indentation.
fn detect_opening_code_fence(line: &str) -> Option<(u8, usize)> {
    let indent = line.bytes().take_while(|&b| b == b' ').count();
    if indent > 3 {
        return None;
    }

    let rest = &line[indent..];
    let &ch = rest.as_bytes().first()?;
    if ch != b'`' && ch != b'~' {
        return None;
    }

    let count = rest.bytes().take_while(|&b| b == ch).count();
    if count < 3 {
        return None;
    }

    // CommonMark: backtick fence info strings must not contain backticks.
    if ch == b'`' && rest[count..].contains('`') {
        return None;
    }

    Some((ch, count))
}

/// Checks whether `line` closes a code fence opened with `fence_char` repeated
/// `min_count` times. Handles up to 3 spaces of leading indentation.
fn is_closing_code_fence(line: &str, fence_char: u8, min_count: usize) -> bool {
    let indent = line.bytes().take_while(|&b| b == b' ').count();
    if indent > 3 {
        return false;
    }

    let rest = &line[indent..];
    let count = rest.bytes().take_while(|&b| b == fence_char).count();
    count >= min_count && rest[count..].trim().is_empty()
}

/// Splits the text after the colons into a directive name and remaining
/// attributes. Supports both Pandoc attribute form (`{.name key=value}`) and
/// simple form (`name key=value`).
fn parse_directive_head(text: &str) -> (&str, &str) {
    let text = text.trim();

    // Pandoc attribute form: {.name key=value ...}
    if let Some(inner) = text.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
        let inner = inner.trim();
        if let Some(after_dot) = inner.strip_prefix('.') {
            return match after_dot.find(char::is_whitespace) {
                Some(pos) => (&after_dot[..pos], after_dot[pos..].trim()),
                None => (after_dot, ""),
            };
        }
        // Braces without leading dot — not valid Pandoc syntax, skip.
        return ("", "");
    }

    // Simple form: name key=value ...
    match text.find(char::is_whitespace) {
        Some(pos) => (&text[..pos], text[pos..].trim()),
        None => (text, ""),
    }
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
    use crate::directive::AdmonitionKind;

    // -- Simple form --

    #[test]
    fn simple() {
        let input = indoc! {"
            ::: note
            Hello world
            :::
        "};
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(
            blocks[0].kind,
            DirectiveKind::Admonition {
                kind: AdmonitionKind::Note,
                title: None,
                open: true,
            }
        );
        assert_eq!(blocks[0].body, "Hello world");
        assert_eq!(blocks[0].range, 0..input.len());
    }

    #[test]
    fn simple_with_attrs() {
        let input = indoc! {r#"
            ::: note title="Read This" open=false
            Body
            :::
        "#};
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(
            blocks[0].kind,
            DirectiveKind::Admonition {
                kind: AdmonitionKind::Note,
                title: Some("Read This".into()),
                open: false,
            }
        );
    }

    #[test]
    fn empty_body() {
        let input = indoc! {"
            ::: note
            :::
        "};
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].body, "");
    }

    #[test]
    fn multiple_sequential() {
        let input = indoc! {"
            ::: note
            First
            :::

            ::: warning
            Second
            :::
        "};
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].body, "First");
        assert_eq!(blocks[1].body, "Second");
    }

    #[test]
    fn unknown_type() {
        let input = indoc! {"
            ::: table cols=3
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

    // -- Pandoc attribute form --

    #[test]
    fn pandoc_attrs_title() {
        let input = indoc! {r#"
            ::: {.tip title="Pro Tip"}
            Body
            :::
        "#};
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(
            blocks[0].kind,
            DirectiveKind::Admonition {
                kind: AdmonitionKind::Tip,
                title: Some("Pro Tip".into()),
                open: true,
            }
        );
    }

    #[test]
    fn pandoc_attrs_title_and_open() {
        let input = indoc! {r#"
            ::: {.warning title="Careful" open=false}
            Body
            :::
        "#};
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(
            blocks[0].kind,
            DirectiveKind::Admonition {
                kind: AdmonitionKind::Warning,
                title: Some("Careful".into()),
                open: false,
            }
        );
    }

    #[test]
    fn pandoc_attrs_simple_form_equivalent() {
        let input = indoc! {"
            ::: {.note}
            Body
            :::
        "};
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(
            blocks[0].kind,
            DirectiveKind::Admonition {
                kind: AdmonitionKind::Note,
                title: None,
                open: true,
            }
        );
    }

    #[test]
    fn pandoc_attrs_unknown_directive() {
        let input = indoc! {"
            ::: {.table cols=3}
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
    }

    #[test]
    fn pandoc_attrs_without_dot_rejected() {
        // Pandoc requires `.class` — braces without dot are not valid.
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
                args: String::new(),
            }
        );
    }

    // -- Nesting --

    #[test]
    fn nested() {
        let input = indoc! {"
            :::: warning
            ::: note
            Inner
            :::
            Outer
            ::::
        "};
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 2, "should find two blocks");

        // Sorted by range.start — outer first.
        assert_eq!(
            blocks[0].kind,
            DirectiveKind::Admonition {
                kind: AdmonitionKind::Warning,
                title: None,
                open: true,
            }
        );
        assert!(
            blocks[0].body.contains("::: note"),
            "outer body should contain inner raw text"
        );
        assert!(
            blocks[0].body.contains("Outer"),
            "outer body should contain text after inner block"
        );

        assert_eq!(
            blocks[1].kind,
            DirectiveKind::Admonition {
                kind: AdmonitionKind::Note,
                title: None,
                open: true,
            }
        );
        assert_eq!(blocks[1].body, "Inner");
    }

    #[test]
    fn nested_siblings() {
        let input = indoc! {"
            ::::: wrapper
            ::: note
            First
            :::
            ::: warning
            Second
            :::
            :::::
        "};
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 3, "should find outer + two inner blocks");

        // Sorted by range.start — outer first.
        assert_eq!(blocks[0].body.matches(":::").count(), 4);
        assert_eq!(blocks[1].body, "First");
        assert_eq!(blocks[2].body, "Second");
    }

    #[test]
    fn unclosed_skipped() {
        let input = indoc! {"
            ::: note
            No closing fence
        "};
        let blocks = parse_directives(input);
        assert!(blocks.is_empty(), "unclosed directive should be skipped");
    }

    #[test]
    fn closing_fence_colon_count() {
        // More colons than opening → matches.
        let input = indoc! {"
            ::: note
            Body
            ::::
        "};
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 1, ":::: should close ::: (4 >= 3)");
        assert_eq!(blocks[0].body, "Body");

        // Fewer colons than opening → does not match.
        let input = indoc! {"
            :::: note
            Body
            :::
        "};
        let blocks = parse_directives(input);
        assert!(blocks.is_empty(), "::: should NOT close :::: (3 < 4)");
    }

    #[test]
    fn closing_fence_cannot_skip_unclosed_inner() {
        // `::::` closes the topmost matching entry (inner-a), not the outer.
        // outer remains unclosed and is silently dropped.
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

        // inner-b closed by first `:::`, inner-a closed by `::::`.
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
    fn colons_inside_code_block_ignored() {
        let input = indoc! {"
            ```
            ::: note
            This is code, not a directive
            :::
            ```
        "};
        let blocks = parse_directives(input);
        assert!(
            blocks.is_empty(),
            "directive fences inside code blocks should be ignored"
        );
    }

    #[test]
    fn code_fence_inside_directive() {
        let input = indoc! {"
            ::: note
            ```
            ::: warning
            not a directive
            :::
            ```
            :::
        "};
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(
            blocks[0].kind,
            DirectiveKind::Admonition {
                kind: AdmonitionKind::Note,
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
    fn tilde_code_fence_ignores_directives() {
        let input = indoc! {"
            ~~~
            ::: note
            Not a directive
            :::
            ~~~
        "};
        let blocks = parse_directives(input);
        assert!(
            blocks.is_empty(),
            "directives inside tilde fences should be ignored"
        );
    }

    #[test]
    fn indented_code_fence_ignores_directives() {
        let input = "   ```\n::: note\nBody\n:::\n   ```\n";
        let blocks = parse_directives(input);
        assert!(
            blocks.is_empty(),
            "directives inside indented code fences should be ignored"
        );
    }

    #[test]
    fn mismatched_code_fence_chars_not_closed() {
        // ~~~ cannot close a ``` fence — directives remain suppressed.
        let input = indoc! {"
            ```
            ::: note
            Body
            :::
            ~~~
        "};
        let blocks = parse_directives(input);
        assert!(
            blocks.is_empty(),
            "~~~ should not close ``` fence; directives inside should be ignored"
        );
    }

    #[test]
    fn backtick_fence_with_backtick_in_info_not_a_fence() {
        // Per CommonMark, backtick fence info strings must not contain backticks.
        let input = indoc! {"
            ```foo`bar
            ::: note
            Body
            :::
        "};
        let blocks = parse_directives(input);
        assert_eq!(
            blocks.len(),
            1,
            "invalid backtick fence should not suppress directives"
        );
        assert_eq!(blocks[0].body, "Body");
    }

    // -- Edge cases --

    #[test]
    fn multiline_body_with_blank_lines() {
        let input = indoc! {"
            ::: note
            First paragraph.

            Second paragraph.
            :::
        "};
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].body, "First paragraph.\n\nSecond paragraph.");
    }

    #[test]
    fn trailing_whitespace_on_fences() {
        // Trailing spaces after colons on both opening and closing fences.
        let input = "::: note   \nBody\n:::   \n";
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].body, "Body");
    }

    #[test]
    fn no_directives_returns_empty() {
        let input = indoc! {"
            Just some regular markdown.

            No directives here.
        "};
        let blocks = parse_directives(input);
        assert!(blocks.is_empty());
    }

    #[test]
    fn eof_without_trailing_newline() {
        let input = "::: note\nBody\n:::";
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
        let input = "::: note\r\nHello\r\n:::\r\n";
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
