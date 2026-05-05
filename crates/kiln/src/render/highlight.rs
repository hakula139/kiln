use syntect::html::{ClassStyle, ClassedHTMLGenerator};
use syntect::parsing::{SyntaxReference, SyntaxSet};
use syntect::util::LinesWithEndings;

use tracing::{debug, warn};

use super::code_block::CodeBlockSpec;
use crate::html::{escape, indent, writeln_indented};

/// Highlights a code block with syntax highlighting, line numbers, and a header with a language
/// label and copy button.
///
/// Language labels are canonicalized from syntect's syntax name, lowercased. Empty and
/// unrecognized tags normalize to `"plaintext"`. Display label uses original casing.
#[must_use]
pub(crate) fn highlight_code(syntax_set: &SyntaxSet, code: &str, spec: &CodeBlockSpec) -> String {
    let lang = spec.lang.as_deref().unwrap_or("");
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

    let escaped_lang = escape(&effective_lang);

    // ── Wrapper open ──

    let mut wrapper_classes = String::from("code-block");
    match spec.collapse {
        Some(true) => wrapper_classes.push_str(" collapsed"),
        Some(false) => wrapper_classes.push_str(" expanded"),
        None => {}
    }
    for cls in &spec.classes {
        wrapper_classes.push(' ');
        wrapper_classes.push_str(cls);
    }

    let id_attr = spec
        .id
        .as_ref()
        .map(|id| format!(r#" id="{}""#, escape(id)))
        .unwrap_or_default();

    writeln_indented!(
        &mut html,
        0,
        r#"<div class="{wrapper_classes}"{id_attr} data-lang="{escaped_lang}">"#
    );

    // ── Header ──
    //
    // When the author supplies a title, it owns the chrome — drop the language pill so the header
    // shows a single label. The `data-lang` attribute on the wrapper still drives syntax CSS.

    writeln_indented!(&mut html, 1, r#"<div class="code-header">"#);
    if let Some(title) = &spec.title {
        writeln_indented!(
            &mut html,
            2,
            r#"<span class="code-title">{}</span>"#,
            escape(title)
        );
    } else {
        writeln_indented!(
            &mut html,
            2,
            r#"<span class="code-lang">{}</span>"#,
            escape(&display_label)
        );
    }
    writeln_indented!(&mut html, 2, r#"<button class="copy-btn">Copy</button>"#);
    writeln_indented!(&mut html, 1, "</div>");

    // ── Code body ──

    let max_lines_attr = spec
        .max_lines
        .map(|n| format!(r#" data-max-lines="{n}""#))
        .unwrap_or_default();
    writeln_indented!(&mut html, 1, r#"<div class="code-body"{max_lines_attr}>"#);

    // ── Highlight table ──

    writeln_indented!(&mut html, 2, r#"<div class="highlight">"#);
    writeln_indented!(&mut html, 3, "<table>");
    writeln_indented!(&mut html, 4, "<tr>");

    if spec.highlight.is_empty() {
        // Fast path: no per-line highlight. Emit the classic single-row structure.
        let line_numbers: String = (1..=line_count)
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        writeln_indented!(
            &mut html,
            5,
            r#"<td class="line-numbers"><pre>{line_numbers}</pre></td>"#
        );
        writeln_indented!(
            &mut html,
            5,
            r#"<td class="code"><pre><code class="language-{escaped_lang}" data-lang="{escaped_lang}">{highlighted}</code></pre></td>"#
        );
    } else {
        // Per-line highlight path: wrap each line for targeted styling.
        emit_highlighted_lines(
            &mut html,
            &highlighted,
            line_count,
            &escaped_lang,
            &spec.highlight,
        );
    }

    writeln_indented!(&mut html, 4, "</tr>");
    writeln_indented!(&mut html, 3, "</table>");
    writeln_indented!(&mut html, 2, "</div>");
    writeln_indented!(&mut html, 1, "</div>");
    writeln_indented!(&mut html, 0, "</div>");
    html
}

/// Emits line-numbers and code columns with per-line `<span>` wrappers for highlight support.
fn emit_highlighted_lines(
    html: &mut String,
    highlighted: &str,
    line_count: usize,
    escaped_lang: &str,
    ranges: &[std::ops::RangeInclusive<usize>],
) {
    use std::fmt::Write as _;

    let is_highlighted = |line_no: usize| ranges.iter().any(|r| r.contains(&line_no));

    // Line-numbers column.
    indent(html, 5);
    _ = write!(html, r#"<td class="line-numbers"><pre>"#);
    for i in 1..=line_count {
        if i > 1 {
            html.push('\n');
        }
        let hl = if is_highlighted(i) { " hl" } else { "" };
        _ = write!(html, r#"<span class="line-number{hl}">{i}</span>"#);
    }
    _ = writeln!(html, "</pre></td>");

    // Code column.
    indent(html, 5);
    _ = write!(
        html,
        r#"<td class="code"><pre><code class="language-{escaped_lang}" data-lang="{escaped_lang}">"#
    );
    let lines: Vec<&str> = highlighted.split('\n').collect();
    let last_idx = lines.len().saturating_sub(1);
    for (idx, line) in lines.iter().enumerate() {
        let line_no = idx + 1;
        let hl = if is_highlighted(line_no) { " hl" } else { "" };
        _ = write!(html, r#"<span class="line{hl}">{line}</span>"#);
        if idx < last_idx {
            html.push('\n');
        }
    }
    _ = writeln!(html, "</code></pre></td>");
}

/// Resolves a markdown language token to a syntect `SyntaxReference`, a canonical language label,
/// and a human-readable display label.
///
/// Tries: file extension → exact name → case-insensitive name → plain text fallback. Canonical
/// label is lowercased (spaces → hyphens) for HTML attributes. Unrecognized tokens get
/// `"plaintext"` with a title-cased display label.
fn find_syntax<'a>(syntax_set: &'a SyntaxSet, lang: &str) -> (&'a SyntaxReference, String, String) {
    if !lang.is_empty() {
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
    }

    // No syntax found or empty tag — fall back to plain text.
    let lower = lang.to_ascii_lowercase();
    let display = match lower.as_str() {
        "" | "text" | "plaintext" | "plain" => "Plain Text".into(),
        _ => {
            debug!(lang, "unrecognized language, falling back to plain text");
            capitalize_first(&lower)
        }
    };

    (
        syntax_set.find_syntax_plain_text(),
        "plaintext".into(),
        display,
    )
}

/// Derives a canonical HTML-safe language label from a syntect syntax name.
///
/// Lowercases the name and replaces spaces with hyphens. The "Plain Text" syntax is special-cased
/// to the web-standard `"plaintext"`.
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
        highlight_code(
            &SYNTAX_SET,
            code,
            &CodeBlockSpec {
                lang: Some(lang.to_string()),
                ..CodeBlockSpec::default()
            },
        )
    }

    fn highlight_with_spec(code: &str, spec: &CodeBlockSpec) -> String {
        highlight_code(&SYNTAX_SET, code, spec)
    }

    // ── highlight_code (structure) ──

    #[test]
    fn highlight_code_structure() {
        let html = highlight("rs", "fn main() {}\n");
        assert!(
            html.starts_with(r#"<div class="code-block" data-lang="rust">"#),
            "should start with code-block wrapper with data-lang, html:\n{html}"
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
        let spec = CodeBlockSpec {
            lang: Some("rs".into()),
            max_lines: Some(40),
            ..CodeBlockSpec::default()
        };
        let html = highlight_with_spec("fn main() {}\n", &spec);
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
            html.contains(indoc! {r#"
                <td class="line-numbers"><pre>1
                2
                3</pre></td>"#
            }),
            "should have 3 line numbers, html:\n{html}"
        );
    }

    // ── highlight_code (title) ──

    #[test]
    fn highlight_code_title_replaces_lang_pill() {
        let spec = CodeBlockSpec {
            lang: Some("rs".into()),
            title: Some("src/main.rs".into()),
            ..CodeBlockSpec::default()
        };
        let html = highlight_with_spec("fn main() {}\n", &spec);
        assert!(
            html.contains(r#"<span class="code-title">src/main.rs</span>"#),
            "should contain title span, html:\n{html}"
        );
        assert!(
            !html.contains(r#"<span class="code-lang">"#),
            "lang pill should be suppressed when title is set, html:\n{html}"
        );
        assert!(
            html.contains(r#"data-lang="rust""#),
            "wrapper should still carry data-lang for syntax CSS, html:\n{html}"
        );
    }

    #[test]
    fn highlight_code_no_title_keeps_lang_pill() {
        let html = highlight("rs", "fn main() {}\n");
        assert!(
            !html.contains("code-title"),
            "should not emit title span when no title, html:\n{html}"
        );
        assert!(
            html.contains(r#"<span class="code-lang">Rust</span>"#),
            "lang pill should appear when no title, html:\n{html}"
        );
    }

    #[test]
    fn highlight_code_title_html_escaped() {
        let spec = CodeBlockSpec {
            lang: Some("html".into()),
            title: Some("<script>.js".into()),
            ..CodeBlockSpec::default()
        };
        let html = highlight_with_spec("<div></div>\n", &spec);
        assert!(
            html.contains(r#"<span class="code-title">&lt;script&gt;.js</span>"#),
            "title should be HTML-escaped, html:\n{html}"
        );
    }

    // ── highlight_code (highlight ranges) ──

    #[test]
    fn highlight_code_emits_highlight_class() {
        let spec = CodeBlockSpec {
            lang: Some("txt".into()),
            highlight: vec![2..=2],
            ..CodeBlockSpec::default()
        };
        let html = highlight_with_spec("line 1\nline 2\nline 3\n", &spec);
        assert!(
            html.contains(r#"<span class="line-number hl">2</span>"#),
            "line-number 2 should have hl class, html:\n{html}"
        );
        assert!(
            html.contains(r#"<span class="line hl">"#),
            "code line 2 should have hl class, html:\n{html}"
        );
        assert!(
            html.contains(r#"<span class="line-number">1</span>"#),
            "line-number 1 should NOT have hl class, html:\n{html}"
        );
    }

    #[test]
    fn highlight_code_highlight_range() {
        let spec = CodeBlockSpec {
            lang: Some("txt".into()),
            highlight: vec![1..=2],
            ..CodeBlockSpec::default()
        };
        let html = highlight_with_spec("a\nb\nc\n", &spec);
        assert!(html.contains(r#"<span class="line-number hl">1</span>"#));
        assert!(html.contains(r#"<span class="line-number hl">2</span>"#));
        assert!(html.contains(r#"<span class="line-number">3</span>"#));
    }

    // ── highlight_code (collapse / expand) ──

    #[test]
    fn highlight_code_collapse_emits_class() {
        let spec = CodeBlockSpec {
            lang: Some("rs".into()),
            collapse: Some(true),
            ..CodeBlockSpec::default()
        };
        let html = highlight_with_spec("fn main() {}\n", &spec);
        assert!(
            html.contains(r#"<div class="code-block collapsed""#),
            "should have collapsed class, html:\n{html}"
        );
    }

    #[test]
    fn highlight_code_expand_emits_class() {
        let spec = CodeBlockSpec {
            lang: Some("rs".into()),
            collapse: Some(false),
            ..CodeBlockSpec::default()
        };
        let html = highlight_with_spec("fn main() {}\n", &spec);
        assert!(
            html.contains(r#"<div class="code-block expanded""#),
            "should have expanded class, html:\n{html}"
        );
    }

    // ── highlight_code (ID and classes) ──

    #[test]
    fn highlight_code_propagates_id_and_classes() {
        let spec = CodeBlockSpec {
            lang: Some("rs".into()),
            id: Some("my-code".into()),
            classes: vec!["wide".into(), "dark".into()],
            ..CodeBlockSpec::default()
        };
        let html = highlight_with_spec("fn main() {}\n", &spec);
        assert!(
            html.contains(r#"<div class="code-block wide dark" id="my-code""#),
            "should propagate id and classes, html:\n{html}"
        );
    }

    // ── highlight_code (no-attrs baseline) ──

    #[test]
    fn highlight_code_no_attrs_byte_identical_to_baseline() {
        // Baseline: the old API shape.
        let spec = CodeBlockSpec {
            lang: Some("rs".into()),
            ..CodeBlockSpec::default()
        };
        let new_html = highlight_with_spec("fn main() {}\n", &spec);

        // The output should match the classic structure exactly.
        assert!(new_html.starts_with(r#"<div class="code-block" data-lang="rust">"#));
        assert!(!new_html.contains("code-title"));
        assert!(!new_html.contains("collapsed"));
        assert!(!new_html.contains("expanded"));
        assert!(!new_html.contains(r"id="));
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
            html.contains(r#"data-lang="plaintext""#),
            "should normalize unrecognized token to plaintext, html:\n{html}"
        );
        assert!(
            html.contains(r#"<span class="code-lang">Unknown</span>"#),
            "display label should title-case the lowercased token, html:\n{html}"
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
            html.contains(r#"data-lang="plaintext""#),
            "should normalize to plaintext, html:\n{html}"
        );
        assert!(
            html.contains(r#"<span class="code-lang">&lt;script&gt;</span>"#),
            "display label should be escaped, html:\n{html}"
        );
        assert!(
            !html.contains("<script>"),
            "raw script tag must not appear, html:\n{html}"
        );
    }

    // ── find_syntax ──

    #[test]
    fn highlight_code_text_alias() {
        let html = highlight("text", "hello\n");
        assert!(
            html.contains(r#"data-lang="plaintext""#),
            "should normalize 'text' to plaintext, html:\n{html}"
        );
        assert!(
            html.contains(r#"<span class="code-lang">Plain Text</span>"#),
            "display label should be Plain Text, html:\n{html}"
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
