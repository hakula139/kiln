use anyhow::{Context, Result};
use serde_yaml::{Mapping, Value};

use crate::content::frontmatter::{FeaturedImage, Frontmatter, ImageCredit};

/// Delimiter around Hugo YAML frontmatter.
pub(crate) const YAML_DELIMITER: &str = "---";

/// Converts supported YAML metadata and reports the sorted fields that need manual migration.
///
/// Hugo's `featuredImage` key and bare-string image sources become a `featured_image` table.
pub(crate) fn convert_frontmatter(yaml_str: &str) -> Result<(String, Vec<String>)> {
    let mut value: Value =
        serde_yaml::from_str(yaml_str).context("failed to parse YAML frontmatter")?;
    let mut unsupported = Vec::new();
    if let Some(mapping) = value.as_mapping_mut() {
        for key in ["featured_image", "featuredImage"] {
            let Some(image) = mapping.get_mut(key).and_then(Value::as_mapping_mut) else {
                continue;
            };
            retain_fields(
                image,
                &format!("{key}."),
                &FeaturedImage::FIELDS,
                &mut unsupported,
            );
            if let Some(credit) = image.get_mut("credit").and_then(Value::as_mapping_mut) {
                retain_fields(
                    credit,
                    &format!("{key}.credit."),
                    &ImageCredit::FIELDS,
                    &mut unsupported,
                );
            }
        }
        normalize_featured_image(mapping);
        retain_fields(mapping, "", &Frontmatter::FIELDS, &mut unsupported);
    }
    unsupported.sort();

    let fm: Frontmatter =
        serde_yaml::from_value(value).context("failed to parse YAML frontmatter")?;
    let toml_str = toml::to_string_pretty(&fm).context("failed to serialize TOML frontmatter")?;
    Ok((toml_str, unsupported))
}

/// Removes string keys outside `supported`, recording them with `prefix`. Other keys stay so
/// deserialization rejects them.
fn retain_fields(
    mapping: &mut Mapping,
    prefix: &str,
    supported: &[&str],
    unsupported: &mut Vec<String>,
) {
    mapping.retain(|key, _| {
        let Some(name) = key.as_str() else {
            return true;
        };
        let keep = supported.contains(&name);
        if !keep {
            unsupported.push(format!("{prefix}{name}"));
        }
        keep
    });
}

fn normalize_featured_image(mapping: &mut Mapping) {
    if !mapping.contains_key("featured_image")
        && let Some(image) = mapping.remove("featuredImage")
    {
        mapping.insert("featured_image".into(), image);
    }
    if let Some(image @ Value::String(_)) = mapping.get_mut("featured_image") {
        let src = std::mem::take(image);
        *image = Value::Mapping(Mapping::from_iter([("src".into(), src)]));
    }
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::*;

    // ── convert_frontmatter ──

    #[test]
    fn convert_frontmatter_minimal() {
        let yaml = indoc! {"
            title: Minimal
            date: ~
            updated: null
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
    fn convert_frontmatter_maps_hugo_featured_image_forms() {
        for (yaml, expected_unsupported) in [
            ("featured_image: /img.webp", Vec::<&str>::new()),
            ("featuredImage: /img.webp", vec![]),
            (
                indoc! {"
                    featuredImage:
                      src: /img.webp
                "},
                vec![],
            ),
            (
                indoc! {"
                    featured_image: /img.webp
                    featuredImage: /other.webp
                "},
                vec!["featuredImage"],
            ),
        ] {
            let (toml, unsupported) = convert_frontmatter(yaml).unwrap();
            assert_eq!(
                toml,
                indoc! {r#"
                    [featured_image]
                    src = "/img.webp"
                "#},
                "{yaml}"
            );
            assert_eq!(unsupported, expected_unsupported, "{yaml}");
        }
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
            ["featuredImage.credit.custom", "featuredImage.width"]
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

    #[test]
    fn convert_frontmatter_invalid_yaml_returns_error() {
        for yaml in [
            indoc! {"
                :
                  invalid: [yaml
            "},
            "- not a mapping",
            "1: numeric key",
        ] {
            let err = convert_frontmatter(yaml).unwrap_err();
            assert!(
                err.to_string().contains("failed to parse YAML"),
                "{yaml}: {err}"
            );
        }
    }
}
