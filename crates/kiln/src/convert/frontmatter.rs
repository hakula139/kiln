use std::collections::BTreeMap;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::content::frontmatter::{Frontmatter, split_delimited_frontmatter};

const DELIMITER: &str = "---";

/// Splits content into raw YAML frontmatter and the remaining body.
///
/// # Errors
///
/// Returns an error if the `---` delimiters are missing or malformed.
pub(crate) fn split_yaml_frontmatter(content: &str) -> Result<(&str, &str)> {
    split_delimited_frontmatter(content, DELIMITER)
}

#[derive(Deserialize)]
struct MigrationFrontmatter {
    #[serde(flatten)]
    supported: Frontmatter,
    #[serde(flatten)]
    unsupported: BTreeMap<String, serde_yaml::Value>,
}

/// Converts supported YAML metadata and reports fields that require manual migration.
pub(crate) fn convert_frontmatter(yaml_str: &str) -> Result<(String, Vec<String>)> {
    let value: serde_yaml::Value =
        serde_yaml::from_str(yaml_str).context("failed to parse YAML frontmatter")?;
    let mut unsupported = Vec::new();
    collect_image_fields(&value, &mut unsupported);
    let fm: MigrationFrontmatter =
        serde_yaml::from_value(value).context("failed to parse YAML frontmatter")?;
    unsupported.extend(fm.unsupported.into_keys());
    let toml_str =
        toml::to_string_pretty(&fm.supported).context("failed to serialize TOML frontmatter")?;
    Ok((toml_str, unsupported))
}

fn collect_image_fields(value: &serde_yaml::Value, unsupported: &mut Vec<String>) {
    for field in ["featured_image", "featuredImage"] {
        let Some(image) = value.get(field).and_then(serde_yaml::Value::as_mapping) else {
            continue;
        };
        collect_unknown_fields(image, field, &["src", "position", "credit"], unsupported);
        if let Some(credit) = image.get("credit").and_then(serde_yaml::Value::as_mapping) {
            collect_unknown_fields(
                credit,
                &format!("{field}.credit"),
                &["title", "author", "url"],
                unsupported,
            );
        }
    }
}

fn collect_unknown_fields(
    mapping: &serde_yaml::Mapping,
    prefix: &str,
    supported: &[&str],
    unsupported: &mut Vec<String>,
) {
    for key in mapping.keys().filter_map(serde_yaml::Value::as_str) {
        if !supported.contains(&key) {
            unsupported.push(format!("{prefix}.{key}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::*;

    // ── split_yaml_frontmatter ──

    #[test]
    fn split_yaml_frontmatter_basic() {
        let input = indoc! {r"
            ---
            title: Hello
            ---
            Body text here.
        "};
        let (fm, body) = split_yaml_frontmatter(input).unwrap();
        assert_eq!(fm, "title: Hello\n");
        assert_eq!(body, "Body text here.\n");
    }

    #[test]
    fn split_yaml_frontmatter_missing_delimiter_returns_error() {
        assert!(split_yaml_frontmatter("No frontmatter here").is_err());
    }

    #[test]
    fn split_yaml_frontmatter_no_body() {
        let input = indoc! {"
            ---
            title: No Body
            ---
        "};
        let (fm, body) = split_yaml_frontmatter(input).unwrap();
        assert_eq!(fm, "title: No Body\n");
        assert_eq!(body, "");
    }

    // ── convert_frontmatter ──

    #[test]
    fn convert_frontmatter_minimal() {
        let yaml = indoc! {"
            title: Minimal
        "};
        let (toml, _) = convert_frontmatter(yaml).unwrap();
        assert_eq!(
            toml,
            indoc! {r#"
                title = "Minimal"
            "#}
        );
    }

    #[test]
    fn convert_frontmatter_full() {
        let yaml = indoc! {"
            title: Full Post
            description: A description
            slug: my-slug
            date: 2024-01-15T10:30:00+08:00
            featuredImage: /img.webp
            tags: [a, b]
            categories: [tutorial]
            draft: true
            weight: -3
            license: CC BY-NC-SA 4.0
        "};
        let (toml, _) = convert_frontmatter(yaml).unwrap();
        assert_eq!(
            toml,
            indoc! {r#"
                title = "Full Post"
                description = "A description"
                slug = "my-slug"
                date = "2024-01-15T02:30:00Z"
                tags = [
                    "a",
                    "b",
                ]
                draft = true
                weight = -3
                license = "CC BY-NC-SA 4.0"

                [featured_image]
                src = "/img.webp"
            "#}
        );
    }

    #[test]
    fn convert_frontmatter_renames_featured_image() {
        let yaml = indoc! {"
            featuredImage: https://example.com/img.webp
        "};
        let (toml, _) = convert_frontmatter(yaml).unwrap();
        assert_eq!(
            toml,
            indoc! {r#"
                [featured_image]
                src = "https://example.com/img.webp"
            "#}
        );
    }

    #[test]
    fn convert_frontmatter_reports_nested_fields() {
        let (toml, unsupported) = convert_frontmatter(indoc! {r"
            featuredImage:
              src: photo.webp
              width: 400
              credit:
                author: Example
                custom: unsupported
        "})
        .unwrap();
        assert_eq!(
            unsupported,
            ["featuredImage.width", "featuredImage.credit.custom"]
        );
        assert_eq!(
            toml,
            indoc! {r#"
                [featured_image]
                src = "photo.webp"

                [featured_image.credit]
                author = "Example"
            "#}
        );
    }

    #[test]
    fn convert_frontmatter_invalid_yaml_returns_error() {
        let yaml = indoc! {"
            :
              invalid: [yaml
        "};
        let err = convert_frontmatter(yaml).unwrap_err();
        assert!(
            err.to_string().contains("failed to parse YAML"),
            "got: {err}"
        );
    }

    #[test]
    fn convert_frontmatter_reports_unsupported_fields() {
        let yaml = indoc! {"
            title: Test
            unknownField: dropped
            code:
              maxShownLines: 10
        "};
        let (toml, unsupported) = convert_frontmatter(yaml).unwrap();
        assert_eq!(unsupported, ["code", "unknownField"]);
        assert_eq!(
            toml,
            indoc! {r#"
                title = "Test"
            "#}
        );
    }
}
