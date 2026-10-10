use anyhow::{Result, bail};
use jiff::Timestamp;
use serde::{Deserialize, Deserializer, Serialize, de};

/// Delimiter around native TOML frontmatter.
pub(crate) const TOML_DELIMITER: &str = "+++";

/// Metadata parsed from the TOML frontmatter of a content file.
#[derive(Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
#[expect(
    clippy::manual_non_exhaustive,
    reason = "the private unit field rejects Hugo's `featuredImage` key during deserialization"
)]
pub struct Frontmatter {
    #[serde(skip_serializing_if = "String::is_empty")]
    pub title: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Explicit slug override. When set, replaces the final component of the page route.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slug: Option<String>,

    #[serde(
        deserialize_with = "timestamp_serde::deserialize_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub date: Option<Timestamp>,

    #[serde(
        deserialize_with = "timestamp_serde::deserialize_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub updated: Option<Timestamp>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub featured_image: Option<FeaturedImage>,

    /// Hugo's `featuredImage` key, rejected so the image is not silently dropped.
    #[serde(
        rename = "featuredImage",
        skip_serializing,
        deserialize_with = "reject_featured_image_alias"
    )]
    pub(crate) featured_image_alias: (),

    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,

    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub draft: bool,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub weight: Option<i64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,

    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub heading_numbering: bool,
}

/// Featured image metadata. `width` / `height` / `lqip_uri` are stamped by
/// the build pipeline from the on-disk source, not authored.
#[derive(Debug, Default, Clone, PartialEq, Deserialize, Serialize)]
#[serde(expecting = "a `[featured_image]` table with a `src` key")]
pub struct FeaturedImage {
    pub src: String,

    /// CSS `background-position` (e.g., `"top"`, `"30% 20%"`).
    /// Defaults to `center` in templates when unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credit: Option<ImageCredit>,

    #[serde(skip_deserializing, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,

    #[serde(skip_deserializing, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,

    #[serde(skip_deserializing, skip_serializing_if = "Option::is_none")]
    pub lqip_uri: Option<String>,
}

impl FeaturedImage {
    /// Authored keys, which `kiln convert` keeps while reporting any others.
    pub(crate) const FIELDS: [&str; 3] = ["src", "position", "credit"];
}

/// Attribution metadata for a featured image.
#[derive(Debug, Default, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct ImageCredit {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,

    /// Link to the original work (e.g., a Pixiv artwork page).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

impl ImageCredit {
    /// Authored keys, which `kiln convert` keeps while reporting any others.
    pub(crate) const FIELDS: [&str; 3] = ["title", "author", "url"];
}

fn reject_featured_image_alias<'de, D: Deserializer<'de>>(_: D) -> Result<(), D::Error> {
    Err(de::Error::custom(
        "`featuredImage` is not supported, use a `[featured_image]` table",
    ))
}

/// Deserializes `jiff::Timestamp` from a string.
///
/// This is format-agnostic: it handles both TOML (where the `toml` crate passes native datetimes
/// through as single-entry maps) and YAML (where datetimes are plain strings).
mod timestamp_serde {
    use std::fmt;

    use jiff::Timestamp;
    use serde::de::{self, Deserializer, MapAccess, Visitor};

    pub fn deserialize_option<'de, D>(deserializer: D) -> Result<Option<Timestamp>, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_option(OptionVisitor)
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
    let (raw_fm, body) = split_delimited_frontmatter(content, TOML_DELIMITER)?;
    let fm: Frontmatter = toml::from_str(raw_fm)?;
    Ok((fm, body))
}

/// Returns `true` if content opens with `delimiter`, after an optional UTF-8 BOM.
pub(crate) fn has_delimited_frontmatter(content: &str, delimiter: &str) -> bool {
    strip_bom(content).starts_with(delimiter)
}

