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
/// Excludes:
/// - Files and directories whose names start with `_`
/// - Non-markdown files
/// - Markdown files without `+++` frontmatter (e.g., CLAUDE.md, README.md)
/// - Pages with `draft = true` in frontmatter
///
/// # Errors
///
/// Returns an error if the content directory cannot be read, or if any
/// non-draft markdown file has invalid frontmatter.
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
        .filter_entry(|e| !is_excluded(e))
    {
        let entry =
            entry.with_context(|| format!("failed to read entry in {}", content_dir.display()))?;

        if !entry.file_type().is_file() {
            continue;
        }

        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "md") && has_frontmatter(path) {
            let mut page = Page::from_file(path)?;
            if !page.frontmatter.draft {
                page.kind = derive_page_kind(&page.source_path, &content_dir);
                pages.push(page);
            }
        }
    }

    // Sort by date descending (newest first), undated pages last.
    // Tiebreak by source path for deterministic output across platforms.
    pages.sort_by(|a, b| {
        b.frontmatter
            .date
            .cmp(&a.frontmatter.date)
            .then_with(|| a.source_path.cmp(&b.source_path))
    });

    Ok(ContentSet { pages, content_dir })
}

/// Returns `true` if the file starts with a `+++` frontmatter delimiter
/// (optionally preceded by a UTF-8 BOM).
///
/// Markdown files without frontmatter (e.g., CLAUDE.md, README.md) are
/// skipped during discovery rather than causing a parse error.
fn has_frontmatter(path: &Path) -> bool {
    std::fs::read_to_string(path).is_ok_and(|content| {
        let content = content.strip_prefix('\u{feff}').unwrap_or(&content);
        content.starts_with("+++")
    })
}

/// Returns `true` for entries whose file name starts with `_`.
fn is_excluded(entry: &walkdir::DirEntry) -> bool {
    entry
        .file_name()
        .to_str()
        .is_some_and(|name| name.starts_with('_'))
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::*;
    use crate::content::page::PageKind;
    use crate::test_utils::write_test_file;

    // ── discover_content ──

    #[test]
    fn discover_content_basic() {
        let root = tempfile::tempdir().unwrap();
        write_test_file(
            root.path(),
            "content/posts/hello/index.md",
            indoc! {r#"
                +++
                title = "Hello"
                +++
                Body
            "#},
        );
        write_test_file(
            root.path(),
            "content/posts/world/index.md",
            indoc! {r#"
                +++
                title = "World"
                +++
                Body
            "#},
        );

        let set = discover_content(root.path()).unwrap();
        assert_eq!(set.pages.len(), 2);
    }

    #[test]
    fn discover_content_excludes_drafts() {
        let root = tempfile::tempdir().unwrap();
        write_test_file(
            root.path(),
            "content/posts/draft/index.md",
            indoc! {r#"
                +++
                title = "Draft"
                draft = true
                +++
                Body
            "#},
        );
        write_test_file(
            root.path(),
            "content/posts/published/index.md",
            indoc! {r#"
                +++
                title = "Published"
                +++
                Body
            "#},
        );

        let set = discover_content(root.path()).unwrap();
        assert_eq!(set.pages.len(), 1);
        assert_eq!(set.pages[0].frontmatter.title, "Published");
    }

    #[test]
    fn discover_content_excludes_underscore_prefixed() {
        let root = tempfile::tempdir().unwrap();
        write_test_file(
            root.path(),
            "content/posts/visible/index.md",
            indoc! {r#"
                +++
                title = "Visible"
                +++
                Body
            "#},
        );
        write_test_file(
            root.path(),
            "content/posts/_hidden/index.md",
            indoc! {r#"
                +++
                title = "Hidden"
                +++
                Body
            "#},
        );

        let set = discover_content(root.path()).unwrap();
        assert_eq!(set.pages.len(), 1);
        assert_eq!(set.pages[0].frontmatter.title, "Visible");
    }

    #[test]
    fn discover_content_skips_markdown_without_frontmatter() {
        let root = tempfile::tempdir().unwrap();
        write_test_file(
            root.path(),
            "content/posts/hello/index.md",
            indoc! {r#"
                +++
                title = "Hello"
                +++
                Body
            "#},
        );
        // CLAUDE.md has no frontmatter — should be silently skipped.
        write_test_file(
            root.path(),
            "content/posts/hello/CLAUDE.md",
            "# Notes\nSome reference notes.",
        );

        let set = discover_content(root.path()).unwrap();
        assert_eq!(set.pages.len(), 1);
        assert_eq!(set.pages[0].frontmatter.title, "Hello");
    }

    #[test]
    fn discover_content_ignores_non_markdown_files() {
        let root = tempfile::tempdir().unwrap();
        write_test_file(
            root.path(),
            "content/posts/hello/index.md",
            indoc! {r#"
                +++
                title = "Hello"
                +++
                Body
            "#},
        );
        write_test_file(root.path(), "content/posts/hello/image.png", "not-a-png");

        let set = discover_content(root.path()).unwrap();
        assert_eq!(set.pages.len(), 1);
    }

    #[test]
    fn discover_content_missing_dir_returns_empty() {
        let root = tempfile::tempdir().unwrap();
        let set = discover_content(root.path()).unwrap();
        assert!(set.pages.is_empty());
    }

    #[test]
    fn discover_content_sorted_by_date_descending() {
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

        let set = discover_content(root.path()).unwrap();
        assert_eq!(set.pages[0].frontmatter.title, "New");
        assert_eq!(set.pages[1].frontmatter.title, "Old");
    }

    #[test]
    fn discover_content_undated_pages_sorted_by_path() {
        let root = tempfile::tempdir().unwrap();
        write_test_file(
            root.path(),
            "content/posts/beta/index.md",
            indoc! {r#"
                +++
                title = "Beta"
                +++
                Body
            "#},
        );
        write_test_file(
            root.path(),
            "content/posts/alpha/index.md",
            indoc! {r#"
                +++
                title = "Alpha"
                +++
                Body
            "#},
        );

        let set = discover_content(root.path()).unwrap();
        assert_eq!(set.pages[0].frontmatter.title, "Alpha");
        assert_eq!(set.pages[1].frontmatter.title, "Beta");
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
}
