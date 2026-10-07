use std::borrow::Cow;

/// Parsed Pandoc-style `{...}` attribute block.
///
/// Extracts `#id` (first wins), `.class` tokens, `key=value` pairs, and bare words.
#[derive(Debug, Default)]
pub(crate) struct PandocAttrs<'a> {
    pub id: Option<&'a str>,
    pub classes: Vec<&'a str>,
    pub kvs: Vec<(&'a str, Cow<'a, str>)>,
    pub bare: Vec<&'a str>,
}

/// Parses a Pandoc-style attribute string (`#id`, `.class`, `key=value`, bare words).
///
/// First `#id` wins. Quoted values support `\"` / `\\` escapes. Unclosed quotes consume the
/// rest of the input. Bare words (no `=`, no `#` / `.` prefix) are surfaced via `bare`.
#[must_use]
pub(crate) fn parse_pandoc_attrs(input: &str) -> PandocAttrs<'_> {
    let mut result = PandocAttrs::default();
    for token in AttrTokens::new(input) {
        match token {
            AttrToken::Id(id) if result.id.is_none() => result.id = Some(id),
            AttrToken::Class(class) => result.classes.push(class),
            AttrToken::Named(key, value) => result.kvs.push((key, value)),
            AttrToken::Bare(value) => result.bare.push(value),
            AttrToken::Id(_) | AttrToken::Quoted(_) => {}
        }
    }

    result
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum AttrToken<'a> {
    Id(&'a str),
    Class(&'a str),
    Named(&'a str, Cow<'a, str>),
    Bare(&'a str),
    Quoted(Cow<'a, str>),
}

pub(crate) struct AttrTokens<'a> {
    rest: &'a str,
    pandoc: bool,
}

impl<'a> AttrTokens<'a> {
    pub(crate) fn new(input: &'a str) -> Self {
        Self {
            rest: input,
            pandoc: true,
        }
    }

    pub(crate) fn values(input: &'a str) -> Self {
        Self {
            rest: input,
            pandoc: false,
        }
    }

    fn value(&mut self) -> Cow<'a, str> {
        if let Some(after_quote) = self.rest.strip_prefix('"') {
            let (end, escaped) = scan_quoted_value(after_quote);
            let value = &after_quote[..end];
            self.rest = after_quote.get(end + 1..).unwrap_or("");
            if escaped {
                Cow::Owned(unescape_quoted(value))
            } else {
                Cow::Borrowed(value)
            }
        } else {
            Cow::Borrowed(self.word())
        }
    }

    fn word(&mut self) -> &'a str {
        let end = self
            .rest
            .find(|c: char| c.is_whitespace() || (self.pandoc && c == '}'))
            .unwrap_or(self.rest.len());
        let word = &self.rest[..end];
        self.rest = &self.rest[end..];
        word
    }
}

impl<'a> Iterator for AttrTokens<'a> {
    type Item = AttrToken<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            self.rest = self.rest.trim_start();
            if self.rest.is_empty() || (self.pandoc && self.rest.starts_with('}')) {
                return None;
            }
            if self.rest.starts_with('"') {
                return Some(AttrToken::Quoted(self.value()));
            }
            if self.pandoc && self.rest.starts_with(['#', '.']) {
                let word = self.word();
                if word.len() == 1 {
                    continue;
                }
                return Some(if let Some(id) = word.strip_prefix('#') {
                    AttrToken::Id(id)
                } else {
                    AttrToken::Class(&word[1..])
                });
            }
            let end = self
                .rest
                .find(|c: char| c.is_whitespace() || (self.pandoc && c == '}'))
                .unwrap_or(self.rest.len());
            if let Some(eq) = self.rest.find('=').filter(|&eq| eq > 0 && eq < end) {
                let key = &self.rest[..eq];
                self.rest = &self.rest[eq + 1..];
                return Some(AttrToken::Named(key, self.value()));
            }
            return Some(AttrToken::Bare(self.word()));
        }
    }
}

/// Returns the byte offset of the first `}` outside quoted positional or named values.
pub(crate) fn find_attr_block_end(input: &str) -> Option<usize> {
    let mut tokens = AttrTokens::new(input);
    for _ in tokens.by_ref() {}
    let rest = tokens.rest.trim_start();
    rest.starts_with('}').then_some(input.len() - rest.len())
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
        assert_eq!(kvs(r#"title="path\\to""#), vec![pair("title", r"path\to")]);
        // Unrecognized escape alone: no escapes detected, takes borrowed path.
        assert_eq!(kvs(r#"title="foo\nbar""#), vec![pair("title", r"foo\nbar")]);
        assert_eq!(kvs(r#"title="a\"b\nc""#), vec![pair("title", r#"a"b\nc"#)]);
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
    fn parse_pandoc_attrs_collects_bare_words() {
        let result = parse_pandoc_attrs(r#"collapse title="Title" expand"#);
        assert_eq!(result.bare, vec!["collapse", "expand"]);
        assert_eq!(
            kvs(r#"collapse title="Title" expand"#),
            vec![pair("title", "Title")]
        );
    }

    #[test]
    fn parse_pandoc_attrs_bare_words_ignore_quoted_content() {
        // Words inside quoted values must not leak into `bare`.
        let result = parse_pandoc_attrs(r#"title="please collapse now""#);
        assert!(result.bare.is_empty(), "got: {:?}", result.bare);
    }

    #[test]
    fn parse_pandoc_attrs_empty() {
        let result = parse_pandoc_attrs("");
        assert!(result.id.is_none());
        assert_eq!(result.classes, Vec::<&str>::new());
        assert_eq!(result.kvs, Vec::<(&str, Cow<'_, str>)>::new());
    }

    #[test]
    fn parse_pandoc_attrs_empty_hash_and_dot_ignored() {
        let result = parse_pandoc_attrs("# . .real");
        assert_eq!(result.id, None);
        assert_eq!(result.classes, vec!["real"]);
    }

    #[test]
    fn parse_pandoc_attrs_unclosed_quote() {
        assert_eq!(
            kvs(r#"key="no closing quote"#),
            vec![pair("key", "no closing quote")]
        );
        assert_eq!(kvs(r#"key="a\"b\"#), vec![pair("key", r#"a"b\"#)]);
    }

    // ── AttrTokens ──

    #[test]
    fn attr_tokens_share_quoted_values_and_preserve_consumer_syntax() {
        assert_eq!(
            AttrTokens::values(r#"# . a}b title="A \"quote\"" "two words""#).collect::<Vec<_>>(),
            vec![
                AttrToken::Bare("#"),
                AttrToken::Bare("."),
                AttrToken::Bare("a}b"),
                AttrToken::Named("title", Cow::Borrowed(r#"A "quote""#)),
                AttrToken::Quoted(Cow::Borrowed("two words")),
            ],
        );
        let input = r#"#id .class =value title="a}b"} trailing"#;
        assert_eq!(find_attr_block_end(input), input.find("} trailing"));
        assert_eq!(
            AttrTokens::new(input).collect::<Vec<_>>(),
            vec![
                AttrToken::Id("id"),
                AttrToken::Class("class"),
                AttrToken::Bare("=value"),
                AttrToken::Named("title", Cow::Borrowed("a}b")),
            ],
        );
    }

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
}
