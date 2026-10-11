use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::frontmatter::{TOML_DELIMITER, has_delimited_frontmatter};
use super::page::Page;
use super::{is_markdown, is_public};
use crate::output::walk_directory;

/// All content discovered from the content directory.
#[derive(Debug)]
pub struct ContentSet {
    pub pages: Vec<Page>,
    pub content_dir: PathBuf,
}

/// Walks the content directory, loading all non-draft markdown pages.
///
/// Returns an empty collection when there is no content directory. Symlinked files and
/// directories are followed like any other source tree.
///
/// Excludes:
/// - Files and directories whose names start with `_`
/// - Non-markdown files
/// - Markdown files without `+++` frontmatter (e.g., CLAUDE.md, README.md)
/// - Pages with `draft = true` in frontmatter
///
/// # Errors
///
/// Returns an error if a content entry or markdown file cannot be read, including broken or
/// cyclic public symlinks, or if frontmatter is invalid, including in draft pages.
pub fn discover_content(root: &Path) -> Result<ContentSet> {
    let content_dir = root.join("content");
    if !content_dir.is_dir() {
        return Ok(ContentSet {
            pages: Vec::new(),
            content_dir,
        });
    }

    let mut pages = Vec::new();

    for entry in walk_directory(&content_dir, &is_public) {
        let entry =
            entry.with_context(|| format!("failed to read entry in {}", content_dir.display()))?;

        if !entry.file_type().is_file() {
            continue;
        }

        let path = entry.path();
        if !is_markdown(path) {
            continue;
        }

        let content = fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        if !has_delimited_frontmatter(&content, TOML_DELIMITER) {
            continue;
        }
        let page = Page::load(&content, path, &content_dir)?;
        if !page.frontmatter.draft {
            pages.push(page);
        }
    }

    // Undated pages sort last because `None < Some`. The source-path tiebreak keeps output
    // deterministic across platforms.
    pages.sort_by(|a, b| {
        b.frontmatter
            .date
            .cmp(&a.frontmatter.date)
            .then_with(|| a.source_path.cmp(&b.source_path))
    });

    Ok(ContentSet { pages, content_dir })
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use std::os::unix::fs::symlink;

    use indoc::{formatdoc, indoc};

    use super::*;
    use crate::test_utils::write_test_file;

    // ── discover_content ──

    #[test]
    fn excludes_drafts_private_paths_and_non_content_files() {
        let root = tempfile::tempdir().unwrap();
        for (path, contents) in [
            (
                "content/posts/published/index.md",
                indoc! {r#"
                    +++
                    title = "Published"
                    +++
                    Body
                "#},
            ),
            (
                "content/posts/draft/index.md",
                indoc! {r#"
                    +++
                    title = "Draft"
                    draft = true
                    +++
                    Body
                "#},
            ),
            (
                "content/posts/_hidden/index.md",
                indoc! {r#"
                    +++
                    title = "Hidden"
                    +++
                    Body
                "#},
            ),
            ("content/posts/published/notes.md", "# Notes"),
            (
                "content/posts/published/image.png",
                indoc! {r#"
                    +++
                    title = "Not Markdown"
                    +++
                    Body
                "#},
            ),
        ] {
            write_test_file(root.path(), path, contents);
        }

        let set = discover_content(root.path()).unwrap();
        assert_eq!(
            set.pages
                .iter()
                .map(|page| page.frontmatter.title.as_str())
                .collect::<Vec<_>>(),
            ["Published"]
        );
    }

    #[test]
    fn sorts_newest_first_then_undated_by_path() {
        let root = tempfile::tempdir().unwrap();
        for (slug, title, date) in [
            ("old", "Old", r#"date = "2023-12-01T00:00:00Z""#),
            ("new", "New", r#"date = "2024-06-01T00:00:00Z""#),
            ("beta", "Beta", ""),
            ("alpha", "Alpha", ""),
        ] {
            write_test_file(
                root.path(),
                &format!("content/posts/{slug}/index.md"),
                &formatdoc! {r#"
                    +++
                    title = "{title}"
                    {date}
                    +++
                    Body
                "#},
            );
        }

        let set = discover_content(root.path()).unwrap();
        assert_eq!(
            set.pages
                .iter()
                .map(|page| page.frontmatter.title.as_str())
                .collect::<Vec<_>>(),
            ["New", "Old", "Alpha", "Beta"]
        );
    }

    #[test]
    fn missing_content_dir_yields_no_pages() {
        let root = tempfile::tempdir().unwrap();
        let set = discover_content(root.path()).unwrap();
        assert!(set.pages.is_empty());
    }

    #[test]
    fn reports_invalid_utf8() {
        let root = tempfile::tempdir().unwrap();
        let content_dir = root.path().join("content");
        fs::create_dir(&content_dir).unwrap();
        let path = content_dir.join("broken.md");
        fs::write(&path, b"+++\n\xff").unwrap();

        let error = discover_content(root.path()).unwrap_err();
        assert_eq!(
            error.to_string(),
            format!("failed to read {}", path.display())
        );
        assert_eq!(
            error.downcast_ref::<std::io::Error>().unwrap().kind(),
            std::io::ErrorKind::InvalidData,
        );
    }

    #[test]
    fn reports_invalid_draft_page() {
        let root = tempfile::tempdir().unwrap();
        write_test_file(
            root.path(),
            "content/posts/draft.md",
            indoc! {r"
                +++
                draft = true
                unknown = 1
                +++
            "},
        );

        let error = discover_content(root.path()).unwrap_err();
        assert_eq!(
            error.to_string(),
            format!(
                "invalid frontmatter in {}",
                root.path()
                    .join("content")
                    .join("posts")
                    .join("draft.md")
                    .display()
            )
        );
    }

    #[cfg(unix)]
    #[test]
    fn follows_symlinks() {
        let root = tempfile::tempdir().unwrap();
        let external = tempfile::tempdir().unwrap();
        for (path, title) in [
            ("bundle/index.md", "Linked Bundle"),
            ("standalone.md", "Linked File"),
        ] {
            write_test_file(
                external.path(),
                path,
                &formatdoc! {r#"
                    +++
                    title = "{title}"
                    +++
                "#},
            );
        }
        write_test_file(external.path(), "bundle/image.png", "image");
        let posts = root.path().join("content/posts");
        fs::create_dir_all(&posts).unwrap();
        symlink(external.path().join("bundle"), posts.join("bundle")).unwrap();
        symlink(
            external.path().join("standalone.md"),
            posts.join("standalone.md"),
        )
        .unwrap();
        symlink(external.path().join("missing"), posts.join("_broken")).unwrap();

        let set = discover_content(root.path()).unwrap();

        assert_eq!(
            set.pages
                .iter()
                .map(|page| (page.frontmatter.title.as_str(), page.source_path.clone()))
                .collect::<Vec<_>>(),
            [
                ("Linked Bundle", posts.join("bundle/index.md")),
                ("Linked File", posts.join("standalone.md")),
            ]
        );
        assert_eq!(set.pages[0].assets, [posts.join("bundle/image.png")]);
    }

    #[cfg(unix)]
    #[test]
    fn reports_broken_symlink() {
        let root = tempfile::tempdir().unwrap();
        let content_dir = root.path().join("content");
        fs::create_dir(&content_dir).unwrap();
        let link = content_dir.join("broken.md");
        symlink(content_dir.join("missing.md"), &link).unwrap();

        let error = discover_content(root.path()).unwrap_err();
        assert_eq!(
            error.downcast_ref::<walkdir::Error>().unwrap().path(),
            Some(link.as_path())
        );
    }
}
