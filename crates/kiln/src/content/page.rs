use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use walkdir::WalkDir;

use super::frontmatter::{self, Frontmatter};

/// Distinguishes blog posts (under `content/posts/`) from standalone pages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PageKind {
    /// A blog post, optionally belonging to a named section (e.g., "note").
    Post { section: Option<String> },
    /// A standalone page (e.g., about-me, about-site).
    Page,
}

/// A content page with parsed frontmatter, body, and derived metadata.
#[derive(Debug)]
pub struct Page {
    pub frontmatter: Frontmatter,
    pub raw_content: String,
    /// Whether this is a blog post or a standalone page.
    /// Set by content discovery based on the file's position in the content
    /// directory; defaults to `PageKind::Page` when created via `from_content`.
    pub kind: PageKind,
    pub slug: String,
    pub summary: Option<String>,
    pub source_path: PathBuf,
    /// Co-located non-markdown files for page bundles (e.g., images).
    /// Empty for standalone pages and pages created via `from_content`.
    pub assets: Vec<PathBuf>,
}

/// Summary separator used in markdown content.
const SUMMARY_SEPARATOR: &str = "<!--more-->";

impl Page {
    /// Returns `true` if this page is a blog post (under `content/posts/`).
    #[must_use]
    pub fn is_post(&self) -> bool {
        matches!(self.kind, PageKind::Post { .. })
    }

    /// Loads a page from a markdown file on disk.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read, the frontmatter is invalid,
    /// or a slug cannot be derived from the file path.
    pub fn from_file(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let mut page = Self::from_content(&content, path)
            .with_context(|| format!("failed to parse {}", path.display()))?;

        // Discover co-located assets for page bundles.
        if is_page_bundle(path)
            && let Some(dir) = path.parent()
        {
            page.assets = discover_assets(dir)
                .with_context(|| format!("failed to read assets in {}", dir.display()))?;
        }

        Ok(page)
    }

    /// Parses a page from its raw content string and source path.
    ///
    /// Separated from `from_file` to allow testing without filesystem I/O.
    ///
    /// # Errors
    ///
    /// Returns an error if the frontmatter is invalid or a slug cannot be derived.
    pub fn from_content(content: &str, path: &Path) -> Result<Self> {
        let (frontmatter, body) = frontmatter::parse(content)
            .with_context(|| format!("invalid frontmatter in {}", path.display()))?;

        // Explicit frontmatter slug takes priority over the filename-derived slug.
        let slug = frontmatter
            .slug
            .clone()
            .or_else(|| derive_slug(path))
            .with_context(|| {
                format!(
                    "cannot derive slug from {}: \
                     page bundles (index.md) must be inside a named directory",
                    path.display()
                )
            })?;
        let summary = extract_summary(body);

        Ok(Self {
            frontmatter,
            raw_content: body.to_owned(),
            kind: PageKind::Page,
            slug,
            summary,
            source_path: path.to_owned(),
            assets: Vec::new(),
        })
    }

    /// Computes the output path relative to the build output directory.
    ///
    /// Strips the `content/` prefix and keeps the remaining directory
    /// structure. Standalone files get pretty URLs (`slug/index.html`
    /// instead of `slug.html`).
    ///
    /// - `content/posts/foo/bar/index.md` → `posts/foo/bar/index.html`
    /// - `content/posts/hello-world.md` → `posts/hello-world/index.html`
    /// - `content/example/index.md` → `example/index.html`
    ///
    /// # Errors
    ///
    /// Returns an error if the source path is not under the given content directory.
    pub fn output_path(&self, content_dir: &Path) -> Result<PathBuf> {
        let relative = self
            .source_path
            .strip_prefix(content_dir)
            .with_context(|| {
                format!(
                    "{} is not under {}",
                    self.source_path.display(),
                    content_dir.display()
                )
            })?;

        // Page bundles (index.md) keep their directory structure.
        // Standalone files get pretty URLs: slug.md → slug/index.html.
        let stem = relative.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        if stem == "index" {
            Ok(relative.with_extension("html"))
        } else {
            Ok(relative.with_extension("").join("index.html"))
        }
    }
}

