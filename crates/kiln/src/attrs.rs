use std::borrow::Cow;

/// Parsed Pandoc-style `{...}` attribute block.
///
/// Extracts `#id` (first wins), `.class` tokens, and `key=value` pairs.
#[derive(Debug, Default)]
pub(crate) struct PandocAttrs<'a> {
    pub id: Option<&'a str>,
    pub classes: Vec<&'a str>,
    pub kvs: Vec<(&'a str, Cow<'a, str>)>,
}

/// Parses a Pandoc-style attribute string (`#id`, `.class`, `key=value`).
///
/// First `#id` wins. Quoted values support `\"` / `\\` escapes. Unclosed quotes consume the
/// rest of the input. Bare words are skipped.
#[must_use]
pub(crate) fn parse_pandoc_attrs(input: &str) -> PandocAttrs<'_> {
    let mut result = PandocAttrs::default();
    let mut rest = input.trim();

    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix('#') {
            let end = after.find(char::is_whitespace).unwrap_or(after.len());
            if result.id.is_none() && end > 0 {
                result.id = Some(&after[..end]);
            }
            rest = after[end..].trim_start();
            continue;
        }

        if let Some(after) = rest.strip_prefix('.') {
            let end = after.find(char::is_whitespace).unwrap_or(after.len());
            if end > 0 {
                result.classes.push(&after[..end]);
            }
            rest = after[end..].trim_start();
            continue;
        }

        let next_eq = rest.find('=');
        let next_ws = rest.find(char::is_whitespace).unwrap_or(rest.len());

        let Some(eq) = next_eq.filter(|&p| p < next_ws) else {
            rest = rest[next_ws..].trim_start();
            continue;
        };

        let key = &rest[..eq];
        let after_eq = &rest[eq + 1..];

        if let Some(after_quote) = after_eq.strip_prefix('"') {
            let (end, has_escapes) = scan_quoted_value(after_quote);
            let raw = &after_quote[..end];
            let value = if has_escapes {
                Cow::Owned(unescape_quoted(raw))
            } else {
                Cow::Borrowed(raw)
            };
            result.kvs.push((key, value));
            rest = after_quote.get(end + 1..).unwrap_or("").trim_start();
        } else {
            let end = after_eq.find(char::is_whitespace).unwrap_or(after_eq.len());
            result.kvs.push((key, Cow::Borrowed(&after_eq[..end])));
            rest = after_eq[end..].trim_start();
        }
    }

    result
}

/// Scans a quoted value for the closing `"`, respecting `\"` and `\\` escapes.
/// Returns `(end_offset, has_escapes)` where `end_offset` is the byte position of the closing
/// quote (or end of string if unclosed).
pub(crate) fn scan_quoted_value(s: &str) -> (usize, bool) {
    let bytes = s.as_bytes();
    let mut i = 0;
    let mut has_escapes = false;

    while i < bytes.len() {
        match bytes[i] {
            b'\\' if i + 1 < bytes.len() && matches!(bytes[i + 1], b'"' | b'\\') => {
                has_escapes = true;
                i += 2;
            }
            b'"' => return (i, has_escapes),
            _ => i += 1,
        }
    }

    (s.len(), has_escapes)
}

/// Unescapes `\"` → `"` and `\\` → `\` in a quoted attribute value.
pub(crate) fn unescape_quoted(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some(c @ ('"' | '\\')) => result.push(c),
                Some(other) => {
                    result.push('\\');
                    result.push(other);
                }
                None => result.push('\\'),
            }
        } else {
            result.push(c);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_pandoc_attrs ──

    fn kvs(input: &str) -> Vec<(&str, String)> {
        parse_pandoc_attrs(input)
            .kvs
            .into_iter()
            .map(|(k, v)| (k, v.into_owned()))
            .collect()
    }

    fn pair<'a>(k: &'a str, v: &str) -> (&'a str, String) {
        (k, v.to_string())
    }

    #[test]
    fn parse_pandoc_attrs_empty() {
        let result = parse_pandoc_attrs("");
        assert!(result.id.is_none());
        assert!(result.classes.is_empty());
        assert!(result.kvs.is_empty());
    }

    #[test]
    fn parse_pandoc_attrs_unquoted_value() {
        assert_eq!(kvs("key=value"), vec![pair("key", "value")]);
    }

    #[test]
    fn parse_pandoc_attrs_quoted_value() {
        assert_eq!(
            kvs(r#"key="hello world""#),
            vec![pair("key", "hello world")]
        );
    }

    #[test]
    fn parse_pandoc_attrs_escaped_quotes() {
        assert_eq!(
            kvs(r#"title="He said \"hi\"""#),
            vec![pair("title", r#"He said "hi""#)]
        );
        // Escaped backslash.
        assert_eq!(kvs(r#"title="path\\to""#), vec![pair("title", r"path\to")]);
        // Unrecognized escape alone — no escapes detected, takes borrowed path.
        assert_eq!(kvs(r#"title="foo\nbar""#), vec![pair("title", r"foo\nbar")]);
        // Mixed recognized and unknown escapes — unknown sequences preserved as-is.
        assert_eq!(kvs(r#"title="a\"b\nc""#), vec![pair("title", r#"a"b\nc"#)]);
    }

    #[test]
    fn parse_pandoc_attrs_unclosed_quote() {
        assert_eq!(
            kvs(r#"key="no closing quote"#),
            vec![pair("key", "no closing quote")]
        );
        // Trailing backslash in unclosed quote.
        assert_eq!(kvs(r#"key="a\"b\"#), vec![pair("key", r#"a"b\"#)]);
    }

    #[test]
    fn parse_pandoc_attrs_multiple_pairs() {
        assert_eq!(
            kvs(r#"title="Title" open=false"#),
            vec![pair("title", "Title"), pair("open", "false")]
        );
    }

    #[test]
    fn parse_pandoc_attrs_extracts_class_and_id() {
        let input = ".highlight #my-id open=false";
        let result = parse_pandoc_attrs(input);
        assert_eq!(result.id, Some("my-id"));
        assert_eq!(result.classes, vec!["highlight"]);
        assert_eq!(kvs(input), vec![pair("open", "false")]);
    }

    #[test]
    fn parse_pandoc_attrs_first_id_wins() {
        let result = parse_pandoc_attrs("#first #second");
        assert_eq!(result.id, Some("first"));
    }

    #[test]
    fn parse_pandoc_attrs_multiple_classes() {
        let result = parse_pandoc_attrs(".a .b .c");
        assert_eq!(result.classes, vec!["a", "b", "c"]);
    }

    #[test]
    fn parse_pandoc_attrs_empty_hash_and_dot_ignored() {
        let result = parse_pandoc_attrs("# . .real");
        assert_eq!(result.id, None);
        assert_eq!(result.classes, vec!["real"]);
    }

    #[test]
    fn parse_pandoc_attrs_skips_bare_words() {
        assert_eq!(kvs(r#"bare title="Title""#), vec![pair("title", "Title")]);
    }
}
