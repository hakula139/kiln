use std::collections::BTreeMap;
use std::path::Path;

use crate::content::frontmatter;
use crate::content::page::{Page, PageKind};
use crate::text::titlecase;

/// A content section derived from the directory structure under `content/posts/`.
#[derive(Debug, Clone)]
pub struct Section {
    pub slug: String,
    pub title: String,
    pub page_count: usize,
}

/// Collects sections from discovered pages.
///
/// A section is the first subdirectory under `content/posts/` for pages with
/// `PageKind::Post { section: Some(_) }`. Each section's display title is loaded
/// from `content/posts/<section>/_index.md` if present, falling back to the
/// titlecased slug.
///
/// Returns sections sorted alphabetically by slug.
#[must_use]
pub fn collect_sections(pages: &[Page], content_dir: &Path) -> Vec<Section> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for page in pages {
        if let PageKind::Post {
            section: Some(ref s),
        } = page.kind
        {
            *counts.entry(s.clone()).or_default() += 1;
        }
    }

    counts
        .into_iter()
        .map(|(slug, page_count)| {
            let section_dir = content_dir.join("posts").join(&slug);
            let title = load_index_title(&section_dir).unwrap_or_else(|| titlecase(&slug));
            Section {
                slug,
                title,
                page_count,
            }
        })
        .collect()
}

/// Loads the display title from `_index.md` in the given directory.
///
/// Returns `None` if the file is missing, has invalid frontmatter, or an
/// empty title.
pub(crate) fn load_index_title(dir: &Path) -> Option<String> {
    let content = std::fs::read_to_string(dir.join("_index.md")).ok()?;
    let (fm, _) = frontmatter::parse(&content).ok()?;
    if fm.title.is_empty() {
        None
    } else {
        Some(fm.title)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use indoc::indoc;

    use super::*;
    use crate::test_utils::test_page;

    fn make_page(title: &str, section: Option<&str>) -> Page {
        let mut page = test_page(title);
        page.kind = PageKind::Post {
            section: section.map(String::from),
        };
        page.source_path = PathBuf::from(format!("content/posts/{title}/index.md"));
        page
    }

    fn make_standalone(title: &str) -> Page {
        test_page(title)
    }

    // ── collect_sections ──

    #[test]
    fn collect_sections_basic() {
        let pages = vec![
            make_page("Post 1", Some("note")),
            make_page("Post 2", Some("note")),
            make_page("Post 3", Some("essay")),
        ];
        let dir = tempfile::tempdir().unwrap();
        let content_dir = dir.path().join("content");
        fs::create_dir_all(&content_dir).unwrap();

        let sections = collect_sections(&pages, &content_dir);
        assert_eq!(sections.len(), 2);
        // Sorted alphabetically by slug.
        assert_eq!(sections[0].slug, "essay");
        assert_eq!(sections[0].title, "Essay");
        assert_eq!(sections[0].page_count, 1);
        assert_eq!(sections[1].slug, "note");
        assert_eq!(sections[1].title, "Note");
        assert_eq!(sections[1].page_count, 2);
    }

    #[test]
    fn collect_sections_uses_index_title() {
        let dir = tempfile::tempdir().unwrap();
        let content_dir = dir.path().join("content");
        let section_dir = content_dir.join("posts").join("note");
        fs::create_dir_all(&section_dir).unwrap();
        fs::write(
            section_dir.join("_index.md"),
            indoc! {r#"
                +++
                title = "笔记"
                +++
            "#},
        )
        .unwrap();

        let pages = vec![make_page("Post 1", Some("note"))];
        let sections = collect_sections(&pages, &content_dir);

        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].title, "笔记");
        assert_eq!(sections[0].slug, "note");
    }

    #[test]
    fn collect_sections_falls_back_to_titlecase() {
        let dir = tempfile::tempdir().unwrap();
        let content_dir = dir.path().join("content");
        fs::create_dir_all(&content_dir).unwrap();

        let pages = vec![make_page("Post 1", Some("hello-world"))];
        let sections = collect_sections(&pages, &content_dir);

        assert_eq!(sections[0].title, "Hello World");
    }

    #[test]
    fn collect_sections_empty_index_title_falls_back() {
        let dir = tempfile::tempdir().unwrap();
        let content_dir = dir.path().join("content");
        let section_dir = content_dir.join("posts").join("note");
        fs::create_dir_all(&section_dir).unwrap();
        fs::write(
            section_dir.join("_index.md"),
            indoc! {r"
                +++
                +++
            "},
        )
        .unwrap();

        let pages = vec![make_page("Post 1", Some("note"))];
        let sections = collect_sections(&pages, &content_dir);

        assert_eq!(sections[0].title, "Note");
    }

    #[test]
    fn collect_sections_excludes_standalone_pages() {
        let pages = vec![
            make_page("Post 1", Some("note")),
            make_standalone("About Me"),
        ];
        let dir = tempfile::tempdir().unwrap();
        let content_dir = dir.path().join("content");
        fs::create_dir_all(&content_dir).unwrap();

        let sections = collect_sections(&pages, &content_dir);
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].slug, "note");
    }

    #[test]
    fn collect_sections_excludes_orphan_posts() {
        let pages = vec![make_page("Post 1", Some("note")), make_page("Orphan", None)];
        let dir = tempfile::tempdir().unwrap();
        let content_dir = dir.path().join("content");
        fs::create_dir_all(&content_dir).unwrap();

        let sections = collect_sections(&pages, &content_dir);
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].slug, "note");
    }

    #[test]
    fn collect_sections_empty() {
        let dir = tempfile::tempdir().unwrap();
        let sections = collect_sections(&[], dir.path());
        assert!(sections.is_empty());
    }
}
