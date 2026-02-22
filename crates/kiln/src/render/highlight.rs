use std::fmt::Write;

use syntect::html::{ClassStyle, ClassedHTMLGenerator};
use syntect::parsing::{SyntaxReference, SyntaxSet};
use syntect::util::LinesWithEndings;

use tracing::warn;

use super::escape_html;

/// Highlights a code block using syntect CSS-class-based spans with line numbers.
///
/// All code blocks are wrapped in a `<div class="highlight">` table with line
/// numbers. Every `<code>` element carries `class="language-..."` and
/// `data-lang="..."` attributes.
///
/// Language labels are canonicalized: derived from syntect's syntax name,
/// lowercased. Empty tags normalize to `"plaintext"`; unrecognized tags
/// lowercase the original token and fall back to plain text highlighting.
#[must_use]
pub fn highlight_code(syntax_set: &SyntaxSet, lang: &str, code: &str) -> String {
    let (syntax, effective_lang) = find_syntax(syntax_set, lang);

    let mut generator =
        ClassedHTMLGenerator::new_with_class_style(syntax, syntax_set, ClassStyle::Spaced);

    for line in LinesWithEndings::from(code) {
        if let Err(e) = generator.parse_html_for_line_which_includes_newline(line) {
            warn!(lang, error = %e, "syntax highlighting failed for line, falling back to plain text");
        }
    }

    let highlighted = generator.finalize();
    let line_count = code.lines().count().max(1);

    let mut html =
        String::with_capacity(highlighted.len() + line_count * 8 + 2 * effective_lang.len() + 192);
    html.push_str("<div class=\"highlight\">\n<table>\n<tr>\n");

    // Line numbers column.
    html.push_str(r#"<td class="line-numbers"><pre>"#);
    for i in 1..=line_count {
        if i > 1 {
            html.push('\n');
        }
        let _ = write!(html, "{i}");
    }
    html.push_str("</pre></td>\n");

    // Code column.
    let escaped_lang = escape_html(&effective_lang);
    let _ = writeln!(
        html,
        r#"<td class="code"><pre><code class="language-{escaped_lang}" data-lang="{escaped_lang}">{highlighted}</code></pre></td>"#
    );

    html.push_str("</tr>\n</table>\n</div>\n");
    html
}

/// Resolves a markdown language token to a syntect `SyntaxReference` and a
/// canonical language label for HTML attributes.
///
/// Tries, in order: file extension match → exact name → case-insensitive name
/// → plain text fallback.
///
/// When a syntax is matched, the label is derived from the syntax name
/// (lowercased, spaces replaced with hyphens), so different tokens that resolve
/// to the same syntax produce the same label (e.g., `"rs"` and `"Rust"` both
/// yield `"rust"`). The "Plain Text" syntax is special-cased to `"plaintext"`.
///
/// Unrecognized non-empty tokens are lowercased and emit a warning.
fn find_syntax<'a>(syntax_set: &'a SyntaxSet, lang: &str) -> (&'a SyntaxReference, String) {
    if lang.is_empty() {
        return (syntax_set.find_syntax_plain_text(), "plaintext".into());
    }

    let syntax = syntax_set
        .find_syntax_by_extension(lang)
        .or_else(|| syntax_set.find_syntax_by_name(lang))
        .or_else(|| {
            syntax_set
                .syntaxes()
                .iter()
                .find(|s| s.name.eq_ignore_ascii_case(lang))
        });

    if let Some(s) = syntax {
        return (s, canonical_lang(&s.name));
    }

    warn!(lang, "unrecognized language, falling back to plain text");
    (
        syntax_set.find_syntax_plain_text(),
        lang.to_ascii_lowercase(),
    )
}

/// Derives a canonical HTML-safe language label from a syntect syntax name.
///
/// Lowercases the name and replaces spaces with hyphens. The "Plain Text"
/// syntax is special-cased to the web-standard `"plaintext"`.
fn canonical_lang(syntax_name: &str) -> String {
    if syntax_name == "Plain Text" {
        return "plaintext".into();
    }
    syntax_name.to_ascii_lowercase().replace(' ', "-")
}

#[cfg(test)]
mod tests {
    use std::sync::LazyLock;

