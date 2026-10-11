use super::{DirectiveArgs, DirectiveBlock};
use crate::attrs::{find_attr_block_end, parse_pandoc_attrs};
use crate::markdown::code_ranges;

struct StackEntry {
    colon_count: usize,
    /// The block with provisional body and range ends, which the closing fence completes.
    block: DirectiveBlock,
    children: Vec<DirectiveNode>,
}

/// A parsed directive block with the directives nested in its body.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct DirectiveNode {
    pub block: DirectiveBlock,
    pub children: Vec<Self>,
}

/// Parses nested `:::` directives in source order, retaining children of unclosed fences.
pub(crate) fn parse_directives(content: &str) -> Vec<DirectiveNode> {
    let mut blocks = Vec::new();
    let mut stack: Vec<StackEntry> = Vec::new();
    let protected = code_ranges(content);
    let mut protected = protected.iter().peekable();
    let mut offset = 0;

    for raw_line in content.split('\n') {
        // +1 for the '\n' delimiter, capped for a final segment without a trailing newline.
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
                // A closing fence only closes the innermost open directive, and only if that one
                // opened with at most as many colons, so it never reaches through unclosed blocks.
                if let Some(mut entry) = stack.pop_if(|entry| entry.colon_count <= colon_count) {
                    entry.block.body.end = body_end(content, entry.block.body.start, offset);
                    entry.block.range.end = next_offset;
                    let node = DirectiveNode {
                        block: entry.block,
                        children: entry.children,
                    };
                    if let Some(parent) = stack.last_mut() {
                        parent.children.push(node);
                    } else {
                        blocks.push(node);
                    }
                }
            } else {
                let (name, args) = parse_directive_head(after_colons);
                stack.push(StackEntry {
                    colon_count,
                    block: DirectiveBlock {
                        name,
                        args,
                        body: next_offset..next_offset,
                        range: offset..offset,
                    },
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
/// Only matches column-0 directives. Indented lines are intentionally ignored since directives
/// are top-level constructs.
fn count_leading_colons(line: &str) -> Option<usize> {
    let count = line.bytes().take_while(|&b| b == b':').count();
    (count >= 3).then_some(count)
}

/// Splits the text after the colons into a lowercased directive name and its `{...}` arguments.
///
/// Accepts `name {attrs}`, `name{attrs}`, bare `name`, or `{attrs}` alone. Text after the name
/// without a closed attribute block is ignored.
fn parse_directive_head(text: &str) -> (String, DirectiveArgs) {
    let end = text
        .find(|c: char| c.is_whitespace() || c == '{')
        .unwrap_or(text.len());
    let (name, rest) = text.split_at(end);
    let args = rest
        .trim_start()
        .strip_prefix('{')
        .and_then(|payload| find_attr_block_end(payload).map(|close| &payload[..close]))
        .map(|attrs| DirectiveArgs::from(parse_pandoc_attrs(attrs)))
        .unwrap_or_default();
    (name.to_lowercase(), args)
}

/// Returns the end of a body that starts at `start`, excluding the line ending before the
/// closing fence at `fence`.
fn body_end(content: &str, start: usize, fence: usize) -> usize {
    let body = &content[start..fence];
    let body = body.strip_suffix('\n').unwrap_or(body);
    let body = body.strip_suffix('\r').unwrap_or(body);
    start + body.len()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use indoc::indoc;

    use super::*;
    use crate::attrs::Identity;
    use crate::test_utils::named_args;

    // ── parse_directives ──

    #[test]
    fn records_name_body_and_range() {
        let input = indoc! {"
            ::: callout
            Hello world
            :::
        "};
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].block.name, "callout");
        assert_eq!(blocks[0].block.args, DirectiveArgs::default());
        assert_eq!(body(input, &blocks[0]), "Hello world");
        assert_eq!(blocks[0].block.range, 0..input.len());
        assert_eq!(blocks[0].children, []);
    }

    #[test]
    fn parses_interleaved_pandoc_attrs() {
        let input = indoc! {r#"
            ::: callout {.highlight type=tip #my-id title="Hello #world .bold" .wide open=false}
            Body
            :::
        "#};
        let blocks = parse_directives(input);
        assert_eq!(
            blocks[0].block.args,
            DirectiveArgs {
                positional: Vec::new(),
                named: named_args(&[
                    ("open", "false"),
                    ("title", "Hello #world .bold"),
                    ("type", "tip"),
                ]),
                identity: Identity {
                    id: Some("my-id".into()),
                    classes: vec!["highlight".into(), "wide".into()],
                },
            }
        );
    }

    #[test]
    fn anonymous_blocks_have_empty_name() {
        let input = indoc! {r#"
            ::: {#section .note}
            :::

            ::: {note title="Custom"}
            :::
        "#};
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].block.name, "");
        assert_eq!(blocks[0].block.args.identity.id.as_deref(), Some("section"));
        assert_eq!(blocks[0].block.args.identity.classes, ["note"]);
        assert_eq!(blocks[1].block.name, "");
        assert_eq!(blocks[1].block.args.positional, ["note"]);
        assert_eq!(
            blocks[1].block.args.named,
            named_args(&[("title", "Custom")])
        );
    }

    #[test]
    fn separates_sequential_blocks_with_multiline_and_empty_bodies() {
        let input = indoc! {"
            ::: first
            First paragraph.

            Second paragraph.
            :::

            ::: second
            :::
        "};
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 2);
        assert_eq!(
            body(input, &blocks[0]),
            indoc! {"
                First paragraph.

                Second paragraph."
            },
        );
        assert_eq!(blocks[1].block.name, "second");
        assert_eq!(body(input, &blocks[1]), "");
    }

    // ── parse_directives: nesting ──

    #[test]
    fn nests_inner_directive_in_outer_body() {
        let input = indoc! {"
            :::: outer
            ::: inner
            Inner
            :::
            Outer
            ::::
        "};
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].block.name, "outer");
        assert_eq!(
            body(input, &blocks[0]),
            indoc! {"
                ::: inner
                Inner
                :::
                Outer"
            }
        );
        assert_eq!(blocks[0].children.len(), 1);
        assert_eq!(blocks[0].children[0].block.name, "inner");
        assert_eq!(body(input, &blocks[0].children[0]), "Inner");
    }

    #[test]
    fn collects_nested_siblings_in_order() {
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
        assert_eq!(body(input, &blocks[0]).matches(":::").count(), 4);
        assert_eq!(body(input, &blocks[0].children[0]), "First");
        assert_eq!(body(input, &blocks[0].children[1]), "Second");
    }

    // ── parse_directives: closing fence ──

    #[test]
    fn closing_fence_needs_at_least_opening_colons() {
        let input = indoc! {"
            ::: callout
            Body
            ::::
        "};
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 1, ":::: should close ::: (4 >= 3)");
        assert_eq!(body(input, &blocks[0]), "Body");

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
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].block.name, "inner-a");
        assert!(body(input, &blocks[0]).contains("::: inner-b"));
        assert_eq!(blocks[0].children.len(), 1);
        assert_eq!(blocks[0].children[0].block.name, "inner-b");
        assert_eq!(body(input, &blocks[0].children[0]), "");
    }

    #[test]
    fn skips_unclosed_directive() {
        let input = indoc! {"
            ::: callout
            No closing fence
        "};
        assert_eq!(parse_directives(input), []);
    }

    // ── parse_directives: code ──

    #[test]
    fn ignores_fences_inside_code() {
        let input = indoc! {"
            ```
            ::: ignored
            :::
            ```

            ::: callout
            ~~~
            ::: callout {type=warning}
            not a directive
            :::
            ~~~
            :::
        "};
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].children, []);
        assert!(
            body(input, &blocks[0]).ends_with("not a directive\n:::\n~~~"),
            "body should contain the code fence"
        );
    }

    #[test]
    fn distinguishes_literal_fences_from_code_in_arguments() {
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
            blocks[0].block.args.named,
            named_args(&[("title", "Use `code`")])
        );
        assert_eq!(body(input, &blocks[0]), "Body");
    }

    // ── parse_directives: edge cases ──

    #[test]
    fn ignores_indented_fence() {
        let input = indoc! {"
             ::: callout
            Body
            :::
        "};
        assert_eq!(parse_directives(input), []);
    }

    #[test]
    fn allows_trailing_whitespace_on_fences() {
        let input = "::: callout   \nBody\n:::   \n";
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(body(input, &blocks[0]), "Body");
    }

    #[test]
    fn measures_utf8_body_and_range_in_bytes() {
        let prefix = "前言：世界\n";
        let directive = indoc! {"
            ::: callout
            你好世界
            :::
        "};
        let input = format!("{prefix}{directive}");
        let blocks = parse_directives(&input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(body(&input, &blocks[0]), "你好世界");
        assert_eq!(
            blocks[0].block.range,
            prefix.len()..input.len(),
            "range should account for multi-byte prefix"
        );
    }

    #[test]
    fn plain_markdown_yields_no_directives() {
        let input = indoc! {"
            Just some regular markdown.

            No directives here.
        "};
        assert_eq!(parse_directives(input), []);
    }

    #[test]
    fn closes_at_eof_without_trailing_newline() {
        let input = indoc! {"
            ::: callout
            Body
            :::"
        };
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(body(input, &blocks[0]), "Body");
        assert_eq!(blocks[0].block.range, 0..input.len());
    }

    #[test]
    fn strips_crlf_line_endings() {
        let input = indoc! {"
            ::: callout\r
            Hello\r
            :::\r
        "};
        let blocks = parse_directives(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(body(input, &blocks[0]), "Hello");
        assert_eq!(blocks[0].block.range, 0..input.len());
    }

    // ── count_leading_colons ──

    #[test]
    fn counts_three_or_more_colons() {
        assert_eq!(count_leading_colons(":::"), Some(3));
        assert_eq!(count_leading_colons("::::"), Some(4));
        assert_eq!(count_leading_colons("::::: callout"), Some(5));
    }

    #[test]
    fn ignores_short_or_indented_runs() {
        for line in ["", ":", "::", " :::", "\t:::"] {
            assert_eq!(count_leading_colons(line), None, "{line:?}");
        }
    }

    // ── parse_directive_head ──

    #[test]
    fn bare_name_has_default_args() {
        assert_eq!(
            parse_directive_head("callout"),
            ("callout".to_owned(), DirectiveArgs::default())
        );
    }

    #[test]
    fn splits_name_from_attrs_with_or_without_space() {
        for text in [
            "callout {.wide type=warning}",
            "callout{.wide type=warning}",
        ] {
            let (name, args) = parse_directive_head(text);
            assert_eq!(name, "callout", "{text}");
            assert_eq!(args.identity.classes, ["wide"], "{text}");
            assert_eq!(args.named, named_args(&[("type", "warning")]), "{text}");
        }
    }

    #[test]
    fn lowercases_name_only() {
        let (name, args) = parse_directive_head("Callout {#Tip-1 .Wide type=Tip}");
        assert_eq!(name, "callout");
        assert_eq!(args.identity.id.as_deref(), Some("Tip-1"));
        assert_eq!(args.identity.classes, ["Wide"]);
        assert_eq!(args.named, named_args(&[("type", "Tip")]));
    }

    #[test]
    fn keeps_quoted_braces_in_attrs() {
        let (_, args) = parse_directive_head(r#"callout {"a}b" title="a\"}b" type=warning}"#);
        assert_eq!(args.positional, ["a}b"]);
        assert_eq!(
            args.named,
            named_args(&[("title", "a\"}b"), ("type", "warning")])
        );
    }

    #[test]
    fn ignores_text_outside_closed_attrs() {
        for (text, expected) in [
            (
                r#"embed {src="example.com"} <!-- note } -->"#,
                named_args(&[("src", "example.com")]),
            ),
            ("embed extra text", BTreeMap::new()),
            (r#"embed {title="a}b"#, BTreeMap::new()),
        ] {
            let (name, args) = parse_directive_head(text);
            assert_eq!(name, "embed", "{text}");
            assert_eq!(args.named, expected, "{text}");
            assert_eq!(args.positional, Vec::<String>::new(), "{text}");
        }
    }

    // ── body_end ──

    #[test]
    fn excludes_one_line_ending() {
        for (content, expected) in [
            ("Hello\n", 5),
            ("Hello\r\n", 5),
            ("Hello\n\n", 6),
            ("Hello", 5),
            ("", 0),
        ] {
            assert_eq!(body_end(content, 0, content.len()), expected, "{content:?}");
        }
    }

    fn body<'a>(input: &'a str, node: &DirectiveNode) -> &'a str {
        &input[node.block.body.clone()]
    }
}
