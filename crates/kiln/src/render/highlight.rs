use std::fmt::Write;
use std::num::NonZeroUsize;
use std::sync::LazyLock;

use itertools::Itertools;
use syntect::html::{ClassStyle, line_tokens_to_classed_spans};
use syntect::parsing::{ParseState, ScopeStack, ScopeStackOp, SyntaxReference, SyntaxSet};
use syntect::util::LinesWithEndings;
use tracing::{debug, warn};

use super::code_block::{CodeBlockSpec, Disclosure};
use crate::html::{escape, indent, writeln_indented};
use crate::text::capitalize;

static SYNTAX_SET: LazyLock<SyntaxSet> = LazyLock::new(two_face::syntax::extra_newlines);

/// Highlights a code block with line numbers and a header (language pill or title, copy button).
///
/// `max_lines` applies unless the fence sets `collapse` or `expand`. Unrecognized languages render
/// as plain text, and the display label comes from the language tag via [`display_label`].
#[must_use]
pub(super) fn highlight_code(
    code: &str,
    spec: &CodeBlockSpec,
    max_lines: Option<NonZeroUsize>,
) -> String {
    highlight_with(&SYNTAX_SET, code, spec, max_lines)
}

fn highlight_with(
    syntax_set: &SyntaxSet,
    code: &str,
    spec: &CodeBlockSpec,
    max_lines: Option<NonZeroUsize>,
) -> String {
    let lang = spec.lang.as_deref().unwrap_or("");
    let syntax = syntax_set.find_syntax_by_token(lang).unwrap_or_else(|| {
        if !is_plain_text(lang) {
            debug!(lang, "unrecognized language, falling back to plain text");
        }
        syntax_set.find_syntax_plain_text()
    });
    let mut html = String::new();

    // ── Wrapper open ──

    let classes = std::iter::once("code-block")
        .chain(spec.classes.iter().map(String::as_str))
        .map(escape)
        .join(" ");
    let id_attr = spec
        .id
        .as_deref()
        .map(|id| format!(r#" id="{}""#, escape(id)))
        .unwrap_or_default();
    let lang_attr = spec
        .lang
        .as_deref()
        .map(|lang| format!(r#" data-lang="{}""#, escape(lang)))
        .unwrap_or_default();
    let open_attr = if spec.disclosure == Disclosure::Collapse {
        ""
    } else {
        " open"
    };
    writeln_indented!(
        &mut html,
        0,
        r#"<details class="{classes}"{id_attr}{lang_attr}{open_attr}>"#
    );

    // ── Header ──

    writeln_indented!(&mut html, 1, r#"<summary class="code-header">"#);
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
            escape(&display_label(lang))
        );
    }
    writeln_indented!(
        &mut html,
        2,
        r#"<button class="copy-btn" aria-label="Copy code">Copy</button>"#
    );
    writeln_indented!(&mut html, 1, "</summary>");

    // ── Code body ──

    let max_lines_attr = max_lines
        .filter(|_| spec.disclosure == Disclosure::Auto)
        .map(|n| format!(r#" data-max-lines="{n}" style="--max-lines: {n}""#))
        .unwrap_or_default();
    writeln_indented!(&mut html, 1, r#"<div class="code-body"{max_lines_attr}>"#);
    writeln_indented!(&mut html, 2, r#"<div class="highlight">"#);
    writeln_indented!(&mut html, 3, "<table>");
    writeln_indented!(&mut html, 4, "<tr>");
    write_rows(&mut html, syntax_set, syntax, code, spec);
    writeln_indented!(&mut html, 4, "</tr>");
    writeln_indented!(&mut html, 3, "</table>");
    writeln_indented!(&mut html, 2, "</div>");
    writeln_indented!(&mut html, 1, "</div>");
    writeln_indented!(&mut html, 0, "</details>");
    html
}

/// Writes the line-number and code cells, one `<span>` per line so highlighted lines can be marked.
fn write_rows(
    html: &mut String,
    syntax_set: &SyntaxSet,
    syntax: &SyntaxReference,
    code: &str,
    spec: &CodeBlockSpec,
) {
    let lines = highlight_lines(syntax_set, syntax, code);
    let hl = |line_no: usize| {
        if spec.highlight.iter().any(|range| range.contains(&line_no)) {
            " hl"
        } else {
            ""
        }
    };

    indent(html, 5);
    _ = writeln!(
        html,
        r#"<td class="line-numbers"><pre>{}</pre></td>"#,
        (1..=lines.len()).format_with("\n", |i, f| f(&format_args!(
            r#"<span class="line-number{}">{i}</span>"#,
            hl(i)
        )))
    );

    indent(html, 5);
    _ = writeln!(
        html,
        r#"<td class="code"><pre><code>{}{}</code></pre></td>"#,
        lines
            .iter()
            .zip(1..)
            .format_with("\n", |(line, i), f| f(&format_args!(
                r#"<span class="line{}">{line}</span>"#,
                hl(i)
            ))),
        if code.ends_with('\n') { "\n" } else { "" }
    );
}

fn highlight_lines(syntax_set: &SyntaxSet, syntax: &SyntaxReference, code: &str) -> Vec<String> {
    let mut parse_state = ParseState::new(syntax);
    let mut scope_stack = ScopeStack::new();
    let mut lines: Vec<_> = LinesWithEndings::from(code)
        .map(|line| {
            highlight_line(syntax_set, &mut parse_state, &mut scope_stack, line).unwrap_or_else(
                |error| {
                    warn!(
                        lang = syntax.name,
                        %error,
                        "syntax highlighting failed for line, falling back to plain text"
                    );
                    escape(line.trim_end_matches(['\r', '\n']))
                },
            )
        })
        .collect();

    if lines.is_empty() {
        lines.push(String::new());
    }

    lines
}

fn highlight_line(
    syntax_set: &SyntaxSet,
    parse_state: &mut ParseState,
    scope_stack: &mut ScopeStack,
    line: &str,
) -> Result<String, syntect::Error> {
    let operations = parse_state.parse_line(line, syntax_set)?;
    // Syntax scopes span lines, but each row needs independently balanced HTML.
    let opening: Vec<_> = scope_stack
        .scopes
        .iter()
        .map(|scope| (0, ScopeStackOp::Push(*scope)))
        .collect();
    let (mut html, _) =
        line_tokens_to_classed_spans("", &opening, ClassStyle::Spaced, &mut ScopeStack::new())?;
    let (content, _) =
        line_tokens_to_classed_spans(line, &operations, ClassStyle::Spaced, scope_stack)?;
    html.push_str(&content.replace("\r\n", "").replace('\n', ""));

    for _ in &scope_stack.scopes {
        html.push_str("</span>");
    }

    Ok(html)
}

/// Derives a human-readable display label from the lowercased language tag.
///
/// syntect's internal names are often verbose (`"Bourne Again Shell (bash)"`) and inconsistently
/// cased. Per-block overrides go through the `title="..."` fence attribute instead.
fn display_label(lang: &str) -> String {
    let mapped: &str = match lang {
        _ if is_plain_text(lang) => "Plain Text",

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

        // ALL-CAPS acronyms, where output is just the input uppercased.
        "asm" | "css" | "csv" | "html" | "http" | "ini" | "json" | "php" | "scss" | "sql"
        | "toml" | "tsv" | "wasm" | "xml" | "yaml" | "yml" => return lang.to_uppercase(),

        _ => return capitalize(lang),
    };
    mapped.to_string()
}

fn is_plain_text(lang: &str) -> bool {
    matches!(lang, "" | "plain" | "plaintext" | "text")
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use indoc::indoc;
    use syntect::parsing::syntax_definition::{Context, MatchOperation, MatchPattern, Pattern};
    use syntect::parsing::{Scope, SyntaxDefinition, SyntaxSetBuilder};

    use super::*;
    use crate::render::code_block::parse_fence_info;

    // ── highlight_code ──

    #[test]
    fn renders_details_table() {
        assert_eq!(
            highlight("text {#my-code .wide}", "a <b>\n\nc\n"),
            indoc! {r#"
                <details class="code-block wide" id="my-code" data-lang="text" open>
                  <summary class="code-header">
                    <span class="code-lang">Plain Text</span>
                    <button class="copy-btn" aria-label="Copy code">Copy</button>
                  </summary>
                  <div class="code-body">
                    <div class="highlight">
                      <table>
                        <tr>
                          <td class="line-numbers"><pre><span class="line-number">1</span>
                <span class="line-number">2</span>
                <span class="line-number">3</span></pre></td>
                          <td class="code"><pre><code><span class="line"><span class="text plain">a &lt;b&gt;</span></span>
                <span class="line"><span class="text plain"></span></span>
                <span class="line"><span class="text plain">c</span></span>
                </code></pre></td>
                        </tr>
                      </table>
                    </div>
                  </div>
                </details>
            "#}
        );
    }

    #[test]
    fn escapes_custom_classes() {
        let html = highlight(r#"rs {.a"b .x&copy;}"#, "fn main() {}\n");
        let fragment = scraper::Html::parse_fragment(&html);
        let selector = scraper::Selector::parse(".code-block").unwrap();
        let wrapper = fragment.select(&selector).next().unwrap();

        assert_eq!(
            wrapper.value().attrs().collect::<Vec<_>>(),
            vec![
                ("class", r#"code-block a"b x&copy;"#),
                ("data-lang", "rs"),
                ("open", "")
            ]
        );
    }

    #[test]
    fn title_replaces_lang_pill() {
        let html = highlight(r#"html {title="<script>.js"}"#, "<div></div>\n");
        assert!(
            html.starts_with(indoc! {r#"
                <details class="code-block" data-lang="html" open>
                  <summary class="code-header">
                    <span class="code-title">&lt;script&gt;.js</span>
                    <button"#}),
            "{html}"
        );
        assert!(!html.contains("code-lang"), "{html}");
    }

    #[test]
    fn lang_token_selects_syntax_and_label() {
        for (info, data_lang, label, scope) in [
            ("Rust", Some("rust"), "Rust", "source rust"),
            ("ts", Some("ts"), "TypeScript", "source ts"),
            ("bash", Some("bash"), "Shell", "source shell bash"),
            ("c++", Some("c++"), "C++", "source c++"),
            ("Unknown", Some("unknown"), "Unknown", "text plain"),
            ("<script>", Some("<script>"), "<script>", "text plain"),
            ("", None, "Plain Text", "text plain"),
        ] {
            let html = highlight(info, "x\n");
            let fragment = scraper::Html::parse_fragment(&html);
            let select = |selector| {
                let selector = scraper::Selector::parse(selector).unwrap();
                fragment.select(&selector).next().unwrap()
            };

            assert_eq!(
                select(".code-block").value().attr("data-lang"),
                data_lang,
                "{info}"
            );
            assert_eq!(
                select(".code-lang").text().collect::<String>(),
                label,
                "{info}"
            );
            assert_eq!(
                select(".line > span").value().attr("class"),
                Some(scope),
                "{info}"
            );
        }
    }

    #[test]
    fn marks_highlighted_rows() {
        let code = indoc! {"
            first line
            second line
            third line
        "};
        let crlf = code.replace('\n', "\r\n");
        for code in [code, code.trim_end(), &crlf] {
            let html = highlight(r#"txt {highlight="2-3"}"#, code);

            assert_eq!(
                rendered_rows(&html),
                (
                    code.replace("\r\n", "\n"),
                    rows(&[
                        ("first line", false),
                        ("second line", true),
                        ("third line", true)
                    ])
                )
            );
            assert!(
                html.contains(indoc! {r#"
                    <pre><span class="line-number">1</span>
                    <span class="line-number hl">2</span>
                    <span class="line-number hl">3</span></pre>"#}),
                "{html}"
            );
        }
    }

    #[test]
    fn preserves_multiline_syntax_scopes() {
        let code = indoc! {"
            /* first
            middle <&>
            last */
            let value = 1;
        "};
        let crlf = code.replace('\n', "\r\n");
        for code in [code, code.trim_end(), &crlf] {
            let html = highlight(r#"rust {highlight="2-3"}"#, code);

            assert_eq!(
                rendered_rows(&html),
                (
                    code.replace("\r\n", "\n"),
                    rows(&[
                        ("/* first", false),
                        ("middle <&>", true),
                        ("last */", true),
                        ("let value = 1;", false),
                    ])
                )
            );
            assert_eq!(
                rows_matching(&html, ".comment.block"),
                [true, true, true, false]
            );
        }
    }

    #[test]
    fn preserves_empty_rows() {
        for (code, expected) in [
            ("", rows(&[("", true)])),
            ("\n", rows(&[("", true)])),
            ("first\n\n", rows(&[("first", true), ("", false)])),
            ("first\r\n\r\n", rows(&[("first", true), ("", false)])),
        ] {
            let html = highlight(r#"{highlight="1"}"#, code);
            assert_eq!(
                rendered_rows(&html),
                (code.replace("\r\n", "\n"), expected),
                "{code:?}"
            );
        }
    }

    #[test]
    fn disclosure_controls_open_state_and_line_limit() {
        for (info, open, max_lines) in [
            ("rust", true, Some("40")),
            ("rust {collapse}", false, None),
            ("rust {expand}", true, None),
        ] {
            let html = highlight_code(
                "fn main() {}\n",
                &parse_fence_info(info),
                NonZeroUsize::new(40),
            );
            let fragment = scraper::Html::parse_fragment(&html);
            let select = |selector| {
                let selector = scraper::Selector::parse(selector).unwrap();
                fragment.select(&selector).next().unwrap()
            };

            assert_eq!(
                select(".code-block").value().attr("open").is_some(),
                open,
                "{info}"
            );
            let body = select(".code-body");
            assert_eq!(body.value().attr("data-max-lines"), max_lines, "{info}");
            assert_eq!(
                body.value().attr("style"),
                max_lines.map(|n| format!("--max-lines: {n}")).as_deref(),
                "{info}"
            );
        }
    }

    #[test]
    fn preserves_source_when_syntax_dependency_is_missing() {
        // Syntect's non-exhaustive context-reference variants require deserialization.
        let missing_scope = serde_yaml::from_str(indoc! {"
            !ByScope
            scope: source.missing
            sub_context: null
            with_escape: false
        "})
        .unwrap();
        let mut main = Context::new(true);
        main.patterns.push(Pattern::Match(MatchPattern::new(
            false,
            "<missing&>".into(),
            Vec::new(),
            None,
            MatchOperation::Push(vec![missing_scope]),
            None,
        )));
        // Hand-built definitions need the startup context normally added by syntect's YAML loader.
        let mut start = Context::new(false);
        start.meta_content_scope = vec![Scope::new("source.example").unwrap()];
        start.patterns.push(Pattern::Include(
            serde_yaml::from_str("!Named main").unwrap(),
        ));
        let mut builder = SyntaxSetBuilder::new();
        builder.add(SyntaxDefinition {
            name: "Example".into(),
            file_extensions: vec!["example".into()],
            scope: Scope::new("source.example").unwrap(),
            first_line_match: None,
            hidden: false,
            variables: HashMap::new(),
            contexts: HashMap::from([("main".into(), main), ("__start".into(), start)]),
        });
        let syntax_set = builder.build();
        let code = indoc! {"
            before
            <missing&>
            after
        "};
        let crlf = code.replace('\n', "\r\n");
        let spec = parse_fence_info(r#"example {highlight="2"}"#);

        for code in [code, code.trim_end(), &crlf] {
            let html = highlight_with(&syntax_set, code, &spec, None);

            assert_eq!(
                rendered_rows(&html),
                (
                    code.replace("\r\n", "\n"),
                    rows(&[("before", false), ("<missing&>", true), ("after", false)])
                )
            );
            assert_eq!(rows_matching(&html, ".source.example"), [true, false, true]);
        }
    }

    // ── display_label ──

    #[test]
    fn capitalizes_unknown_tokens() {
        assert_eq!(display_label("rust"), "Rust");
        assert_eq!(display_label("python"), "Python");
        assert_eq!(display_label("go"), "Go");
        assert_eq!(display_label("dockerfile"), "Dockerfile");
        assert_eq!(display_label("unknown-lang"), "Unknown-lang");
    }

    #[test]
    fn labels_plain_text_aliases() {
        for token in ["", "plain", "plaintext", "text"] {
            assert_eq!(display_label(token), "Plain Text", "for input {token:?}");
        }
    }

    #[test]
    fn expands_short_forms() {
        assert_eq!(display_label("js"), "JavaScript");
        assert_eq!(display_label("kt"), "Kotlin");
        assert_eq!(display_label("md"), "Markdown");
        assert_eq!(display_label("ps1"), "PowerShell");
        assert_eq!(display_label("py"), "Python");
        assert_eq!(display_label("rs"), "Rust");
        assert_eq!(display_label("ts"), "TypeScript");
    }

    #[test]
    fn collapses_shell_family() {
        // Different shell flavors share the syntect grammar, so the label collapses to "Shell".
        for shell in ["bash", "fish", "sh", "zsh"] {
            assert_eq!(display_label(shell), "Shell", "for input {shell:?}");
        }
    }

    #[test]
    fn spells_punctuated_names() {
        assert_eq!(display_label("cs"), "C#");
        assert_eq!(display_label("cpp"), "C++");
        assert_eq!(display_label("c++"), "C++");
        assert_eq!(display_label("fs"), "F#");
    }

    #[test]
    fn keeps_brand_casing() {
        assert_eq!(display_label("gql"), "GraphQL");
        assert_eq!(display_label("graphql"), "GraphQL");
        assert_eq!(display_label("sass"), "Sass");
    }

    #[test]
    fn uppercases_acronyms() {
        for token in ["css", "html", "json", "php", "scss", "toml", "yaml"] {
            assert_eq!(
                display_label(token),
                token.to_uppercase(),
                "for input {token:?}"
            );
        }
    }

    fn highlight(info: &str, code: &str) -> String {
        highlight_code(code, &parse_fence_info(info), None)
    }

    /// Returns the rendered code text and each row's text and `hl` flag.
    fn rendered_rows(html: &str) -> (String, Vec<(String, bool)>) {
        let fragment = scraper::Html::parse_fragment(html);
        let code = scraper::Selector::parse(".code code").unwrap();
        let line = scraper::Selector::parse(".code .line").unwrap();
        let text = fragment.select(&code).next().unwrap().text().collect();
        let rows = fragment
            .select(&line)
            .map(|row| {
                (
                    row.text().collect(),
                    row.value().classes().any(|class| class == "hl"),
                )
            })
            .collect();
        (text, rows)
    }

    /// Returns whether each row contains an element matching `selector`.
    fn rows_matching(html: &str, selector: &str) -> Vec<bool> {
        let fragment = scraper::Html::parse_fragment(html);
        let line = scraper::Selector::parse(".code .line").unwrap();
        let selector = scraper::Selector::parse(selector).unwrap();
        fragment
            .select(&line)
            .map(|row| row.select(&selector).next().is_some())
            .collect()
    }

    fn rows(expected: &[(&str, bool)]) -> Vec<(String, bool)> {
        expected
            .iter()
            .map(|&(text, hl)| (text.to_owned(), hl))
            .collect()
    }
}
