use std::collections::BTreeMap;

use super::{DirectiveBlock, DirectiveKind, parse_directive_args};
use crate::attrs::find_attr_block_end;
use crate::markdown::code_ranges;

struct StackEntry {
    colon_count: usize,
    kind: DirectiveKind,
    id: Option<String>,
    classes: Vec<String>,
    /// Byte offset of the first line after the opening fence.
    body_start: usize,
    /// Byte offset of the opening fence line.
    range_start: usize,
    children: Vec<DirectiveNode>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct DirectiveNode {
    pub(crate) block: DirectiveBlock,
    pub(crate) body_start: usize,
    pub(crate) children: Vec<Self>,
}

/// Parsed result from the text after the opening colon fence.
struct DirectiveHead {
    name: String,
    positional_args: Vec<String>,
    named_args: BTreeMap<String, String>,
    id: Option<String>,
    classes: Vec<String>,
}

/// Parses nested `:::` directives in source order, retaining children of unclosed fences.
pub(crate) fn parse_directives(content: &str) -> Vec<DirectiveNode> {
    let mut blocks = Vec::new();
    let mut stack = Vec::new();
    let protected = code_ranges(content);
    let mut protected = protected.iter().peekable();
    let mut offset = 0;

    for raw_line in content.split('\n') {
        // +1 for the '\n' delimiter, but cap at content length for the final
        // segment which has no trailing newline.
        let next_offset = (offset + raw_line.len() + 1).min(content.len());
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);

        while protected.peek().is_some_and(|range| range.end <= offset) {
            protected.next();
        }
        if protected.peek().is_some_and(|range| range.start <= offset) {
            offset = next_offset;
            continue;
        }

        if let Some(colon_count) = count_leading_colons(line) {
            let after_colons = line[colon_count..].trim();

            if after_colons.is_empty() {
                // A closing fence only matches the topmost stack entry if its
                // opening colon count ≤ the closing count. This prevents a
                // closing fence from "reaching through" unclosed inner blocks.
                if stack
                    .last()
                    .is_some_and(|e: &StackEntry| e.colon_count <= colon_count)
                    && let Some(entry) = stack.pop()
                {
                    let body = extract_body(content, entry.body_start, offset);
                    let node = DirectiveNode {
                        block: DirectiveBlock {
                            kind: entry.kind,
                            id: entry.id,
                            classes: entry.classes,
                            body,
                            range: entry.range_start..next_offset,
                        },
                        body_start: entry.body_start,
                        children: entry.children,
                    };
                    if let Some(parent) = stack.last_mut() {
                        parent.children.push(node);
                    } else {
                        blocks.push(node);
                    }
                }
            } else {
                let head = parse_directive_head(after_colons);
                stack.push(StackEntry {
                    colon_count,
                    kind: DirectiveKind::from_parsed(
                        &head.name,
                        head.positional_args,
                        head.named_args,
                    ),
                    id: head.id,
                    classes: head.classes,
                    body_start: next_offset,
                    range_start: offset,
                    children: Vec::new(),
                });
            }
        }

        offset = next_offset;
    }

    for entry in stack {
        blocks.extend(entry.children);
    }
    blocks.sort_by_key(|node| node.block.range.start);
    blocks
}

/// Returns the number of leading `:` characters if there are at least 3.
///
/// Only matches column-0 directives. Indented lines are intentionally ignored
/// since directives are top-level constructs.
fn count_leading_colons(line: &str) -> Option<usize> {
    let count = line.bytes().take_while(|&b| b == b':').count();
    (count >= 3).then_some(count)
}

