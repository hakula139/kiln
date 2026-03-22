use std::sync::LazyLock;

use regex::Regex;

use crate::markdown::for_each_non_code_line;

// -- Shortcode parsing --

/// Matches an opening or self-closing Hugo shortcode: `{{< name args >}}`.
static SHORTCODE_OPEN_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\{\{<\s*([\w-]+)\s*(.*?)\s*>\}\}").expect("shortcode open regex should compile")
});

/// Matches a closing Hugo shortcode: `{{< /name >}}`.
static SHORTCODE_CLOSE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\{\{<\s*/\s*([\w-]+)\s*>\}\}").expect("shortcode close regex should compile")
});

/// Parsed shortcode arguments: positional values and named `key="value"` pairs.
struct ShortcodeArgs<'a> {
    positional: Vec<&'a str>,
    named: Vec<(&'a str, &'a str)>,
}

impl<'a> ShortcodeArgs<'a> {
    fn get(&self, key: &str) -> Option<&'a str> {
        self.named.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
    }
}

/// Tokenizes a shortcode argument string into positional and named args.
///
/// Handles `"quoted"` and bare-word positional args, plus `key="value"` and
/// `key=value` named params. Unclosed quotes consume the rest of the input.
fn parse_shortcode_args(input: &str) -> ShortcodeArgs<'_> {
    let mut positional = Vec::new();
    let mut named = Vec::new();
    let mut rest = input.trim();

    while !rest.is_empty() {
        if let Some(after_quote) = rest.strip_prefix('"') {
            let end = after_quote.find('"').unwrap_or(after_quote.len());
            positional.push(&after_quote[..end]);
            rest = after_quote.get(end + 1..).unwrap_or("").trim_start();
            continue;
        }

        let next_eq = rest.find('=');
        let next_ws = rest.find(char::is_whitespace).unwrap_or(rest.len());

        if let Some(eq) = next_eq.filter(|&p| p < next_ws) {
            let key = &rest[..eq];
            let after_eq = &rest[eq + 1..];

            if let Some(after_quote) = after_eq.strip_prefix('"') {
                let end = after_quote.find('"').unwrap_or(after_quote.len());
                named.push((key, &after_quote[..end]));
                rest = after_quote.get(end + 1..).unwrap_or("").trim_start();
            } else {
                let end = after_eq.find(char::is_whitespace).unwrap_or(after_eq.len());
                named.push((key, &after_eq[..end]));
                rest = after_eq[end..].trim_start();
            }
            continue;
        }

        positional.push(&rest[..next_ws]);
        rest = rest[next_ws..].trim_start();
    }

    ShortcodeArgs { positional, named }
}

// -- Conversion --

/// Converts all shortcodes in the body text, skipping code blocks.
pub(crate) fn convert_shortcodes(content: &str) -> String {
    let mut output = String::with_capacity(content.len());

    for_each_non_code_line(content, &mut output, |line, out| {
        convert_line(line, out);
    });

    output
}

fn convert_line(line: &str, out: &mut String) {
    if let Some(caps) = SHORTCODE_CLOSE_RE.captures(line) {
        emit_closing(&caps[1], out);
        return;
    }

    let Some(caps) = SHORTCODE_OPEN_RE.captures(line) else {
        out.push_str(line);
        return;
    };

    let name = &caps[1];
    let sc = parse_shortcode_args(&caps[2]);

    // Paired shortcodes always occupy their own line.
    match name {
        "admonition" => {
            emit_callout(&sc, out);
            return;
        }
        "style" => {
            out.push_str("<!-- TODO: style shortcode not yet supported: ");
            out.push_str(line.trim());
            out.push_str(" -->\n");
            return;
        }
        "mermaid" => {
            out.push_str("```mermaid\n");
            return;
        }
        _ => {}
    }

    // Self-closing shortcodes may appear inline — replace in-place.
    out.push_str(
        &SHORTCODE_OPEN_RE.replace_all(line, |caps: &regex::Captures| {
            let sc = parse_shortcode_args(&caps[2]);
            emit_self_closing(&caps[1], &sc)
        }),
    );
}

// -- Paired shortcodes --

fn emit_closing(name: &str, out: &mut String) {
    match name {
        "admonition" => out.push_str(":::\n"),
        "style" => out.push_str("<!-- /style -->\n"),
        "mermaid" => out.push_str("```\n"),
        _ => {}
    }
}

