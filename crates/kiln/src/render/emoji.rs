use std::sync::LazyLock;

use regex::Regex;

use crate::markdown::replace_shortcodes;

/// Matches GitHub-style emoji shortcodes, e.g., `:smile:`, `:+1:`.
///
/// Character set mirrors GitHub's shortcode names: lowercase ASCII, digits,
/// underscores, hyphens, and `+`.
static EMOJI_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^:([a-z0-9_+\-]+):").expect("emoji regex should compile"));

/// Replaces `:shortcode:` emoji shortcodes with Unicode emoji characters.
///
/// Only shortcodes recognized by GitHub's emoji set are replaced, and unknown
/// shortcodes pass through unchanged. Skips replacements inside fenced code
/// blocks (` ``` ` / `~~~`) and inline code spans (`` ` ``).
#[must_use]
pub fn replace_emojis(input: &str) -> String {
    replace_shortcodes(input, |rest, output| {
        let caps = EMOJI_RE.captures(rest)?;
        let emoji = gh_emoji::get(&caps[1])?;
        output.push_str(emoji);
        Some(caps[0].len())
    })
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::*;

    // ── replace_emojis ──

    #[test]
    fn replace_emojis_single() {
        assert_eq!(replace_emojis("Hello :smile:!"), "Hello \u{1f604}!");
    }

    #[test]
    fn replace_emojis_multiple() {
        let output = replace_emojis(":rocket: and :+1:");
        assert_eq!(output, "\u{1f680} and \u{1f44d}");
    }

    #[test]
    fn replace_emojis_preserves_nonmatching_prefix_before_shortcode() {
        assert_eq!(replace_emojis(": invalid :smile:"), ": invalid \u{1f604}");
    }

    #[test]
    fn replace_emojis_no_match_passthrough() {
        let input = "plain text";
        let output = replace_emojis(input);
        assert_eq!(output, input);
    }

    #[test]
    fn replace_emojis_unknown_shortcode_passthrough() {
        let input = ":not_a_real_emoji:";
        let output = replace_emojis(input);
        assert_eq!(output, input);
    }

    #[test]
    fn replace_emojis_colon_in_url_passthrough() {
        let input = "Visit https://example.com for more:";
        let output = replace_emojis(input);
        assert_eq!(output, input);
    }

    #[test]
    fn replace_emojis_time_format_passthrough() {
        let input = "Meet at 12:30 today:";
        let output = replace_emojis(input);
        assert_eq!(output, input);
    }

    // ── replace_emojis (code awareness) ──

    #[test]
    fn replace_emojis_skips_inline_code() {
        let input = "use `:smile:` syntax";
        let output = replace_emojis(input);
        assert_eq!(output, input);
    }

    #[test]
    fn replace_emojis_skips_fenced_code() {
        let input = indoc! {"
            ```
            :smile:
            ```
        "};
        let output = replace_emojis(input);
        assert_eq!(output, input);

        let input = indoc! {"
            ~~~
            :smile:
            ~~~
        "};
        let output = replace_emojis(input);
        assert_eq!(output, input);
    }

    #[test]
    fn replace_emojis_after_fenced_code() {
        let input = indoc! {"
            ```
            code
            ```
            :smile:
        "};
        let output = replace_emojis(input);
        assert_eq!(output, input.replace(":smile:", "\u{1f604}"));
    }

    #[test]
    fn replace_emojis_unclosed_backtick() {
        let input = "`:smile:";
        let output = replace_emojis(input);
        assert_eq!(output, "`\u{1f604}");
    }
}
