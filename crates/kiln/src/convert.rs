mod frontmatter;
mod shortcode;

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use walkdir::WalkDir;

/// Converts a Hugo content directory to kiln format.
///
/// Walks `source` recursively. For each `.md` file, converts YAML frontmatter
/// to TOML and translates shortcodes to kiln directives. Non-markdown files
/// (co-located assets) are copied as-is.
///
/// Hugo category index files (`categories/<slug>/_index.md`) are converted to
/// kiln section indexes at `posts/<slug>/_index.md`. Tag index files
/// (`tags/<slug>/_index.md`) are converted in place. Other `_index.md` files
/// (Hugo section files) are skipped since kiln derives sections from directory
/// structure.
///
/// Existing files in `dest` are never overwritten.
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

        // Handle _index.md files: convert category / tag term indexes,
        // redirect category indexes to section indexes, skip others.
        if file_name == "_index.md" {
            if let Some(dest_path) = index_dest_path(rel_path, dest)
                && !dest_path.exists()
            {
                if let Some(parent) = dest_path.parent() {
                    fs::create_dir_all(parent)?;
                }
                convert_or_copy_markdown(entry.path(), &dest_path)?;
            }
            continue;
        }

        let dest_path = dest.join(rel_path);

        // Never overwrite existing files.
        if dest_path.exists() {
            continue;
        }

        if let Some(parent) = dest_path.parent() {
            fs::create_dir_all(parent)?;
        }

        if Path::new(file_name)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
        {
            convert_or_copy_markdown(entry.path(), &dest_path)?;
        } else {
            fs::copy(entry.path(), &dest_path)?;
        }
    }

    Ok(())
}

/// Computes the destination path for an `_index.md` file, or `None` to skip.
///
/// - `categories/<slug>/_index.md` → `posts/<slug>/_index.md` (section index)
/// - `tags/<slug>/_index.md` → `tags/<slug>/_index.md` (tag term index)
/// - Everything else → `None` (skipped)
fn index_dest_path(rel_path: &Path, dest: &Path) -> Option<PathBuf> {
    let components: Vec<_> = rel_path.components().collect();
    // Expect exactly: <kind>/<slug>/_index.md (3 components).
    if components.len() != 3 {
        return None;
    }
    let kind = components[0].as_os_str().to_str().unwrap_or("");
    let slug = components[1].as_os_str();
    match kind {
        "categories" => Some(dest.join("posts").join(slug).join("_index.md")),
        "tags" => Some(dest.join(rel_path)),
        _ => None,
    }
}

/// Converts a markdown file if it has YAML frontmatter, otherwise copies it as-is.
/// Frontmatter-less `.md` files (e.g. page bundle resources) are not convertible.
fn convert_or_copy_markdown(src: &Path, dest: &Path) -> Result<()> {
    let content =
        fs::read_to_string(src).with_context(|| format!("failed to read {}", src.display()))?;

    if let Ok((yaml_fm, body)) = frontmatter::split_yaml_frontmatter(&content) {
        convert_markdown_file(yaml_fm, body, dest)
    } else {
        fs::copy(src, dest)?;
        Ok(())
    }
}

fn convert_markdown_file(yaml_fm: &str, body: &str, dest: &Path) -> Result<()> {
    let toml_fm = frontmatter::convert_frontmatter(yaml_fm)
        .with_context(|| format!("failed to convert frontmatter for {}", dest.display()))?;

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

        convert_or_copy_markdown(&src, &dest).unwrap();
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

                ::: callout { type=info title="Note" open=false }
                Body
                :::
            "#}
        );
    }

    #[test]
    fn convert_markdown_file_no_frontmatter_copies_as_is() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("raw.md");
        let dest = dir.path().join("out.md");

        fs::write(&src, "No frontmatter here\n").unwrap();

        convert_or_copy_markdown(&src, &dest).unwrap();
        assert_eq!(fs::read_to_string(&dest).unwrap(), "No frontmatter here\n");
    }

    #[test]
    fn convert_markdown_file_unreadable_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("missing.md");
        let dest = dir.path().join("output.md");

        let err = convert_or_copy_markdown(&src, &dest).unwrap_err();
        assert!(err.to_string().contains("failed to read"), "got: {err}");
    }

    #[test]
    fn convert_markdown_file_invalid_yaml_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("bad.md");
        let dest = dir.path().join("output.md");

        fs::write(
            &src,
            indoc! {"
                ---
                :
                  invalid: [yaml
                ---
                Body
            "},
        )
        .unwrap();

        let err = convert_or_copy_markdown(&src, &dest).unwrap_err();
        assert!(
            err.to_string().contains("failed to convert frontmatter"),
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

    #[test]
    fn convert_category_index_to_section_index() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        let dest = dir.path().join("dest");

        // Category _index.md → should become section index at posts/<slug>/.
        let cat_dir = source.join("categories/anime");
        fs::create_dir_all(&cat_dir).unwrap();
        fs::write(
            cat_dir.join("_index.md"),
            indoc! {r"
                ---
                title: 动画
                ---
            "},
        )
        .unwrap();

        // Tag _index.md → should be converted in place.
        let tag_dir = source.join("tags/rust");
        fs::create_dir_all(&tag_dir).unwrap();
        fs::write(
            tag_dir.join("_index.md"),
            indoc! {r"
                ---
                title: Rust
                ---
            "},
        )
        .unwrap();

        // Section _index.md → should be skipped.
        fs::create_dir_all(source.join("posts")).unwrap();
        fs::write(
            source.join("posts/_index.md"),
            indoc! {r"
                ---
                title: Posts
                ---
            "},
        )
        .unwrap();

        convert(&source, &dest).unwrap();

        // Category index redirected to section index.
        let section = fs::read_to_string(dest.join("posts/anime/_index.md")).unwrap();
        assert_eq!(
            section,
            indoc! {r#"
                +++
                title = "动画"
                +++
            "#}
        );
        assert!(
            !dest.join("categories/anime/_index.md").exists(),
            "should NOT create categories/ output"
        );

        // Tag term converted in place.
        let tag = fs::read_to_string(dest.join("tags/rust/_index.md")).unwrap();
        assert_eq!(
            tag,
            indoc! {r#"
                +++
                title = "Rust"
                +++
            "#}
        );

        // Section _index.md still skipped.
        assert!(!dest.join("posts/_index.md").exists());
    }

    #[test]
    fn convert_does_not_overwrite_existing_files() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        let dest = dir.path().join("dest");

        // Source markdown + asset.
        let post_dir = source.join("posts/hello");
        fs::create_dir_all(&post_dir).unwrap();
        fs::write(
            post_dir.join("index.md"),
            indoc! {r"
                ---
                title: New
                ---
                New content
            "},
        )
        .unwrap();
        fs::write(post_dir.join("image.webp"), "new-image").unwrap();

        // Pre-existing files at dest with different content.
        let dest_post_dir = dest.join("posts/hello");
        fs::create_dir_all(&dest_post_dir).unwrap();
        fs::write(dest_post_dir.join("index.md"), "existing markdown").unwrap();
        fs::write(dest_post_dir.join("image.webp"), "existing image").unwrap();

        convert(&source, &dest).unwrap();

        // Neither markdown nor asset should be overwritten.
        assert_eq!(
            fs::read_to_string(dest.join("posts/hello/index.md")).unwrap(),
            "existing markdown",
            "should not overwrite existing markdown"
        );
        assert_eq!(
            fs::read_to_string(dest.join("posts/hello/image.webp")).unwrap(),
            "existing image",
            "should not overwrite existing asset"
        );
    }
}
