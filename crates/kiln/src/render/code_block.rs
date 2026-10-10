use std::ops::RangeInclusive;

use crate::attrs::parse_pandoc_attrs;

/// Attributes extracted from a fenced code block's info string.
#[derive(Debug, Default)]
pub(crate) struct CodeBlockSpec {
    pub lang: Option<String>,
    pub id: Option<String>,
    pub classes: Vec<String>,
    pub title: Option<String>,
    pub highlight: Vec<RangeInclusive<usize>>,
    pub disclosure: Disclosure,
}

/// Authored disclosure state of a code block.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Disclosure {
    /// Open, limited by the site's `code_max_lines`.
    #[default]
    Auto,
    /// Closed, without a line limit.
    Collapse,
    /// Open without a line limit.
    Expand,
}

impl CodeBlockSpec {
    pub(crate) fn is_mermaid(&self) -> bool {
        self.lang
            .as_deref()
            .is_some_and(|lang| lang.eq_ignore_ascii_case("mermaid"))
    }
}

/// Parses a fence info string of the form `lang {#id .class key=value ...}`.
///
/// The language ends at the first whitespace or `{`. Only the first `{...}` block after it is read.
#[must_use]
pub(crate) fn parse_fence_info(info: &str) -> CodeBlockSpec {
    let info = info.trim();
    let (lang, rest) = info.split_at(
        info.find(|c: char| c == '{' || c.is_whitespace())
            .unwrap_or(info.len()),
    );
    let attrs = parse_pandoc_attrs(rest.find('{').map_or("", |open| &rest[open + 1..]));

    let mut title = None;
    let mut highlight = Vec::new();
    for (key, value) in &attrs.named {
        match *key {
            "title" => title = Some(value.to_string()),
            "highlight" => highlight = parse_highlight_ranges(value),
            _ => {}
        }
    }

    let disclosure = if attrs.positional.iter().any(|word| word == "collapse") {
        Disclosure::Collapse
    } else if attrs.positional.iter().any(|word| word == "expand") {
        Disclosure::Expand
    } else {
        Disclosure::Auto
    };

    CodeBlockSpec {
        lang: (!lang.is_empty()).then(|| lang.to_owned()),
        id: attrs.id.map(str::to_owned),
        classes: attrs.classes.into_iter().map(str::to_owned).collect(),
        title,
        highlight,
        disclosure,
    }
}

