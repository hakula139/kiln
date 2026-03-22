use anyhow::Result;
use jiff::Timestamp;
use serde::{Deserialize, Serialize};

const DELIMITER: &str = "+++";

/// Metadata parsed from the TOML frontmatter of a content file.
#[derive(Debug, Default, PartialEq, Deserialize, Serialize)]
pub struct Frontmatter {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Explicit slug override. When set, takes priority over the filename-derived slug.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slug: Option<String>,

    #[serde(
        default,
        deserialize_with = "timestamp_serde::deserialize_option",
        serialize_with = "timestamp_serde::serialize_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub date: Option<Timestamp>,

    #[serde(
        default,
        deserialize_with = "timestamp_serde::deserialize_option",
        serialize_with = "timestamp_serde::serialize_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub updated: Option<Timestamp>,

    #[serde(
        default,
        alias = "featuredImage",
        skip_serializing_if = "Option::is_none"
    )]
    pub featured_image: Option<String>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub categories: Vec<String>,

    #[serde(default, skip_serializing_if = "is_default")]
    pub draft: bool,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weight: Option<i64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
}

fn is_default<T: Default + PartialEq>(t: &T) -> bool {
    *t == T::default()
}

/// Handles (de)serialization of `jiff::Timestamp` as a string.
///
/// This is format-agnostic: it handles both TOML (where the `toml` crate
/// passes native datetimes through as single-entry maps) and YAML (where
/// datetimes are plain strings).
mod timestamp_serde {
    use std::fmt;

    use jiff::Timestamp;
    use serde::Serializer;
    use serde::de::{self, Deserializer, MapAccess, Visitor};

    pub fn deserialize_option<'de, D>(deserializer: D) -> Result<Option<Timestamp>, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_option(OptionVisitor)
    }

    // Signature is dictated by serde's `serialize_with` attribute.
    #[expect(clippy::ref_option)]
    pub fn serialize_option<S>(ts: &Option<Timestamp>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match ts {
            Some(ts) => serializer.serialize_str(&ts.to_string()),
            None => serializer.serialize_none(),
        }
    }

    struct OptionVisitor;

    impl<'de> Visitor<'de> for OptionVisitor {
        type Value = Option<Timestamp>;

        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("a datetime string with UTC offset, or null")
        }

        fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_some<D: Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
            d.deserialize_any(TimestampVisitor).map(Some)
        }
    }

    struct TimestampVisitor;

    impl<'de> Visitor<'de> for TimestampVisitor {
        type Value = Timestamp;

        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("a datetime string with UTC offset")
        }

        fn visit_str<E: de::Error>(self, s: &str) -> Result<Self::Value, E> {
            parse_timestamp(s).map_err(de::Error::custom)
        }

        // TOML native datetimes are passed as single-entry maps by the `toml` crate.
        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
            let (_, value): (String, String) = map
                .next_entry()?
                .ok_or_else(|| de::Error::custom("expected datetime"))?;
            parse_timestamp(&value).map_err(de::Error::custom)
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

/// Splits content into raw TOML frontmatter and the remaining body.
///
/// Expects the content to start with `+++` on its own line, followed by TOML,
/// then a closing `+++` on its own line.
///
/// # Errors
///
/// Returns an error if the `+++` delimiters are missing or malformed.
pub(crate) fn split_frontmatter(content: &str) -> Result<(&str, &str)> {
    split_delimited_frontmatter(content, DELIMITER)
}

/// Splits content into raw frontmatter and the remaining body using the given
/// delimiter (e.g., `+++` for TOML, `---` for YAML).
///
/// Expects the content to start with the delimiter on its own line, followed by
/// frontmatter content, then a closing delimiter on its own line.
///
/// # Errors
///
/// Returns an error if the delimiters are missing or malformed.
pub(crate) fn split_delimited_frontmatter<'a>(
    content: &'a str,
    delimiter: &str,
) -> Result<(&'a str, &'a str)> {
    let content = content.strip_prefix('\u{feff}').unwrap_or(content);
    let rest = content
        .strip_prefix(delimiter)
        .ok_or_else(|| anyhow::anyhow!("missing opening `{delimiter}` delimiter"))?;

    // The opening delimiter must be followed by a newline (or be the entire file).
    let rest = rest
        .strip_prefix('\n')
        .or_else(|| rest.strip_prefix("\r\n"))
        .ok_or_else(|| anyhow::anyhow!("opening `{delimiter}` must be on its own line"))?;

    // Find the closing delimiter on its own line.
    let newline_delimiter = format!("\n{delimiter}");
    let closing = find_closing_delimiter(rest, delimiter, &newline_delimiter)
        .ok_or_else(|| anyhow::anyhow!("missing closing `{delimiter}` delimiter"))?;

    let frontmatter = &rest[..closing];
    let after_delim = &rest[closing + delimiter.len()..];

    // Skip the newline after the closing delimiter.
    let body = after_delim
        .strip_prefix('\n')
        .or_else(|| after_delim.strip_prefix("\r\n"))
        .unwrap_or(after_delim);

    Ok((frontmatter, body))
}

