use std::ops::RangeInclusive;

use crate::attrs::parse_pandoc_attrs;

/// Attributes extracted from a fenced code block's Pandoc-style `{...}` block.
#[derive(Debug, Clone, Default)]
pub(crate) struct CodeBlockSpec {
    pub lang: Option<String>,
    pub id: Option<String>,
    pub classes: Vec<String>,
    pub title: Option<String>,
    pub highlight: Vec<RangeInclusive<usize>>,
    pub collapse: Option<bool>,
    pub max_lines: Option<usize>,
}

/// Parses a fence info-string into a `CodeBlockSpec`.
///
/// The info-string format is `lang {#id .class key=value ...}`. The language is the first
/// whitespace-delimited word. The `{...}` payload (if present) is parsed as Pandoc attrs.
/// Content outside `{...}` (after the language) is ignored.
#[must_use]
pub(crate) fn parse_fence_info(info: &str, default_max_lines: Option<usize>) -> CodeBlockSpec {
    let trimmed = info.trim();
    if trimmed.is_empty() {
        return CodeBlockSpec {
            max_lines: default_max_lines,
            ..CodeBlockSpec::default()
        };
    }

    let lang = trimmed
        .split_ascii_whitespace()
        .next()
        .filter(|s| !s.is_empty())
        .map(String::from);

    let after_lang = lang
        .as_ref()
        .map_or(trimmed, |l| trimmed[l.len()..].trim_start());

    let payload = extract_brace_payload(after_lang);
    parse_code_block_attrs(lang, payload, default_max_lines)
}

/// Extracts the content between the first `{` and its matching `}`.
fn extract_brace_payload(s: &str) -> &str {
    let Some(open) = s.find('{') else { return "" };
    let inner = &s[open + 1..];
    let close = inner.find('}').unwrap_or(inner.len());
    &inner[..close]
}

/// Parses a pandoc-attrs payload into a `CodeBlockSpec`.
fn parse_code_block_attrs(
    lang: Option<String>,
    payload: &str,
    default_max_lines: Option<usize>,
) -> CodeBlockSpec {
    if payload.is_empty() {
        return CodeBlockSpec {
            lang,
            max_lines: default_max_lines,
            ..CodeBlockSpec::default()
        };
    }

    let pandoc = parse_pandoc_attrs(payload);

    let mut title = None;
    let mut highlight = Vec::new();
    let mut collapse = None;

    for (key, value) in &pandoc.kvs {
        match *key {
            "title" => title = Some(value.to_string()),
            "highlight" => highlight = parse_highlight_ranges(value),
            _ => {}
        }
    }

    // Collapse / expand are signaled via bare words (skipped by `parse_pandoc_attrs`),
    // so we check for them directly in the payload.
    if has_bare_flag(payload, "collapse") {
        collapse = Some(true);
    } else if has_bare_flag(payload, "expand") {
        collapse = Some(false);
    }

    let max_lines = match collapse {
        Some(false) => None,
        _ => default_max_lines,
    };

    CodeBlockSpec {
        lang,
        id: pandoc.id.map(str::to_string),
        classes: pandoc.classes.into_iter().map(str::to_string).collect(),
        title,
        highlight,
        collapse,
        max_lines,
    }
}

/// Checks whether a bare (unquoted, no `=`) flag word appears in the payload.
fn has_bare_flag(payload: &str, flag: &str) -> bool {
    payload.split_ascii_whitespace().any(|word| {
        word == flag && !word.contains('=') && !word.starts_with('#') && !word.starts_with('.')
    })
}

