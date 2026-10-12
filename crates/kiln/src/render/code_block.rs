use std::ops::RangeInclusive;

use crate::attrs::{Identity, parse_pandoc_attrs};

/// Attributes extracted from a fenced code block's info string.
#[derive(Debug, Default)]
pub(super) struct CodeBlockSpec {
    pub(super) lang: Option<String>,
    pub(super) identity: Identity,
    pub(super) title: Option<String>,
    pub(super) highlight: Vec<RangeInclusive<usize>>,
    pub(super) disclosure: Disclosure,
}

impl CodeBlockSpec {
    pub(super) fn is_mermaid(&self) -> bool {
        self.lang.as_deref() == Some("mermaid")
    }
}

/// Authored disclosure state of a code block.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) enum Disclosure {
    /// Open, limited by the site's `code_max_lines`.
    #[default]
    Auto,
    /// Closed, without a line limit.
    Collapse,
    /// Open without a line limit.
    Expand,
}

/// Parses a fence info string of the form `lang {#id .class key=value ...}`.
///
/// The language tag ends at the first whitespace or `{`. Without one, the first class names the
/// language, as in Pandoc. Only the first `{...}` block is read, and the language is lowercased.
pub(super) fn parse_fence_info(info: &str) -> CodeBlockSpec {
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

    let mut identity = attrs.identity();
    let lang = if lang.is_empty() {
        (!identity.classes.is_empty()).then(|| identity.classes.remove(0))
    } else {
        Some(lang.to_owned())
    };

    CodeBlockSpec {
        lang: lang.map(|mut lang| {
            lang.make_ascii_lowercase();
            lang
        }),
        identity,
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
    fn matches_lang_case_insensitively() {
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
    fn lang_only_has_default_attrs() {
        let spec = parse_fence_info("rust");
        assert_eq!(spec.lang.as_deref(), Some("rust"));
        assert!(spec.identity.id.is_none());
        assert_eq!(spec.identity.classes, Vec::<String>::new());
        assert!(spec.title.is_none());
        assert_eq!(spec.highlight, Vec::<RangeInclusive<usize>>::new());
        assert_eq!(spec.disclosure, Disclosure::Auto);
    }

    #[test]
    fn reads_all_attrs() {
        let spec = parse_fence_info(
            r#"rust {#my-id .special .wide title="main.rs" highlight="1,3-5" collapse}"#,
        );
        assert_eq!(spec.lang.as_deref(), Some("rust"));
        assert_eq!(spec.identity.id.as_deref(), Some("my-id"));
        assert_eq!(spec.identity.classes, ["special", "wide"]);
        assert_eq!(spec.title.as_deref(), Some("main.rs"));
        assert_eq!(spec.highlight, [1..=1, 3..=5]);
        assert_eq!(spec.disclosure, Disclosure::Collapse);
    }

    #[test]
    fn reads_disclosure_flags() {
        for (info, expected) in [
            ("rust {expand}", Disclosure::Expand),
            ("rust {expand collapse}", Disclosure::Collapse),
            (r#"rust {title="please collapse this"}"#, Disclosure::Auto),
        ] {
            assert_eq!(parse_fence_info(info).disclosure, expected, "{info}");
        }
    }

    #[test]
    fn first_class_names_missing_lang() {
        for (info, lang, classes) in [
            (r#"{title="T"}"#, None, vec![]),
            (r#"Rust{.wide title="T"}"#, Some("rust"), vec!["wide"]),
            (r#"  {.Rust .wide title="T"} "#, Some("rust"), vec!["wide"]),
        ] {
            let spec = parse_fence_info(info);
            assert_eq!(spec.lang.as_deref(), lang, "{info}");
            assert_eq!(spec.identity.classes, classes, "{info}");
            assert_eq!(spec.title.as_deref(), Some("T"), "{info}");
        }
    }

    #[test]
    fn keeps_quoted_brace_and_ignores_later_blocks() {
        let spec = parse_fence_info(r#"rust {title="a}b.rs" highlight="2"} ignored {.x}"#);
        assert_eq!(spec.title.as_deref(), Some("a}b.rs"));
        assert_eq!(spec.highlight, vec![2..=2]);
        assert_eq!(spec.identity.classes, Vec::<String>::new());
    }

    #[test]
    fn ignores_unbraced_words_and_unknown_attrs() {
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
    fn blank_info_has_no_lang() {
        let spec = parse_fence_info("  ");
        assert!(spec.lang.is_none());
        assert!(spec.title.is_none());
        assert_eq!(spec.disclosure, Disclosure::Auto);
    }

    // ── parse_highlight_ranges ──

    #[test]
    fn parses_single_lines_and_ranges() {
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
    fn skips_malformed_entries() {
        for (input, expected) in [("1,bad,x-2,2-x,3", vec![1..=1, 3..=3]), ("", vec![])] {
            assert_eq!(parse_highlight_ranges(input), expected, "{input}");
        }
    }
}