/// Finds the byte offset of the closing delimiter within the frontmatter region.
///
/// NOTE: This is a text-level search. It cannot distinguish a real closing delimiter
/// from one appearing on its own line inside a multi-line string literal.
/// This is a known limitation shared with Hugo and other delimiter-based parsers.
fn find_closing_delimiter(s: &str, delimiter: &str, newline_delimiter: &str) -> Option<usize> {
    // Check the very start (empty frontmatter).
    if let Some(after) = s.strip_prefix(delimiter)
        && at_line_boundary(after)
    {
        return Some(0);
    }

    // Search for `\n{delimiter}` on its own line.
    let mut search_from = 0;
    while let Some(pos) = s[search_from..].find(newline_delimiter) {
        let abs = search_from + pos + 1; // skip the `\n`
        let after = &s[abs + delimiter.len()..];
        if at_line_boundary(after) {
            return Some(abs);
        }
        search_from = abs + delimiter.len();
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

    // -- parse --

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
            slug = "my-post"
            date = "2024-06-15T12:34:56+08:00"
            updated = 2025-07-01T23:59:59Z
            featured_image = "/images/example.webp"
            tags = ["rust", "ssg"]
            categories = ["tutorial"]
            draft = true
            weight = 10
            license = "CC BY-NC-SA 4.0"
            +++
            Content here.
        "#};
        let (fm, body) = parse(input).unwrap();
        assert_eq!(fm.title, "My Post");
        assert_eq!(fm.description.as_deref(), Some("A test post"));
        assert_eq!(fm.slug.as_deref(), Some("my-post"));
        assert_eq!(
            fm.date.unwrap(),
            "2024-06-15T04:34:56Z".parse::<Timestamp>().unwrap()
        );
        assert_eq!(
            fm.updated.unwrap(),
            "2025-07-01T23:59:59Z".parse::<Timestamp>().unwrap()
        );
        assert_eq!(fm.featured_image.as_deref(), Some("/images/example.webp"));
        assert_eq!(fm.tags, vec!["rust", "ssg"]);
        assert_eq!(fm.categories, vec!["tutorial"]);
        assert!(fm.draft);
        assert_eq!(fm.weight, Some(10));
        assert_eq!(fm.license.as_deref(), Some("CC BY-NC-SA 4.0"));
        assert_eq!(body, "Content here.\n");
    }

    #[test]
    fn parse_invalid_toml_returns_error() {
        let input = indoc! {r"
            +++
            {{invalid toml
            +++
            Body
        "};
        assert!(parse(input).is_err());
    }

    #[test]
    fn parse_wrong_type_for_date_returns_error() {
        let input = indoc! {r"
            +++
            date = 42
            +++
        "};
        assert!(parse(input).is_err());
    }

    #[test]
    fn parse_local_datetime_without_offset_returns_error() {
        let input = indoc! {"
            +++
            date = 2024-06-15T10:30:00
            +++
        "};
        // Local datetimes come through as TOML maps; jiff rejects the missing offset.
        let err = parse(input).unwrap_err().to_string();
        assert!(
            err.contains("UTC offset"),
            "error should mention UTC offset requirement, got: {err}"
        );
    }

    // -- split_frontmatter --

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
    fn split_closing_delimiter_inside_value_ignored() {
        // `+++` appears mid-line in a value, should not be treated as closing delimiter.
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
    fn split_closing_delimiter_must_end_line() {
        // `+++not_end` should not be treated as a closing delimiter.
        let input = indoc! {r#"
            +++
            title = "test"
            +++not_end
            +++
            Body
        "#};
        let (fm, body) = split_frontmatter(input).unwrap();
        assert_eq!(fm, "title = \"test\"\n+++not_end\n");
        assert_eq!(body, "Body\n");
    }

    #[test]
    fn split_opening_delimiter_not_on_own_line_returns_error() {
        let input = indoc! {"
            +++extra
            +++
        "};
        let err = split_frontmatter(input).unwrap_err().to_string();
        assert!(
            err.contains("must be on its own line"),
            "should reject opening delimiter with trailing content, got: {err}"
        );
    }

    #[test]
    fn split_missing_opening_delimiter_returns_error() {
        let input = indoc! {r#"
            title = "Hello"
            +++
            Body
        "#};
        let err = split_frontmatter(input).unwrap_err().to_string();
        assert!(
            err.contains("missing opening `+++` delimiter"),
            "should report missing opening delimiter, got: {err}"
        );
    }

    #[test]
    fn split_missing_closing_delimiter_returns_error() {
        let input = indoc! {r#"
            +++
            title = "Hello"
        "#};
        let err = split_frontmatter(input).unwrap_err().to_string();
        assert!(
            err.contains("missing closing `+++` delimiter"),
            "should report missing closing delimiter, got: {err}"
        );
    }

    // -- yaml deserialization --

    #[test]
    fn yaml_basic() {
        let yaml = indoc! {"
            title: Hello
            date: 2024-06-15T12:34:56+08:00
        "};
        let fm: Frontmatter = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(fm.title, "Hello");
        assert_eq!(
            fm.date.unwrap(),
            "2024-06-15T04:34:56Z".parse::<Timestamp>().unwrap()
        );
    }

    #[test]
    fn yaml_alias_featured_image() {
        let yaml = indoc! {"
            featuredImage: /img.webp
        "};
        let fm: Frontmatter = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(fm.featured_image.as_deref(), Some("/img.webp"));
    }

    #[test]
    fn yaml_null_date() {
        let yaml = indoc! {"
            date: ~
        "};
        let fm: Frontmatter = serde_yaml::from_str(yaml).unwrap();
        assert!(fm.date.is_none());
    }

    #[test]
    fn yaml_unknown_fields_ignored() {
        let yaml = indoc! {"
            title: Test
            unknownField: dropped
            code:
              maxShownLines: 10
        "};
        let fm: Frontmatter = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(fm.title, "Test");
    }
}
