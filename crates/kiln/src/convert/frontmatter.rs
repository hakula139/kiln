use anyhow::{Context, Result};

use crate::content::frontmatter::{Frontmatter, split_delimited_frontmatter};

const DELIMITER: &str = "---";

/// Splits content into raw YAML frontmatter and the remaining body.
///
/// Expects the content to start with `---` on its own line, followed by YAML,
/// then a closing `---` on its own line.
///
/// # Errors
///
/// Returns an error if the `---` delimiters are missing or malformed.
pub(crate) fn split_yaml_frontmatter(content: &str) -> Result<(&str, &str)> {
    split_delimited_frontmatter(content, DELIMITER)
}

/// Converts a YAML frontmatter string to a TOML frontmatter string.
///
/// Unknown fields are silently dropped. Fields are emitted in struct declaration
/// order via `Serialize`.
pub(crate) fn convert_frontmatter(yaml_str: &str) -> Result<String> {
    let fm: Frontmatter =
        serde_yaml::from_str(yaml_str).context("failed to parse YAML frontmatter")?;
    let toml_str = toml::to_string_pretty(&fm).context("failed to serialize TOML frontmatter")?;
    Ok(toml_str)
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::*;

    // -- split_yaml_frontmatter --

    #[test]
    fn split_basic() {
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
    fn split_no_yaml_returns_error() {
        assert!(split_yaml_frontmatter("No frontmatter here").is_err());
    }

    #[test]
    fn split_no_body() {
        let input = indoc! {"
            ---
            title: No Body
            ---
        "};
        let (fm, body) = split_yaml_frontmatter(input).unwrap();
        assert_eq!(fm, "title: No Body\n");
        assert_eq!(body, "");
    }

    // -- convert_frontmatter --

    #[test]
    fn convert_minimal() {
        let yaml = indoc! {"
            title: Minimal
        "};
        let toml = convert_frontmatter(yaml).unwrap();
        assert_eq!(
            toml,
            indoc! {r#"
                title = "Minimal"
            "#}
        );
    }

    #[test]
    fn convert_full() {
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
        let toml = convert_frontmatter(yaml).unwrap();
        assert_eq!(
            toml,
            indoc! {r#"
                title = "Full Post"
                description = "A description"
                slug = "my-slug"
                date = "2024-01-15T02:30:00Z"
                featured_image = "/img.webp"
                tags = [
                    "a",
                    "b",
                ]
                categories = ["tutorial"]
                draft = true
                weight = -3
                license = "CC BY-NC-SA 4.0"
            "#}
        );
    }

    #[test]
    fn convert_renames_featured_image() {
        let yaml = indoc! {"
            featuredImage: https://example.com/img.webp
        "};
        let toml = convert_frontmatter(yaml).unwrap();
        assert_eq!(
            toml,
            indoc! {r#"
                featured_image = "https://example.com/img.webp"
            "#}
        );
    }

    #[test]
    fn convert_drops_unknown_fields() {
        let yaml = indoc! {"
            title: Test
            unknownField: dropped
            code:
              maxShownLines: 10
        "};
        let toml = convert_frontmatter(yaml).unwrap();
        assert_eq!(
            toml,
            indoc! {r#"
                title = "Test"
            "#}
        );
    }
}
