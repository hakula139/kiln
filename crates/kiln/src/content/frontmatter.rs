use anyhow::Result;
use jiff::Timestamp;
use serde::Deserialize;

const DELIMITER: &str = "+++";
const NEWLINE_DELIMITER: &str = "\n+++";

/// Metadata parsed from the TOML frontmatter of a content file.
#[derive(Debug, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Frontmatter {
    #[serde(default)]
    pub title: String,

    #[serde(default)]
    pub description: Option<String>,

    #[serde(default)]
    pub featured_image: Option<String>,

    #[serde(default, deserialize_with = "toml_timestamp::deserialize_option")]
    pub date: Option<Timestamp>,

    #[serde(default, deserialize_with = "toml_timestamp::deserialize_option")]
    pub updated: Option<Timestamp>,

    #[serde(default)]
    pub draft: bool,

    #[serde(default)]
    pub tags: Vec<String>,

    #[serde(default)]
    pub categories: Vec<String>,

    /// Explicit slug override. When set, takes priority over the filename-derived slug.
    #[serde(default)]
    pub slug: Option<String>,
}

/// Handles deserialization of `jiff::Timestamp` from both quoted strings and
/// TOML native datetimes. The `toml` crate serializes native datetimes as maps,
/// which jiff's default serde impl can't handle.
mod toml_timestamp {
    use jiff::Timestamp;
    use serde::{Deserialize, Deserializer, de};

    pub fn deserialize_option<'de, D>(deserializer: D) -> Result<Option<Timestamp>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let Some(value) = Option::<toml::Value>::deserialize(deserializer)? else {
            return Ok(None);
        };

        match value {
            toml::Value::String(s) => parse_timestamp(&s).map(Some).map_err(de::Error::custom),
            toml::Value::Datetime(dt) => parse_timestamp(&dt.to_string())
                .map(Some)
                .map_err(de::Error::custom),
            other => Err(de::Error::custom(format!(
                "expected datetime or string, got {other}"
            ))),
        }
    }

    fn parse_timestamp(s: &str) -> Result<Timestamp, String> {
        s.parse::<Timestamp>().map_err(|e| {
            format!(
                "invalid timestamp `{s}`: {e} \
                 (dates must include a UTC offset, e.g., 2024-01-15T10:30:00+08:00)"
            )
        })
    }
}

/// Splits content into raw TOML frontmatter and the remaining body.
///
/// Expects the content to start with `+++` on its own line, followed by TOML,
/// then a closing `+++` on its own line.
///
/// # Errors
///
/// Returns an error if the `+++` delimiters are missing or malformed.
pub(crate) fn split_frontmatter(content: &str) -> Result<(&str, &str)> {
    let content = content.strip_prefix('\u{feff}').unwrap_or(content);
    let rest = content
        .strip_prefix(DELIMITER)
        .ok_or_else(|| anyhow::anyhow!("missing opening `+++` delimiter"))?;

    // The opening delimiter must be followed by a newline (or be the entire file).
    let rest = rest
        .strip_prefix('\n')
        .or_else(|| rest.strip_prefix("\r\n"))
        .ok_or_else(|| anyhow::anyhow!("opening `+++` must be on its own line"))?;

    // Find the closing delimiter on its own line.
    let closing = find_closing_delimiter(rest)
        .ok_or_else(|| anyhow::anyhow!("missing closing `+++` delimiter"))?;

    let frontmatter = &rest[..closing];
    let after_delim = &rest[closing + DELIMITER.len()..];

    // Skip the newline after the closing delimiter.
    let body = after_delim
        .strip_prefix('\n')
        .or_else(|| after_delim.strip_prefix("\r\n"))
        .unwrap_or(after_delim);

    Ok((frontmatter, body))
}

/// Parses a content file into its `Frontmatter` and body text.
///
/// # Errors
///
/// Returns an error if the frontmatter delimiters are missing or the TOML is invalid.
pub(crate) fn parse(content: &str) -> Result<(Frontmatter, &str)> {
    let (raw_fm, body) = split_frontmatter(content)?;
    let fm: Frontmatter = toml::from_str(raw_fm)?;
    Ok((fm, body))
}

/// Finds the byte offset of the closing `+++` delimiter within the frontmatter region.
///
/// NOTE: This is a text-level search. It cannot distinguish a real closing delimiter
/// from `+++` appearing on its own line inside a TOML multi-line string (`"""`).
/// This is a known limitation shared with Hugo and other delimiter-based parsers.
fn find_closing_delimiter(s: &str) -> Option<usize> {
    // Check the very start (empty frontmatter).
    if let Some(after) = s.strip_prefix(DELIMITER)
        && at_line_boundary(after)
    {
        return Some(0);
    }

    // Search for `\n+++` on its own line.
    let mut search_from = 0;
    while let Some(pos) = s[search_from..].find(NEWLINE_DELIMITER) {
        let abs = search_from + pos + 1; // skip the `\n`
        let after = &s[abs + DELIMITER.len()..];
        if at_line_boundary(after) {
            return Some(abs);
        }
        search_from = abs + DELIMITER.len();
    }

    None
}

