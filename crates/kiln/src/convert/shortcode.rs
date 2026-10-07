use std::borrow::Cow;
use std::fmt::Write as _;

use anyhow::{Result, bail, ensure};
use percent_encoding::{AsciiSet, CONTROLS, utf8_percent_encode};

use crate::attrs::{AttrToken, AttrTokens, scan_quoted_value};
use crate::directive::CalloutKind;
use crate::markdown::code_ranges;

#[derive(Default)]
struct ShortcodeArgs<'a> {
    positional: Vec<Cow<'a, str>>,
    named: Vec<(&'a str, Cow<'a, str>)>,
}

impl ShortcodeArgs<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        self.named
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v.as_ref())
    }
}

/// Converts supported Hugo shortcodes while preserving prose and literal code.
pub(crate) fn convert_shortcodes(content: &str) -> Result<String> {
    let mut output = String::with_capacity(content.len());
    let mut stack = Vec::new();
    convert_body(content, &mut output, &mut stack)?;
    if let Some(name) = stack.last() {
        bail!("unclosed Hugo shortcode `{name}`");
    }
    Ok(output)
}

fn convert_body(mut input: &str, out: &mut String, stack: &mut Vec<String>) -> Result<()> {
    let source_len = input.len();
    let protected = code_ranges(input);
    while let Some(start) = [input.find("{{<"), input.find("{{%")]
        .into_iter()
        .flatten()
        .min()
    {
        let offset = source_len - input.len();
        let candidate = offset + start;
        let protected_index = protected.partition_point(|range| range.end <= candidate);
        if let Some(range) = protected
            .get(protected_index)
            .filter(|range| range.contains(&candidate))
        {
            let end = range.end - offset;
            out.push_str(&input[..end]);
            input = &input[end..];
            continue;
        }
        out.push_str(&input[..start]);
        let marker = &input[start + 2..start + 3];
        let rest = &input[start + 3..];
        let end = shortcode_end(rest, marker)?;
        let shortcode = rest[..end].trim();
        input = &rest[end + 3..];
        let (name, arguments) = shortcode
            .split_once(char::is_whitespace)
            .unwrap_or((shortcode, ""));
        let identifier = name.strip_prefix('/').unwrap_or(name);
        ensure!(
            !identifier.is_empty()
                && identifier
                    .chars()
                    .all(|ch| ch.is_alphanumeric() || matches!(ch, '-' | '_')),
            "invalid Hugo shortcode name `{name}`"
        );
        let block = name != "image";
        if block && !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }

        if let Some(name) = name.strip_prefix('/') {
            ensure!(
                arguments.is_empty(),
                "closing shortcode `{name}` cannot have arguments"
            );
            ensure!(
                stack.pop().as_deref() == Some(name),
                "unmatched closing shortcode `{name}`"
            );
            out.push_str(if name == "mermaid" { "```" } else { ":::" });
        } else {
            let args = parse_shortcode_args(arguments)?;
            match name {
                "admonition" => {
                    emit_callout(&args, out)?;
                    stack.push(name.to_owned());
                }
                "mermaid" => {
                    ensure!(
                        arguments.is_empty(),
                        "mermaid shortcode arguments are unsupported"
                    );
                    out.push_str("```mermaid");
                    stack.push(name.to_owned());
                }
                "style" => bail!("style shortcode requires manual conversion to page CSS"),
                "image" => out.push_str(&emit_image(&args)?),
                _ => out.push_str(&emit_directive(name, &args)),
            }
        }
        if block {
            out.push('\n');
            input = input
                .strip_prefix("\r\n")
                .or_else(|| input.strip_prefix('\n'))
                .unwrap_or(input);
        }
    }
    out.push_str(input);
    Ok(())
}

fn shortcode_end(input: &str, marker: &str) -> Result<usize> {
    let closing = format!("{}{}", if marker == "<" { ">" } else { "%" }, "}}");
    let mut offset = 0;
    while offset < input.len() {
        let rest = &input[offset..];
        if rest.starts_with(&closing) {
            return Ok(offset);
        }
        if let Some(quoted) = rest.strip_prefix('"') {
            let (end, _) = scan_quoted_value(quoted);
            ensure!(end < quoted.len(), "unclosed quote in Hugo shortcode");
            offset += end + 2;
        } else {
            offset += rest.chars().next().map_or(0, char::len_utf8);
        }
    }
    bail!("unclosed Hugo shortcode")
}