/// Derives the page kind from its position in the content directory.
///
/// Pages under `content/posts/` are posts. If the relative path after `posts/`
/// has 3+ components (e.g., `posts/note/my-post/index.md`), the first component
/// is the section. Posts with fewer components (e.g., `posts/hello/index.md`)
/// are orphan posts with no section.
///
/// Everything outside `content/posts/` is a standalone page.
pub fn derive_page_kind(source_path: &Path, content_dir: &Path) -> PageKind {
    let Ok(relative) = source_path.strip_prefix(content_dir) else {
        return PageKind::Page;
    };

    let Ok(after_posts) = relative.strip_prefix("posts") else {
        return PageKind::Page;
    };

    let components: Vec<_> = after_posts.components().collect();
    let section = if components.len() >= 3 {
        components[0].as_os_str().to_str().map(String::from)
    } else {
        None
    };

    PageKind::Post { section }
}

/// Returns `true` if the file is a page bundle entry point (`index.md`).
fn is_page_bundle(path: &Path) -> bool {
    path.file_stem().and_then(|s| s.to_str()) == Some("index")
}

/// Recursively discovers co-located non-markdown files in a page bundle directory.
///
/// Returns sorted absolute paths for deterministic output.
fn discover_assets(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut assets = Vec::new();
    for entry in WalkDir::new(dir).follow_links(false) {
        let entry = entry.with_context(|| format!("failed to read entry in {}", dir.display()))?;
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.into_path();
        if path.extension().is_none_or(|ext| ext != "md") {
            assets.push(path);
        }
    }
    assets.sort();
    Ok(assets)
}

/// Derives the page slug from its file path.
///
/// For page bundles (`index.md`), uses the parent directory name.
/// For standalone files (`my-post.md`), uses the file stem.
///
/// Returns `None` if the slug would be empty (e.g., a bare `index.md` with
/// no parent directory).
fn derive_slug(path: &Path) -> Option<String> {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");

    let slug = if stem == "index" {
        // Page bundle: use parent directory name.
        path.parent()
            .and_then(Path::file_name)
            .and_then(|s| s.to_str())
            .unwrap_or("")
    } else {
        stem
    };

    if slug.is_empty() {
        None
    } else {
        Some(slug.to_owned())
    }
}

/// Extracts the summary from markdown content (text before `<!--more-->`).
///
/// The raw markdown is stripped to plain text so that link syntax, formatting,
/// and reference definitions do not leak into descriptions.
fn extract_summary(body: &str) -> Option<String> {
    let idx = body.find(SUMMARY_SEPARATOR)?;
    let raw = body[..idx].trim();
    if raw.is_empty() {
        None
    } else {
        let plain = strip_markdown(raw);
        if plain.is_empty() { None } else { Some(plain) }
    }
}