    use indoc::indoc;

    use super::*;

    static SYNTAX_SET: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_newlines);

    /// Suffix shared by all highlight output: closes `</code>` through `</div>`.
    const SUFFIX: &str = indoc! {"
        </code></pre></td>
        </tr>
        </table>
        </div>
    "};

    #[test]
    fn highlight_empty_code() {
        let html = highlight_code(&SYNTAX_SET, "rs", "");
        let expected = indoc! {r#"
            <div class="highlight">
            <table>
            <tr>
            <td class="line-numbers"><pre>1</pre></td>
            <td class="code"><pre><code class="language-rust" data-lang="rust"></code></pre></td>
            </tr>
            </table>
            </div>
        "#};
        assert_eq!(html, expected);
    }

    #[test]
    fn highlight_known_language() {
        let html = highlight_code(&SYNTAX_SET, "rs", "fn main() {}\n");
        let prefix = indoc! {r#"
            <div class="highlight">
            <table>
            <tr>
            <td class="line-numbers"><pre>1</pre></td>
            <td class="code"><pre><code class="language-rust" data-lang="rust">"#};
        assert!(html.starts_with(prefix), "unexpected prefix, html:\n{html}");
        assert!(html.ends_with(SUFFIX), "unexpected suffix, html:\n{html}");
        assert!(
            html.contains("<span class="),
            "should contain syntax spans, html:\n{html}"
        );
    }

    #[test]
    fn highlight_language_matched_by_name() {
        let html = highlight_code(&SYNTAX_SET, "Rust", "fn main() {}\n");
        assert!(
            html.contains(r#"class="language-rust""#),
            "should canonicalize to lowercase, html:\n{html}"
        );
        assert!(
            html.contains(r#"data-lang="rust""#),
            "should canonicalize to lowercase, html:\n{html}"
        );
        assert!(
            html.contains("<span class="),
            "should contain syntax spans, html:\n{html}"
        );
    }

    #[test]
    fn highlight_no_language_normalizes_to_plaintext() {
        let html = highlight_code(&SYNTAX_SET, "", "hello\n");
        let prefix = indoc! {r#"
            <div class="highlight">
            <table>
            <tr>
            <td class="line-numbers"><pre>1</pre></td>
            <td class="code"><pre><code class="language-plaintext" data-lang="plaintext">"#};
        assert!(html.starts_with(prefix), "unexpected prefix, html:\n{html}");
        assert!(html.ends_with(SUFFIX), "unexpected suffix, html:\n{html}");
    }

    #[test]
    fn highlight_unknown_language_lowercases_token() {
        let html = highlight_code(&SYNTAX_SET, "Unknown", "hello\n");
        let prefix = indoc! {r#"
            <div class="highlight">
            <table>
            <tr>
            <td class="line-numbers"><pre>1</pre></td>
            <td class="code"><pre><code class="language-unknown" data-lang="unknown">"#};
        assert!(html.starts_with(prefix), "unexpected prefix, html:\n{html}");
        assert!(html.ends_with(SUFFIX), "unexpected suffix, html:\n{html}");
    }

    #[test]
    fn highlight_language_tag_with_special_characters() {
        let html = highlight_code(&SYNTAX_SET, "c++", "int main() {}\n");
        assert!(
            html.contains(r"language-c++"),
            "should preserve c++ in class, html:\n{html}"
        );
        assert!(
            html.contains(r#"data-lang="c++""#),
            "should preserve c++ in data-lang, html:\n{html}"
        );
    }

    #[test]
    fn highlight_line_numbers() {
        let code = indoc! {"
            line 1
            line 2
            line 3
        "};
        let html = highlight_code(&SYNTAX_SET, "txt", code);
        let prefix = indoc! {r#"
            <div class="highlight">
            <table>
            <tr>
            <td class="line-numbers"><pre>1
            2
            3</pre></td>
            <td class="code"><pre><code class="language-plaintext" data-lang="plaintext">"#};
        assert!(html.starts_with(prefix), "unexpected prefix, html:\n{html}");
        assert!(html.ends_with(SUFFIX), "unexpected suffix, html:\n{html}");
    }
}