/// Splits content into raw frontmatter and body using the given delimiter.
///
/// This line-level search cannot tell a real closing delimiter from one on its own line inside a
/// multi-line string literal. Hugo and other delimiter-based parsers share this limitation.
///
/// # Errors
///
/// Returns an error if the delimiters are missing or malformed.
pub(crate) fn split_delimited_frontmatter<'a>(
    content: &'a str,
    delimiter: &str,
) -> Result<(&'a str, &'a str)> {
    let Some(rest) = strip_bom(content).strip_prefix(delimiter) else {
        bail!("missing opening `{delimiter}` delimiter");
    };
    let Some(rest) = rest
        .strip_prefix('\n')
        .or_else(|| rest.strip_prefix("\r\n"))
    else {
        bail!("opening `{delimiter}` must be on its own line");
    };

    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        let text = line
            .strip_suffix('\n')
            .map_or(line, |line| line.strip_suffix('\r').unwrap_or(line));
        if text == delimiter {
            return Ok((&rest[..offset], &rest[offset + line.len()..]));
        }
        offset += line.len();
    }
    bail!("missing closing `{delimiter}` delimiter")
}

fn strip_bom(content: &str) -> &str {
    content.strip_prefix('\u{feff}').unwrap_or(content)
}

#[cfg(test)]
mod tests {
    use indoc::{formatdoc, indoc};

    use super::*;

    // ── Frontmatter::serialize ──

