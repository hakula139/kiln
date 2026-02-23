pub mod admonition;
pub mod parser;

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
pub(super) fn parse_attrs(input: &str) -> Vec<(&str, &str)> {
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
            let close = after_quote.find('"').unwrap_or(after_quote.len());
            pairs.push((key, &after_quote[..close]));
            rest = after_quote.get(close + 1..).unwrap_or("").trim_start();
        } else {
            let end = after_eq.find(char::is_whitespace).unwrap_or(after_eq.len());
            pairs.push((key, &after_eq[..end]));
            rest = after_eq[end..].trim_start();
        }
    }

    pairs
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
    fn from_str_unknown_returns_err() {
        assert!("foobar".parse::<AdmonitionKind>().is_err());
        assert!("".parse::<AdmonitionKind>().is_err());
    }

    // -- parse_attrs --

    #[test]
    fn parse_attrs_empty() {
        assert!(parse_attrs("").is_empty());
    }

    #[test]
    fn parse_attrs_unquoted_value() {
        assert_eq!(parse_attrs("key=value"), vec![("key", "value")]);
    }

    #[test]
    fn parse_attrs_quoted_value() {
        assert_eq!(
            parse_attrs(r#"key="hello world""#),
            vec![("key", "hello world")]
        );
    }

    #[test]
    fn parse_attrs_multiple_pairs() {
        assert_eq!(
            parse_attrs(r#"title="Title" open=false"#),
            vec![("title", "Title"), ("open", "false")]
        );
    }

    #[test]
    fn parse_attrs_skips_class_and_id() {
        assert_eq!(
            parse_attrs(".class #id open=false"),
            vec![("open", "false")]
        );
    }

    #[test]
    fn parse_attrs_skips_bare_words() {
        assert_eq!(
            parse_attrs(r#"bare title="Title""#),
            vec![("title", "Title")]
        );
    }
}
