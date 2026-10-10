use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};
use serde::Serialize;

/// Callout types. Themes style each type and title it with the `callout_<type>` translation.
pub(crate) const TYPES: [&str; 12] = [
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
            None => true,
            Some(value) if value.eq_ignore_ascii_case("true") => true,
            Some(value) if value.eq_ignore_ascii_case("false") => false,
            Some(value) => bail!(r#"callout open must be true or false, got "{value}""#),
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

#[cfg(test)]
mod tests {
    use super::*;

    // ── Callout::from_args ──

    #[test]
    fn callout_from_args_defaults() {
        assert_eq!(
            Callout::from_args(&BTreeMap::new()).unwrap(),
            Callout {
                kind: "note",
                title: None,
                open: true,
            }
        );
    }

    #[test]
    fn callout_from_args_reads_all_keys() {
        let named = args(&[
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
    fn callout_from_args_empty_title_is_none() {
        let named = args(&[("title", ""), ("open", "true")]);
        assert_eq!(
            Callout::from_args(&named).unwrap(),
            Callout {
                kind: "note",
                title: None,
                open: true,
            }
        );
    }

    #[test]
    fn callout_from_args_invalid_value_returns_error() {
        for (named, message) in [
            (
                args(&[("type", "warn")]),
                r#"unknown callout type "warn", expected one of: abstract, bug,"#,
            ),
            (
                args(&[("open", "no")]),
                r#"callout open must be true or false, got "no""#,
            ),
        ] {
            let error = Callout::from_args(&named).unwrap_err().to_string();
            assert!(error.starts_with(message), "{error}");
        }
    }

    // ── find_type ──

    #[test]
    fn find_type_returns_canonical_name() {
        assert_eq!(find_type("quote"), Some("quote"));
        assert_eq!(find_type("TIP"), Some("tip"));
        assert_eq!(find_type("tips"), None);
        assert_eq!(find_type(""), None);
    }

    fn args(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|&(key, value)| (key.to_owned(), value.to_owned()))
            .collect()
    }
}
