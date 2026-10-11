use std::borrow::Cow;
use std::fmt::Write as _;

use serde::Serialize;

use crate::html::escape;

/// Parsed Pandoc-style `{...}` attribute block.
///
/// The first `#id` wins. Positional values are bare words and quoted strings in source order.
#[derive(Debug, Default)]
pub(crate) struct PandocAttrs<'a> {
    pub id: Option<&'a str>,
    pub classes: Vec<&'a str>,
    pub named: Vec<(&'a str, Cow<'a, str>)>,
    pub positional: Vec<Cow<'a, str>>,
}

impl PandocAttrs<'_> {
    /// Returns an owned copy of the `#id` and `.class` tokens.
    pub(crate) fn identity(&self) -> Identity {
        Identity {
            id: self.id.map(str::to_owned),
            classes: self.classes.iter().map(|&class| class.to_owned()).collect(),
        }
    }
}

/// Parses attribute tokens up to the first unquoted `}` or the end of `input`.
///
/// Quoted values support `\"` / `\\` escapes. An unclosed quote consumes the rest of the input.
#[must_use]
pub(crate) fn parse_pandoc_attrs(input: &str) -> PandocAttrs<'_> {
    let mut result = PandocAttrs::default();
    for token in AttrTokens::new(input) {
        match token {
            AttrToken::Id(id) => {
                result.id.get_or_insert(id);
            }
            AttrToken::Class(class) => result.classes.push(class),
            AttrToken::Arg(Arg::Named(key, value)) => result.named.push((key, value)),
            AttrToken::Arg(Arg::Positional(value)) => result.positional.push(value),
        }
    }

    result
}

/// Returns the byte offset of the first `}` outside quoted positional or named values.
pub(crate) fn find_attr_block_end(input: &str) -> Option<usize> {
    let mut tokens = AttrTokens::new(input);
    for _ in tokens.by_ref() {}
    let rest = tokens.0.rest.trim_start();
    rest.starts_with('}').then_some(input.len() - rest.len())
}

/// Authored `#id` and `.class` attributes of a rendered element.
#[derive(Debug, Default, PartialEq, Eq, Serialize)]
pub(crate) struct Identity {
    pub id: Option<String>,
    pub classes: Vec<String>,
}

impl Identity {
    /// Returns escaped ` id="..." class="..."` attributes, listing `base` classes before authored
    /// ones. Empty attributes are omitted.
    pub(crate) fn html_attrs(&self, base: &[&str]) -> String {
        let mut attrs = self
            .id
            .as_ref()
            .map(|id| format!(r#" id="{}""#, escape(id)))
            .unwrap_or_default();
        let classes = itertools::join(
            base.iter()
                .copied()
                .chain(self.classes.iter().map(String::as_str)),
            " ",
        );
        if !classes.is_empty() {
            _ = write!(attrs, r#" class="{}""#, escape(&classes));
        }
        attrs
    }
}

/// A named or positional argument value.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Arg<'a> {
    Named(&'a str, Cow<'a, str>),
    Positional(Cow<'a, str>),
}

/// Splits whitespace-separated arguments, where `#`, `.` and `}` have no special meaning.
pub(crate) struct ArgTokens<'a>(Scanner<'a>);

impl<'a> ArgTokens<'a> {
    pub(crate) fn new(input: &'a str) -> Self {
        Self(Scanner {
            rest: input,
            braced: false,
        })
    }
}

impl<'a> Iterator for ArgTokens<'a> {
    type Item = Arg<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.advance().then(|| self.0.arg())
    }
}

#[derive(Debug, PartialEq, Eq)]
enum AttrToken<'a> {
    Id(&'a str),
    Class(&'a str),
    Arg(Arg<'a>),
}

/// Splits Pandoc attribute content into tokens, stopping at the first unquoted `}`.
struct AttrTokens<'a>(Scanner<'a>);

impl<'a> AttrTokens<'a> {
    fn new(input: &'a str) -> Self {
        Self(Scanner {
            rest: input,
            braced: true,
        })
    }
}

impl<'a> Iterator for AttrTokens<'a> {
    type Item = AttrToken<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        while self.0.advance() {
            if !self.0.rest.starts_with(['#', '.']) {
                return Some(AttrToken::Arg(self.0.arg()));
            }
            match self.0.word().split_at(1) {
                (_, "") => {}
                ("#", id) => return Some(AttrToken::Id(id)),
                (_, class) => return Some(AttrToken::Class(class)),
            }
        }
        None
    }
}

struct Scanner<'a> {
    rest: &'a str,
    /// Whether an unquoted `}` ends the input, as in a Pandoc attribute block.
    braced: bool,
}

