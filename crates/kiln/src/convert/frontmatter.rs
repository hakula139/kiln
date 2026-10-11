use anyhow::{Context, Result};
use serde_yaml::{Mapping, Value};

use crate::content::frontmatter::{FeaturedImage, Frontmatter, ImageCredit};

/// Delimiter around Hugo YAML frontmatter.
pub(crate) const YAML_DELIMITER: &str = "---";

/// Converts supported YAML metadata and reports the sorted fields that need manual migration.
///
/// Hugo's `lastmod` becomes `updated`, and its `featuredImage` key and bare-string image sources
/// become a `featured_image` table.
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
        normalize_hugo_keys(mapping);
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

/// Renames Hugo keys to their kiln equivalents unless the kiln key is already authored.
fn normalize_hugo_keys(mapping: &mut Mapping) {
    for (hugo, kiln) in [("featuredImage", "featured_image"), ("lastmod", "updated")] {
        if !mapping.contains_key(kiln)
            && let Some(value) = mapping.remove(hugo)
        {
            mapping.insert(kiln.into(), value);
        }
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
    fn omits_null_dates() {
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
    fn converts_every_supported_field() {
        let yaml = indoc! {"
            title: Full Post
            description: A description
            slug: my-slug
            date: 2024-01-15T10:30:00+08:00
            lastmod: 2024-02-01T08:00:00+08:00
            featuredImage:
              src: /img.webp
              position: top
              credit:
                title: Artwork
                author: Example
                url: https://example.com/art
            tags: [a, b]
            categories: [tutorial]
            draft: true
            weight: -3
            license: CC BY-NC-SA 4.0
            heading_numbering: true
        "};
        let (toml, unsupported) = convert_frontmatter(yaml).unwrap();
        assert_eq!(
            toml,
            indoc! {r#"
                title = "Full Post"
                description = "A description"
                slug = "my-slug"
                date = "2024-01-15T02:30:00Z"
                updated = "2024-02-01T00:00:00Z"
                tags = [
                    "a",
                    "b",
                ]
                draft = true
                weight = -3
                license = "CC BY-NC-SA 4.0"
                heading_numbering = true

                [featured_image]
                src = "/img.webp"
                position = "top"

                [featured_image.credit]
                title = "Artwork"
                author = "Example"
                url = "https://example.com/art"
            "#}
        );
        assert_eq!(unsupported, ["categories"]);
    }

    #[test]
    fn keeps_authored_updated_over_lastmod() {
        let (toml, unsupported) = convert_frontmatter(indoc! {"
            updated: 2024-03-01T00:00:00Z
            lastmod: 2024-02-01T00:00:00Z
        "})
        .unwrap();
        assert_eq!(
            toml,
            indoc! {r#"
                updated = "2024-03-01T00:00:00Z"
            "#}
        );
        assert_eq!(unsupported, ["lastmod"]);
    }

    #[test]
    fn maps_hugo_featured_image_forms() {
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
    fn reports_unsupported_top_level_fields() {
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
    fn reports_unsupported_nested_fields() {
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
    fn rejects_malformed_or_non_mapping_yaml() {
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
