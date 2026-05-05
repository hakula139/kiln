use syntect::html::{ClassStyle, ClassedHTMLGenerator};
use syntect::parsing::{SyntaxReference, SyntaxSet};
use syntect::util::LinesWithEndings;

use tracing::{debug, warn};

use super::code_block::CodeBlockSpec;
use crate::html::{escape, indent, writeln_indented};

/// Highlights a code block with line numbers and a header (language pill or title, copy button).
///
/// Empty and unrecognized fence tags normalize to `plaintext`. The display label is derived from
/// the author's input by [`display_label`], not from syntect's internal name.
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
    // Title (when present) replaces the language pill so the header shows one label; `data-lang`
    // on the wrapper still drives syntax CSS.

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

/// Resolves a markdown language token to a syntect `SyntaxReference`, a canonical HTML-safe label,
/// and a human-readable display label.
///
/// Lookup order: file extension → exact name → case-insensitive name → plain-text fallback.
fn find_syntax<'a>(syntax_set: &'a SyntaxSet, lang: &str) -> (&'a SyntaxReference, String, String) {
    let display = display_label(lang);

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
            return (s, canonical_lang(&s.name), display);
        }
        debug!(lang, "unrecognized language, falling back to plain text");
    }

    (
        syntax_set.find_syntax_plain_text(),
        "plaintext".into(),
        display,
    )
}

/// Returns a canonical HTML-safe label from a syntect syntax name.
///
/// Lowercases and replaces spaces with hyphens; "Plain Text" maps to the web-standard `plaintext`.
fn canonical_lang(syntax_name: &str) -> String {
    if syntax_name == "Plain Text" {
        return "plaintext".into();
    }
    syntax_name.to_ascii_lowercase().replace(' ', "-")
}

/// Derives a human-readable display label from the author's fence token.
///
/// Author-driven so the label tracks the input — syntect's internal name is often verbose
/// (`"Bourne Again Shell (bash)"`) and inconsistently cased. Per-block overrides go through the
/// `title="..."` fence attribute instead.
fn display_label(lang: &str) -> String {
    let lower = lang.to_ascii_lowercase();
    let mapped: &str = match lower.as_str() {
        "" | "plain" | "plaintext" | "text" => "Plain Text",

        // Short alias → canonical name (sorted by output).
        "js" | "jsx" => "JavaScript",
        "kt" | "kts" => "Kotlin",
        "latex" | "tex" => "LaTeX",
        "md" | "mdx" => "Markdown",
        "objc" => "Objective-C",
        "ps1" => "PowerShell",
        "py" => "Python",
        "rb" => "Ruby",
        "rs" => "Rust",
        "bash" | "fish" | "sh" | "zsh" => "Shell",
        "ts" | "tsx" => "TypeScript",

        // Special punctuation in the canonical name (sorted by output).
        "cs" | "csharp" => "C#",
        "c++" | "cc" | "cpp" | "cxx" | "h++" | "hpp" => "C++",
        "fs" | "fsharp" => "F#",

        // Mixed-case brand names (sorted by output).
        "gql" | "graphql" => "GraphQL",
        "sass" => "Sass",

        // ALL-CAPS acronyms; output is just the input uppercased.
        "asm" | "css" | "csv" | "html" | "http" | "ini" | "json" | "php" | "scss" | "sql"
        | "toml" | "tsv" | "wasm" | "xml" | "yaml" | "yml" => return lower.to_uppercase(),

        _ => return capitalize_first(&lower),
    };
    mapped.to_string()
}

