pub mod admonition;
pub mod parser;

use std::borrow::Cow;
use std::fmt;
use std::ops::Range;

use strum::{AsRefStr, EnumIter, EnumString};

/// Known admonition types.
///
/// - `AsRefStr` yields the lowercase identifier (e.g., `"note"`).
/// - `EnumString` provides case-insensitive [`FromStr`](std::str::FromStr).
/// - `Display` yields the titlecase form (e.g., `"Note"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, AsRefStr, EnumString, EnumIter)]
#[strum(serialize_all = "lowercase", ascii_case_insensitive)]
pub enum AdmonitionKind {
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

impl fmt::Display for AdmonitionKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut chars = self.as_ref().chars();
        if let Some(c) = chars.next() {
            write!(f, "{}{}", c.to_ascii_uppercase(), chars.as_str())
        } else {
            Ok(())
        }
    }
}

/// Parsed directive type — either a known admonition or an unrecognized name
/// preserved for future extension.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DirectiveKind {
    Admonition {
        kind: AdmonitionKind,
        title: Option<String>,
        open: bool,
    },
    /// Unrecognized type — callers pass through body as-is.
    Unknown { name: String, args: String },
}

impl DirectiveKind {
    /// Parses a directive name and raw arguments into the appropriate variant.
    /// Each variant owns its argument grammar.
    fn from_name(name: &str, args: &str) -> Self {
        match name.parse::<AdmonitionKind>() {
            Ok(kind) => {
                let (title, open) = admonition::parse_args(args);
                Self::Admonition { kind, title, open }
            }
            Err(_) => Self::Unknown {
                name: name.to_string(),
                args: args.to_string(),
            },
        }
    }
}

/// Parses Pandoc-style key-value attributes.
///
/// Handles `key=value` and `key="quoted value"` pairs.
/// Skips `.class` and `#id` tokens.
///
/// Quoted values support `\"` and `\\` escape sequences. Unclosed quotes
/// consume the rest of the input as the value.
#[must_use]
fn parse_attrs(input: &str) -> Vec<(&str, Cow<'_, str>)> {
    let mut pairs = Vec::new();
    let mut rest = input.trim();

    while !rest.is_empty() {
        // Skip Pandoc .class and #id markers.
        if rest.starts_with('.') || rest.starts_with('#') {
            let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            rest = rest[end..].trim_start();
            continue;
        }

        let next_eq = rest.find('=');
        let next_ws = rest.find(char::is_whitespace).unwrap_or(rest.len());

        let Some(eq) = next_eq.filter(|&p| p < next_ws) else {
            // Bare word (no = before next whitespace) — skip.
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
            pairs.push((key, value));
            rest = after_quote.get(end + 1..).unwrap_or("").trim_start();
        } else {
            let end = after_eq.find(char::is_whitespace).unwrap_or(after_eq.len());
            pairs.push((key, Cow::Borrowed(&after_eq[..end])));
            rest = after_eq[end..].trim_start();
        }
    }

    pairs
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

    // -- AdmonitionKind --

    #[test]
    fn all_variants_round_trip() {
        for kind in AdmonitionKind::iter() {
            let s: &str = kind.as_ref();

            // Round-trip through FromStr.
            assert_eq!(s.parse::<AdmonitionKind>().unwrap(), kind);

            // Case-insensitive.
            assert_eq!(s.to_uppercase().parse::<AdmonitionKind>().unwrap(), kind);

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
        assert!("foobar".parse::<AdmonitionKind>().is_err());
        assert!("".parse::<AdmonitionKind>().is_err());
    }

    // -- parse_attrs --

    /// Helper to compare `parse_attrs` output without `Cow` noise.
    fn attrs(input: &str) -> Vec<(&str, String)> {
        parse_attrs(input)
            .into_iter()
            .map(|(k, v)| (k, v.into_owned()))
            .collect()
    }

    fn pair<'a>(k: &'a str, v: &str) -> (&'a str, String) {
        (k, v.to_string())
    }

    #[test]
    fn parse_attrs_empty() {
        assert!(parse_attrs("").is_empty());
    }

    #[test]
    fn parse_attrs_unquoted_value() {
        assert_eq!(attrs("key=value"), vec![pair("key", "value")]);
    }

    #[test]
    fn parse_attrs_quoted_value() {
        assert_eq!(
            attrs(r#"key="hello world""#),
            vec![pair("key", "hello world")]
        );
    }

    #[test]
    fn parse_attrs_escaped_quotes() {
        assert_eq!(
            attrs(r#"title="He said \"hi\"""#),
            vec![pair("title", r#"He said "hi""#)]
        );
        // Escaped backslash.
        assert_eq!(
            attrs(r#"title="path\\to""#),
            vec![pair("title", r"path\to")]
        );
        // Unrecognized escape alone — no escapes detected, takes borrowed path.
        assert_eq!(
            attrs(r#"title="foo\nbar""#),
            vec![pair("title", r"foo\nbar")]
        );
        // Mixed recognized and unknown escapes — unknown sequences preserved as-is.
        assert_eq!(
            attrs(r#"title="a\"b\nc""#),
            vec![pair("title", r#"a"b\nc"#)]
        );
    }

    #[test]
    fn parse_attrs_unclosed_quote() {
        assert_eq!(
            attrs(r#"key="no closing quote"#),
            vec![pair("key", "no closing quote")]
        );
        // Trailing backslash in unclosed quote.
        assert_eq!(attrs(r#"key="a\"b\"#), vec![pair("key", r#"a"b\"#)]);
    }

    #[test]
    fn parse_attrs_multiple_pairs() {
        assert_eq!(
            attrs(r#"title="Title" open=false"#),
            vec![pair("title", "Title"), pair("open", "false")]
        );
    }

    #[test]
    fn parse_attrs_skips_class_and_id() {
        assert_eq!(attrs(".class #id open=false"), vec![pair("open", "false")]);
    }

    #[test]
    fn parse_attrs_skips_bare_words() {
        assert_eq!(attrs(r#"bare title="Title""#), vec![pair("title", "Title")]);
    }
}
