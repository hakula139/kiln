pub mod callout;
pub mod div;
pub mod parser;

use std::collections::BTreeMap;
use std::fmt;
use std::ops::Range;

use serde::Serialize;
use strum::{AsRefStr, EnumIter, EnumString};

use crate::attrs::{scan_quoted_value, unescape_quoted};

/// Known callout types.
///
/// `AsRefStr` → lowercase, `EnumString` → case-insensitive `FromStr`, `Display` → titlecase.
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

/// Parsed directive type — either a callout or an unrecognized name preserved for extension.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DirectiveKind {
    Callout {
        kind: CalloutKind,
        title: Option<String>,
        open: bool,
    },
    /// Unrecognized type — rendered as a `<div>` or passed through as-is.
    Unknown {
        name: String,
        positional_args: Vec<String>,
        named_args: BTreeMap<String, String>,
    },
}

impl DirectiveKind {
    /// Parses a directive name and structured arguments into the appropriate variant.
    pub(crate) fn from_parsed(
        name: &str,
        positional_args: Vec<String>,
        named_args: BTreeMap<String, String>,
    ) -> Self {
        if name.eq_ignore_ascii_case("callout") {
            let (kind, title, open) = callout::parse_named_args(&named_args);
            return Self::Callout { kind, title, open };
        }
        Self::Unknown {
            name: name.to_string(),
            positional_args,
            named_args,
        }
    }
}

/// Serializable context passed to directive templates.
///
/// `body_html` is pre-rendered markdown; `body_raw` is the unprocessed source for templates
/// that parse structured content (e.g., CSV).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DirectiveContext {
    pub name: String,
    pub positional_args: Vec<String>,
    pub named_args: BTreeMap<String, String>,
    pub id: Option<String>,
    pub classes: Vec<String>,
    pub body_html: String,
    pub body_raw: String,
    pub source_dir: Option<String>,
}

/// Parsed directive arguments from a `{...}` attribute block.
#[derive(Debug)]
pub(crate) struct DirectiveArgs {
    pub positional: Vec<String>,
    pub named: BTreeMap<String, String>,
    pub id: Option<String>,
    pub classes: Vec<String>,
}

