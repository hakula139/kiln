use std::fmt::Write;
use std::sync::LazyLock;

use regex::Regex;

use super::escape_html;
use crate::markdown::{for_each_non_code_line, scan_code_span};

/// Matches icon shortcodes, e.g., `:(fas fa-link):`.
static ICON_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r":\(([^)]+)\):").expect("icon regex should compile"));

/// Replaces `:(class):` shortcodes with `<i>` tags.
///
/// Skips replacements inside fenced code blocks (` ``` ` / `~~~`) and
/// inline code spans (`` ` ``).
#[must_use]
pub fn replace_icons(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    for_each_non_code_line(input, &mut output, |line, out| {
        replace_icons_in_line(line, out);
    });
    output
}

fn replace_icons_in_line(line: &str, output: &mut String) {
    let bytes = line.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'`' {
            let (end, span) = scan_code_span(line, i);
            output.push_str(span);
            i = end;
        } else if bytes[i] == b':'
            && let Some(caps) = ICON_RE.captures(&line[i..])
            && caps.get(0).unwrap().start() == 0
        {
            _ = write!(
                output,
                "<i class=\"{}\" aria-hidden=\"true\"></i>",
                escape_html(&caps[1])
            );
            i += caps[0].len();
        } else {
            let ch = line[i..].chars().next().unwrap();
            output.push(ch);
            i += ch.len_utf8();
        }
    }
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::*;

    // -- replace_icons --

    #[test]
    fn replace_icons_single() {
        let input = "Click :(fas fa-link): here";
        let output = replace_icons(input);
        assert_eq!(
            output,
            r#"Click <i class="fas fa-link" aria-hidden="true"></i> here"#
        );
    }

    #[test]
    fn replace_icons_multiple() {
        let input = ":(fas fa-home): and :(fas fa-cog):";
        let output = replace_icons(input);
        assert!(output.contains(r#"class="fas fa-home""#));
        assert!(output.contains(r#"class="fas fa-cog""#));
    }

    #[test]
    fn replace_icons_no_match_passthrough() {
        let input = "plain text";
        let output = replace_icons(input);
        assert_eq!(output, input);
    }

    #[test]
    fn replace_icons_escapes_html() {
        let input = ":(fas fa-<script>):";
        let output = replace_icons(input);
        assert!(output.contains("fa-&lt;script&gt;"), "output:\n{output}");
    }

    #[test]
    fn replace_icons_unclosed_backtick() {
        // Unclosed backtick is treated as literal, so the icon is still replaced.
        let input = "`:(fas fa-link):";
        let output = replace_icons(input);
        assert!(
            output.contains(r#"class="fas fa-link""#),
            "output:\n{output}"
        );
    }

    // -- replace_icons (code awareness) --

    #[test]
    fn replace_icons_skips_inline_code() {
        let input = "use `:(fas fa-link):` syntax";
        let output = replace_icons(input);
        assert!(output.contains(":(fas fa-link):"), "output:\n{output}");
    }

    #[test]
    fn replace_icons_skips_fenced_code() {
        // Backtick fences.
        let input = indoc! {"
            ```
            :(fas fa-link):
            ```
        "};
        let output = replace_icons(input);
        assert!(output.contains(":(fas fa-link):"), "output:\n{output}");

        // Tilde fences.
        let input = indoc! {"
            ~~~
            :(fas fa-link):
            ~~~
        "};
        let output = replace_icons(input);
        assert!(output.contains(":(fas fa-link):"), "output:\n{output}");
    }

    #[test]
    fn replace_icons_after_fenced_code() {
        let input = indoc! {"
            ```
            code
            ```
            :(fas fa-link):
        "};
        let output = replace_icons(input);
        assert!(
            output.contains(r#"class="fas fa-link""#),
            "output:\n{output}"
        );
    }
}
