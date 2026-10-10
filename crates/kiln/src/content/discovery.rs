use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use walkdir::WalkDir;

use super::page::{Page, derive_page_kind};

/// All content discovered from the content directory.
#[derive(Debug)]
pub struct ContentSet {
    pub pages: Vec<Page>,
    pub content_dir: PathBuf,
}

/// Walks the content directory, loading all non-draft markdown pages.
///
/// Returns an empty collection when there is no content directory.
///
/// Excludes:
/// - Files and directories whose names start with `_`
/// - Non-markdown files
/// - Markdown files without `+++` frontmatter (e.g., CLAUDE.md, README.md)
/// - Pages with `draft = true` in frontmatter
///
/// # Errors
///
/// Returns an error if a content entry or markdown file cannot be read, or if frontmatter is
/// invalid, including in draft pages.
pub fn discover_content(root: &Path) -> Result<ContentSet> {
    let content_dir = root.join("content");
    if !content_dir.is_dir() {
        return Ok(ContentSet {
            pages: Vec::new(),
            content_dir,
        });
    }

    let mut pages = Vec::new();

    for entry in WalkDir::new(&content_dir)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| entry.depth() == 0 || !super::is_private(entry.file_name()))
    {
        let entry =
            entry.with_context(|| format!("failed to read entry in {}", content_dir.display()))?;

        if !entry.file_type().is_file() {
            continue;
        }

        let path = entry.path();
        if !super::is_markdown(path) {
            continue;
        }

        let content = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        if has_frontmatter(&content) {
            let mut page = Page::from_content_with_assets(&content, path)?;
            if !page.frontmatter.draft {
                page.kind = derive_page_kind(&page.source_path, &content_dir);
                pages.push(page);
            }
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

/// Returns `true` if content starts with a `+++` frontmatter delimiter (optionally preceded
/// by a UTF-8 BOM). Files without frontmatter are skipped during discovery.
fn has_frontmatter(content: &str) -> bool {
    let content = content.strip_prefix('\u{feff}').unwrap_or(content);
    content.starts_with("+++")
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::*;
    use crate::content::page::PageKind;
    use crate::test_utils::write_test_file;

    // ── discover_content ──

    #[test]
    fn discover_content_preserves_bundle_assets() {
        let root = tempfile::tempdir().unwrap();
        write_test_file(
            root.path(),
            "content/posts/example/index.md",
            indoc! {r#"
                +++
                title = "Example"
                +++
                Body
            "#},
        );
        write_test_file(
            root.path(),
            "content/posts/example/images/photo.png",
            "image",
        );

        let set = discover_content(root.path()).unwrap();
        assert_eq!(set.pages[0].frontmatter.title, "Example");
        assert_eq!(
            set.pages[0].assets,
            [root.path().join("content/posts/example/images/photo.png")],
        );
    }

    #[test]
    fn discover_content_excludes_drafts_private_paths_and_non_content_files() {
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
    fn discover_content_sorts_dated_pages_before_undated_pages() {
        let root = tempfile::tempdir().unwrap();
        write_test_file(
            root.path(),
            "content/posts/old/index.md",
            indoc! {r#"
                +++
                title = "Old"
                date = "2023-12-01T00:00:00Z"
                +++
                Body
            "#},
        );
        write_test_file(
            root.path(),
            "content/posts/new/index.md",
            indoc! {r#"
                +++
                title = "New"
                date = "2024-06-01T00:00:00Z"
                +++
                Body
            "#},
        );

        for (path, contents) in [
            (
                "content/posts/beta/index.md",
                indoc! {r#"
                    +++
                    title = "Beta"
                    +++
                    Body
                "#},
            ),
            (
                "content/posts/alpha/index.md",
                indoc! {r#"
                    +++
                    title = "Alpha"
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
            ["New", "Old", "Alpha", "Beta"]
        );
    }

    #[test]
    fn discover_content_assigns_page_kind() {
        let root = tempfile::tempdir().unwrap();
        write_test_file(
            root.path(),
            "content/posts/note/sectioned/index.md",
            indoc! {r#"
                +++
                title = "Sectioned Post"
                +++
                Body
            "#},
        );
        write_test_file(
            root.path(),
            "content/posts/orphan/index.md",
            indoc! {r#"
                +++
                title = "Orphan Post"
                +++
                Body
            "#},
        );
        write_test_file(
            root.path(),
            "content/about-me/index.md",
            indoc! {r#"
                +++
                title = "About Me"
                +++
                Body
            "#},
        );

        let set = discover_content(root.path()).unwrap();
        assert_eq!(set.pages.len(), 3);

        let section_post = set
            .pages
            .iter()
            .find(|p| p.frontmatter.title == "Sectioned Post")
            .unwrap();
        assert_eq!(
            section_post.kind,
            PageKind::Post {
                section: Some("note".into())
            }
        );

        let orphan = set
            .pages
            .iter()
            .find(|p| p.frontmatter.title == "Orphan Post")
            .unwrap();
        assert_eq!(orphan.kind, PageKind::Post { section: None });

        let about = set
            .pages
            .iter()
            .find(|p| p.frontmatter.title == "About Me")
            .unwrap();
        assert_eq!(about.kind, PageKind::Page);
    }

    #[test]
    fn discover_content_missing_dir_returns_empty() {
        let root = tempfile::tempdir().unwrap();
        let set = discover_content(root.path()).unwrap();
        assert!(set.pages.is_empty());
    }

    #[test]
    fn discover_content_invalid_utf8_returns_error() {
        let root = tempfile::tempdir().unwrap();
        let content_dir = root.path().join("content");
        std::fs::create_dir(&content_dir).unwrap();
        let path = content_dir.join("broken.md");
        std::fs::write(&path, b"+++\n\xff").unwrap();

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
}