/// Returns `true` if the string is empty or starts with a line ending.
fn at_line_boundary(s: &str) -> bool {
    s.is_empty() || s.starts_with('\n') || s.starts_with("\r\n")
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::*;

    #[test]
    fn split_basic() {
        let input = indoc! {r#"
            +++
            title = "Hello"
            +++
            Body text here.
        "#};
        let (fm, body) = split_frontmatter(input).unwrap();
        assert_eq!(fm, "title = \"Hello\"\n");
        assert_eq!(body, "Body text here.\n");
    }

    #[test]
    fn split_with_bom() {
        // BOM (\u{feff}) can't appear in raw strings, so use concat! here.
        let input = concat!("\u{feff}", "+++\ntitle = \"BOM\"\n+++\nBody\n");
        let (fm, body) = split_frontmatter(input).unwrap();
        assert_eq!(fm, "title = \"BOM\"\n");
        assert_eq!(body, "Body\n");
    }

    #[test]
    fn split_crlf() {
        // Explicit \r\n line endings can't be expressed in indoc.
        let input = "+++\r\ntitle = \"Windows\"\r\n+++\r\nBody\r\n";
        let (fm, body) = split_frontmatter(input).unwrap();
        assert_eq!(fm, "title = \"Windows\"\r\n");
        assert_eq!(body, "Body\r\n");
    }

    #[test]
    fn split_empty_frontmatter() {
        let input = indoc! {r"
            +++
            +++
            Body
        "};
        let (fm, body) = split_frontmatter(input).unwrap();
        assert_eq!(fm, "");
        assert_eq!(body, "Body\n");
    }

    #[test]
    fn split_no_body() {
        let input = indoc! {r#"
            +++
            title = "No Body"
            +++
        "#};
        let (fm, body) = split_frontmatter(input).unwrap();
        assert_eq!(fm, "title = \"No Body\"\n");
        assert_eq!(body, "");
    }

    #[test]
    fn split_delimiter_not_on_own_line() {
        // `+++` appears mid-line, should not be treated as closing delimiter.
        let input = indoc! {r#"
            +++
            foo = "+++ not a delimiter"
            +++
            Body
        "#};
        let (fm, body) = split_frontmatter(input).unwrap();
        assert_eq!(fm, "foo = \"+++ not a delimiter\"\n");
        assert_eq!(body, "Body\n");
    }

    #[test]
    fn split_missing_opening_delimiter() {
        let input = indoc! {r#"
            title = "Hello"
            +++
            Body
        "#};
        assert!(split_frontmatter(input).is_err());
    }

    #[test]
    fn split_missing_closing_delimiter() {
        let input = indoc! {r#"
            +++
            title = "Hello"
        "#};
        assert!(split_frontmatter(input).is_err());
    }

    #[test]
    fn parse_minimal() {
        let input = indoc! {r"
            +++
            +++
            Hello, world!
        "};
        let (fm, body) = parse(input).unwrap();
        assert_eq!(fm, Frontmatter::default());
        assert_eq!(body, "Hello, world!\n");
    }

    #[test]
    fn parse_full() {
        let input = indoc! {r#"
            +++
            title = "My Post"
            description = "A test post"
            featured_image = "/images/example.webp"
            date = "2024-06-15T12:34:56+08:00"
            updated = 2025-07-01T23:59:59Z
            draft = true
            tags = ["rust", "ssg"]
            categories = ["tutorial"]
            slug = "my-post"
            +++
            Content here.
        "#};
        let (fm, body) = parse(input).unwrap();
        assert_eq!(fm.title, "My Post");
        assert_eq!(fm.description.as_deref(), Some("A test post"));
        assert_eq!(fm.featured_image.as_deref(), Some("/images/example.webp"));
        assert_eq!(
            fm.date.unwrap(),
            "2024-06-15T04:34:56Z".parse::<Timestamp>().unwrap()
        );
        assert_eq!(
            fm.updated.unwrap(),
            "2025-07-01T23:59:59Z".parse::<Timestamp>().unwrap()
        );
        assert!(fm.draft);
        assert_eq!(fm.tags, vec!["rust", "ssg"]);
        assert_eq!(fm.categories, vec!["tutorial"]);
        assert_eq!(fm.slug.as_deref(), Some("my-post"));
        assert_eq!(body, "Content here.\n");
    }

    #[test]
    fn parse_invalid_toml() {
        let input = indoc! {r"
            +++
            {{invalid toml
            +++
            Body
        "};
        assert!(parse(input).is_err());
    }

    #[test]
    fn parse_unknown_field_errors() {
        let input = indoc! {r"
            +++
            daft = true
            +++
            Body
        "};
        let err = parse(input).unwrap_err().to_string();
        assert!(
            err.contains("unknown field"),
            "should reject unknown fields, got: {err}"
        );
    }

    #[test]
    fn parse_wrong_type_for_date_errors() {
        let input = indoc! {r"
            +++
            date = 42
            +++
        "};
        let err = parse(input).unwrap_err().to_string();
        assert!(
            err.contains("expected datetime or string"),
            "should reject non-datetime types, got: {err}"
        );
    }

    #[test]
    fn parse_local_datetime_without_offset_errors() {
        let input = indoc! {"
            +++
            date = 2024-06-15T10:30:00
            +++
        "};
        let err = parse(input).unwrap_err().to_string();
        assert!(
            err.contains("UTC offset"),
            "error should mention UTC offset requirement, got: {err}"
        );
    }
}
