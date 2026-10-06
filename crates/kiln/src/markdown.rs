/// Replaces colon-prefixed shortcodes outside fenced blocks and inline code spans.
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
        replace_shortcodes_in_line(line, output, &mut replace);
    });
    output
}

fn replace_shortcodes_in_line(
    line: &str,
    output: &mut String,
    replace: &mut impl FnMut(&str, &mut String) -> Option<usize>,
) {
    let bytes = line.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'`' {
            let (end, span) = scan_code_span(line, i);
            output.push_str(span);
            i = end;
            continue;
        }

        if bytes[i] == b':'
            && let Some(consumed) = replace(&line[i..], output)
        {
            i += consumed;
            continue;
        }

        let ch = line[i..].chars().next().unwrap();
        output.push(ch);
        i += ch.len_utf8();
    }
}

/// Processes markdown line-by-line, passing each line outside fenced code
/// blocks to `f`. Lines inside code blocks are appended to `output` unchanged.
pub(crate) fn for_each_non_code_line(
    input: &str,
    output: &mut String,
    mut f: impl FnMut(&str, &mut String),
) {
    let mut in_fenced_code = false;
    let mut fence_char: u8 = 0;
    let mut fence_count: usize = 0;

    for line in input.split_inclusive('\n') {
        if in_fenced_code {
            if is_closing_code_fence(line, fence_char, fence_count) {
                in_fenced_code = false;
            }
            output.push_str(line);
            continue;
        }
        if let Some((ch, count)) = detect_opening_code_fence(line) {
            in_fenced_code = true;
            fence_char = ch;
            fence_count = count;
            output.push_str(line);
            continue;
        }
        f(line, output);
    }
}