/// Uppercases the first ASCII character of a string.
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
    fn highlight_code_no_attrs_omits_optional_chrome() {
        let spec = CodeBlockSpec {
            lang: Some("rs".into()),
            ..CodeBlockSpec::default()
        };
        let html = highlight_with_spec("fn main() {}\n", &spec);
        assert!(html.starts_with(r#"<div class="code-block" data-lang="rust">"#));
        assert!(!html.contains("code-title"));
        assert!(!html.contains("collapsed"));
        assert!(!html.contains("expanded"));
        assert!(!html.contains(r"id="));
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

    // ── highlight_code (title) ──

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

    // ── highlight_code (max lines) ──

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
    fn highlight_code_no_max_lines_omits_attr() {
        let html = highlight("rs", "fn main() {}\n");
        assert!(
            !html.contains("data-max-lines"),
            "should not have data-max-lines when None, html:\n{html}"
        );
    }

    // ── highlight_code (language resolution) ──

    #[test]
    fn highlight_code_known_language() {
        let html = highlight("rs", "fn main() {}\n");
        assert!(
            html.contains(r#"data-lang="rust""#),
            "should canonicalize extension to name, html:\n{html}"
        );
        assert!(
            html.contains(r#"<span class="code-lang">Rust</span>"#),
            "display label should be proper-cased, html:\n{html}"
        );

        let html = highlight("Rust", "fn main() {}\n");
        assert!(
            html.contains(r#"data-lang="rust""#),
            "should canonicalize to lowercase, html:\n{html}"
        );
    }

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

    #[test]
    fn highlight_code_special_chars_in_language() {
        let html = highlight("c++", "int main() {}\n");
        assert!(
            html.contains(r#"data-lang="c++""#),
            "should preserve special chars, html:\n{html}"
        );
        assert!(
            html.contains(r#"<span class="code-lang">C++</span>"#),
            "display label should preserve special punctuation, html:\n{html}"
        );
    }

    #[test]
    fn highlight_code_display_label_independent_of_syntect_name() {
        // syntect names bash as "Bourne Again Shell (bash)"; display_label sidesteps that.
        let html = highlight("bash", "echo hi\n");
        assert!(
            html.contains(r#"<span class="code-lang">Shell</span>"#),
            "display label should come from display_label, html:\n{html}"
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

    // ── display_label ──

    #[test]
    fn display_label_plaintext_aliases() {
        for token in ["", "plain", "plaintext", "text"] {
            assert_eq!(display_label(token), "Plain Text", "for input {token:?}");
        }
    }

    #[test]
    fn display_label_short_form_aliases() {
        assert_eq!(display_label("js"), "JavaScript");
        assert_eq!(display_label("kt"), "Kotlin");
        assert_eq!(display_label("md"), "Markdown");
        assert_eq!(display_label("ps1"), "PowerShell");
        assert_eq!(display_label("py"), "Python");
        assert_eq!(display_label("rs"), "Rust");
        assert_eq!(display_label("ts"), "TypeScript");
    }

    #[test]
    fn display_label_shell_family_canonicalized() {
        // Different shell flavors share the syntect grammar; the label collapses to "Shell".
        for shell in ["bash", "fish", "sh", "zsh"] {
            assert_eq!(display_label(shell), "Shell", "for input {shell:?}");
        }
    }

    #[test]
    fn display_label_special_punctuation() {
        assert_eq!(display_label("cs"), "C#");
        assert_eq!(display_label("cpp"), "C++");
        assert_eq!(display_label("c++"), "C++");
        assert_eq!(display_label("fs"), "F#");
    }

    #[test]
    fn display_label_mixed_case_brand_names() {
        assert_eq!(display_label("gql"), "GraphQL");
        assert_eq!(display_label("graphql"), "GraphQL");
        assert_eq!(display_label("sass"), "Sass");
    }

    #[test]
    fn display_label_all_caps_acronyms() {
        for token in ["css", "html", "json", "php", "scss", "toml", "yaml"] {
            assert_eq!(
                display_label(token),
                token.to_uppercase(),
                "for input {token:?}"
            );
        }
    }

    #[test]
    fn display_label_default_capitalizes_first() {
        assert_eq!(display_label("rust"), "Rust");
        assert_eq!(display_label("python"), "Python");
        assert_eq!(display_label("go"), "Go");
        assert_eq!(display_label("dockerfile"), "Dockerfile");
        assert_eq!(display_label("unknown-lang"), "Unknown-lang");
    }

    #[test]
    fn display_label_input_case_insensitive() {
        assert_eq!(display_label("RUST"), "Rust");
        assert_eq!(display_label("Rust"), "Rust");
        assert_eq!(display_label("BASH"), "Shell");
        assert_eq!(display_label("GraphQL"), "GraphQL");
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
