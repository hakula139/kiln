mod frontmatter;
mod shortcode;

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use walkdir::WalkDir;

use crate::content::is_markdown;

/// Converts a Hugo site root to kiln format.
///
/// Converts `source/content` into `dest/content` and copies any `source/static` to `dest/static`.
///
/// Hugo category index files (`categories/<slug>/_index.md`) are converted to kiln section indexes
/// at `posts/<slug>/_index.md`. Tag index files (`tags/<slug>/_index.md`) are converted in place.
/// Other `_index.md` files (Hugo section files) are skipped since kiln derives sections from
/// directory structure.
///
/// Existing files in `dest` are never overwritten.
///
/// # Errors
///
/// Returns an error if `source/content` is missing or any file fails to read, convert, or write.
pub fn convert(source: &Path, dest: &Path) -> Result<()> {
    let content_source = source.join("content");
    let content_dest = dest.join("content");
    ensure!(
        content_source.is_dir(),
        "convert source must contain content/: {}",
        source.display()
    );

    let source = fs::canonicalize(source).context("failed to resolve conversion source")?;
    let dest = resolve_destination(dest)?;
    ensure!(
        !source.starts_with(&dest) && !dest.starts_with(&source),
        "conversion source and destination must not overlap: {} and {}",
        source.display(),
        dest.display()
    );

    for directory in ["content", "static"] {
        ensure!(
            resolve_destination(&dest.join(directory))?.starts_with(&dest),
            "conversion destination escapes its root: {}",
            dest.join(directory).display()
        );
    }

    convert_tree(&content_source, &content_dest, true)?;
    let static_source = source.join("static");
    if static_source.is_dir() {
        convert_tree(&static_source, &dest.join("static"), false)?;
    }
    Ok(())
}

fn resolve_destination(path: &Path) -> Result<PathBuf> {
    if fs::symlink_metadata(path).is_ok() {
        return fs::canonicalize(path)
            .with_context(|| format!("failed to resolve {}", path.display()));
    }

    let absolute = std::path::absolute(path)?;
    let parent = absolute.parent().context("destination has no parent")?;
    let name = absolute
        .file_name()
        .context("destination has no file name")?;
    Ok(resolve_destination(parent)?.join(name))
}

