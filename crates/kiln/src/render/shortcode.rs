use std::sync::LazyLock;

use regex::Regex;

/// Matches GitHub-style `:shortcode:` names (lowercase ASCII, digits, `_`, `-`, and `+`).
static EMOJI_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^:([a-z0-9_+\-]+):").expect("emoji regex should compile"));

/// Matches `:(class):` icon shortcodes. The class charset keeps emoticons literal and needs no
/// HTML escaping.
static ICON_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^:\(([\w -]+)\):").expect("icon regex should compile"));

/// Returns the emoji for a shortcode in GitHub's set at the start of `text`, with the shortcode's
/// byte length.
pub(super) fn match_emoji(text: &str) -> Option<(&str, usize)> {
    let (shortcode, [name]) = EMOJI_RE.captures(text)?.extract();
    Some((gh_emoji::get(name)?, shortcode.len()))
}

/// Returns an `<i>` element for an icon shortcode at the start of `text`, with the shortcode's
/// byte length.
pub(super) fn match_icon(text: &str) -> Option<(String, usize)> {
    let (shortcode, [class]) = ICON_RE.captures(text)?.extract();
    let html = format!(r#"<i class="{class}" aria-hidden="true"></i>"#);
    Some((html, shortcode.len()))
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

    // ── match_icon ──

    #[test]
    fn renders_class_shortcode_as_icon() {
        assert_eq!(
            match_icon(":(fas fa-link): here"),
            Some((
                r#"<i class="fas fa-link" aria-hidden="true"></i>"#.to_owned(),
                15
            ))
        );
    }

    #[test]
    fn ignores_emoticons_and_markup() {
        for text in [
            ":( (really):",
            ":(fas fa-<script>):",
            ":():",
            "(fas fa-link):",
        ] {
            assert_eq!(match_icon(text), None, "{text}");
        }
    }
}