fn parse_shortcode_args(input: &str) -> Result<ShortcodeArgs<'_>> {
    let mut args = ShortcodeArgs::default();
    for token in AttrTokens::values(input) {
        match token {
            AttrToken::Named(key, value) => {
                ensure!(
                    args.get(key).is_none(),
                    "duplicate shortcode argument `{key}`"
                );
                args.named.push((key, value));
            }
            AttrToken::Quoted(value) => args.positional.push(value),
            AttrToken::Bare(value) => args.positional.push(Cow::Borrowed(value)),
            AttrToken::Id(value) => args.positional.push(Cow::Owned(format!("#{value}"))),
            AttrToken::Class(value) => args.positional.push(Cow::Owned(format!(".{value}"))),
        }
    }
    ensure!(
        args.positional
            .iter()
            .chain(args.named.iter().map(|(_, value)| value))
            .all(|value| !value.contains(['\r', '\n'])),
        "multiline shortcode values require manual conversion"
    );
    ensure!(
        args.named.is_empty() || args.positional.is_empty(),
        "cannot mix named and positional Hugo shortcode arguments"
    );
    Ok(args)
}

fn emit_callout(args: &ShortcodeArgs, out: &mut String) -> Result<()> {
    ensure!(
        args.positional.len() <= 3,
        "admonition accepts type, title and open arguments"
    );
    for (name, _) in &args.named {
        ensure!(
            ["type", "title", "open"].contains(name),
            "unsupported admonition argument `{name}`"
        );
    }
    let positional = |index| args.positional.get(index).map(AsRef::as_ref);
    let kind = args.get("type").or_else(|| positional(0)).unwrap_or("note");
    ensure!(
        kind.parse::<CalloutKind>().is_ok(),
        "unsupported admonition type `{kind}`"
    );
    let title = args.get("title").or_else(|| positional(1));
    let open = args.get("open").or_else(|| positional(2)).unwrap_or("true");
    ensure!(
        ["true", "false"].contains(&open),
        "admonition open must be true or false"
    );

    _ = write!(out, "::: callout {{type={kind}");
    if let Some(title) = title {
        _ = write!(out, " title={}", quote(title));
    }
    if open == "false" {
        out.push_str(" open=false");
    }
    out.push('}');
    Ok(())
}

fn emit_image(args: &ShortcodeArgs) -> Result<String> {
    const DESTINATION_ESCAPE: &AsciiSet = &CONTROLS
        .add(b' ')
        .add(b'<')
        .add(b'>')
        .add(b'(')
        .add(b')')
        .add(b'"')
        .add(b'\\');

    ensure!(
        args.positional.is_empty(),
        "image shortcode requires named arguments"
    );
    for (name, _) in &args.named {
        ensure!(
            ["src", "alt", "caption", "width", "height"].contains(name),
            "unsupported image argument `{name}`"
        );
    }
    let src = args
        .get("src")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("image shortcode requires src"))?;
    let alt = args
        .get("alt")
        .or_else(|| args.get("caption"))
        .unwrap_or("");
    if let (Some(alt), Some(caption)) = (args.get("alt"), args.get("caption"))
        && alt != caption
    {
        tracing::warn!(
            argument = "caption",
            "image caption differs from alt and requires manual migration"
        );
    }
    let mut escaped_alt = String::new();
    for ch in alt.chars() {
        if ch.is_ascii_punctuation() {
            escaped_alt.push('\\');
        }
        escaped_alt.push(ch);
    }
    let src = utf8_percent_encode(src, DESTINATION_ESCAPE)
        .to_string()
        .replace('&', "&amp;");
    let mut output = format!("![{escaped_alt}]({src})");
    let mut attributes = Vec::new();
    for key in ["width", "height"] {
        if let Some(value) = args.get(key) {
            ensure!(
                value.parse::<u32>().is_ok(),
                "image {key} must be an unsigned integer"
            );
            attributes.push(format!("{key}={value}"));
        }
    }
    if !attributes.is_empty() {
        _ = write!(output, "{{{}}}", attributes.join(" "));
    }
    Ok(output)
}

fn emit_directive(name: &str, args: &ShortcodeArgs) -> String {
    let mut output = format!("::: {name}");
    let arguments: Vec<_> = args
        .positional
        .iter()
        .map(|value| quote(value))
        .chain(
            args.named
                .iter()
                .map(|(key, value)| format!("{key}={}", quote(value))),
        )
        .collect();
    if !arguments.is_empty() {
        _ = write!(output, " {{{}}}", arguments.join(" "));
    }
    output.push_str("\n:::");
    output
}