fn emit_callout(sc: &ShortcodeArgs, out: &mut String) {
    let type_name = sc.positional.first().copied().unwrap_or("");
    let remaining = &sc.positional[1..];

    let (title, open) = match remaining {
        [.., "false"] => (remaining[..remaining.len() - 1].first().copied(), false),
        _ => (remaining.first().copied(), true),
    };

    let mut attrs = vec![format!("type={type_name}")];
    if let Some(title) = title {
        attrs.push(format!(r#"title="{title}""#));
    }
    if !open {
        attrs.push("open=false".to_string());
    }
    out.push_str("::: callout { ");
    out.push_str(&attrs.join(" "));
    out.push_str(" }\n");
}

// -- Self-closing shortcodes --

fn emit_self_closing(name: &str, sc: &ShortcodeArgs) -> String {
    match name {
        "image" => emit_image(sc),
        _ => emit_directive(name, sc),
    }
}

fn emit_image(sc: &ShortcodeArgs) -> String {
    let src = sc.get("src").unwrap_or("");
    let alt = sc.get("alt").or_else(|| sc.get("caption")).unwrap_or("");
    let width = sc.get("width");
    let height = sc.get("height");

    let mut out = format!("![{alt}]({src})");
    let attrs: Vec<String> = [("width", width), ("height", height)]
        .into_iter()
        .filter_map(|(k, v)| v.map(|v| format!("{k}={v}")))
        .collect();
    if !attrs.is_empty() {
        out.push('{');
        out.push_str(&attrs.join(" "));
        out.push('}');
    }
    out
}

/// Emits a generic kiln directive for self-closing shortcodes.
fn emit_directive(name: &str, sc: &ShortcodeArgs) -> String {
    let mut out = format!("::: {name}");
    if !sc.positional.is_empty() || !sc.named.is_empty() {
        let mut args: Vec<String> = Vec::new();
        for arg in &sc.positional {
            args.push(format!(r#""{arg}""#));
        }
        for (key, value) in &sc.named {
            args.push(format!(r#"{key}="{value}""#));
        }
        out.push_str(" { ");
        out.push_str(&args.join(" "));
        out.push_str(" }");
    }
    out.push_str("\n:::");
    out
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::*;

    // -- parse_shortcode_args --

    #[test]
    fn parse_positional() {
        let sc = parse_shortcode_args(r#"info "Title" false"#);
        assert_eq!(sc.positional, vec!["info", "Title", "false"]);
        assert!(sc.named.is_empty());
    }

    #[test]
    fn parse_named() {
        let sc = parse_shortcode_args(r#"src="test.webp" width="500""#);
        assert!(sc.positional.is_empty());
        assert_eq!(sc.named, vec![("src", "test.webp"), ("width", "500")]);
    }

    #[test]
    fn parse_unquoted_value() {
        let sc = parse_shortcode_args(r#"src="icon.svg" linked=false"#);
        assert!(sc.positional.is_empty());
        assert_eq!(sc.named, vec![("src", "icon.svg"), ("linked", "false")]);
    }

    #[test]
    fn parse_unquoted_cjk() {
        let sc = parse_shortcode_args("info 封面出处 false");
        assert_eq!(sc.positional, vec!["info", "封面出处", "false"]);
        assert!(sc.named.is_empty());
    }

    // -- callout (from admonition) --

    #[test]
    fn callout_basic() {
        let input = indoc! {r#"
            {{< admonition info "Title" >}}
            Body content
            {{< /admonition >}}
        "#};
        let result = convert_shortcodes(input);
        assert_eq!(
            result,
            indoc! {r#"
                ::: callout { type=info title="Title" }
                Body content
                :::
            "#}
        );
    }

    #[test]
    fn callout_no_title() {
        let input = indoc! {"
            {{< admonition warning >}}
            Content
            {{< /admonition >}}
        "};
        let result = convert_shortcodes(input);
        assert_eq!(
            result,
            indoc! {"
                ::: callout { type=warning }
                Content
                :::
            "}
        );
    }

    #[test]
    fn callout_unquoted_title() {
        let input = indoc! {"
            {{< admonition info 封面出处 >}}
            Body
            {{< /admonition >}}
        "};
        let result = convert_shortcodes(input);
        assert_eq!(
            result,
            indoc! {r#"
                ::: callout { type=info title="封面出处" }
                Body
                :::
            "#}
        );
    }

    #[test]
    fn callout_collapsed() {
        let input = indoc! {r#"
            {{< admonition abstract "Collapsed Block" false >}}
            Hidden content
            {{< /admonition >}}
        "#};
        let result = convert_shortcodes(input);
        assert_eq!(
            result,
            indoc! {r#"
                ::: callout { type=abstract title="Collapsed Block" open=false }
                Hidden content
                :::
            "#}
        );
    }

    // -- image --

    #[test]
    fn image_minimal() {
        let input = indoc! {r#"
            {{< image src="assets/test.webp" >}}
        "#};
        let result = convert_shortcodes(input);
        assert_eq!(result, "![](assets/test.webp)\n");
    }

    #[test]
    fn image_with_caption() {
        let input = indoc! {r#"
            {{< image src="assets/test.webp" caption="My Image" >}}
        "#};
        let result = convert_shortcodes(input);
        assert_eq!(result, "![My Image](assets/test.webp)\n");
    }

    #[test]
    fn image_prefers_alt_over_caption() {
        let input = indoc! {r#"
            {{< image src="icon.svg" alt="C++" caption="Ignored" >}}
        "#};
        let result = convert_shortcodes(input);
        assert_eq!(result, "![C++](icon.svg)\n");
    }

    #[test]
    fn image_with_width() {
        let input = indoc! {r#"
            {{< image src="assets/test.webp" caption="My Image" width="500" >}}
        "#};
        let result = convert_shortcodes(input);
        assert_eq!(result, "![My Image](assets/test.webp){width=500}\n");
    }

    #[test]
    fn image_with_width_and_height() {
        let input = indoc! {r#"
            {{< image src="icon.svg" alt="C++" width="50" height="50" >}}
        "#};
        let result = convert_shortcodes(input);
        assert_eq!(result, "![C++](icon.svg){width=50 height=50}\n");
    }

    #[test]
    fn image_inline() {
        let input = indoc! {r#"
            [{{< image src="icon.svg" alt="Rust" width="50" height="50" >}}](https://rust-lang.org)
        "#};
        let result = convert_shortcodes(input);
        assert_eq!(
            result,
            "[![Rust](icon.svg){width=50 height=50}](https://rust-lang.org)\n"
        );
    }

    // -- style --

    #[test]
    fn style_leaves_todo() {
        let input = indoc! {r#"
            {{< style "table { min-width: initial; }" >}}
            | A | B |
            {{< /style >}}
        "#};
        let result = convert_shortcodes(input);
        assert!(
            result.contains("<!-- TODO: style shortcode not yet supported:"),
            "got:\n{result}"
        );
        assert!(result.contains("| A | B |"), "got:\n{result}");
        assert!(result.contains("<!-- /style -->"), "got:\n{result}");
    }

    // -- mermaid --

    #[test]
    fn mermaid_block() {
        let input = indoc! {"
            {{< mermaid >}}
            graph TB
              A --> B
            {{< /mermaid >}}
        "};
        let result = convert_shortcodes(input);
        assert_eq!(
            result,
            indoc! {"
                ```mermaid
                graph TB
                  A --> B
                ```
            "}
        );
    }

    // -- self-closing directives --

    #[test]
    fn directive_with_positional_args() {
        let input = indoc! {r#"
            {{< my-widget "Title" "https://example.com" "Description" >}}
        "#};
        let result = convert_shortcodes(input);
        assert_eq!(
            result,
            indoc! {r#"
                ::: my-widget { "Title" "https://example.com" "Description" }
                :::
            "#}
        );
    }

    #[test]
    fn directive_with_named_args() {
        let input = indoc! {r#"
            {{< music server="abc" type="song" id="123" >}}
        "#};
        let result = convert_shortcodes(input);
        assert_eq!(
            result,
            indoc! {r#"
                ::: music { server="abc" type="song" id="123" }
                :::
            "#}
        );
    }

    // -- unknown closing tag --

    #[test]
    fn unknown_closing_tag_dropped() {
        let input = "{{< /unknown >}}\n";
        let result = convert_shortcodes(input);
        assert_eq!(
            result, "",
            "unknown closing tags should be silently dropped"
        );
    }

    // -- code block skipping --

    #[test]
    fn shortcode_inside_code_block_skipped() {
        let input = indoc! {r#"
            ```markdown
            {{< admonition info "Title" >}}
            ```
        "#};
        let result = convert_shortcodes(input);
        assert_eq!(
            result, input,
            "shortcodes inside code blocks should be preserved"
        );
    }
}
