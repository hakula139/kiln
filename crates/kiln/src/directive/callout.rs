use std::collections::BTreeMap;

use anyhow::{Context, Result};
use serde::Serialize;

/// Callout types. Themes style each type and title it with the `callout_<type>` translation.
const TYPES: [&str; 12] = [
    "abstract", "bug", "danger", "example", "failure", "info", "note", "question", "quote",
    "success", "tip", "warning",
];

/// Validated options of a `callout` directive, exposed to its template.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub(crate) struct Callout<'a> {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub title: Option<&'a str>,
    pub open: bool,
}

impl<'a> Callout<'a> {
    /// Reads `type` (default `note`), a non-empty `title` and `open` (default `true`).
    ///
    /// # Errors
    ///
    /// Returns an error for an unknown type or an `open` value other than `true` / `false`.
    pub(crate) fn from_args(named: &'a BTreeMap<String, String>) -> Result<Self> {
        let kind = match named.get("type") {
            Some(value) => find_type(value).with_context(|| {
                format!(
                    r#"unknown callout type "{value}", expected one of: {}"#,
                    TYPES.join(", ")
                )
            })?,
            None => "note",
        };
        let open = match named.get("open") {
            Some(value) => parse_open(value)
                .with_context(|| format!(r#"callout open must be true or false, got "{value}""#))?,
            None => true,
        };
        let title = named
            .get("title")
            .map(String::as_str)
            .filter(|title| !title.is_empty());
        Ok(Self { kind, title, open })
    }
}

/// Returns the callout type matching `name` case-insensitively.
pub(crate) fn find_type(name: &str) -> Option<&'static str> {
    TYPES
        .into_iter()
        .find(|kind| kind.eq_ignore_ascii_case(name))
}

/// Parses an `open` value of `true` or `false` case-insensitively.
pub(crate) fn parse_open(value: &str) -> Option<bool> {
    value.to_ascii_lowercase().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::named_args;

    // ── Callout::from_args ──

    #[test]
    fn defaults_to_open_untitled_note() {
        for named in [
            BTreeMap::new(),
            named_args(&[("title", ""), ("open", "true")]),
        ] {
            assert_eq!(
                Callout::from_args(&named).unwrap(),
                Callout {
                    kind: "note",
                    title: None,
                    open: true,
                },
                "{named:?}"
            );
        }
    }

    #[test]
    fn reads_type_title_and_open() {
        let named = named_args(&[
            ("open", "FALSE"),
            ("title", "Careful"),
            ("type", "Warning"),
            ("unknown", "x"),
        ]);
        assert_eq!(
            Callout::from_args(&named).unwrap(),
            Callout {
                kind: "warning",
                title: Some("Careful"),
                open: false,
            }
        );
    }

    #[test]
    fn rejects_unknown_type_and_open_value() {
        for (named, message) in [
            (
                named_args(&[("type", "warn")]),
                r#"unknown callout type "warn", expected one of: abstract, bug,"#,
            ),
            (
                named_args(&[("open", "no")]),
                r#"callout open must be true or false, got "no""#,
            ),
        ] {
            let error = Callout::from_args(&named).unwrap_err().to_string();
            assert!(error.starts_with(message), "{error}");
        }
    }

    // ── find_type ──

    #[test]
    fn returns_canonical_name() {
        assert_eq!(find_type("quote"), Some("quote"));
        assert_eq!(find_type("TIP"), Some("tip"));
        assert_eq!(find_type("tips"), None);
        assert_eq!(find_type(""), None);
    }

    // ── parse_open ──

    #[test]
    fn accepts_booleans_case_insensitively() {
        assert_eq!(parse_open("True"), Some(true));
        assert_eq!(parse_open("FALSE"), Some(false));
        assert_eq!(parse_open("yes"), None);
        assert_eq!(parse_open(""), None);
    }
}
