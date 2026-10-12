use std::sync::LazyLock;

use regex::Regex;

/// Matches icon shortcodes, e.g., `:(fas fa-link):`. The class charset keeps emoticons such as
/// `:( (really):` literal and needs no HTML escaping.
static ICON_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^:\(([\w -]+)\):").expect("icon regex should compile"));

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