/// Parses a comma-separated highlight range string into inclusive ranges.
///
/// Format: `"1,3-5,7"` → `[1..=1, 3..=5, 7..=7]`. Malformed entries are silently skipped.
fn parse_highlight_ranges(input: &str) -> Vec<RangeInclusive<usize>> {
    input
        .split(',')
        .filter_map(|entry| {
            let entry = entry.trim();
            if let Some((start, end)) = entry.split_once('-') {
                let start: usize = start.trim().parse().ok()?;
                let end: usize = end.trim().parse().ok()?;
                Some(start..=end)
            } else {
                let n: usize = entry.parse().ok()?;
                Some(n..=n)
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── is_mermaid ──

    #[test]
    fn is_mermaid_matches_lang_case_insensitively() {
        for (info, expected) in [
            ("mermaid", true),
            ("Mermaid {#flow}", true),
            ("mermaid-js", false),
            ("", false),
        ] {
            assert_eq!(parse_fence_info(info).is_mermaid(), expected, "{info}");
        }
    }

    // ── parse_fence_info ──

    #[test]
    fn parse_fence_info_lang_only() {
        let spec = parse_fence_info("rust");
        assert_eq!(spec.lang.as_deref(), Some("rust"));
        assert!(spec.id.is_none());
        assert_eq!(spec.classes, Vec::<String>::new());
        assert!(spec.title.is_none());
        assert_eq!(spec.highlight, Vec::<RangeInclusive<usize>>::new());
        assert_eq!(spec.disclosure, Disclosure::Auto);
    }

    #[test]
    fn parse_fence_info_all_attrs() {
        let spec = parse_fence_info(
            r#"rust {#my-id .special .wide title="main.rs" highlight="1,3-5" collapse}"#,
        );
        assert_eq!(spec.lang.as_deref(), Some("rust"));
        assert_eq!(spec.id.as_deref(), Some("my-id"));
        assert_eq!(spec.classes, ["special", "wide"]);
        assert_eq!(spec.title.as_deref(), Some("main.rs"));
        assert_eq!(spec.highlight, [1..=1, 3..=5]);
        assert_eq!(spec.disclosure, Disclosure::Collapse);
    }

    #[test]
    fn parse_fence_info_disclosure_flags() {
        for (info, expected) in [
            ("rust {expand}", Disclosure::Expand),
            ("rust {expand collapse}", Disclosure::Collapse),
            (r#"rust {title="please collapse this"}"#, Disclosure::Auto),
        ] {
            assert_eq!(parse_fence_info(info).disclosure, expected, "{info}");
        }
    }

    #[test]
    fn parse_fence_info_attrs_without_separated_lang() {
        for (info, lang) in [
            (r#"{title="T"}"#, None),
            (r#"rust{title="T"}"#, Some("rust")),
            (r#"  {.rust title="T"} "#, None),
        ] {
            let spec = parse_fence_info(info);
            assert_eq!(spec.lang.as_deref(), lang, "{info}");
            assert_eq!(spec.title.as_deref(), Some("T"), "{info}");
        }
    }

    #[test]
    fn parse_fence_info_quoted_brace_in_title() {
        let spec = parse_fence_info(r#"rust {title="a}b.rs" highlight="2"} ignored {.x}"#);
        assert_eq!(spec.title.as_deref(), Some("a}b.rs"));
        assert_eq!(spec.highlight, vec![2..=2]);
        assert_eq!(spec.classes, Vec::<String>::new());
    }

    #[test]
    fn parse_fence_info_ignores_unbraced_words_and_unknown_attrs() {
        let spec = parse_fence_info(r#"rust no_run {title="T" unknown="val" foo=bar}"#);
        assert_eq!(spec.lang.as_deref(), Some("rust"));
        assert_eq!(spec.title.as_deref(), Some("T"));
        assert_eq!(spec.highlight, Vec::<RangeInclusive<usize>>::new());
        assert_eq!(spec.disclosure, Disclosure::Auto);

        let spec = parse_fence_info("rust no_run playground");
        assert_eq!(spec.lang.as_deref(), Some("rust"));
        assert!(spec.title.is_none());
    }

    #[test]
    fn parse_fence_info_empty_returns_default() {
        let spec = parse_fence_info("  ");
        assert!(spec.lang.is_none());
        assert!(spec.title.is_none());
        assert_eq!(spec.disclosure, Disclosure::Auto);
    }

    // ── parse_highlight_ranges ──

    #[test]
    fn parse_highlight_ranges_valid_ranges() {
        for (input, expected) in [
            ("3", std::iter::once(3..=3).collect()),
            ("2-5", std::iter::once(2..=5).collect()),
            ("1,3-5,7", vec![1..=1, 3..=5, 7..=7]),
            (" 1 , 3 - 5 , 7 ", vec![1..=1, 3..=5, 7..=7]),
        ] {
            assert_eq!(parse_highlight_ranges(input), expected, "{input}");
        }
    }

    #[test]
    fn parse_highlight_ranges_malformed_skipped() {
        assert_eq!(
            parse_highlight_ranges("1,bad,x-2,2-x,3"),
            vec![1..=1, 3..=3]
        );
    }

    #[test]
    fn parse_highlight_ranges_empty() {
        assert_eq!(
            parse_highlight_ranges(""),
            Vec::<RangeInclusive<usize>>::new()
        );
    }
}
