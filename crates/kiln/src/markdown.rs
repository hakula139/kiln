/// Strips up to 3 spaces of leading indentation for code fence detection.
fn strip_fence_indent(line: &str) -> Option<&str> {
    let indent = line.bytes().take_while(|&b| b == b' ').count();
    (indent <= 3).then_some(&line[indent..])
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

#[cfg(test)]
mod tests {
    use super::*;

    // -- strip_fence_indent --

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

    // -- detect_opening_code_fence --

    #[test]
    fn detect_opening_backtick_fence() {
        assert_eq!(detect_opening_code_fence("```"), Some((b'`', 3)));
        assert_eq!(detect_opening_code_fence("````"), Some((b'`', 4)));
    }

    #[test]
    fn detect_opening_tilde_fence() {
        assert_eq!(detect_opening_code_fence("~~~"), Some((b'~', 3)));
        assert_eq!(detect_opening_code_fence("~~~~"), Some((b'~', 4)));
    }

    #[test]
    fn detect_opening_with_info_string() {
        assert_eq!(detect_opening_code_fence("```rust"), Some((b'`', 3)));
        assert_eq!(detect_opening_code_fence("~~~python"), Some((b'~', 3)));
    }

    #[test]
    fn detect_opening_indented() {
        assert_eq!(detect_opening_code_fence("   ```"), Some((b'`', 3)));
    }

    #[test]
    fn detect_opening_fewer_than_three_returns_none() {
        assert_eq!(detect_opening_code_fence("``"), None);
        assert_eq!(detect_opening_code_fence("~~"), None);
    }

    #[test]
    fn detect_opening_over_indented_returns_none() {
        assert_eq!(detect_opening_code_fence("    ```"), None);
    }

    #[test]
    fn detect_opening_non_fence_char_returns_none() {
        assert_eq!(detect_opening_code_fence("---"), None);
        assert_eq!(detect_opening_code_fence("plain text"), None);
    }

    #[test]
    fn detect_opening_backtick_in_info_string_returns_none() {
        assert_eq!(detect_opening_code_fence("```foo`bar"), None);
    }

    // -- is_closing_code_fence --

    #[test]
    fn is_closing_matching_fence() {
        assert!(is_closing_code_fence("```", b'`', 3));
        assert!(is_closing_code_fence("~~~", b'~', 3));
    }

    #[test]
    fn is_closing_longer_fence_closes() {
        assert!(is_closing_code_fence("````", b'`', 3));
    }

    #[test]
    fn is_closing_trailing_whitespace_allowed() {
        assert!(is_closing_code_fence("```   ", b'`', 3));
    }

    #[test]
    fn is_closing_shorter_fence_returns_false() {
        assert!(!is_closing_code_fence("```", b'`', 4));
    }

    #[test]
    fn is_closing_mismatched_char_returns_false() {
        assert!(!is_closing_code_fence("~~~", b'`', 3));
    }

    #[test]
    fn is_closing_trailing_text_returns_false() {
        assert!(!is_closing_code_fence("``` foo", b'`', 3));
    }

    #[test]
    fn is_closing_over_indented_returns_false() {
        assert!(!is_closing_code_fence("    ```", b'`', 3));
    }
}
