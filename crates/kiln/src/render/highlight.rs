use syntect::html::{ClassStyle, ClassedHTMLGenerator};
use syntect::parsing::{SyntaxReference, SyntaxSet};
use syntect::util::LinesWithEndings;

use tracing::{debug, warn};

use crate::html::{escape, writeln_indented};

/// Highlights a code block with syntax highlighting, line numbers, and a
/// header with a language label and copy button.
///
/// Output structure:
///
/// ```html
/// <div class="code-block">
///   <div class="code-header">
///     <span class="code-lang">Rust</span>
///     <button class="copy-btn">Copy</button>
///   </div>
///   <div class="code-body" data-max-lines="40">
///     <div class="highlight">
///       <table>
///         <tr>
///           <td class="line-numbers"><pre>...</pre></td>
///           <td class="code"><pre><code>...</code></pre></td>
///         </tr>
///       </table>
///     </div>
///   </div>
/// </div>
/// ```
///
/// Language labels are canonicalized: derived from syntect's syntax name,
/// lowercased. Empty tags normalize to `"plaintext"`; unrecognized tags
/// lowercase the original token and fall back to plain text highlighting.
/// The header's display label uses the original syntax name casing.
#[must_use]
pub fn highlight_code(
    syntax_set: &SyntaxSet,
    lang: &str,
    code: &str,
    max_lines: Option<usize>,
) -> String {
    let (syntax, effective_lang, display_label) = find_syntax(syntax_set, lang);

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
        String::with_capacity(highlighted.len() + line_count * 8 + 2 * effective_lang.len() + 512);

    // Outer wrapper + header.
    writeln_indented!(&mut html, 0, r#"<div class="code-block">"#);
    writeln_indented!(&mut html, 1, r#"<div class="code-header">"#);
    writeln_indented!(
        &mut html,
        2,
        r#"<span class="code-lang">{}</span>"#,
        escape(&display_label)
    );
    writeln_indented!(&mut html, 2, r#"<button class="copy-btn">Copy</button>"#);
    writeln_indented!(&mut html, 1, "</div>");

    // Code body (with optional max-lines for JS-driven collapse).
    let max_lines_attr = max_lines
        .map(|n| format!(r#" data-max-lines="{n}""#))
        .unwrap_or_default();
    writeln_indented!(&mut html, 1, r#"<div class="code-body"{max_lines_attr}>"#);

    // Highlight table.
    writeln_indented!(&mut html, 2, r#"<div class="highlight">"#);
    writeln_indented!(&mut html, 3, "<table>");
    writeln_indented!(&mut html, 4, "<tr>");

    // Line numbers column.
    let line_numbers: String = (1..=line_count)
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    writeln_indented!(
        &mut html,
        5,
        r#"<td class="line-numbers"><pre>{line_numbers}</pre></td>"#
    );

    // Code column.
    let escaped_lang = escape(&effective_lang);
    writeln_indented!(
        &mut html,
        5,
        r#"<td class="code"><pre><code class="language-{escaped_lang}" data-lang="{escaped_lang}">{highlighted}</code></pre></td>"#
    );

    writeln_indented!(&mut html, 4, "</tr>");
    writeln_indented!(&mut html, 3, "</table>");
    writeln_indented!(&mut html, 2, "</div>");
    writeln_indented!(&mut html, 1, "</div>");
    writeln_indented!(&mut html, 0, "</div>");
    html
}

/// Resolves a markdown language token to a syntect `SyntaxReference`, a
/// canonical language label, and a human-readable display label.
///
/// Tries, in order: file extension match → exact name → case-insensitive name
/// → plain text fallback.
///
/// The canonical label is lowercased (spaces → hyphens) for HTML class / data
/// attributes. The display label uses the original syntax name casing (e.g.,
/// "Rust", "JavaScript", "C++") for the code block header.
///
/// Unrecognized non-empty tokens are lowercased and logged at debug level.
fn find_syntax<'a>(syntax_set: &'a SyntaxSet, lang: &str) -> (&'a SyntaxReference, String, String) {
    if lang.is_empty() {
        return (
            syntax_set.find_syntax_plain_text(),
            "plaintext".into(),
            "Plain Text".into(),
        );
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
        return (s, canonical_lang(&s.name), s.name.clone());
    }

    let lower = lang.to_ascii_lowercase();
    let display = capitalize_first(&lower);

    if !is_plain_text_alias(lang) {
        debug!(lang, "unrecognized language, falling back to plain text");
    }

    (syntax_set.find_syntax_plain_text(), lower, display)
}

/// Returns `true` for language tokens that are intentionally not highlighted
/// (e.g., diagram DSLs) and should not trigger an "unrecognized" warning.
fn is_plain_text_alias(lang: &str) -> bool {
    lang.eq_ignore_ascii_case("mermaid")
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

/// Capitalizes the first ASCII character of a string.
fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => {
            let mut result = String::with_capacity(s.len());
            result.push(c.to_ascii_uppercase());
            result.push_str(chars.as_str());
            result
        }
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::LazyLock;

    use indoc::indoc;

    use super::*;

    static SYNTAX_SET: LazyLock<SyntaxSet> = LazyLock::new(two_face::syntax::extra_newlines);

    fn highlight(lang: &str, code: &str) -> String {
        highlight_code(&SYNTAX_SET, lang, code, None)
    }

    // ── highlight_code (structure) ──

    #[test]
    fn highlight_code_structure() {
        let html = highlight("rs", "fn main() {}\n");
        assert!(
            html.starts_with(r#"<div class="code-block">"#),
            "should start with code-block wrapper, html:\n{html}"
        );
        assert!(
            html.contains(r#"<div class="code-header">"#),
            "should have code-header, html:\n{html}"
        );
        assert!(
            html.contains(r#"<span class="code-lang">Rust</span>"#),
            "should have display label, html:\n{html}"
        );
        assert!(
            html.contains(r#"<button class="copy-btn">Copy</button>"#),
            "should have copy button, html:\n{html}"
        );
        assert!(
            html.contains(r#"<div class="code-body">"#),
            "should have code-body, html:\n{html}"
        );
        assert!(
            html.contains(r#"<div class="highlight">"#),
            "should have highlight table, html:\n{html}"
        );
        assert!(
            html.ends_with("</div>\n"),
            "should end with closing tag, html:\n{html}"
        );
    }

    #[test]
    fn highlight_code_max_lines() {
        let html = highlight_code(&SYNTAX_SET, "rs", "fn main() {}\n", Some(40));
        assert!(
            html.contains(r#"<div class="code-body" data-max-lines="40">"#),
            "should have data-max-lines attribute, html:\n{html}"
        );
    }

    #[test]
    fn highlight_code_no_max_lines() {
        let html = highlight("rs", "fn main() {}\n");
        assert!(
            !html.contains("data-max-lines"),
            "should not have data-max-lines when None, html:\n{html}"
        );
    }

    #[test]
    fn highlight_code_line_numbers() {
        let html = highlight(
            "txt",
            indoc! {"
                line 1
                line 2
                line 3
            "},
        );
        assert!(
            html.contains("<td class=\"line-numbers\"><pre>1\n2\n3</pre></td>"),
            "should have 3 line numbers, html:\n{html}"
        );
    }

    // ── highlight_code (language resolution) ──

    #[test]
    fn highlight_code_empty_input() {
        let html = highlight("rs", "");
        assert!(
            html.contains(r#"data-lang="rust""#),
            "should still resolve language, html:\n{html}"
        );
        assert!(
            html.contains("<pre>1</pre>"),
            "should have single line number, html:\n{html}"
        );
    }

    #[test]
    fn highlight_code_known_language() {
        // By extension
        let html = highlight("rs", "fn main() {}\n");
        assert!(
            html.contains(r#"data-lang="rust""#),
            "should canonicalize extension to name, html:\n{html}"
        );
        assert!(
            html.contains(r#"<span class="code-lang">Rust</span>"#),
            "display label should be proper-cased, html:\n{html}"
        );

        // By name (case-insensitive)
        let html = highlight("Rust", "fn main() {}\n");
        assert!(
            html.contains(r#"data-lang="rust""#),
            "should canonicalize to lowercase, html:\n{html}"
        );
    }

    #[test]
    fn highlight_code_empty_language() {
        let html = highlight("", "hello\n");
        assert!(
            html.contains(r#"data-lang="plaintext""#),
            "should default to plaintext, html:\n{html}"
        );
        assert!(
            html.contains(r#"<span class="code-lang">Plain Text</span>"#),
            "display label should be Plain Text, html:\n{html}"
        );
    }

    #[test]
    fn highlight_code_unknown_language() {
        let html = highlight("Unknown", "hello\n");
        assert!(
            html.contains(r#"data-lang="unknown""#),
            "should lowercase unknown token, html:\n{html}"
        );
        assert!(
            html.contains(r#"<span class="code-lang">Unknown</span>"#),
            "display label should capitalize first char, html:\n{html}"
        );
    }

    #[test]
    fn highlight_code_special_chars_in_language() {
        let html = highlight("c++", "int main() {}\n");
        assert!(
            html.contains(r#"data-lang="c++""#),
            "should preserve special chars, html:\n{html}"
        );
        assert!(
            html.contains(r#"<span class="code-lang">C++</span>"#),
            "display label should preserve original casing, html:\n{html}"
        );
    }

    #[test]
    fn highlight_code_html_chars_in_language() {
        let html = highlight("<script>", "alert(1)\n");
        assert!(
            html.contains(r#"data-lang="&lt;script&gt;""#),
            "should escape HTML chars, html:\n{html}"
        );
        assert!(
            html.contains(r#"<span class="code-lang">&lt;script&gt;</span>"#),
            "display label should also be escaped, html:\n{html}"
        );
        assert!(
            !html.contains("<script>"),
            "raw script tag must not appear, html:\n{html}"
        );
    }

    #[test]
    fn highlight_code_mermaid() {
        let html = highlight("mermaid", "graph TD\n");
        assert!(
            html.contains(r#"data-lang="mermaid""#),
            "should treat mermaid as plain text alias, html:\n{html}"
        );
        assert!(
            html.contains(r#"<span class="code-lang">Mermaid</span>"#),
            "display label should be Mermaid, html:\n{html}"
        );
    }

    #[test]
    fn highlight_code_two_face_languages() {
        let html = highlight("ts", "const x = 1;\n");
        assert!(
            html.contains(r#"data-lang="typescript""#),
            "should resolve ts to TypeScript, html:\n{html}"
        );

        let html = highlight("toml", "[table]\n");
        assert!(
            html.contains(r#"data-lang="toml""#),
            "should resolve toml, html:\n{html}"
        );
    }

    // ── capitalize_first ──

    #[test]
    fn capitalize_first_basic() {
        assert_eq!(capitalize_first("rust"), "Rust");
    }

    #[test]
    fn capitalize_first_empty() {
        assert_eq!(capitalize_first(""), "");
    }
}