    #[test]
    fn serialize_omits_unset_and_derived_fields() {
        let frontmatter = Frontmatter {
            featured_image: Some(FeaturedImage {
                src: "/img.webp".into(),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(
            toml::to_string(&frontmatter).unwrap(),
            indoc! {r#"
                [featured_image]
                src = "/img.webp"
            "#}
        );
    }

    #[test]
    fn serialize_round_trips_flags() {
        for (enabled, expected) in [
            (false, ""),
            (
                true,
                indoc! {"
                    draft = true
                    heading_numbering = true
                "},
            ),
        ] {
            let frontmatter = Frontmatter {
                draft: enabled,
                heading_numbering: enabled,
                ..Default::default()
            };
            let serialized = toml::to_string(&frontmatter).unwrap();
            assert_eq!(serialized, expected);
            assert_eq!(
                toml::from_str::<Frontmatter>(&serialized).unwrap(),
                frontmatter
            );
        }
    }

    // ── FeaturedImage::FIELDS / ImageCredit::FIELDS ──

    #[test]
    fn fields_match_authored_keys() {
        let credit = ImageCredit {
            title: Some("Example".into()),
            author: Some("Artist".into()),
            url: Some("https://example.com/artworks/123".into()),
        };
        let image = FeaturedImage {
            src: "/img.webp".into(),
            position: Some("top".into()),
            credit: Some(credit.clone()),
            width: None,
            height: None,
            lqip_uri: None,
        };
        for (table, fields) in [
            (
                toml::Table::try_from(&image).unwrap(),
                FeaturedImage::FIELDS,
            ),
            (toml::Table::try_from(&credit).unwrap(), ImageCredit::FIELDS),
        ] {
            let mut keys: Vec<_> = table.keys().map(String::as_str).collect();
            let mut fields = fields.to_vec();
            keys.sort_unstable();
            fields.sort_unstable();
            assert_eq!(keys, fields);
        }
    }

    // ── parse ──

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
            tags = ["rust", "ssg"]
            draft = true
            weight = 10
            license = "CC BY-NC-SA 4.0"
            heading_numbering = true

            [featured_image]
            src = "/images/example.webp"
            position = "top"

            [featured_image.credit]
            title = "Example"
            author = "Artist"
            url = "https://example.com/artworks/123"
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
        let fi = fm.featured_image.as_ref().unwrap();
        assert_eq!(fi.src, "/images/example.webp");
        assert_eq!(fi.position.as_deref(), Some("top"));
        let credit = fi.credit.as_ref().unwrap();
        assert_eq!(credit.title.as_deref(), Some("Example"));
        assert_eq!(credit.author.as_deref(), Some("Artist"));
        assert_eq!(
            credit.url.as_deref(),
            Some("https://example.com/artworks/123")
        );
        assert_eq!(fm.tags, vec!["rust", "ssg"]);
        assert!(fm.draft);
        assert_eq!(fm.weight, Some(10));
        assert_eq!(fm.license.as_deref(), Some("CC BY-NC-SA 4.0"));
        assert!(fm.heading_numbering);
        assert_eq!(body, "Content here.\n");
    }

    #[test]
    fn parse_featured_image_minimal() {
        let input = indoc! {r#"
            +++
            [featured_image]
            src = "/images/cover.webp"
            +++
        "#};
        let (fm, _) = parse(input).unwrap();
        let fi = fm.featured_image.as_ref().unwrap();
        assert_eq!(fi.src, "/images/cover.webp");
        assert!(fi.position.is_none());
        assert!(fi.credit.is_none());
        assert!(fi.width.is_none());
        assert!(fi.height.is_none());
        assert!(fi.lqip_uri.is_none());
    }

    #[test]
    fn parse_featured_image_ignores_derived_metadata() {
        let (frontmatter, _) = parse(indoc! {r#"
            +++
            [featured_image]
            src = "photo.webp"
            width = 800
            height = 600
            lqip_uri = "authored-placeholder"
            +++
        "#})
        .unwrap();
        let image = frontmatter.featured_image.unwrap();
        assert_eq!(image.src, "photo.webp");
        assert_eq!(
            (image.width, image.height, image.lqip_uri),
            (None, None, None)
        );
    }

    #[test]
    fn parse_heading_numbering_disabled() {
        let input = indoc! {"
            +++
            heading_numbering = false
            +++
        "};
        let (frontmatter, _) = parse(input).unwrap();
        assert!(!frontmatter.heading_numbering);
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
    fn parse_wrong_type_returns_error() {
        for (field, expected) in [
            ("date = 42", "a datetime string with UTC offset"),
            (
                r#"featured_image = "/images/cover.webp""#,
                "expected a `[featured_image]` table with a `src` key",
            ),
            (
                r#"featuredImage = "/images/cover.webp""#,
                "`featuredImage` is not supported, use a `[featured_image]` table",
            ),
        ] {
            let input = formatdoc! {"
                +++
                {field}
                +++
            "};
            let error = format!("{:#}", parse(&input).unwrap_err());
            assert!(error.contains(expected), "{field}: {error}");
        }
    }

    #[test]
    fn parse_local_datetime_without_offset_returns_error() {
        let input = indoc! {"
            +++
            date = 2024-06-15T10:30:00
            +++
        "};
        // Local datetimes come through as TOML maps, and jiff rejects the missing offset.
        let err = parse(input).unwrap_err().to_string();
        assert!(
            err.contains("UTC offset"),
            "error should mention UTC offset requirement, got: {err}"
        );
    }

    #[test]
    fn parse_invalid_heading_numbering_returns_error() {
        for value in ["2", r#""true""#, "{}"] {
            let input = formatdoc! {"
                +++
                heading_numbering = {value}
                +++
            "};
            assert!(parse(&input).is_err(), "{value}");
        }
    }

    // ── has_delimited_frontmatter ──

    #[test]
    fn has_delimited_frontmatter_accepts_bom_and_rejects_other_markdown() {
        for (content, expected) in [
            ("+++\ntitle = \"A\"\n+++\n", true),
            ("\u{feff}+++\n+++\n", true),
            ("# Notes\n", false),
            ("---\ntitle: A\n---\n", false),
        ] {
            assert_eq!(
                has_delimited_frontmatter(content, TOML_DELIMITER),
                expected,
                "{content:?}"
            );
        }
    }

    // ── split_delimited_frontmatter ──

    #[test]
    fn split_delimited_frontmatter_basic() {
        let input = indoc! {r#"
            +++
            title = "Hello"
            +++
            Body text here.
        "#};
        let (fm, body) = split_delimited_frontmatter(input, TOML_DELIMITER).unwrap();
        assert_eq!(
            fm,
            indoc! {r#"
                title = "Hello"
            "#},
        );
        assert_eq!(body, "Body text here.\n");
    }

    #[test]
    fn split_delimited_frontmatter_with_bom() {
        // BOM (\u{feff}) can't appear in raw strings, so use concat! here.
        let input = concat!("\u{feff}", "+++\ntitle = \"BOM\"\n+++\nBody\n");
        let (fm, body) = split_delimited_frontmatter(input, TOML_DELIMITER).unwrap();
        assert_eq!(fm, "title = \"BOM\"\n");
        assert_eq!(body, "Body\n");
    }

    #[test]
    fn split_delimited_frontmatter_crlf() {
        // Explicit \r\n line endings can't be expressed in indoc.
        let input = "+++\r\ntitle = \"Windows\"\r\n+++\r\nBody\r\n";
        let (fm, body) = split_delimited_frontmatter(input, TOML_DELIMITER).unwrap();
        assert_eq!(fm, "title = \"Windows\"\r\n");
        assert_eq!(body, "Body\r\n");
    }

    #[test]
    fn split_delimited_frontmatter_empty() {
        let input = indoc! {r"
            +++
            +++
            Body
        "};
        let (fm, body) = split_delimited_frontmatter(input, TOML_DELIMITER).unwrap();
        assert_eq!(fm, "");
        assert_eq!(body, "Body\n");
    }

    #[test]
    fn split_delimited_frontmatter_no_body() {
        let input = indoc! {r#"
            +++
            title = "No Body"
            +++
        "#};
        let (fm, body) = split_delimited_frontmatter(input, TOML_DELIMITER).unwrap();
        assert_eq!(
            fm,
            indoc! {r#"
                title = "No Body"
            "#},
        );
        assert_eq!(body, "");
    }

    #[test]
    fn split_delimited_frontmatter_closing_delimiter_inside_value_ignored() {
        let input = indoc! {r#"
            +++
            foo = "+++ not a delimiter"
            +++
            Body
        "#};
        let (fm, body) = split_delimited_frontmatter(input, TOML_DELIMITER).unwrap();
        assert_eq!(
            fm,
            indoc! {r#"
                foo = "+++ not a delimiter"
            "#},
        );
        assert_eq!(body, "Body\n");
    }

    #[test]
    fn split_delimited_frontmatter_closing_delimiter_must_end_line() {
        let input = indoc! {r#"
            +++
            title = "test"
            +++not_end
            +++
            Body
        "#};
        let (fm, body) = split_delimited_frontmatter(input, TOML_DELIMITER).unwrap();
        assert_eq!(
            fm,
            indoc! {r#"
                title = "test"
                +++not_end
            "#},
        );
        assert_eq!(body, "Body\n");
    }

    #[test]
    fn split_delimited_frontmatter_opening_not_on_own_line_returns_error() {
        let input = indoc! {"
            +++extra
            +++
        "};
        let err = split_delimited_frontmatter(input, TOML_DELIMITER)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("must be on its own line"),
            "should reject opening delimiter with trailing content, got: {err}"
        );
    }

    #[test]
    fn split_delimited_frontmatter_missing_opening_returns_error() {
        let input = indoc! {r#"
            title = "Hello"
            +++
            Body
        "#};
        let err = split_delimited_frontmatter(input, TOML_DELIMITER)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("missing opening `+++` delimiter"),
            "should report missing opening delimiter, got: {err}"
        );
    }

    #[test]
    fn split_delimited_frontmatter_missing_closing_returns_error() {
        let input = indoc! {r#"
            +++
            title = "Hello"
        "#};
        let err = split_delimited_frontmatter(input, TOML_DELIMITER)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("missing closing `+++` delimiter"),
            "should report missing closing delimiter, got: {err}"
        );
    }
}
