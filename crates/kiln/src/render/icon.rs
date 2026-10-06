use std::fmt::Write;
use std::sync::LazyLock;

use regex::Regex;

use crate::html::escape;
use crate::markdown::replace_shortcodes;

/// Matches icon shortcodes, e.g., `:(fas fa-link):`.
static ICON_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^:\(([^)]+)\):").expect("icon regex should compile"));

/// Replaces `:(class):` shortcodes with `<i>` tags.
///
/// Skips replacements inside fenced code blocks (` ``` ` / `~~~`) and inline code spans (`` ` ``).
#[must_use]
pub fn replace_icons(input: &str) -> String {
    replace_shortcodes(input, |rest, output| {
        let caps = ICON_RE.captures(rest)?;
        _ = write!(
            output,
            r#"<i class="{}" aria-hidden="true"></i>"#,
            escape(&caps[1])
        );
        Some(caps[0].len())
    })
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::*;

    // ── replace_icons ──

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
        assert_eq!(
            output,
            r#"<i class="fas fa-home" aria-hidden="true"></i> and <i class="fas fa-cog" aria-hidden="true"></i>"#
        );
    }

    #[test]
    fn replace_icons_escapes_html() {
        let input = ":(fas fa-<script>):";
        let output = replace_icons(input);
        assert_eq!(
            output,
            r#"<i class="fas fa-&lt;script&gt;" aria-hidden="true"></i>"#
        );
    }

    #[test]
    fn replace_icons_preserves_nonmatching_prefix_before_shortcode() {
        assert_eq!(
            replace_icons(": invalid :(fas fa-link):"),
            r#": invalid <i class="fas fa-link" aria-hidden="true"></i>"#
        );
    }

    #[test]
    fn replace_icons_no_match_passthrough() {
        let input = "plain text";
        let output = replace_icons(input);
        assert_eq!(output, input);
    }

    // ── replace_icons (code awareness) ──

    #[test]
    fn replace_icons_skips_inline_code() {
        let input = "use `:(fas fa-link):` syntax";
        let output = replace_icons(input);
        assert_eq!(output, input);
    }

    #[test]
    fn replace_icons_skips_fenced_code() {
        let input = indoc! {"
            ```
            :(fas fa-link):
            ```
        "};
        let output = replace_icons(input);
        assert_eq!(output, input);

        let input = indoc! {"
            ~~~
            :(fas fa-link):
            ~~~
        "};
        let output = replace_icons(input);
        assert_eq!(output, input);
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
        assert_eq!(
            output,
            input.replace(
                ":(fas fa-link):",
                r#"<i class="fas fa-link" aria-hidden="true"></i>"#,
            )
        );
    }

    #[test]
    fn replace_icons_unclosed_backtick() {
        let input = "`:(fas fa-link):";
        let output = replace_icons(input);
        assert_eq!(output, r#"`<i class="fas fa-link" aria-hidden="true"></i>"#);
    }
}