/// Parses a directive `{...}` block into positional args, named args, `#id`, and `.class` tokens.
/// Named args use `BTreeMap` (last-wins).
#[must_use]
pub(crate) fn parse_directive_args(input: &str) -> DirectiveArgs {
    let mut result = DirectiveArgs {
        positional: Vec::new(),
        named: BTreeMap::new(),
        id: None,
        classes: Vec::new(),
    };
    let mut rest = input.trim();

    while !rest.is_empty() {
        // #id (bare token only).
        if let Some(after) = rest.strip_prefix('#') {
            let end = after.find(char::is_whitespace).unwrap_or(after.len());
            if result.id.is_none() && end > 0 {
                result.id = Some(after[..end].to_string());
            }
            rest = after[end..].trim_start();
            continue;
        }

        // .class (bare token only).
        if let Some(after) = rest.strip_prefix('.') {
            let end = after.find(char::is_whitespace).unwrap_or(after.len());
            if end > 0 {
                result.classes.push(after[..end].to_string());
            }
            rest = after[end..].trim_start();
            continue;
        }

        // Quoted string → positional arg.
        if let Some(after_quote) = rest.strip_prefix('"') {
            let (end, has_escapes) = scan_quoted_value(after_quote);
            let raw = &after_quote[..end];
            let value = if has_escapes {
                unescape_quoted(raw)
            } else {
                raw.to_string()
            };
            result.positional.push(value);
            rest = after_quote.get(end + 1..).unwrap_or("").trim_start();
            continue;
        }

        let next_ws = rest.find(char::is_whitespace).unwrap_or(rest.len());
        let next_eq = rest.find('=');

        // Named arg: key=value or key="quoted value".
        // Require a non-empty key; treat `=value` as a bare word.
        if let Some(eq) = next_eq.filter(|&p| p > 0 && p < next_ws) {
            let key = &rest[..eq];
            let after_eq = &rest[eq + 1..];

            if let Some(after_q) = after_eq.strip_prefix('"') {
                let (end, has_escapes) = scan_quoted_value(after_q);
                let raw = &after_q[..end];
                let value = if has_escapes {
                    unescape_quoted(raw)
                } else {
                    raw.to_string()
                };
                result.named.insert(key.to_string(), value);
                rest = after_q.get(end + 1..).unwrap_or("").trim_start();
            } else {
                let end = after_eq.find(char::is_whitespace).unwrap_or(after_eq.len());
                result
                    .named
                    .insert(key.to_string(), after_eq[..end].to_string());
                rest = after_eq[end..].trim_start();
            }
            continue;
        }

        // Bare word → positional arg.
        result.positional.push(rest[..next_ws].to_string());
        rest = rest[next_ws..].trim_start();
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
    /// Body text between the opening and closing fences. For nested directives, the outer block's
    /// body contains inner fences verbatim; callers process recursively (inner-first).
    pub body: String,
    /// Byte range in the original content (opening fence through closing fence).
    pub range: Range<usize>,
}

#[cfg(test)]
mod tests {
    use strum::IntoEnumIterator;

    use super::*;

    // ── CalloutKind ──

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

    // ── parse_directive_args ──

    #[test]
    fn parse_directive_args_empty() {
        let args = parse_directive_args("");
        assert!(args.positional.is_empty());
        assert!(args.named.is_empty());
        assert!(args.id.is_none());
        assert!(args.classes.is_empty());
    }

    #[test]
    fn parse_directive_args_positional_quoted() {
        let args = parse_directive_args(r#""title" "url""#);
        assert_eq!(args.positional, vec!["title", "url"]);
        assert!(args.named.is_empty());
    }

    #[test]
    fn parse_directive_args_named_only() {
        let args = parse_directive_args(r#"server="netease" type="song""#);
        assert!(args.positional.is_empty());
        assert_eq!(args.named["server"], "netease");
        assert_eq!(args.named["type"], "song");
    }

    #[test]
    fn parse_directive_args_mixed() {
        let args = parse_directive_args(r#""scores.csv" format="table""#);
        assert_eq!(args.positional, vec!["scores.csv"]);
        assert_eq!(args.named["format"], "table");
    }

    #[test]
    fn parse_directive_args_bare_word() {
        let args = parse_directive_args("bare word");
        assert_eq!(args.positional, vec!["bare", "word"]);
        assert!(args.named.is_empty());
    }

    #[test]
    fn parse_directive_args_escaped_quotes() {
        let args = parse_directive_args(r#""He said \"hi\"""#);
        assert_eq!(args.positional, vec![r#"He said "hi""#]);
    }

    #[test]
    fn parse_directive_args_unclosed_quote() {
        let args = parse_directive_args(r#""no closing quote"#);
        assert_eq!(args.positional, vec!["no closing quote"]);
    }

    #[test]
    fn parse_directive_args_named_unquoted_value() {
        let args = parse_directive_args("key=value");
        assert!(args.positional.is_empty());
        assert_eq!(args.named["key"], "value");
    }

    #[test]
    fn parse_directive_args_duplicate_named_last_wins() {
        let args = parse_directive_args(r#"key="first" key="second""#);
        assert_eq!(args.named["key"], "second");
    }

    #[test]
    fn parse_directive_args_named_escaped_quotes() {
        let args = parse_directive_args(r#"key="a\"b""#);
        assert_eq!(args.named["key"], r#"a"b"#);
    }

    #[test]
    fn parse_directive_args_leading_equals_treated_as_bare_word() {
        let args = parse_directive_args("=value");
        assert_eq!(args.positional, vec!["=value"]);
        assert!(args.named.is_empty());
    }

    #[test]
    fn parse_directive_args_mixed_bare_and_named() {
        let args = parse_directive_args(r#"bare key="val" another"#);
        assert_eq!(args.positional, vec!["bare", "another"]);
        assert_eq!(args.named["key"], "val");
    }

    #[test]
    fn parse_directive_args_pandoc_id_and_classes() {
        let args = parse_directive_args("#my-id .highlight .wide type=tip");
        assert_eq!(args.id.as_deref(), Some("my-id"));
        assert_eq!(args.classes, vec!["highlight", "wide"]);
        assert!(args.positional.is_empty());
        assert_eq!(args.named["type"], "tip");
    }

    #[test]
    fn parse_directive_args_quoted_hash_stays_positional() {
        let args = parse_directive_args(r##""#literal" ".keep""##);
        assert_eq!(args.positional, vec!["#literal", ".keep"]);
        assert!(args.id.is_none());
        assert!(args.classes.is_empty());
    }
}