fn convert_tree(source: &Path, dest: &Path, markdown: bool) -> Result<()> {
    for entry in WalkDir::new(source).follow_links(false) {
        let entry = entry?;
        if entry.file_type().is_dir() {
            continue;
        }

        let relative = entry.path().strip_prefix(source)?;
        let destination = if markdown
            && is_markdown(relative)
            && relative.file_stem().is_some_and(|stem| stem == "_index")
        {
            let Some(path) = index_dest_path(relative, dest) else {
                tracing::warn!(path = %entry.path().display(), "section index requires manual migration and was omitted");
                continue;
            };
            path
        } else {
            dest.join(relative)
        };
        if fs::symlink_metadata(&destination).is_ok() {
            continue;
        }
        let resolved = resolve_destination(&destination)?;
        ensure!(
            resolved.starts_with(resolve_destination(dest)?),
            "conversion destination escapes its root: {}",
            destination.display()
        );
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }

        if markdown && is_markdown(relative) {
            convert_or_copy_markdown(entry.path(), &destination)?;
        } else {
            fs::copy(entry.path(), &destination)?;
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
    if components.len() != 3 {
        return None;
    }
    let kind = components[0].as_os_str().to_str().unwrap_or("");
    let slug = components[1].as_os_str();
    match kind {
        "categories" => Some(dest.join("posts").join(slug).join("_index.md")),
        "tags" => Some(dest.join("tags").join(slug).join("_index.md")),
        _ => None,
    }
}

/// Converts a markdown file if it has YAML frontmatter, otherwise copies it as-is.
/// Frontmatter-less `.md` files (e.g. page bundle resources) are not convertible.
fn convert_or_copy_markdown(src: &Path, dest: &Path) -> Result<()> {
    let content =
        fs::read_to_string(src).with_context(|| format!("failed to read {}", src.display()))?;

    if content.trim_start_matches('\u{feff}').starts_with("---") {
        let (yaml_fm, body) = frontmatter::split_yaml_frontmatter(&content)
            .with_context(|| format!("malformed YAML frontmatter in {}", src.display()))?;
        convert_markdown_file(yaml_fm, body, dest)
    } else {
        fs::copy(src, dest)?;
        Ok(())
    }
}

fn convert_markdown_file(yaml_fm: &str, body: &str, dest: &Path) -> Result<()> {
    let (toml_fm, unsupported) = frontmatter::convert_frontmatter(yaml_fm)
        .with_context(|| format!("failed to convert frontmatter for {}", dest.display()))?;

    for field in unsupported {
        tracing::warn!(path = %dest.display(), field, "frontmatter field requires manual migration and was omitted");
    }

    let converted_body = shortcode::convert_shortcodes(body)
        .with_context(|| format!("failed to convert shortcodes for {}", dest.display()))?;

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

    // ── convert ──

    #[test]
    fn convert_directory_structure() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        let dest = dir.path().join("dest");
        let content_source = source.join("content");

        let bundle = content_source.join("posts/my-post");
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

        fs::create_dir_all(content_source.join("pages")).unwrap();
        fs::write(
            content_source.join("pages/about.md"),
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
            content_source.join("posts/_index.md"),
            indoc! {r"
                ---
                title: Section
                ---
            "},
        )
        .unwrap();

        fs::create_dir_all(source.join("static/images")).unwrap();
        fs::write(source.join("static/images/logo.webp"), "site-image").unwrap();

        convert(&source, &dest).unwrap();

        let post = fs::read_to_string(dest.join("content/posts/my-post/index.md")).unwrap();
        assert_eq!(
            post,
            indoc! {r#"
                +++
                title = "Post"
                +++
                Content
            "#}
        );

        assert!(dest.join("content/posts/my-post/image.webp").exists());
        assert_eq!(
            fs::read_to_string(dest.join("static/images/logo.webp")).unwrap(),
            "site-image"
        );

        let about = fs::read_to_string(dest.join("content/pages/about.md")).unwrap();
        assert_eq!(
            about,
            indoc! {r#"
                +++
                title = "About"
                +++
                About page
            "#}
        );

        assert!(!dest.join("content/posts/_index.md").exists());
    }

    #[test]
    fn convert_category_index_to_section_index() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        let dest = dir.path().join("dest");
        let content_source = source.join("content");

        // Category _index.md → should become section index at posts/<slug>/.
        let cat_dir = content_source.join("categories/anime");
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
        let tag_dir = content_source.join("tags/rust");
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
        fs::create_dir_all(content_source.join("posts")).unwrap();
        fs::write(
            content_source.join("posts/_index.md"),
            indoc! {r"
                ---
                title: Posts
                ---
            "},
        )
        .unwrap();

        // Unknown kind _index.md → should be skipped.
        let other_dir = content_source.join("other/slug");
        fs::create_dir_all(&other_dir).unwrap();
        fs::write(
            other_dir.join("_index.md"),
            indoc! {r"
                ---
                title: Other
                ---
            "},
        )
        .unwrap();

        convert(&source, &dest).unwrap();

        let section = fs::read_to_string(dest.join("content/posts/anime/_index.md")).unwrap();
        assert_eq!(
            section,
            indoc! {r#"
                +++
                title = "动画"
                +++
            "#}
        );
        assert!(!dest.join("content/categories/anime/_index.md").exists());

        let tag = fs::read_to_string(dest.join("content/tags/rust/_index.md")).unwrap();
        assert_eq!(
            tag,
            indoc! {r#"
                +++
                title = "Rust"
                +++
            "#}
        );

        assert!(!dest.join("content/posts/_index.md").exists());

        assert!(!dest.join("content/other/slug/_index.md").exists());
    }

    #[test]
    fn convert_does_not_overwrite_existing_files() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        let dest = dir.path().join("dest");
        let content_source = source.join("content");

        let post_dir = content_source.join("posts/hello");
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

        let dest_post_dir = dest.join("content/posts/hello");
        fs::create_dir_all(&dest_post_dir).unwrap();
        fs::write(dest_post_dir.join("index.md"), "existing markdown").unwrap();
        fs::write(dest_post_dir.join("image.webp"), "existing image").unwrap();

        for (source_path, dest_path, source_content, dest_content) in [
            (
                "categories/topic/_index.md",
                "posts/topic/_index.md",
                indoc! {r"
                    ---
                    title: Source category
                    ---
                "},
                "Existing section",
            ),
            (
                "tags/topic/_index.md",
                "tags/topic/_index.md",
                indoc! {r"
                    ---
                    title: Source tag
                    ---
                "},
                "Existing tag",
            ),
        ] {
            let source_index = content_source.join(source_path);
            let dest_index = dest.join("content").join(dest_path);
            fs::create_dir_all(source_index.parent().unwrap()).unwrap();
            fs::create_dir_all(dest_index.parent().unwrap()).unwrap();
            fs::write(source_index, source_content).unwrap();
            fs::write(dest_index, dest_content).unwrap();
        }

        convert(&source, &dest).unwrap();

        assert_eq!(
            fs::read_to_string(dest.join("content/posts/hello/index.md")).unwrap(),
            "existing markdown",
            "should not overwrite existing markdown"
        );
        assert_eq!(
            fs::read_to_string(dest.join("content/posts/hello/image.webp")).unwrap(),
            "existing image",
            "should not overwrite existing asset"
        );
        assert_eq!(
            fs::read_to_string(dest.join("content/posts/topic/_index.md")).unwrap(),
            "Existing section"
        );
        assert_eq!(
            fs::read_to_string(dest.join("content/tags/topic/_index.md")).unwrap(),
            "Existing tag"
        );
    }

    #[test]
    fn convert_does_not_overwrite_existing_static_files() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        let dest = dir.path().join("dest");

        fs::create_dir_all(source.join("content")).unwrap();
        fs::create_dir_all(source.join("static/images")).unwrap();
        fs::create_dir_all(dest.join("static/images")).unwrap();
        fs::write(source.join("static/images/logo.webp"), "new static").unwrap();
        fs::write(dest.join("static/images/logo.webp"), "existing static").unwrap();

        convert(&source, &dest).unwrap();

        assert_eq!(
            fs::read_to_string(dest.join("static/images/logo.webp")).unwrap(),
            "existing static",
            "should not overwrite existing static asset"
        );
    }

    #[test]
    fn convert_missing_content_dir_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        let dest = dir.path().join("dest");

        fs::create_dir_all(&source).unwrap();

        let err = convert(&source, &dest).unwrap_err();
        assert!(
            err.to_string()
                .contains("convert source must contain content/"),
            "got: {err}"
        );
    }

    // ── convert_tree ──

    #[test]
    fn convert_tree_copies_files() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        let dest = dir.path().join("dest");

        fs::create_dir_all(source.join("images/icons")).unwrap();
        fs::write(source.join("images/icons/logo.webp"), "site-image").unwrap();

        convert_tree(&source, &dest, false).unwrap();

        assert_eq!(
            fs::read_to_string(dest.join("images/icons/logo.webp")).unwrap(),
            "site-image"
        );
    }

    #[test]
    fn convert_tree_does_not_overwrite_existing_files() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        let dest = dir.path().join("dest");

        fs::create_dir_all(source.join("images")).unwrap();
        fs::create_dir_all(dest.join("images")).unwrap();
        fs::write(source.join("images/logo.webp"), "new static").unwrap();
        fs::write(dest.join("images/logo.webp"), "existing static").unwrap();

        convert_tree(&source, &dest, false).unwrap();

        assert_eq!(
            fs::read_to_string(dest.join("images/logo.webp")).unwrap(),
            "existing static"
        );
    }

    // ── index_dest_path ──

    #[test]
    fn index_dest_path_categories_returns_posts_section_path() {
        let dest = Path::new("/tmp/dest");

        assert_eq!(
            index_dest_path(Path::new("categories/anime/_index.md"), dest),
            Some(dest.join("posts/anime/_index.md"))
        );
    }

    #[test]
    fn index_dest_path_tags_returns_same_relative_path() {
        let dest = Path::new("/tmp/dest");

        assert_eq!(
            index_dest_path(Path::new("tags/rust/_index.md"), dest),
            Some(dest.join("tags/rust/_index.md"))
        );
    }

    #[test]
    fn index_dest_path_non_term_layout_returns_none() {
        let dest = Path::new("/tmp/dest");

        assert_eq!(index_dest_path(Path::new("posts/_index.md"), dest), None);
    }

    #[test]
    fn index_dest_path_unknown_kind_returns_none() {
        let dest = Path::new("/tmp/dest");

        assert_eq!(
            index_dest_path(Path::new("series/rust/_index.md"), dest),
            None
        );
    }

    // ── convert_or_copy_markdown ──

    #[test]
    fn convert_or_copy_markdown_converts_yaml_frontmatter() {
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

                ::: callout {type=info title="Note" open=false}
                Body
                :::
            "#}
        );
    }

    #[test]
    fn convert_or_copy_markdown_no_frontmatter_copies_as_is() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("raw.md");
        let dest = dir.path().join("out.md");

        fs::write(&src, "No frontmatter here\n").unwrap();

        convert_or_copy_markdown(&src, &dest).unwrap();
        assert_eq!(fs::read_to_string(&dest).unwrap(), "No frontmatter here\n");
    }

    #[test]
    fn convert_or_copy_markdown_unreadable_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("missing.md");
        let dest = dir.path().join("output.md");

        let err = convert_or_copy_markdown(&src, &dest).unwrap_err();
        assert!(err.to_string().contains("failed to read"), "got: {err}");
    }

    #[test]
    fn convert_or_copy_markdown_invalid_yaml_returns_error() {
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

    // ── convert_markdown_file ──

    #[test]
    fn convert_markdown_file_basic() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("output.md");

        convert_markdown_file(
            indoc! {r"
                title: Hello, world!
                tags: [rust]
            "},
            indoc! {r"
                Summary

                <!--more-->

                Full content
            "},
            &dest,
        )
        .unwrap();

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
            "#}
        );
    }

    #[test]
    fn convert_markdown_file_invalid_yaml_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("output.md");

        let err = convert_markdown_file(":\n  invalid: [yaml", "Body\n", &dest).unwrap_err();
        assert!(
            err.to_string().contains("failed to convert frontmatter"),
            "got: {err}"
        );
    }
}
