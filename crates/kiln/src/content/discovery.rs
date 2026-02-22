use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use walkdir::WalkDir;

use crate::content::page::Page;

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
        if path.extension().is_some_and(|ext| ext == "md") {
            let page = Page::from_file(path)?;
            if !page.frontmatter.draft {
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

/// Returns `true` for entries whose file name starts with `_`.
fn is_excluded(entry: &walkdir::DirEntry) -> bool {
    entry
        .file_name()
        .to_str()
        .is_some_and(|name| name.starts_with('_'))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use indoc::indoc;

    use super::*;

    fn write_page(dir: &Path, rel_path: &str, content: &str) {
        let path = dir.join(rel_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }

    #[test]
    fn discovers_markdown_files() {
        let root = tempfile::tempdir().unwrap();
        write_page(
            root.path(),
            "content/posts/hello/index.md",
            indoc! {r#"
                +++
                title = "Hello"
                +++
                Body
            "#},
        );
        write_page(
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
    fn excludes_drafts() {
        let root = tempfile::tempdir().unwrap();
        write_page(
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
        write_page(
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
    fn excludes_underscore_prefixed() {
        let root = tempfile::tempdir().unwrap();
        write_page(
            root.path(),
            "content/posts/visible/index.md",
            indoc! {r#"
                +++
                title = "Visible"
                +++
                Body
            "#},
        );
        write_page(
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
    fn ignores_non_markdown_files() {
        let root = tempfile::tempdir().unwrap();
        write_page(
            root.path(),
            "content/posts/hello/index.md",
            indoc! {r#"
                +++
                title = "Hello"
                +++
                Body
            "#},
        );
        write_page(root.path(), "content/posts/hello/image.png", "not-a-png");

        let set = discover_content(root.path()).unwrap();
        assert_eq!(set.pages.len(), 1);
    }

    #[test]
    fn missing_content_dir_returns_empty() {
        let root = tempfile::tempdir().unwrap();
        let set = discover_content(root.path()).unwrap();
        assert!(set.pages.is_empty());
    }

    #[test]
    fn sorted_by_date_descending() {
        let root = tempfile::tempdir().unwrap();
        write_page(
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
        write_page(
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
    fn undated_pages_sorted_by_path() {
        let root = tempfile::tempdir().unwrap();
        write_page(
            root.path(),
            "content/posts/beta/index.md",
            indoc! {r#"
                +++
                title = "Beta"
                +++
                Body
            "#},
        );
        write_page(
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
}
