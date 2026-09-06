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
pub fn slugify(text: &str) -> String {
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

/// Converts a hyphenated slug to titlecase (e.g., "hello-world" → "Hello World").
#[must_use]
pub fn titlecase(s: &str) -> String {
    s.split('-')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(c) => {
                    let upper: String = c.to_uppercase().collect();
                    upper + chars.as_str()
                }
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── slugify ──

    #[test]
    fn slugify_ascii() {
        assert_eq!(slugify("Hello World"), "hello-world");
    }

    #[test]
    fn slugify_cjk() {
        assert_eq!(slugify("你好世界"), "你好世界");
    }

    #[test]
    fn slugify_accented_latin() {
        assert_eq!(slugify("Café Résumé"), "café-résumé");
    }

    #[test]
    fn slugify_mixed() {
        assert_eq!(slugify("1.1 Foobar - 测试文本"), "1.1-foobar-测试文本");
    }

    #[test]
    fn slugify_preserves_url_safe_punctuation() {
        assert_eq!(slugify("C++"), "c++");
        assert_eq!(slugify(".NET"), ".net");
        assert_eq!(slugify("C/C++"), "c-c++");
        assert_eq!(slugify("C"), "c", "should stay distinct from C++");
    }

    #[test]
    fn slugify_separates_on_unpreserved_punctuation() {
        assert_eq!(slugify("CS:APP"), "cs-app");
        assert_eq!(slugify("Rock & Roll"), "rock-roll");
    }

    #[test]
    fn slugify_collapses_dashes() {
        assert_eq!(slugify("a - - b"), "a-b");
    }

    #[test]
    fn slugify_strips_leading_trailing() {
        assert_eq!(slugify(" hello "), "hello");
    }

    #[test]
    fn slugify_empty() {
        assert_eq!(slugify(""), "");
    }

    #[test]
    fn slugify_only_punctuation() {
        assert_eq!(slugify("..."), "");
    }

    // ── titlecase ──

    #[test]
    fn titlecase_basic() {
        assert_eq!(titlecase("hello-world"), "Hello World");
    }

    #[test]
    fn titlecase_single_word() {
        assert_eq!(titlecase("note"), "Note");
    }

    #[test]
    fn titlecase_already_capitalized() {
        assert_eq!(titlecase("VPS"), "VPS");
    }

    #[test]
    fn titlecase_empty() {
        assert_eq!(titlecase(""), "");
    }
}
