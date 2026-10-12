mod frontmatter;
mod shortcode;

use std::collections::HashMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use walkdir::WalkDir;

use self::frontmatter::YAML_DELIMITER;
use crate::content::frontmatter::{
    TOML_DELIMITER, has_delimited_frontmatter, split_delimited_frontmatter,
};
use crate::content::is_markdown;
use crate::output::guard::canonicalize_via_parent;
use crate::output::{copy_file, paths_overlap, validate_source_link, write_output};

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
/// Returns an error if `source/content` is missing, any file fails to read, convert, or write, a
/// source link resolves into the destination, or two indexes map to the same destination. The
/// link and index checks run before anything in their tree is written.
pub fn convert(source: &Path, dest: &Path) -> Result<()> {
    ensure!(
        source.join("content").is_dir(),
        "convert source must contain content/: {}",
        source.display()
    );

    let source = fs::canonicalize(source).context("failed to resolve conversion source")?;
    let dest = canonicalize_via_parent(dest)?;
    ensure!(
        !paths_overlap(&source, &dest),
        "conversion source and destination must not overlap: {} and {}",
        source.display(),
        dest.display()
    );

    for directory in ["content", "static"] {
        ensure!(
            canonicalize_via_parent(&dest.join(directory))?.starts_with(&dest),
            "conversion destination escapes its root: {}",
            dest.join(directory).display()
        );
    }

    let content_source = source.join("content");
    convert_tree(&content_source, &dest.join("content"), true)?;
    let static_source = source.join("static");
    if static_source.is_dir() {
        convert_tree(&static_source, &dest.join("static"), false)?;
    }
    Ok(())
}

fn convert_tree(source: &Path, dest: &Path, markdown: bool) -> Result<()> {
    let resolved_dest = canonicalize_via_parent(dest)?;
    for (source_file, destination) in plan_tree(source, dest, &resolved_dest, markdown)? {
        if fs::symlink_metadata(&destination).is_ok() {
            continue;
        }
        ensure!(
            canonicalize_via_parent(&destination)?.starts_with(&resolved_dest),
            "conversion destination escapes its root: {}",
            destination.display()
        );

        if markdown && is_markdown(&source_file) {
            convert_or_copy_markdown(&source_file, &destination)?;
        } else {
            copy_file(&source_file, &destination)?;
        }
    }
    Ok(())
}