/// Parses a comma-separated highlight range string into inclusive ranges.
///
/// Format: `"1,3-5,7"` → `[1..=1, 3..=5, 7..=7]`. Malformed entries are silently skipped.
pub(crate) fn parse_highlight_ranges(input: &str) -> Vec<RangeInclusive<usize>> {
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

    // ── parse_fence_info ──

    #[test]
    fn parse_fence_info_empty() {
        let spec = parse_fence_info("", Some(40));
        assert!(spec.lang.is_none());
        assert!(spec.title.is_none());
        assert_eq!(spec.max_lines, Some(40));
    }

    #[test]
    fn parse_fence_info_lang_only() {
        let spec = parse_fence_info("rust", Some(40));
        assert_eq!(spec.lang.as_deref(), Some("rust"));
        assert!(spec.title.is_none());
        assert!(spec.highlight.is_empty());
        assert!(spec.collapse.is_none());
        assert_eq!(spec.max_lines, Some(40));
    }

    #[test]
    fn parse_fence_info_lang_with_title() {
        let spec = parse_fence_info(r#"rust {title="src/main.rs"}"#, Some(40));
        assert_eq!(spec.lang.as_deref(), Some("rust"));
        assert_eq!(spec.title.as_deref(), Some("src/main.rs"));
    }

    #[test]
    fn parse_fence_info_all_attrs() {
        let spec = parse_fence_info(
            r#"rust {#my-id .special title="main.rs" highlight="1,3-5" collapse}"#,
            Some(40),
        );
        assert_eq!(spec.lang.as_deref(), Some("rust"));
        assert_eq!(spec.id.as_deref(), Some("my-id"));
        assert_eq!(spec.classes, vec!["special"]);
        assert_eq!(spec.title.as_deref(), Some("main.rs"));
        assert_eq!(spec.highlight, vec![1..=1, 3..=5]);
        assert_eq!(spec.collapse, Some(true));
        assert_eq!(spec.max_lines, Some(40));
    }

    #[test]
    fn parse_fence_info_expand_clears_max_lines() {
        let spec = parse_fence_info("rust {expand}", Some(40));
        assert_eq!(spec.collapse, Some(false));
        assert_eq!(spec.max_lines, None);
    }

    #[test]
    fn parse_fence_info_trailing_content_after_braces_ignored() {
        let spec = parse_fence_info(r#"rust {title="T"} ignored"#, Some(40));
        assert_eq!(spec.title.as_deref(), Some("T"));
    }

    #[test]
    fn parse_fence_info_no_braces_discards_trailing() {
        let spec = parse_fence_info("rust no_run playground", Some(40));
        assert_eq!(spec.lang.as_deref(), Some("rust"));
        assert!(spec.title.is_none());
    }

    #[test]
    fn parse_fence_info_id_and_class_propagate() {
        let spec = parse_fence_info("js {#snippet .wide .dark}", None);
        assert_eq!(spec.id.as_deref(), Some("snippet"));
        assert_eq!(spec.classes, vec!["wide", "dark"]);
    }

    // ── parse_highlight_ranges ──

    #[test]
    fn parse_highlight_ranges_single_line() {
        assert_eq!(parse_highlight_ranges("3"), vec![3..=3]);
    }

    #[test]
    fn parse_highlight_ranges_range() {
        assert_eq!(parse_highlight_ranges("2-5"), vec![2..=5]);
    }

    #[test]
    fn parse_highlight_ranges_mixed() {
        assert_eq!(parse_highlight_ranges("1,3-5,7"), vec![1..=1, 3..=5, 7..=7]);
    }

    #[test]
    fn parse_highlight_ranges_with_spaces() {
        assert_eq!(
            parse_highlight_ranges(" 1 , 3 - 5 , 7 "),
            vec![1..=1, 3..=5, 7..=7]
        );
    }

    #[test]
    fn parse_highlight_ranges_malformed_skipped() {
        assert_eq!(parse_highlight_ranges("1,bad,3"), vec![1..=1, 3..=3]);
    }

    #[test]
    fn parse_highlight_ranges_empty() {
        assert!(parse_highlight_ranges("").is_empty());
    }

    // ── has_bare_flag ──

    #[test]
    fn has_bare_flag_detects_collapse() {
        assert!(has_bare_flag(r#"title="T" collapse"#, "collapse"));
    }

    #[test]
    fn has_bare_flag_ignores_within_value() {
        assert!(!has_bare_flag(r#"title="collapse""#, "collapse"));
    }

    #[test]
    fn has_bare_flag_ignores_key_value_pair() {
        assert!(!has_bare_flag("collapse=true", "collapse"));
    }
}
