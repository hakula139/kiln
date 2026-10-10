mod frontmatter;
mod shortcode;

use std::collections::HashMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use walkdir::WalkDir;

use crate::content::is_markdown;

/// Converts a Hugo site root to kiln format.
///
/// Converts `source/content` into `dest/content` and copies any `source/static` to `dest/static`.
///
/// `_index.md` files are kept where kiln reads archive titles: `posts/`, `posts/<slug>/` and
/// `tags/<slug>/`. Hugo category indexes (`categories/<slug>/_index.md`) become section indexes
/// at `posts/<slug>/_index.md`. Other `_index.md` files are skipped with a warning.
///
/// Existing files in `dest` are never overwritten.
///
/// # Errors
///
/// Returns an error if `source/content` is missing, two indexes map to the same destination, or
/// any file fails to read, convert, or write.
pub fn convert(source: &Path, dest: &Path) -> Result<()> {
    ensure!(
        source.join("content").is_dir(),
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

    convert_tree(&source.join("content"), &dest.join("content"), true)?;
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
    let resolved_dest = resolve_destination(dest)?;
    let mut index_sources = HashMap::new();
    for entry in WalkDir::new(source).follow_links(false) {
        let entry = entry?;
        if entry.file_type().is_dir() {
            continue;
        }

        let relative = entry.path().strip_prefix(source)?;
        let markdown_file = markdown && is_markdown(relative);
        let destination = if markdown_file
            && relative.file_stem().is_some_and(|stem| stem == "_index")
        {
            let Some(index) = index_destination(relative) else {
                tracing::warn!(path = %entry.path().display(), "section index requires manual migration and was omitted");
                continue;
            };
            if let Some(previous) = index_sources.insert(index.clone(), relative.to_owned()) {
                bail!(
                    "{} and {} both convert to {}",
                    previous.display(),
                    relative.display(),
                    index.display()
                );
            }
            dest.join(index)
        } else {
            dest.join(relative)
        };
        if fs::symlink_metadata(&destination).is_ok() {
            continue;
        }
        ensure!(
            resolve_destination(&destination)?.starts_with(&resolved_dest),
            "conversion destination escapes its root: {}",
            destination.display()
        );
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }

        if markdown_file {
            convert_or_copy_markdown(entry.path(), &destination)?;
        } else {
            fs::copy(entry.path(), &destination)?;
        }
    }
    Ok(())
}

/// Returns the content-relative destination of an `_index.md` file, or `None` to skip it.
///
/// - `posts/_index.md`, `posts/<slug>/_index.md` and `tags/<slug>/_index.md` → unchanged
/// - `categories/<slug>/_index.md` → `posts/<slug>/_index.md` (section index)
fn index_destination(relative: &Path) -> Option<PathBuf> {
    let components: Vec<_> = relative.components().map(Component::as_os_str).collect();
    match components[..] {
        [kind, _] if kind == "posts" => Some(relative.to_owned()),
        [kind, _, _] if kind == "posts" || kind == "tags" => Some(relative.to_owned()),
        [kind, slug, file] if kind == "categories" => {
            Some(Path::new("posts").join(slug).join(file))
        }
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

    let output = format!("+++\n{toml_fm}+++\n{converted_body}");
    fs::write(dest, output).with_context(|| format!("failed to write {}", dest.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use indoc::{formatdoc, indoc};

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

        assert_eq!(
            fs::read_to_string(dest.join("content/posts/my-post/image.webp")).unwrap(),
            "fake-image"
        );
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
    }

    #[test]
    fn convert_maps_indexes_to_archive_title_paths() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        let dest = dir.path().join("dest");
        let content_source = source.join("content");

        for (path, title) in [
            ("posts/_index.md", "Posts"),
            ("posts/notes/_index.md", "Notes"),
            ("categories/anime/_index.md", "动画"),
            ("tags/rust/_index.md", "Rust"),
            ("other/slug/_index.md", "Other"),
            ("posts/notes/deeper/_index.md", "Deeper"),
        ] {
            let index = content_source.join(path);
            fs::create_dir_all(index.parent().unwrap()).unwrap();
            fs::write(
                index,
                formatdoc! {"
                    ---
                    title: {title}
                    ---
                "},
            )
            .unwrap();
        }

        convert(&source, &dest).unwrap();

        for (path, title) in [
            ("posts/_index.md", "Posts"),
            ("posts/notes/_index.md", "Notes"),
            ("posts/anime/_index.md", "动画"),
            ("tags/rust/_index.md", "Rust"),
        ] {
            assert_eq!(
                fs::read_to_string(dest.join("content").join(path)).unwrap(),
                formatdoc! {r#"
                    +++
                    title = "{title}"
                    +++
                "#},
                "{path}"
            );
        }
        for path in [
            "categories/anime/_index.md",
            "other/slug/_index.md",
            "posts/notes/deeper/_index.md",
        ] {
            assert!(!dest.join("content").join(path).exists(), "{path}");
        }
    }

    #[test]
    fn convert_colliding_section_indexes_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        for path in ["posts/anime/_index.md", "categories/anime/_index.md"] {
            let index = source.join("content").join(path);
            fs::create_dir_all(index.parent().unwrap()).unwrap();
            fs::write(
                index,
                indoc! {"
                    ---
                    title: Anime
                    ---
                "},
            )
            .unwrap();
        }

        let error = convert(&source, &dir.path().join("dest")).unwrap_err();
        assert!(
            error.to_string().ends_with(&format!(
                "both convert to {}",
                Path::new("posts/anime/_index.md").display()
            )),
            "{error}"
        );
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
        fs::create_dir_all(source.join("static")).unwrap();
        fs::create_dir_all(dest.join("static")).unwrap();
        fs::write(source.join("static/logo.webp"), "new static").unwrap();
        fs::write(dest.join("static/logo.webp"), "existing static").unwrap();

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
        assert_eq!(
            fs::read_to_string(dest.join("static/logo.webp")).unwrap(),
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
}