/// Splits the text after the colons into a directive name and `{...}` attributes.
///
/// Accepts `name {attrs}`, bare `name`, or `{attrs}` alone.
fn parse_directive_head(text: &str) -> DirectiveHead {
    let text = text.trim();

    let (name, rest) = if text.starts_with('{') {
        ("", text)
    } else {
        let pos = text.find(char::is_whitespace).unwrap_or(text.len());
        (&text[..pos], text[pos..].trim_start())
    };

    if let Some(payload) = rest.strip_prefix('{')
        && let Some(close) = find_attr_block_end(payload)
    {
        let inner = &payload[..close];
        let args = parse_directive_args(inner.trim());
        return DirectiveHead {
            name: name.to_string(),
            positional_args: args.positional,
            named_args: args.named,
            id: args.id,
            classes: args.classes,
        };
    }

    // Name only: text after the name without braces is ignored.
    DirectiveHead {
        name: name.to_string(),
        positional_args: Vec::new(),
        named_args: BTreeMap::new(),
        id: None,
        classes: Vec::new(),
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
    use std::collections::BTreeMap;

    use indoc::indoc;

    use super::*;
    use crate::directive::CalloutKind;

    // ── parse_directives ──

    #[test]
    fn parse_directives_distinguishes_literal_fences_from_code_in_arguments() {
        let input = indoc! {r#"
            `literal
            ::: ignored
            :::
            `

            ::: callout {title="Use `code`"}
            Body
            :::
        "#};
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(
            blocks[0].block.kind,
            DirectiveKind::Callout {
                kind: CalloutKind::Note,
                title: Some("Use `code`".into()),
                open: true,
            }
        );
        assert_eq!(blocks[0].block.body, "Body");
    }

    // ── parse_directives: callout ──

    #[test]
    fn parse_directives_callout_default_type() {
        let input = indoc! {"
            ::: callout
            Hello world
            :::
        "};
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(
            blocks[0].block.kind,
            DirectiveKind::Callout {
                kind: CalloutKind::Note,
                title: None,
                open: true,
            }
        );
        assert_eq!(blocks[0].block.id, None);
        assert_eq!(blocks[0].block.classes, Vec::<String>::new());
        assert_eq!(blocks[0].block.body, "Hello world");
        assert_eq!(blocks[0].block.range, 0..input.len());
    }

    #[test]
    fn parse_directives_callout_name_case_insensitive() {
        let input = indoc! {"
            ::: Callout
            Body
            :::
        "};
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(
            blocks[0].block.kind,
            DirectiveKind::Callout {
                kind: CalloutKind::Note,
                title: None,
                open: true,
            }
        );
    }

    #[test]
    fn parse_directives_callout_with_type_and_attrs() {
        let input = indoc! {r#"
            ::: callout {type=warning title="Careful" open=false}
            Body
            :::
        "#};
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(
            blocks[0].block.kind,
            DirectiveKind::Callout {
                kind: CalloutKind::Warning,
                title: Some("Careful".into()),
                open: false,
            }
        );
    }

    #[test]
    fn parse_directives_callout_multiple_sequential() {
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
        assert_eq!(blocks[0].block.body, "First");
        assert_eq!(blocks[1].block.body, "Second");
    }

    #[test]
    fn parse_directives_callout_multiline_body() {
        let input = indoc! {"
            ::: callout
            First paragraph.

            Second paragraph.
            :::
        "};
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(
            blocks[0].block.body,
            indoc! {"
                First paragraph.

                Second paragraph."
            },
        );
    }

    #[test]
    fn parse_directives_callout_empty_body() {
        let input = indoc! {"
            ::: callout
            :::
        "};
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].block.body, "");
    }

    // ── parse_directives: unknown ──

    #[test]
    fn parse_directives_unknown_name_only() {
        let input = indoc! {"
            ::: custom
            Body
            :::
        "};
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(
            blocks[0].block.kind,
            DirectiveKind::Unknown {
                name: "custom".into(),
                positional_args: Vec::new(),
                named_args: BTreeMap::new(),
            }
        );
    }

    #[test]
    fn parse_directives_unknown_name_and_args() {
        let input = indoc! {"
            ::: table {cols=3}
            Body
            :::
        "};
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(
            blocks[0].block.kind,
            DirectiveKind::Unknown {
                name: "table".into(),
                positional_args: Vec::new(),
                named_args: BTreeMap::from([("cols".into(), "3".into())]),
            }
        );
        assert_eq!(blocks[0].block.body, "Body");
    }

    // ── parse_directives: pandoc attributes ──

    #[test]
    fn parse_directives_pandoc_id_extracted() {
        let input = indoc! {"
            ::: callout {#my-id}
            Body
            :::
        "};
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(
            blocks[0].block.kind,
            DirectiveKind::Callout {
                kind: CalloutKind::Note,
                title: None,
                open: true,
            }
        );
        assert_eq!(blocks[0].block.id.as_deref(), Some("my-id"));
        assert_eq!(blocks[0].block.classes, Vec::<String>::new());
    }

    #[test]
    fn parse_directives_pandoc_extra_classes_collected() {
        let input = indoc! {"
            ::: callout {.highlight .compact}
            Body
            :::
        "};
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].block.id, None);
        assert_eq!(blocks[0].block.classes, ["highlight", "compact"]);
    }

    #[test]
    fn parse_directives_pandoc_id_and_classes_with_args() {
        let input = indoc! {r#"
            ::: callout {#box .wide type=warning title="Careful"}
            Body
            :::
        "#};
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(
            blocks[0].block.kind,
            DirectiveKind::Callout {
                kind: CalloutKind::Warning,
                title: Some("Careful".into()),
                open: true,
            }
        );
        assert_eq!(blocks[0].block.id.as_deref(), Some("box"));
        assert_eq!(blocks[0].block.classes, ["wide"]);
    }

    #[test]
    fn parse_directives_pandoc_id_after_class() {
        let input = indoc! {"
            ::: callout {.extra #late-id}
            Body
            :::
        "};
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].block.id.as_deref(), Some("late-id"));
        assert_eq!(blocks[0].block.classes, ["extra"]);
    }

    #[test]
    fn parse_directives_pandoc_interleaved_attrs() {
        let input = indoc! {"
            ::: callout {.highlight type=tip #my-id .wide}
            Body
            :::
        "};
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(
            blocks[0].block.kind,
            DirectiveKind::Callout {
                kind: CalloutKind::Tip,
                title: None,
                open: true,
            }
        );
        assert_eq!(blocks[0].block.id.as_deref(), Some("my-id"));
        assert_eq!(blocks[0].block.classes, ["highlight", "wide"]);
    }

    #[test]
    fn parse_directives_pandoc_class_only_no_name() {
        // {.note} without a name word is a generic div, not a callout.
        let input = indoc! {"
            ::: {.note}
            Body
            :::
        "};
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(
            blocks[0].block.kind,
            DirectiveKind::Unknown {
                name: String::new(),
                positional_args: Vec::new(),
                named_args: BTreeMap::new(),
            }
        );
        assert_eq!(blocks[0].block.classes, ["note"]);
    }

    #[test]
    fn parse_directives_pandoc_id_only_no_name() {
        // {#section} without a name word is a generic div, not a callout.
        let input = indoc! {"
            ::: {#section}
            Body
            :::
        "};
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(
            blocks[0].block.kind,
            DirectiveKind::Unknown {
                name: String::new(),
                positional_args: Vec::new(),
                named_args: BTreeMap::new(),
            }
        );
        assert_eq!(blocks[0].block.id.as_deref(), Some("section"));
        assert_eq!(blocks[0].block.classes, Vec::<String>::new());
    }

    #[test]
    fn parse_directives_pandoc_attrs_bare_words_become_positional_args() {
        let input = indoc! {r#"
            ::: {note title="Custom"}
            Body
            :::
        "#};
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(
            blocks[0].block.kind,
            DirectiveKind::Unknown {
                name: String::new(),
                positional_args: vec!["note".into()],
                named_args: BTreeMap::from([("title".into(), "Custom".into())]),
            }
        );
    }

    #[test]
    fn parse_directives_pandoc_multiple_ids_first_wins() {
        let input = indoc! {"
            ::: callout {#first #second .extra}
            Body
            :::
        "};
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].block.id.as_deref(), Some("first"));
        assert_eq!(blocks[0].block.classes, ["extra"]);
    }

    #[test]
    fn parse_directives_pandoc_empty_hash_and_dot_ignored() {
        let input = indoc! {"
            ::: callout {# . .real}
            Body
            :::
        "};
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].block.id, None);
        assert_eq!(blocks[0].block.classes, ["real"]);
    }

    #[test]
    fn parse_directives_pandoc_quoted_value_shields_hash_and_dot() {
        let input = indoc! {r#"
            ::: callout {title="Hello #world .bold" #real-id .real-class}
            Body
            :::
        "#};
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(
            blocks[0].block.kind,
            DirectiveKind::Callout {
                kind: CalloutKind::Note,
                title: Some("Hello #world .bold".into()),
                open: true,
            }
        );
        assert_eq!(blocks[0].block.id.as_deref(), Some("real-id"));
        assert_eq!(blocks[0].block.classes, ["real-class"]);
    }

    // ── parse_directives: nesting ──

    #[test]
    fn parse_directives_nested_directives() {
        let input = indoc! {"
            :::: callout {type=warning}
            ::: callout
            Inner
            :::
            Outer
            ::::
        "};
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 1);

        assert_eq!(
            blocks[0].block.kind,
            DirectiveKind::Callout {
                kind: CalloutKind::Warning,
                title: None,
                open: true,
            }
        );
        assert!(
            blocks[0].block.body.contains("::: callout"),
            "outer body should contain inner raw text"
        );
        assert!(
            blocks[0].block.body.contains("Outer"),
            "outer body should contain text after inner block"
        );

        assert_eq!(
            blocks[0].children[0].block.kind,
            DirectiveKind::Callout {
                kind: CalloutKind::Note,
                title: None,
                open: true,
            }
        );
        assert_eq!(blocks[0].children[0].block.body, "Inner");
    }

    #[test]
    fn parse_directives_nested_directive_siblings() {
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
        assert_eq!(blocks.len(), 1);

        assert_eq!(blocks[0].block.body.matches(":::").count(), 4);
        assert_eq!(blocks[0].children[0].block.body, "First");
        assert_eq!(blocks[0].children[1].block.body, "Second");
    }

    // ── parse_directives: closing fence ──

    #[test]
    fn parse_directives_closing_fence_colon_count() {
        let input = indoc! {"
            ::: callout
            Body
            ::::
        "};
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 1, ":::: should close ::: (4 >= 3)");
        assert_eq!(blocks[0].block.body, "Body");

        let input = indoc! {"
            :::: callout
            Body
            :::
        "};
        let blocks = parse_directives(input);
        assert!(blocks.is_empty(), "::: should NOT close :::: (3 < 4)");
    }

    #[test]
    fn parse_directives_closing_fence_cannot_skip_unclosed_inner() {
        let input = indoc! {"
            :::: outer
            ::: inner-a
            ::: inner-b
            :::
            ::::
        "};
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 1);
        assert!(blocks[0].block.body.contains("::: inner-b"));
        assert_eq!(blocks[0].children.len(), 1);
        assert_eq!(blocks[0].children[0].block.body, "");
    }

    #[test]
    fn parse_directives_unclosed_directive_skipped() {
        let input = indoc! {"
            ::: callout
            No closing fence
        "};
        let blocks = parse_directives(input);
        assert!(blocks.is_empty(), "unclosed directive should be skipped");
    }

    // ── parse_directives: code fence ──

    #[test]
    fn parse_directives_directives_inside_code_fences_ignored() {
        let input = indoc! {"
            ```
            ::: callout
            Body
            :::
            ```
        "};
        assert_eq!(parse_directives(input), Vec::<DirectiveNode>::new());

        let input = indoc! {"
            ~~~
            ::: callout
            Body
            :::
            ~~~
        "};
        assert_eq!(parse_directives(input), Vec::<DirectiveNode>::new());
    }

    #[test]
    fn parse_directives_code_fence_inside_directive() {
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
            blocks[0].block.kind,
            DirectiveKind::Callout {
                kind: CalloutKind::Note,
                title: None,
                open: true,
            }
        );
        assert!(
            blocks[0].block.body.contains("```"),
            "body should contain the code fence"
        );
    }

    #[test]
    fn parse_directives_indented_code_fence_ignores_directives() {
        let input = indoc! {"
               ```
            ::: callout
            Body
            :::
               ```
        "};
        assert!(
            parse_directives(input).is_empty(),
            "directives inside indented code fences should be ignored"
        );
    }

    #[test]
    fn parse_directives_over_indented_code_fence_not_recognized() {
        let input = indoc! {"
                ```
            ::: callout
            Body
            :::
        "};
        assert_eq!(
            parse_directives(input).len(),
            1,
            "over-indented opening fence should not suppress directives"
        );

        let input = indoc! {"
            ```
            ::: callout
            Body
            :::
                ```
        "};
        assert!(
            parse_directives(input).is_empty(),
            "over-indented closing fence should not close the code block"
        );
    }

    #[test]
    fn parse_directives_short_backtick_run_not_a_code_fence() {
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
    fn parse_directives_mismatched_code_fence_chars_not_closed() {
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
    fn parse_directives_backtick_fence_with_backtick_in_info_not_a_fence() {
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

    // ── parse_directives: edge cases ──

    #[test]
    fn parse_directives_indented_directive_fence_ignored() {
        let input = indoc! {"
             ::: callout
            Body
            :::
        "};
        assert!(
            parse_directives(input).is_empty(),
            "indented directive fences should not be recognized"
        );
    }

    #[test]
    fn parse_directives_trailing_whitespace_on_fences() {
        let input = concat!("::: callout   \n", "Body\n", ":::   \n",);
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].block.body, "Body");
    }

    #[test]
    fn parse_directives_trailing_content_after_attrs_preserved() {
        let input = indoc! {r#"
            ::: embed { src="example.com" mode="full" } <!-- comment -->
            :::
        "#};
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(
            blocks[0].block.kind,
            DirectiveKind::Unknown {
                name: "embed".into(),
                positional_args: Vec::new(),
                named_args: BTreeMap::from([
                    ("src".into(), "example.com".into()),
                    ("mode".into(), "full".into()),
                ]),
            }
        );
    }

    #[test]
    fn parse_directives_utf8_body_and_range() {
        let prefix = "前言：世界\n";
        let directive = indoc! {"
            ::: callout
            你好世界
            :::
        "};
        let input = format!("{prefix}{directive}");
        let blocks = parse_directives(&input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].block.body, "你好世界");
        assert_eq!(
            blocks[0].block.range,
            prefix.len()..input.len(),
            "range should account for multi-byte prefix"
        );
    }

    #[test]
    fn parse_directives_no_directives_returns_empty() {
        let input = indoc! {"
            Just some regular markdown.

            No directives here.
        "};
        assert_eq!(parse_directives(input), Vec::<DirectiveNode>::new());
    }

    #[test]
    fn parse_directives_eof_without_trailing_newline() {
        let input = indoc! {"
            ::: callout
            Body
            :::"
        };
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].block.body, "Body");
        assert_eq!(
            blocks[0].block.range,
            0..input.len(),
            "range should span entire input"
        );
    }

    #[test]
    fn parse_directives_crlf_line_endings() {
        let input = indoc! {"
            ::: callout\r
            Hello\r
            :::\r
        "};
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].block.body, "Hello");
        assert_eq!(
            blocks[0].block.range,
            0..input.len(),
            "range should span entire input"
        );
    }

    // ── count_leading_colons ──

    #[test]
    fn count_leading_colons_returns_count_for_three_or_more() {
        assert_eq!(count_leading_colons(":::"), Some(3));
        assert_eq!(count_leading_colons("::::"), Some(4));
        assert_eq!(count_leading_colons("::::: callout"), Some(5));
    }

    #[test]
    fn count_leading_colons_returns_none_for_fewer_than_three() {
        assert_eq!(count_leading_colons(""), None);
        assert_eq!(count_leading_colons(":"), None);
        assert_eq!(count_leading_colons("::"), None);
    }

    #[test]
    fn count_leading_colons_returns_none_for_indented_line() {
        assert_eq!(count_leading_colons(" :::"), None);
        assert_eq!(count_leading_colons("\t:::"), None);
    }

    // ── parse_directive_head ──

    #[test]
    fn parse_directive_head_bare_name() {
        let head = parse_directive_head("callout");
        assert_eq!(head.name, "callout");
        assert_eq!(head.positional_args, Vec::<String>::new());
        assert!(head.named_args.is_empty());
        assert_eq!(head.id, None);
        assert_eq!(head.classes, Vec::<String>::new());
    }

    #[test]
    fn parse_directive_head_name_and_attrs() {
        let head = parse_directive_head(r#"callout {#box .wide type=warning title="Careful"}"#);
        assert_eq!(head.name, "callout");
        assert_eq!(head.id.as_deref(), Some("box"));
        assert_eq!(head.classes, ["wide"]);
        assert_eq!(
            head.named_args,
            BTreeMap::from([
                ("type".into(), "warning".into()),
                ("title".into(), "Careful".into()),
            ]),
        );
    }

    #[test]
    fn parse_directive_head_quoted_braces() {
        let head = parse_directive_head(r#"callout {"a}b" title="a\"}b" type=warning}"#);
        assert_eq!(head.positional_args, ["a}b"]);
        assert_eq!(
            head.named_args,
            BTreeMap::from([
                ("title".into(), "a\"}b".into()),
                ("type".into(), "warning".into()),
            ]),
        );
    }

    #[test]
    fn parse_directive_head_attrs_only_yields_empty_name() {
        let head = parse_directive_head("{#section .note}");
        assert_eq!(head.name, "");
        assert_eq!(head.id.as_deref(), Some("section"));
        assert_eq!(head.classes, ["note"]);
    }

    #[test]
    fn parse_directive_head_trailing_content_after_close_brace_kept() {
        let head = parse_directive_head(r#"embed {src="example.com"} <!-- note } -->"#);
        assert_eq!(head.name, "embed");
        assert_eq!(head.positional_args, Vec::<String>::new());
        assert_eq!(
            head.named_args,
            BTreeMap::from([("src".into(), "example.com".into())]),
        );
    }

    #[test]
    fn parse_directive_head_extra_text_after_name_without_braces_ignored() {
        let head = parse_directive_head("name extra text");
        assert_eq!(head.name, "name");
        assert_eq!(head.positional_args, Vec::<String>::new());
        assert!(head.named_args.is_empty());
        assert_eq!(head.id, None);
        assert_eq!(head.classes, Vec::<String>::new());
    }

    #[test]
    fn parse_directive_head_unclosed_quoted_value() {
        let head = parse_directive_head(r#"callout {title="a}b"#);
        assert_eq!(head.name, "callout");
        assert_eq!(head.positional_args, Vec::<String>::new());
        assert!(head.named_args.is_empty());
    }

    // ── extract_body ──

    #[test]
    fn extract_body_strips_trailing_newline() {
        let content = "Hello\n";
        assert_eq!(extract_body(content, 0, content.len()), "Hello");
    }

    #[test]
    fn extract_body_strips_trailing_crlf() {
        let content = "Hello\r\n";
        assert_eq!(extract_body(content, 0, content.len()), "Hello");
    }

    #[test]
    fn extract_body_keeps_content_without_trailing_newline() {
        let content = "Hello";
        assert_eq!(extract_body(content, 0, content.len()), "Hello");
    }

    #[test]
    fn extract_body_returns_empty_for_empty_or_inverted_range() {
        assert_eq!(extract_body("Hello", 3, 3), "");
        assert_eq!(extract_body("Hello", 5, 2), "");
    }
}