impl<'a> Scanner<'a> {
    /// Skips leading whitespace and returns whether another token follows.
    fn advance(&mut self) -> bool {
        self.rest = self.rest.trim_start();
        !(self.rest.is_empty() || (self.braced && self.rest.starts_with('}')))
    }

    fn arg(&mut self) -> Arg<'a> {
        if self.rest.starts_with('"') {
            return Arg::Positional(self.value());
        }
        if let Some(eq) = self.rest[..self.word_end()].find('=').filter(|&eq| eq > 0) {
            let key = &self.rest[..eq];
            self.rest = &self.rest[eq + 1..];
            return Arg::Named(key, self.value());
        }
        Arg::Positional(Cow::Borrowed(self.word()))
    }

    fn value(&mut self) -> Cow<'a, str> {
        let Some(after_quote) = self.rest.strip_prefix('"') else {
            return Cow::Borrowed(self.word());
        };
        let (end, escaped) = scan_quoted_value(after_quote);
        let value = &after_quote[..end];
        self.rest = after_quote.get(end + 1..).unwrap_or("");
        if escaped {
            Cow::Owned(unescape_quoted(value))
        } else {
            Cow::Borrowed(value)
        }
    }

    fn word(&mut self) -> &'a str {
        let (word, rest) = self.rest.split_at(self.word_end());
        self.rest = rest;
        word
    }

    fn word_end(&self) -> usize {
        self.rest
            .find(|c: char| c.is_whitespace() || (self.braced && c == '}'))
            .unwrap_or(self.rest.len())
    }
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

