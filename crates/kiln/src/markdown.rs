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

#[cfg(test)]
mod tests {
    use indoc::{formatdoc, indoc};

    use super::*;

    // ── code_ranges ──

    #[test]
    fn code_ranges_cover_code_spans_and_fences_with_line_endings() {
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
        for newline in ["\n", "\r\n"] {
            let input = input.replace('\n', newline);
            let protected: Vec<_> = code_ranges(&input)
                .into_iter()
                .map(|range| &input[range])
                .collect();
            let backtick_fence = formatdoc! {"
                ```text
                :x:
                ```
            "};
            let tilde_fence = formatdoc! {"
                ~~~
                :x:
                ~~~
            "};
            assert_eq!(
                protected,
                [
                    "`:x:`",
                    "``:x: ` :x:``",
                    &backtick_fence.replace('\n', newline),
                    &tilde_fence.replace('\n', newline),
                ]
            );
        }
    }

    #[test]
    fn code_ranges_cover_multiline_indented_and_nested_code() {
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
            let ranges = code_ranges(input);
            let (offset, _) = input.match_indices(":x:").next().unwrap();
            assert!(
                ranges.iter().any(|range| range.contains(&offset)),
                "{input:?}: {ranges:?}"
            );
        }
    }

    #[test]
    fn code_ranges_cover_fenced_blocks_through_closing_line() {
        for input in [
            indoc! {"
                ```
                ::: a
                ```
            "},
            indoc! {"
                ~~~
                ::: a
                ~~~\r
            "},
            indoc! {"
                   ```
                ::: a
                   ```
            "},
        ] {
            let start = input.find(['`', '~']).unwrap();
            assert_eq!(only_code_range(input), start..input.len(), "{input:?}");
        }
    }

    #[test]
    fn code_ranges_unmatched_closing_fence_extends_to_end() {
        for input in [
            indoc! {"
                ```
                ::: a
                ~~~
            "},
            indoc! {"
                ```
                ::: a
                    ```
            "},
        ] {
            assert_eq!(only_code_range(input), 0..input.len(), "{input:?}");
        }
    }

    #[test]
    fn code_ranges_reject_invalid_fence_openers() {
        let indented = indoc! {"
                ```
            ::: a
        "};
        assert_eq!(
            only_code_range(indented),
            indented.find('`').unwrap()..indented.find(':').unwrap()
        );
        for input in [
            indoc! {"
                ``
                ::: a
            "},
            indoc! {"
                ```a`b
                ::: a
            "},
        ] {
            assert_eq!(code_ranges(input), Vec::<Range<usize>>::new(), "{input:?}");
        }
    }

    fn only_code_range(input: &str) -> Range<usize> {
        let [range] = code_ranges(input)
            .try_into()
            .unwrap_or_else(|ranges| panic!("{input:?}: {ranges:?}"));
        range
    }
}