/// Detects an opening code fence (three or more `` ` `` or `~` characters).
/// Handles up to 3 spaces of leading indentation.
#[must_use]
pub(crate) fn detect_opening_code_fence(line: &str) -> Option<(u8, usize)> {
    let rest = strip_fence_indent(line)?;
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
#[must_use]
pub(crate) fn is_closing_code_fence(line: &str, fence_char: u8, min_count: usize) -> bool {
    let Some(rest) = strip_fence_indent(line) else {
        return false;
    };
    let count = rest.bytes().take_while(|&b| b == fence_char).count();
    count >= min_count && rest[count..].trim().is_empty()
}

fn strip_fence_indent(line: &str) -> Option<&str> {
    let indent = line.bytes().take_while(|&b| b == b' ').count();
    (indent <= 3).then_some(&line[indent..])
}

/// Scans a backtick code span starting at `start` within `line`.
///
/// Returns `(end, span)` where `end` is the byte offset past the closing backticks and `span` is
/// the raw slice. Treats unclosed backtick runs as literal text.
#[must_use]
pub(crate) fn scan_code_span(line: &str, start: usize) -> (usize, &str) {
    let bytes = line.as_bytes();
    let mut i = start;

    let open_count = count_backticks(bytes, i);
    i += open_count;

    while i < bytes.len() {
        if bytes[i] == b'`' {
            let close_count = count_backticks(bytes, i);
            if close_count == open_count {
                let end = i + close_count;
                return (end, &line[start..end]);
            }
            i += close_count;
        } else {
            i += 1;
        }
    }

    (start + open_count, &line[start..start + open_count])
}

fn count_backticks(bytes: &[u8], start: usize) -> usize {
    bytes[start..].iter().take_while(|&&b| b == b'`').count()
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

    fn replace_test_shortcode(rest: &str, output: &mut String) -> Option<usize> {
        rest.strip_prefix(":x:")?;
        output.push('X');
        Some(3)
    }

    // ── for_each_non_code_line ──

    #[test]
    fn for_each_non_code_line_processes_normal_lines() {
        let input = indoc! {"
            a
            b
        "};
        let mut out = String::new();
        for_each_non_code_line(input, &mut out, |line, o| o.push_str(line));
        assert_eq!(out, input);
    }

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
            o.push_str(line);
        });
        assert_eq!(processed, vec!["before", "after"]);
    }

    // ── detect_opening_code_fence ──

    #[test]
    fn detect_opening_code_fence_backtick_fence() {
        assert_eq!(detect_opening_code_fence("```"), Some((b'`', 3)));
        assert_eq!(detect_opening_code_fence("````"), Some((b'`', 4)));
    }

    #[test]
    fn detect_opening_code_fence_tilde_fence() {
        assert_eq!(detect_opening_code_fence("~~~"), Some((b'~', 3)));
        assert_eq!(detect_opening_code_fence("~~~~"), Some((b'~', 4)));
    }

    #[test]
    fn detect_opening_code_fence_with_info_string() {
        assert_eq!(detect_opening_code_fence("```rust"), Some((b'`', 3)));
        assert_eq!(detect_opening_code_fence("~~~python"), Some((b'~', 3)));
    }

    #[test]
    fn detect_opening_code_fence_indented() {
        assert_eq!(detect_opening_code_fence("   ```"), Some((b'`', 3)));
    }

    #[test]
    fn detect_opening_code_fence_fewer_than_three_returns_none() {
        assert_eq!(detect_opening_code_fence("``"), None);
        assert_eq!(detect_opening_code_fence("~~"), None);
    }

    #[test]
    fn detect_opening_code_fence_over_indented_returns_none() {
        assert_eq!(detect_opening_code_fence("    ```"), None);
    }

    #[test]
    fn detect_opening_code_fence_non_fence_char_returns_none() {
        assert_eq!(detect_opening_code_fence("---"), None);
        assert_eq!(detect_opening_code_fence("plain text"), None);
    }

    #[test]
    fn detect_opening_code_fence_backtick_in_info_string_returns_none() {
        assert_eq!(detect_opening_code_fence("```foo`bar"), None);
    }

    // ── is_closing_code_fence ──

    #[test]
    fn is_closing_code_fence_matching_fence() {
        assert!(is_closing_code_fence("```", b'`', 3));
        assert!(is_closing_code_fence("~~~", b'~', 3));
    }

    #[test]
    fn is_closing_code_fence_longer_fence_closes() {
        assert!(is_closing_code_fence("````", b'`', 3));
    }

    #[test]
    fn is_closing_code_fence_trailing_whitespace_allowed() {
        assert!(is_closing_code_fence("```   ", b'`', 3));
    }

    #[test]
    fn is_closing_code_fence_shorter_fence_returns_false() {
        assert!(!is_closing_code_fence("```", b'`', 4));
    }

    #[test]
    fn is_closing_code_fence_mismatched_char_returns_false() {
        assert!(!is_closing_code_fence("~~~", b'`', 3));
    }

    #[test]
    fn is_closing_code_fence_trailing_text_returns_false() {
        assert!(!is_closing_code_fence("``` foo", b'`', 3));
    }

    #[test]
    fn is_closing_code_fence_over_indented_returns_false() {
        assert!(!is_closing_code_fence("    ```", b'`', 3));
    }

    // ── strip_fence_indent ──

    #[test]
    fn strip_fence_indent_zero_to_three_spaces() {
        assert_eq!(strip_fence_indent("```"), Some("```"));
        assert_eq!(strip_fence_indent(" ```"), Some("```"));
        assert_eq!(strip_fence_indent("  ```"), Some("```"));
        assert_eq!(strip_fence_indent("   ```"), Some("```"));
    }

    #[test]
    fn strip_fence_indent_empty_line() {
        assert_eq!(strip_fence_indent(""), Some(""));
    }

    #[test]
    fn strip_fence_indent_four_spaces_returns_none() {
        assert_eq!(strip_fence_indent("    ```"), None);
    }

    // ── scan_code_span ──

    #[test]
    fn scan_code_span_matched() {
        assert_eq!(scan_code_span("`code` rest", 0), (6, "`code`"));
    }

    #[test]
    fn scan_code_span_double_backtick() {
        assert_eq!(scan_code_span("``co`de`` rest", 0), (9, "``co`de``"));
    }

    #[test]
    fn scan_code_span_mid_line() {
        assert_eq!(scan_code_span("pre `code` post", 4), (10, "`code`"));
    }

    #[test]
    fn scan_code_span_unclosed() {
        assert_eq!(scan_code_span("`unclosed", 0), (1, "`"));
    }
}