/// Strips markdown syntax, producing a plain-text representation.
///
/// Uses pulldown-cmark to parse the markdown and extracts only text content.
/// Link display text is preserved; reference definitions, images, and
/// formatting syntax are removed.
fn strip_markdown(text: &str) -> String {
    let parser = Parser::new_ext(text, Options::all());

    let mut plain = String::with_capacity(text.len());
    let mut in_image = false;

    for event in parser {
        match event {
            Event::Text(t) | Event::Code(t) | Event::InlineMath(t) | Event::DisplayMath(t)
                if !in_image =>
            {
                plain.push_str(&t);
            }
            Event::SoftBreak | Event::HardBreak if !in_image => plain.push(' '),
            Event::Start(Tag::Image { .. }) => in_image = true,
            Event::End(TagEnd::Image) => in_image = false,
            Event::Start(Tag::Paragraph) if !plain.is_empty() => plain.push(' '),
            _ => {}
        }
    }

    // Collapse whitespace runs and trim.
    plain.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use indoc::indoc;

    use super::*;
    use crate::test_utils::{PermissionGuard, test_page};

    // -- from_file: basic --

    #[test]
    fn from_file_basic() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.md");
        fs::write(
            &file,
            indoc! {r#"
                +++
                title = "Test"
                +++

                Summary here.

                <!--more-->

                Full content here.
            "#},
        )
        .unwrap();

        let page = Page::from_file(&file).unwrap();
        assert_eq!(page.frontmatter.title, "Test");
        assert_eq!(page.slug, "test");
        assert_eq!(page.summary.unwrap(), "Summary here.");
    }

    #[test]
    fn from_file_nonexistent_returns_error() {
        let err = Page::from_file(Path::new("/nonexistent/test.md"))
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("failed to read"),
            "should report read failure, got: {err}"
        );
    }

    #[test]
    fn from_file_invalid_frontmatter_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("bad.md");
        fs::write(
            &file,
            indoc! {"
                +++
                not valid {{{
                +++
            "},
        )
        .unwrap();

        let err = Page::from_file(&file).unwrap_err().to_string();
        assert!(
            err.contains("failed to parse"),
            "should report parse failure, got: {err}"
        );
    }

    // -- from_file: asset discovery --

    #[test]
    fn from_file_page_bundle_discovers_assets_recursively() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = dir.path().join("content").join("posts").join("hello");
        let assets_dir = bundle.join("assets");
        fs::create_dir_all(&assets_dir).unwrap();
        fs::write(
            bundle.join("index.md"),
            indoc! {r#"
                +++
                title = "Hello"
                +++
                Body
            "#},
        )
        .unwrap();
        fs::write(bundle.join("cover.webp"), "fake-webp").unwrap();
        fs::write(assets_dir.join("screenshot.webp"), "fake-webp").unwrap();
        fs::write(assets_dir.join("data.json"), "{}").unwrap();

        let page = Page::from_file(&bundle.join("index.md")).unwrap();
        let relative_paths: Vec<_> = page
            .assets
            .iter()
            .map(|p| p.strip_prefix(&bundle).unwrap().to_str().unwrap())
            .collect();
        assert_eq!(
            relative_paths,
            vec!["assets/data.json", "assets/screenshot.webp", "cover.webp"]
        );
    }

    #[test]
    fn from_file_page_bundle_excludes_markdown() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = dir.path().join("hello");
        fs::create_dir_all(&bundle).unwrap();
        fs::write(
            bundle.join("index.md"),
            indoc! {r#"
                +++
                title = "Hello"
                +++
                Body
            "#},
        )
        .unwrap();
        fs::write(bundle.join("notes.md"), "other markdown").unwrap();
        fs::write(bundle.join("image.png"), "fake-png").unwrap();

        let page = Page::from_file(&bundle.join("index.md")).unwrap();
        let relative_paths: Vec<_> = page
            .assets
            .iter()
            .map(|p| p.strip_prefix(&bundle).unwrap().to_str().unwrap())
            .collect();
        assert_eq!(relative_paths, vec!["image.png"]);
    }

    #[test]
    fn from_file_non_index_has_no_assets() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("standalone.md");
        fs::write(
            &file,
            indoc! {r#"
                +++
                title = "Standalone"
                +++
                Body
            "#},
        )
        .unwrap();
        fs::write(dir.path().join("image.png"), "fake-png").unwrap();

        let page = Page::from_file(&file).unwrap();
        assert!(page.assets.is_empty());
    }

    #[test]
    fn from_file_unreadable_bundle_dir_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = dir.path().join("hello");
        fs::create_dir_all(&bundle).unwrap();
        fs::write(
            bundle.join("index.md"),
            indoc! {r#"
                +++
                title = "Hello"
                +++
                Body
            "#},
        )
        .unwrap();

        // Remove read permission but keep execute so the file can still be read by path.
        let _guard = PermissionGuard::restrict(&bundle, 0o111);

        let err = Page::from_file(&bundle.join("index.md"))
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("failed to read assets"),
            "should report asset discovery failure, got: {err}"
        );
    }

    #[test]
    fn from_file_unreadable_subdir_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = dir.path().join("hello");
        let subdir = bundle.join("broken");
        fs::create_dir_all(&subdir).unwrap();
        fs::write(
            bundle.join("index.md"),
            indoc! {r#"
                +++
                title = "Hello"
                +++
                Body
            "#},
        )
        .unwrap();
        fs::write(subdir.join("file.txt"), "content").unwrap();

        // Make the subdirectory unreadable so WalkDir yields an error entry.
        let _guard = PermissionGuard::restrict(&subdir, 0o000);

        let err = Page::from_file(&bundle.join("index.md"))
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("failed to read"),
            "should report entry read failure, got: {err}"
        );
    }

    // -- from_content --

    #[test]
    fn from_content_basic() {
        let content = indoc! {r#"
            +++
            title = "Test"
            +++

            Summary here.

            <!--more-->

            Full content here.
        "#};
        let page = Page::from_content(content, Path::new("content/posts/test/index.md")).unwrap();
        assert_eq!(page.frontmatter.title, "Test");
        assert_eq!(page.slug, "test");
        assert_eq!(page.summary.unwrap(), "Summary here.");
    }

    #[test]
    fn from_content_explicit_slug_overrides_filename() {
        let content = indoc! {r#"
            +++
            title = "My Post"
            slug = "custom-slug"
            +++
            Body
        "#};
        let page = Page::from_content(content, Path::new("content/posts/foobar/index.md")).unwrap();
        assert_eq!(page.slug, "custom-slug");
    }

    #[test]
    fn from_content_bare_index_no_slug_returns_error() {
        let content = indoc! {r#"
            +++
            title = "Test"
            +++
            Body
        "#};
        let err = Page::from_content(content, Path::new("index.md"))
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("cannot derive slug"),
            "should report slug derivation failure, got: {err}"
        );
    }

    // -- output_path --

    #[test]
    fn output_path_post() {
        let mut page = test_page("bar");
        page.source_path = PathBuf::from("/site/content/posts/foo/bar/index.md");
        let out = page.output_path(Path::new("/site/content")).unwrap();
        assert_eq!(out, PathBuf::from("posts/foo/bar/index.html"));
    }

    #[test]
    fn output_path_non_post() {
        let mut page = test_page("example");
        page.source_path = PathBuf::from("/site/content/example/index.md");
        let out = page.output_path(Path::new("/site/content")).unwrap();
        assert_eq!(out, PathBuf::from("example/index.html"));
    }

    #[test]
    fn output_path_non_index() {
        let mut page = test_page("hello-world");
        page.source_path = PathBuf::from("/site/content/posts/hello-world.md");
        let out = page.output_path(Path::new("/site/content")).unwrap();
        assert_eq!(out, PathBuf::from("posts/hello-world/index.html"));
    }

    #[test]
    fn output_path_outside_content_dir_returns_error() {
        let mut page = test_page("test");
        page.source_path = PathBuf::from("/other/dir/test.md");
        assert!(page.output_path(Path::new("/site/content")).is_err());
    }

    // -- derive_page_kind --

    #[test]
    fn derive_page_kind_section_post_deep() {
        let kind = derive_page_kind(
            Path::new("/site/content/posts/note/deep/nested/index.md"),
            Path::new("/site/content"),
        );
        assert_eq!(
            kind,
            PageKind::Post {
                section: Some("note".into())
            }
        );
    }

    #[test]
    fn derive_page_kind_section_post_shallow() {
        let kind = derive_page_kind(
            Path::new("/site/content/posts/note/my-post/index.md"),
            Path::new("/site/content"),
        );
        assert_eq!(
            kind,
            PageKind::Post {
                section: Some("note".into())
            }
        );
    }

    #[test]
    fn derive_page_kind_orphan_post_bundle() {
        let kind = derive_page_kind(
            Path::new("/site/content/posts/hello/index.md"),
            Path::new("/site/content"),
        );
        assert_eq!(kind, PageKind::Post { section: None });
    }

    #[test]
    fn derive_page_kind_orphan_post_non_index() {
        let kind = derive_page_kind(
            Path::new("/site/content/posts/hello.md"),
            Path::new("/site/content"),
        );
        assert_eq!(kind, PageKind::Post { section: None });
    }

    #[test]
    fn derive_page_kind_non_post() {
        let kind = derive_page_kind(
            Path::new("/site/content/about-me/index.md"),
            Path::new("/site/content"),
        );
        assert_eq!(kind, PageKind::Page);
    }

    #[test]
    fn derive_page_kind_outside_content_dir() {
        let kind = derive_page_kind(Path::new("/other/path/page.md"), Path::new("/site/content"));
        assert_eq!(kind, PageKind::Page);
    }

    // -- derive_slug --

    #[test]
    fn derive_slug_page_bundle() {
        let path = Path::new("content/posts/foo/bar/index.md");
        assert_eq!(derive_slug(path).unwrap(), "bar");
    }

    #[test]
    fn derive_slug_non_index() {
        let path = Path::new("content/posts/hello-world.md");
        assert_eq!(derive_slug(path).unwrap(), "hello-world");
    }

    #[test]
    fn derive_slug_bare_index_returns_none() {
        assert!(derive_slug(Path::new("index.md")).is_none());
    }

    // -- extract_summary --

    #[test]
    fn extract_summary_basic() {
        let body = indoc! {r"
            This is the summary.

            <!--more-->

            Full content here.
        "};
        assert_eq!(extract_summary(body).unwrap(), "This is the summary.");
    }

    #[test]
    fn extract_summary_no_separator() {
        let body = "No summary separator in this content.";
        assert!(extract_summary(body).is_none());
    }

    #[test]
    fn extract_summary_empty_before_separator() {
        let body = indoc! {r"
            <!--more-->

            Content after.
        "};
        assert!(extract_summary(body).is_none());
    }

    #[test]
    fn extract_summary_strips_reference_links() {
        let body = indoc! {r"
            See [the docs][docs-ref] and the [home page].

            [docs-ref]: https://example.com/docs
            [home page]: https://example.com

            <!--more-->

            Full content here.
        "};
        assert_eq!(
            extract_summary(body).unwrap(),
            "See the docs and the home page."
        );
    }

    #[test]
    fn extract_summary_strips_inline_links_and_formatting() {
        let body = indoc! {r"
            A **bold** and *italic* intro with [a link](https://example.com).

            <!--more-->
        "};
        assert_eq!(
            extract_summary(body).unwrap(),
            "A bold and italic intro with a link."
        );
    }

    #[test]
    fn extract_summary_strips_images() {
        let body = indoc! {r"
            Text before ![an image](photo.jpg) and after.

            <!--more-->
        "};
        assert_eq!(extract_summary(body).unwrap(), "Text before and after.");
    }

    #[test]
    fn extract_summary_preserves_inline_code() {
        let body = indoc! {r"
            Use `strip_markdown` to clean text.

            <!--more-->
        "};
        assert_eq!(
            extract_summary(body).unwrap(),
            "Use strip_markdown to clean text."
        );
    }

    #[test]
    fn extract_summary_joins_paragraphs() {
        let body = indoc! {r"
            First paragraph.

            Second paragraph.

            <!--more-->
        "};
        assert_eq!(
            extract_summary(body).unwrap(),
            "First paragraph. Second paragraph."
        );
    }
}
