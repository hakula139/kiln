use std::ops::Range;

use pulldown_cmark::{Event, Options, Parser, Tag};

pub(crate) fn markdown_options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_HEADING_ATTRIBUTES
        | Options::ENABLE_MATH
}

/// Source ranges protected from Markdown extensions, including container-nested code.
pub(crate) fn code_ranges(input: &str) -> Vec<Range<usize>> {
    Parser::new_ext(input, markdown_options())
        .into_offset_iter()
        .filter_map(|(event, mut range)| match event {
            Event::Code(_) => Some(range),
            Event::Start(Tag::CodeBlock(_)) => {
                if input[range.end..].starts_with("\r\n") {
                    range.end += 2;
                } else if input[range.end..].starts_with('\n') {
                    range.end += 1;
                }
                Some(range)
            }
            _ => None,
        })
        .collect()
}

/// Replaces colon-prefixed shortcodes outside Markdown code.
///
/// `replace` appends a replacement and returns its consumed byte count, or returns `None`
/// without writing. A match must consume a nonempty prefix ending at a UTF-8 boundary.
#[must_use]
pub(crate) fn replace_shortcodes(
    input: &str,
    mut replace: impl FnMut(&str, &mut String) -> Option<usize>,
) -> String {
    let mut output = String::with_capacity(input.len());
    for_each_non_code_line(input, &mut output, |line, output| {
        let mut rest = line;
        while !rest.is_empty() {
            if rest.starts_with(':')
                && let Some(consumed) = replace(rest, output)
            {
                rest = &rest[consumed..];
            } else if let Some(ch) = rest.chars().next() {
                output.push(ch);
                rest = &rest[ch.len_utf8()..];
            }
        }
    });
    output
}

/// Processes non-code line fragments, preserving protected source ranges verbatim.
pub(crate) fn for_each_non_code_line(
    input: &str,
    output: &mut String,
    mut f: impl FnMut(&str, &mut String),
) {
    let mut copied = 0;
    for range in code_ranges(input) {
        for line in input[copied..range.start].split_inclusive('\n') {
            f(line, output);
        }
        output.push_str(&input[range.clone()]);
        copied = range.end;
    }
    for line in input[copied..].split_inclusive('\n') {
        f(line, output);
    }
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::*;

    // ── replace_shortcodes ──

    #[test]
    fn replace_shortcodes_preserves_unicode_and_unmatched_colons() {
        assert_eq!(
            replace_shortcodes("文 :unknown: :x::x: 尾", replace_test_shortcode),
            "文 :unknown: XX 尾"
        );
    }

    #[test]
    fn replace_shortcodes_preserves_code_and_line_endings() {
        let input = indoc! {"
            :x: `:x:` ``:x: ` :x:``
            ```text
            :x:
            ```
            ~~~
            :x:
            ~~~
            `:x:
        "};
        let expected = indoc! {"
            X `:x:` ``:x: ` :x:``
            ```text
            :x:
            ```
            ~~~
            :x:
            ~~~
            `X
        "};
        for (input, expected) in [
            (input.to_owned(), expected.to_owned()),
            (input.replace('\n', "\r\n"), expected.replace('\n', "\r\n")),
            (input.trim_end().to_owned(), expected.trim_end().to_owned()),
        ] {
            assert_eq!(replace_shortcodes(&input, replace_test_shortcode), expected);
        }
    }

    #[test]
    fn replace_shortcodes_preserves_all_markdown_code_contexts() {
        for input in [
            indoc! {"
                `first
                :x:`
            "},
            "    :x:",
            indoc! {"
                > ```text
                > :x:
                > ```
            "},
            indoc! {"
                - ```text
                  :x:
                  ```
            "},
        ] {
            assert_eq!(replace_shortcodes(input, replace_test_shortcode), input);
        }
    }

    fn replace_test_shortcode(rest: &str, output: &mut String) -> Option<usize> {
        rest.strip_prefix(":x:")?;
        output.push('X');
        Some(3)
    }

    // ── for_each_non_code_line ──

    #[test]
    fn for_each_non_code_line_skips_fenced_code() {
        let input = indoc! {"
            before
            ```
            code
            ```
            after
        "};
        let mut processed = Vec::new();
        let mut out = String::new();
        for_each_non_code_line(input, &mut out, |line, o| {
            processed.push(line.trim_end().to_string());
            o.push_str(&line.to_uppercase());
        });
        assert_eq!(processed, vec!["before", "after"]);
        assert_eq!(
            out,
            indoc! {"
                BEFORE
                ```
                code
                ```
                AFTER
            "}
        );
    }
}