fn quote(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::*;

    // ── convert_shortcodes ──

    #[test]
    fn convert_shortcodes_preserves_inline_body_and_surrounding_prose() {
        assert_eq!(
            convert_shortcodes(r#"Before{{< admonition type="warning" title="A \"quote\"" open=false >}}Keep **this**.{{< /admonition >}}After"#).unwrap(),
            indoc! {r#"
                Before
                ::: callout {type=warning title="A \"quote\"" open=false}
                Keep **this**.
                :::
                After"#},
        );
    }

    #[test]
    fn convert_shortcodes_defaults_and_nested_blocks() {
        let input = indoc! {r#"
            {{< admonition >}}
            Outer
            {{< admonition info "A \"quoted\" title" false >}}
            Inner
            {{< /admonition >}}
            {{< /admonition >}}
        "#};
        assert_eq!(
            convert_shortcodes(input).unwrap(),
            indoc! {r#"
                ::: callout {type=note}
                Outer
                ::: callout {type=info title="A \"quoted\" title" open=false}
                Inner
                :::
                :::
            "#}
        );
    }

    #[test]
    fn convert_shortcodes_preserves_image_link_dimensions_and_alt() {
        assert_eq!(convert_shortcodes(r#"[{{< image src="a b.svg" alt="[Icon]" width=50 height=30 >}}](https://example.com)"#).unwrap(),
            r"[![\[Icon\]](a%20b.svg){width=50 height=30}](https://example.com)");

        for (arguments, expected_alt) in [
            (r#"alt="Authored" caption="Caption""#, "Authored"),
            (r#"caption="Caption""#, "Caption"),
        ] {
            assert_eq!(
                convert_shortcodes(&format!("{{{{< image src=x {arguments} >}}}}")).unwrap(),
                format!("![{expected_alt}](x)"),
            );
        }
    }

    #[test]
    fn convert_shortcodes_preserves_mermaid_and_generic_arguments() {
        assert_eq!(
            convert_shortcodes(indoc! {r#"
                {{< mermaid >}}
                graph TB
                  A --> B
                {{< /mermaid >}}
                {{< widget title="A \"quote\" >}} B" >}}
            "#})
            .unwrap(),
            indoc! {r#"
                ```mermaid
                graph TB
                  A --> B
                ```
                ::: widget {title="A \"quote\" >}} B"}
                :::
            "#}
        );
    }

    #[test]
    fn convert_shortcodes_literal_code_is_unchanged() {
        let input = indoc! {r"
            ```markdown
            {{< admonition >}}
            ```
        "};
        assert_eq!(convert_shortcodes(input).unwrap(), input);
    }

    #[test]
    fn convert_shortcodes_multiline_arguments_preserve_backticks() {
        assert_eq!(
            convert_shortcodes(indoc! {r#"
                {{< admonition
                    type="note"
                    title="Use `code`"
                >}}
                Body
                {{< /admonition >}}
            "#})
            .unwrap(),
            indoc! {r#"
                ::: callout {type=note title="Use `code`"}
                Body
                :::
            "#}
        );
    }

    #[test]
    fn convert_shortcodes_malformed_or_lossy_inputs_returns_error() {
        for input in [
            "{{< /unknown >}}",
            "{{< admonition >}}body",
            "{{< image src=x",
            "{{< style >}}body{{< /style >}}",
            "{{< admonition >}}body{{< /mermaid >}}",
            "{{< image src=\"unfinished >}}",
            "{{< image src=x linked=false >}}",
            "{{< admonition open=maybe >}}",
            "{{< image src=x src=y >}}",
            "{{< >}}",
            indoc! {r#"
                {{< admonition title="multiple
                lines" >}}
            "#},
        ] {
            assert!(convert_shortcodes(input).is_err(), "{input}");
        }
    }

    // ── parse_shortcode_args ──

    #[test]
    fn parse_shortcode_args_decodes_escaped_values() {
        let args = parse_shortcode_args(r#"title="A \"quoted\" title" path="C:\\docs""#).unwrap();
        assert_eq!(args.get("title"), Some(r#"A "quoted" title"#));
        assert_eq!(args.get("path"), Some(r"C:\docs"));
    }
}
