use std::sync::LazyLock;

use regex::Regex;

/// Matches GitHub-style emoji shortcodes, e.g., `:smile:`, `:+1:`.
///
/// Character set mirrors GitHub's shortcode names: lowercase ASCII, digits,
/// underscores, hyphens, and `+`.
static EMOJI_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^:([a-z0-9_+\-]+):").expect("emoji regex should compile"));

/// Returns the emoji for a shortcode in GitHub's set at the start of `text`, with the shortcode's
/// byte length.
pub(super) fn match_emoji(text: &str) -> Option<(&str, usize)> {
    let (shortcode, [name]) = EMOJI_RE.captures(text)?.extract();
    Some((gh_emoji::get(name)?, shortcode.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── match_emoji ──

    #[test]
    fn matches_known_shortcodes_with_length() {
        assert_eq!(match_emoji(":rocket: and more"), Some(("\u{1f680}", 8)));
        assert_eq!(match_emoji(":+1:"), Some(("\u{1f44d}", 4)));
    }

    #[test]
    fn ignores_unknown_or_malformed_shortcodes() {
        for text in [":not_a_real_emoji:", ": smile:", ":30 today:", "smile:"] {
            assert_eq!(match_emoji(text), None, "{text}");
        }
    }
}
