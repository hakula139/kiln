mod frontmatter;
mod shortcode;

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use walkdir::WalkDir;

/// Converts a Hugo content directory to kiln format.
///
/// Walks `source` recursively. For each `.md` file, converts YAML frontmatter
/// to TOML and translates shortcodes to kiln directives. Non-markdown files
/// (co-located assets) are copied as-is. Hugo section files (`_index.md`) are
/// skipped.
///
/// # Errors
///
/// Returns an error if any file cannot be read, converted, or written.
pub fn convert(source: &Path, dest: &Path) -> Result<()> {
    for entry in WalkDir::new(source) {
        let entry = entry?;
        if entry.file_type().is_dir() {
            continue;
        }

        let rel_path = entry
            .path()
            .strip_prefix(source)
            .context("failed to compute relative path")?;

        let file_name = rel_path.file_name().and_then(|n| n.to_str()).unwrap_or("");

        // Skip Hugo section files.
        if file_name == "_index.md" {
            continue;
        }

        let dest_path = dest.join(rel_path);
        if let Some(parent) = dest_path.parent() {
            fs::create_dir_all(parent)?;
        }

        if Path::new(file_name)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
        {
            convert_markdown_file(entry.path(), &dest_path)?;
        } else {
            fs::copy(entry.path(), &dest_path)?;
        }
    }

    Ok(())
}

fn convert_markdown_file(src: &Path, dest: &Path) -> Result<()> {
    let content =
        fs::read_to_string(src).with_context(|| format!("failed to read {}", src.display()))?;

    let (yaml_fm, body) = frontmatter::split_yaml_frontmatter(&content)
        .with_context(|| format!("failed to split frontmatter in {}", src.display()))?;

    let toml_fm = frontmatter::convert_frontmatter(yaml_fm)
        .with_context(|| format!("failed to convert frontmatter in {}", src.display()))?;

    let converted_body = shortcode::convert_shortcodes(body);

    let mut output = String::with_capacity(toml_fm.len() + converted_body.len() + 10);
    output.push_str("+++\n");
    output.push_str(&toml_fm);
    output.push_str("+++\n");
    output.push_str(&converted_body);

    fs::write(dest, output).with_context(|| format!("failed to write {}", dest.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::*;

    #[test]
    fn convert_markdown_file_basic() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("input.md");
        let dest = dir.path().join("output.md");

        fs::write(
            &src,
            indoc! {r"
                ---
                title: Hello, world!
                tags: [rust]
                ---

                Summary

                <!--more-->

                Full content

                {{< admonition info Note false >}}
                Body
                {{< /admonition >}}
            "},
        )
        .unwrap();

        convert_markdown_file(&src, &dest).unwrap();
        let result = fs::read_to_string(&dest).unwrap();

        assert_eq!(
            result,
            indoc! {r#"
                +++
                title = "Hello, world!"
                tags = ["rust"]
                +++

                Summary

                <!--more-->

                Full content

                ::: callout {type=info title="Note" open=false}
                Body
                :::
            "#}
        );
    }

    #[test]
    fn convert_markdown_file_no_frontmatter_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("bad.md");
        let dest = dir.path().join("out.md");

        fs::write(&src, "No frontmatter here\n").unwrap();

        let err = convert_markdown_file(&src, &dest).unwrap_err();
        assert!(
            err.to_string().contains("failed to split frontmatter"),
            "got: {err}"
        );
    }

    #[test]
    fn convert_directory_structure() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        let dest = dir.path().join("dest");

        // Create page bundle.
        let bundle = source.join("posts/my-post");
        fs::create_dir_all(&bundle).unwrap();
        fs::write(
            bundle.join("index.md"),
            indoc! {r"
                ---
                title: Post
                ---
                Content
            "},
        )
        .unwrap();
        fs::write(bundle.join("image.webp"), "fake-image").unwrap();

        // Create standalone file.
        fs::create_dir_all(source.join("pages")).unwrap();
        fs::write(
            source.join("pages/about.md"),
            indoc! {r"
                ---
                title: About
                ---
                About page
            "},
        )
        .unwrap();

        // Create Hugo section file (should be skipped).
        fs::write(
            source.join("posts/_index.md"),
            indoc! {r"
                ---
                title: Section
                ---
            "},
        )
        .unwrap();

        convert(&source, &dest).unwrap();

        // Verify converted markdown.
        let post = fs::read_to_string(dest.join("posts/my-post/index.md")).unwrap();
        assert_eq!(
            post,
            indoc! {r#"
                +++
                title = "Post"
                +++
                Content
            "#}
        );

        // Verify asset copied.
        assert!(dest.join("posts/my-post/image.webp").exists());

        // Verify standalone.
        let about = fs::read_to_string(dest.join("pages/about.md")).unwrap();
        assert_eq!(
            about,
            indoc! {r#"
                +++
                title = "About"
                +++
                About page
            "#}
        );

        // Verify section file skipped.
        assert!(!dest.join("posts/_index.md").exists());
    }
}
