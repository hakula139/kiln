use std::borrow::Cow;
use std::fmt::Write as _;

use anyhow::{Context, Result, bail, ensure};
use percent_encoding::{AsciiSet, CONTROLS, utf8_percent_encode};

use crate::attrs::{Arg, ArgTokens, quote, scan_quoted_value};
use crate::directive::callout;
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
            let (head, paired) = match name {
                "admonition" => (emit_callout(&args)?, true),
                "mermaid" => {
                    ensure!(
                        arguments.is_empty(),
                        "mermaid shortcode arguments are unsupported"
                    );
                    ("```mermaid".to_owned(), true)
                }
                "style" => bail!("style shortcode requires manual conversion to page CSS"),
                "image" => (emit_image(&args)?, false),
                _ => (emit_directive(name, &args), false),
            };
            out.push_str(&head);
            if paired {
                stack.push(name.to_owned());
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
    let closing = if marker == "<" { ">}}" } else { "%}}" };
    let mut offset = 0;
    while offset < input.len() {
        let rest = &input[offset..];
        if rest.starts_with(closing) {
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
    for arg in ArgTokens::new(input) {
        match arg {
            Arg::Named(key, value) => {
                ensure!(
                    args.get(key).is_none(),
                    "duplicate shortcode argument `{key}`"
                );
                args.named.push((key, value));
            }
            Arg::Positional(value) => args.positional.push(value),
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

fn emit_callout(args: &ShortcodeArgs) -> Result<String> {
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
    let kind = callout::find_type(kind)
        .with_context(|| format!("unsupported admonition type `{kind}`"))?;
    let title = args.get("title").or_else(|| positional(1));
    let open = match args.get("open").or_else(|| positional(2)) {
        Some(value) => {
            callout::parse_open(value).context("admonition open must be true or false")?
        }
        None => true,
    };

    let mut output = format!("::: callout {{type={kind}");
    if let Some(title) = title {
        _ = write!(output, " title={}", quote(title));
    }
    if !open {
        output.push_str(" open=false");
    }
    output.push('}');
    Ok(output)
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
        .context("image shortcode requires src")?;
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
    // CommonMark decodes entities in link destinations, so a raw `&copy;` would alter the URL.
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

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::*;

    // ── convert_shortcodes ──

    #[test]
    fn preserves_inline_body_and_surrounding_prose() {
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
    fn defaults_type_and_nests_blocks() {
        let input = indoc! {r#"
            {{< admonition >}}
            Outer
            {{% admonition info "A \"quoted\" title" false %}}
            Inner
            {{% /admonition %}}
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
    fn canonicalizes_admonition_type_and_open() {
        assert_eq!(
            convert_shortcodes(
                r#"{{< admonition Warning "Careful" FALSE >}}Body{{< /admonition >}}"#
            )
            .unwrap(),
            indoc! {r#"
                ::: callout {type=warning title="Careful" open=false}
                Body
                :::
            "#},
        );
    }

    #[test]
    fn preserves_image_link_dimensions_and_alt() {
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
    fn preserves_mermaid_and_generic_arguments() {
        assert_eq!(
            convert_shortcodes(indoc! {r#"
                {{< mermaid >}}
                graph TB
                  A --> B
                {{< /mermaid >}}
                {{< widget #anchor .class >}}
                {{< widget title="A \"quote\" >}} B" >}}
                {{< side-note_box >}}
            "#})
            .unwrap(),
            indoc! {r##"
                ```mermaid
                graph TB
                  A --> B
                ```
                ::: widget {"#anchor" ".class"}
                :::
                ::: widget {title="A \"quote\" >}} B"}
                :::
                ::: side-note_box
                :::
            "##}
        );
    }

    #[test]
    fn leaves_literal_code_unchanged() {
        let input = indoc! {r"
            ```markdown
            {{< admonition >}}
            ```
        "};
        assert_eq!(convert_shortcodes(input).unwrap(), input);
    }

    #[test]
    fn multiline_arguments_preserve_backticks() {
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
    fn rejects_malformed_or_lossy_inputs() {
        for (input, expected) in [
            ("{{< /unknown >}}", "unmatched closing shortcode `unknown`"),
            (
                "{{< admonition >}}body",
                "unclosed Hugo shortcode `admonition`",
            ),
            ("{{< image src=x", "unclosed Hugo shortcode"),
            (
                "{{< style >}}body{{< /style >}}",
                "style shortcode requires manual conversion to page CSS",
            ),
            (
                "{{< admonition >}}body{{< /mermaid >}}",
                "unmatched closing shortcode `mermaid`",
            ),
            (
                "{{< image src=\"unfinished >}}",
                "unclosed quote in Hugo shortcode",
            ),
            (
                "{{< image src=x linked=false >}}",
                "unsupported image argument `linked`",
            ),
            (
                "{{< admonition bogus >}}",
                "unsupported admonition type `bogus`",
            ),
            (
                "{{< admonition open=maybe >}}",
                "admonition open must be true or false",
            ),
            ("{{< image alt=x >}}", "image shortcode requires src"),
            (
                "{{< image src=x src=y >}}",
                "duplicate shortcode argument `src`",
            ),
            ("{{< >}}", "invalid Hugo shortcode name ``"),
            (
                "{{< my.widget >}}",
                "invalid Hugo shortcode name `my.widget`",
            ),
            (
                indoc! {r#"
                    {{< admonition title="multiple
                    lines" >}}
                "#},
                "multiline shortcode values require manual conversion",
            ),
        ] {
            let error = convert_shortcodes(input).unwrap_err();
            assert_eq!(error.to_string(), expected, "{input}");
        }
    }

    // ── parse_shortcode_args ──

    #[test]
    fn decodes_escaped_values() {
        let args = parse_shortcode_args(r#"title="A \"quoted\" title" path="C:\\docs""#).unwrap();
        assert_eq!(args.get("title"), Some(r#"A "quoted" title"#));
        assert_eq!(args.get("path"), Some(r"C:\docs"));
    }
}