/// Pairs each file under `source`, following symlinks, with its destination under `dest`, in
/// file name order.
///
/// # Errors
///
/// Returns an error if the walk fails, a source link overlaps `resolved_dest`, or two indexes
/// convert to the same destination.
fn plan_tree(
    source: &Path,
    dest: &Path,
    resolved_dest: &Path,
    markdown: bool,
) -> Result<Vec<(PathBuf, PathBuf)>> {
    let mut files = Vec::new();
    let mut index_sources = HashMap::new();
    for entry in WalkDir::new(source).follow_links(true).sort_by_file_name() {
        let entry =
            entry.with_context(|| format!("failed to read entry in {}", source.display()))?;
        if entry.path_is_symlink() {
            validate_source_link(entry.path(), resolved_dest)?;
        }
        if entry.file_type().is_dir() {
            continue;
        }

        let relative = entry.path().strip_prefix(source)?;
        let destination = if markdown
            && is_markdown(relative)
            && relative.file_stem().is_some_and(|stem| stem == "_index")
        {
            let Some(index) = index_destination(relative) else {
                tracing::warn!(
                    path = %entry.path().display(),
                    "section index requires manual migration and was omitted"
                );
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
        files.push((entry.into_path(), destination));
    }
    Ok(files)
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
        [kind, slug, _] if kind == "categories" => {
            Some(Path::new("posts").join(slug).join("_index.md"))
        }
        _ => None,
    }
}

/// Converts a Markdown file with YAML frontmatter, or copies it unchanged.
fn convert_or_copy_markdown(src: &Path, dest: &Path) -> Result<()> {
    let content =
        fs::read_to_string(src).with_context(|| format!("failed to read {}", src.display()))?;

    if has_delimited_frontmatter(&content, YAML_DELIMITER) {
        let (yaml_fm, body) = split_delimited_frontmatter(&content, YAML_DELIMITER)
            .with_context(|| format!("malformed YAML frontmatter in {}", src.display()))?;
        convert_markdown_file(yaml_fm, body, dest)
    } else {
        copy_file(src, dest)
    }
}

fn convert_markdown_file(yaml_fm: &str, body: &str, dest: &Path) -> Result<()> {
    let _span = tracing::warn_span!("convert", path = %dest.display()).entered();
    let (toml_fm, unsupported) = frontmatter::convert_frontmatter(yaml_fm)
        .with_context(|| format!("failed to convert frontmatter for {}", dest.display()))?;

    for field in unsupported {
        tracing::warn!(
            field,
            "frontmatter field requires manual migration and was omitted"
        );
    }

    let converted_body = shortcode::convert_shortcodes(body)
        .with_context(|| format!("failed to convert shortcodes for {}", dest.display()))?;

    write_output(
        dest,
        &format!("{TOML_DELIMITER}\n{toml_fm}{TOML_DELIMITER}\n{converted_body}"),
    )
}

#[cfg(test)]
mod tests {
    use indoc::{formatdoc, indoc};

    use super::*;

    // ── convert ──

    #[test]
    fn converts_content_and_copies_static_files() {
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
                aliases: [/old-post/]
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
    fn resolves_parent_components_of_missing_destination() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        fs::create_dir_all(source.join("content")).unwrap();
        fs::write(source.join("content/note.txt"), "note").unwrap();

        convert(&source, &dir.path().join("missing/../out")).unwrap();

        assert_eq!(
            fs::read_to_string(dir.path().join("out/content/note.txt")).unwrap(),
            "note"
        );
        assert!(!dir.path().join("missing").exists());
    }

    #[test]
    fn maps_indexes_to_archive_title_paths() {
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
                formatdoc! {r"
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
    fn keeps_existing_destination_files() {
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
    fn rejects_colliding_indexes_before_writing() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        let dest = dir.path().join("dest");
        let content_source = source.join("content");

        for path in [
            "about.md",
            "posts/anime/_index.md",
            "categories/anime/_index.md",
        ] {
            let file = content_source.join(path);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(
                file,
                indoc! {r"
                    ---
                    title: Anime
                    ---
                "},
            )
            .unwrap();
        }

        let error = convert(&source, &dest).unwrap_err();
        let section = Path::new("posts").join("anime").join("_index.md");
        assert_eq!(
            error.to_string(),
            format!(
                "{} and {} both convert to {}",
                Path::new("categories")
                    .join("anime")
                    .join("_index.md")
                    .display(),
                section.display(),
                section.display()
            )
        );
        assert!(!dest.exists());
    }

    #[test]
    fn requires_source_content_dir() {
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

    #[test]
    fn reports_shortcode_failures_with_destination() {
        let dir = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        let source = root.join("source");
        let dest = root.join("dest");
        fs::create_dir_all(source.join("content")).unwrap();
        fs::write(
            source.join("content/styled.md"),
            indoc! {"
                ---
                title: Styled
                ---
                {{< style >}}body{{< /style >}}
            "},
        )
        .unwrap();

        let error = convert(&source, &dest).unwrap_err();
        assert_eq!(
            format!("{error:#}"),
            format!(
                "failed to convert shortcodes for {}: style shortcode requires manual conversion to page CSS",
                dest.join("content").join("styled.md").display()
            )
        );
    }

    #[cfg(unix)]
    #[test]
    fn reports_unreadable_assets() {
        use crate::test_utils::PermissionGuard;

        let dir = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        let source = root.join("source");
        let image = source.join("content/image.webp");
        fs::create_dir_all(image.parent().unwrap()).unwrap();
        fs::write(&image, "image").unwrap();
        let _guard = PermissionGuard::restrict(&image, 0o000);

        let error = convert(&source, &root.join("dest")).unwrap_err();
        assert_eq!(
            error.to_string(),
            format!(
                "failed to copy {} to {}",
                image.display(),
                root.join("dest/content/image.webp").display()
            )
        );
    }

    // ── convert_or_copy_markdown ──

    #[test]
    fn converts_yaml_frontmatter_and_shortcodes() {
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
    fn copies_markdown_without_frontmatter() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("raw.md");
        let dest = dir.path().join("out.md");

        fs::write(&src, "No frontmatter here\n").unwrap();

        convert_or_copy_markdown(&src, &dest).unwrap();
        assert_eq!(fs::read_to_string(&dest).unwrap(), "No frontmatter here\n");
    }

    #[test]
    fn reports_unreadable_source() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("missing.md");
        let dest = dir.path().join("output.md");

        let err = convert_or_copy_markdown(&src, &dest).unwrap_err();
        assert_eq!(err.to_string(), format!("failed to read {}", src.display()));
    }

    #[test]
    fn reports_invalid_yaml() {
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
