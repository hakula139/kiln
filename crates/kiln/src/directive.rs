pub mod callout;
pub mod div;
pub mod parser;

use std::borrow::Cow;
use std::fmt;
use std::ops::Range;

use serde::Serialize;
use strum::{AsRefStr, EnumIter, EnumString};

/// Known callout types.
///
/// - `AsRefStr` yields the lowercase identifier (e.g., `"note"`).
/// - `EnumString` provides case-insensitive [`FromStr`](std::str::FromStr).
/// - `Display` yields the titlecase form (e.g., `"Note"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, AsRefStr, EnumString, EnumIter)]
#[strum(serialize_all = "lowercase", ascii_case_insensitive)]
pub enum CalloutKind {
    Abstract,
    Bug,
    Danger,
    Example,
    Failure,
    Info,
    Note,
    Question,
    Quote,
    Success,
    Tip,
    Warning,
}

impl fmt::Display for CalloutKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut chars = self.as_ref().chars();
        if let Some(c) = chars.next() {
            write!(f, "{}{}", c.to_ascii_uppercase(), chars.as_str())
        } else {
            Ok(())
        }
    }
}

/// Parsed directive type — either a callout or an unrecognized name preserved
/// for future extension.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DirectiveKind {
    Callout {
        kind: CalloutKind,
        title: Option<String>,
        open: bool,
    },
    /// Unrecognized type — rendered as a `<div>` or passed through as-is.
    Unknown { name: String, args: String },
}

impl DirectiveKind {
    /// Parses a directive name and raw arguments into the appropriate variant.
    fn from_name(name: &str, args: &str) -> Self {
        if name.eq_ignore_ascii_case("callout") {
            let (kind, title, open) = callout::parse_args(args);
            return Self::Callout { kind, title, open };
        }
        Self::Unknown {
            name: name.to_string(),
            args: args.to_string(),
        }
    }
}

/// Serializable context passed to directive templates.
///
/// Templates receive all directive metadata so they can render accordingly.
/// `body_html` is the markdown-rendered body; `body_raw` is the unprocessed
/// source for templates that need to parse structured content (e.g., CSV).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DirectiveContext {
    pub name: String,
    pub args: String,
    pub id: Option<String>,
    pub classes: Vec<String>,
    pub body_html: String,
    pub body_raw: String,
}

/// Parsed Pandoc-style `{...}` attribute block.
///
/// Extracts `#id` (first wins), `.class` tokens, and `key=value` pairs.
#[derive(Debug, Default)]
pub(crate) struct PandocAttrs<'a> {
    pub id: Option<&'a str>,
    pub classes: Vec<&'a str>,
    pub kvs: Vec<(&'a str, Cow<'a, str>)>,
}

/// Parses a Pandoc-style attribute string into structured components.
///
/// Handles `#id`, `.class`, `key=value`, and `key="quoted value"` tokens.
/// The first `#id` wins; duplicates are silently ignored. Bare words
/// (tokens without `=`) are skipped.
///
/// Quoted values support `\"` and `\\` escape sequences. Unclosed quotes
/// consume the rest of the input as the value.
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
/// Returns `(end_offset, has_escapes)` where `end_offset` is the byte position
/// of the closing quote (or end of string if unclosed).
fn scan_quoted_value(s: &str) -> (usize, bool) {
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
fn unescape_quoted(s: &str) -> String {
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

/// A single `:::`-fenced directive block extracted from content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectiveBlock {
    pub kind: DirectiveKind,
    /// Pandoc `#id` attribute (first one wins if multiple specified).
    pub id: Option<String>,
    /// Extra CSS classes from Pandoc `.class` tokens (excluding the directive name).
    pub classes: Vec<String>,
    /// Body text between the opening and closing fences.
    ///
    /// For nested directives, the outer block's body contains the inner directive
    /// fences verbatim. Callers must process directives recursively (inner-first)
    /// when rendering.
    pub body: String,
    /// Byte range in the original content (opening fence through closing fence).
    pub range: Range<usize>,
}

#[cfg(test)]
mod tests {
    use strum::IntoEnumIterator;

    use super::*;

    // -- CalloutKind --

    #[test]
    fn all_variants_round_trip() {
        for kind in CalloutKind::iter() {
            let s: &str = kind.as_ref();

            // Round-trip through FromStr.
            assert_eq!(s.parse::<CalloutKind>().unwrap(), kind);

            // Case-insensitive.
            assert_eq!(s.to_uppercase().parse::<CalloutKind>().unwrap(), kind);

            // Display is titlecase of as_ref.
            let mut expected = String::new();
            let mut chars = s.chars();
            if let Some(c) = chars.next() {
                expected.push(c.to_ascii_uppercase());
                expected.push_str(chars.as_str());
            }
            assert_eq!(kind.to_string(), expected);
        }
    }

    #[test]
    fn from_str_unknown_returns_error() {
        assert!("foobar".parse::<CalloutKind>().is_err());
        assert!("".parse::<CalloutKind>().is_err());
    }

    // -- parse_pandoc_attrs --

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
        assert_eq!(
            kvs(r#"title="path\\to""#),
            vec![pair("title", r"path\to")]
        );
        // Unrecognized escape alone — no escapes detected, takes borrowed path.
        assert_eq!(
            kvs(r#"title="foo\nbar""#),
            vec![pair("title", r"foo\nbar")]
        );
        // Mixed recognized and unknown escapes — unknown sequences preserved as-is.
        assert_eq!(
            kvs(r#"title="a\"b\nc""#),
            vec![pair("title", r#"a"b\nc"#)]
        );
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
        let result = parse_pandoc_attrs(".highlight #my-id open=false");
        assert_eq!(result.id, Some("my-id"));
        assert_eq!(result.classes, vec!["highlight"]);
        assert_eq!(kvs(".highlight #my-id open=false"), vec![pair("open", "false")]);
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