/// Quotes a value so that it parses back unchanged.
pub(crate) fn quote(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── PandocAttrs::identity ──

    #[test]
    fn identity_copies_id_and_classes() {
        assert_eq!(
            parse_pandoc_attrs("#main .a key=value .b").identity(),
            Identity {
                id: Some("main".into()),
                classes: vec!["a".into(), "b".into()],
            }
        );
    }

    // ── parse_pandoc_attrs ──

    #[test]
    fn parse_pandoc_attrs_unquoted_value() {
        assert_eq!(named("key=value"), vec![pair("key", "value")]);
    }

    #[test]
    fn parse_pandoc_attrs_quoted_value() {
        assert_eq!(
            named(r#"key="hello world""#),
            vec![pair("key", "hello world")]
        );
    }

    #[test]
    fn parse_pandoc_attrs_escaped_quotes() {
        assert_eq!(
            named(r#"title="He said \"hi\"""#),
            vec![pair("title", r#"He said "hi""#)]
        );
        assert_eq!(
            named(r#"title="path\\to""#),
            vec![pair("title", r"path\to")]
        );
        // Unrecognized escape alone: no escapes detected, takes borrowed path.
        assert_eq!(
            named(r#"title="foo\nbar""#),
            vec![pair("title", r"foo\nbar")]
        );
        assert_eq!(
            named(r#"title="a\"b\nc""#),
            vec![pair("title", r#"a"b\nc"#)]
        );
    }

    #[test]
    fn parse_pandoc_attrs_multiple_pairs_keep_source_order() {
        assert_eq!(
            named(r#"title="Title" open=false title=again"#),
            vec![
                pair("title", "Title"),
                pair("open", "false"),
                pair("title", "again")
            ]
        );
    }

    #[test]
    fn parse_pandoc_attrs_extracts_class_and_id() {
        let input = ".highlight #my-id open=false";
        let result = parse_pandoc_attrs(input);
        assert_eq!(result.id, Some("my-id"));
        assert_eq!(result.classes, vec!["highlight"]);
        assert_eq!(named(input), vec![pair("open", "false")]);
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
    fn parse_pandoc_attrs_collects_bare_and_quoted_positional_values() {
        let input = r##"collapse title="please collapse now" "#literal" expand =value"##;
        assert_eq!(
            parse_pandoc_attrs(input).positional,
            ["collapse", "#literal", "expand", "=value"]
        );
        assert_eq!(named(input), vec![pair("title", "please collapse now")]);
    }

    #[test]
    fn parse_pandoc_attrs_stops_at_unquoted_close_brace() {
        let result = parse_pandoc_attrs(r#"title="a}b" a}b"#);
        assert_eq!(result.named, [("title", Cow::Borrowed("a}b"))]);
        assert_eq!(result.positional, ["a"]);
    }

    #[test]
    fn parse_pandoc_attrs_splits_tokens_on_unicode_whitespace() {
        let result = parse_pandoc_attrs("title=\"a}b\"\u{3000}\"c d\"\u{3000}highlight=2");
        assert_eq!(
            result.named,
            [
                ("title", Cow::Borrowed("a}b")),
                ("highlight", Cow::Borrowed("2"))
            ]
        );
        assert_eq!(result.positional, ["c d"]);
    }

    #[test]
    fn parse_pandoc_attrs_keeps_quotes_inside_unquoted_tokens() {
        let result = parse_pandoc_attrs(r#"#a"b .c"d .last title=e"f"#);
        assert_eq!(result.id, Some(r#"a"b"#));
        assert_eq!(result.classes, [r#"c"d"#, "last"]);
        assert_eq!(result.named, [("title", Cow::Borrowed(r#"e"f"#))]);
        assert_eq!(result.positional, Vec::<Cow<'_, str>>::new());
    }

    #[test]
    fn parse_pandoc_attrs_empty() {
        let result = parse_pandoc_attrs("");
        assert!(result.id.is_none());
        assert_eq!(result.classes, Vec::<&str>::new());
        assert_eq!(result.named, Vec::<(&str, Cow<'_, str>)>::new());
        assert_eq!(result.positional, Vec::<Cow<'_, str>>::new());
    }

    #[test]
    fn parse_pandoc_attrs_empty_hash_and_dot_ignored() {
        let result = parse_pandoc_attrs("# . .real");
        assert_eq!(result.id, None);
        assert_eq!(result.classes, vec!["real"]);
        assert_eq!(result.positional, Vec::<Cow<'_, str>>::new());
    }

    #[test]
    fn parse_pandoc_attrs_unclosed_quote() {
        assert_eq!(
            named(r#"key="no closing quote"#),
            vec![pair("key", "no closing quote")]
        );
        assert_eq!(named(r#"key="a\"b\"#), vec![pair("key", r#"a"b\"#)]);
    }

    // ── find_attr_block_end ──

    #[test]
    fn find_attr_block_end_skips_quoted_braces() {
        let input = r#"#id .class title="a}b"} trailing"#;
        assert_eq!(find_attr_block_end(input), input.find("} trailing"));
    }

    #[test]
    fn find_attr_block_end_unclosed_returns_none() {
        assert_eq!(find_attr_block_end(r#"title="a}b"#), None);
        assert_eq!(find_attr_block_end("#id .class"), None);
    }

    // ── Identity::html_attrs ──

    #[test]
    fn html_attrs_writes_id_then_base_and_authored_classes() {
        let identity = Identity {
            id: Some("main".into()),
            classes: vec!["extra".into(), "wide".into()],
        };
        assert_eq!(
            identity.html_attrs(&["wrapper"]),
            r#" id="main" class="wrapper extra wide""#
        );
        assert_eq!(identity.html_attrs(&[]), r#" id="main" class="extra wide""#);
    }

    #[test]
    fn html_attrs_omits_empty_attributes() {
        assert_eq!(Identity::default().html_attrs(&[]), "");
        assert_eq!(
            Identity::default().html_attrs(&["wrapper"]),
            r#" class="wrapper""#
        );
    }

    #[test]
    fn html_attrs_escapes_values() {
        let identity = Identity {
            id: Some(r#"x"y"#.into()),
            classes: vec!["a<b".into()],
        };
        assert_eq!(
            identity.html_attrs(&["<script>"]),
            r#" id="x&quot;y" class="&lt;script&gt; a&lt;b""#
        );
    }

    // ── ArgTokens ──

    #[test]
    fn arg_tokens_treat_pandoc_syntax_as_values() {
        assert_eq!(
            ArgTokens::new(r#"# .a a}b title="A \"quote\"" "two words""#).collect::<Vec<_>>(),
            vec![
                Arg::Positional(Cow::Borrowed("#")),
                Arg::Positional(Cow::Borrowed(".a")),
                Arg::Positional(Cow::Borrowed("a}b")),
                Arg::Named("title", Cow::Borrowed(r#"A "quote""#)),
                Arg::Positional(Cow::Borrowed("two words")),
            ],
        );
    }

    // ── quote ──

    #[test]
    fn quote_round_trips_through_parser() {
        for value in ["plain", r#"He said "hi""#, r"path\to\", r#"a\"b"#, ""] {
            let quoted = quote(value);
            assert_eq!(
                ArgTokens::new(&quoted).collect::<Vec<_>>(),
                [Arg::Positional(Cow::Borrowed(value))],
                "quoted: {quoted}"
            );
        }
    }

    fn named(input: &str) -> Vec<(&str, String)> {
        parse_pandoc_attrs(input)
            .named
            .into_iter()
            .map(|(k, v)| (k, v.into_owned()))
            .collect()
    }

    fn pair<'a>(k: &'a str, v: &str) -> (&'a str, String) {
        (k, v.to_string())
    }
}
