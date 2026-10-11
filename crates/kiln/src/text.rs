use itertools::Itertools;

/// Non-alphanumeric characters kept literally, because they carry meaning in technology names
/// (`C++`, `.NET`) and stay safe unescaped in a URL path. `#` cannot join them since it delimits
/// a URL fragment.
const PRESERVED_CHARS: [char; 4] = ['+', '.', '_', '~'];

/// Converts text into a URL-safe slug.
///
/// Alphanumerics are lowercased, Unicode-aware, so CJK and accented characters survive.
/// [`PRESERVED_CHARS`] are kept as-is. Every other character becomes a single `-`, collapsing runs
/// and stripping leading / trailing `-`. Returns an empty string when no alphanumeric character
/// survives the pass.
#[must_use]
pub(crate) fn slugify(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut prev_dash = true; // strip leading dashes
    let mut has_alphanumeric = false;

    for ch in text.chars() {
        if ch.is_alphanumeric() {
            for lower in ch.to_lowercase() {
                result.push(lower);
            }
            prev_dash = false;
            has_alphanumeric = true;
        } else if PRESERVED_CHARS.contains(&ch) {
            result.push(ch);
            prev_dash = false;
        } else if !prev_dash {
            result.push('-');
            prev_dash = true;
        }
    }

    if !has_alphanumeric {
        return String::new();
    }
    if result.ends_with('-') {
        result.pop();
    }

    result
}

/// Returns the slug of `text`, or `fallback` when no alphanumeric character survives.
#[must_use]
pub(crate) fn slugify_or(text: &str, fallback: &str) -> String {
    let slug = slugify(text);
    if slug.is_empty() {
        fallback.to_owned()
    } else {
        slug
    }
}

/// Converts a hyphenated slug to titlecase (e.g., "hello-world" → "Hello World").
#[must_use]
pub(crate) fn titlecase(s: &str) -> String {
    s.split('-').map(capitalize).join(" ")
}

/// Uppercases the first character of `word`.
#[must_use]
pub(crate) fn capitalize(word: &str) -> String {
    let mut chars = word.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── slugify ──

    #[test]
    fn preserves_letters_and_url_safe_punctuation() {
        for (input, expected) in [
            ("Hello World", "hello-world"),
            ("你好世界", "你好世界"),
            ("Café Résumé", "café-résumé"),
            ("1.1 Foobar - 测试文本", "1.1-foobar-测试文本"),
            ("C++", "c++"),
            (".NET", ".net"),
            ("C/C++", "c-c++"),
            ("C", "c"),
        ] {
            assert_eq!(slugify(input), expected, "{input}");
        }
    }

    #[test]
    fn collapses_separators_and_trims_edges() {
        for (input, expected) in [
            ("CS:APP", "cs-app"),
            ("Rock & Roll", "rock-roll"),
            ("a - - b", "a-b"),
            (" hello ", "hello"),
            ("", ""),
            ("...", ""),
        ] {
            assert_eq!(slugify(input), expected, "{input}");
        }
    }

    // ── slugify_or ──

    #[test]
    fn falls_back_only_without_alphanumerics() {
        for (input, expected) in [("Hello World", "hello-world"), ("!?", "note"), ("", "note")] {
            assert_eq!(slugify_or(input, "note"), expected, "{input}");
        }
    }

    // ── titlecase ──

    #[test]
    fn capitalizes_words() {
        for (input, expected) in [
            ("hello-world", "Hello World"),
            ("note", "Note"),
            ("VPS", "VPS"),
            ("", ""),
        ] {
            assert_eq!(titlecase(input), expected, "{input}");
        }
    }
}
