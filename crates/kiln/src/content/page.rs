use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::content::frontmatter::{self, Frontmatter};

/// A content page with parsed frontmatter, body, and derived metadata.
#[derive(Debug)]
pub struct Page {
    pub frontmatter: Frontmatter,
    pub raw_content: String,
    pub slug: String,
    pub summary: Option<String>,
    pub source_path: PathBuf,
}

/// Summary separator used in markdown content.
const SUMMARY_SEPARATOR: &str = "<!--more-->";

impl Page {
    /// Loads a page from a markdown file on disk.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read, the frontmatter is invalid,
    /// or a slug cannot be derived from the file path.
    pub fn from_file(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        Self::from_content(&content, path)
            .with_context(|| format!("failed to parse {}", path.display()))
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
            slug,
            summary,
            source_path: path.to_owned(),
        })
    }

    /// Computes the output path relative to the build output directory.
    ///
    /// Strips the `content/` prefix and the `posts/` segment to match a
    /// site-specific permalink layout (Hugo `:sections[2:]`).
    ///
    /// - `content/posts/foo/bar/index.md` → `foo/bar/index.html`
    /// - `content/example/index.md` → `example/index.html`
    ///
    /// TODO: Make this configurable via `[permalinks]` in `config.toml`.
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

        let stripped = strip_posts_prefix(relative);
        Ok(stripped.with_extension("html"))
    }
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
fn extract_summary(body: &str) -> Option<String> {
    let idx = body.find(SUMMARY_SEPARATOR)?;
    let summary = body[..idx].trim();
    if summary.is_empty() {
        None
    } else {
        Some(summary.to_owned())
    }
}

/// Strips the `posts/` prefix from a content-relative path.
fn strip_posts_prefix(path: &Path) -> PathBuf {
    path.strip_prefix("posts").unwrap_or(path).to_owned()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use indoc::indoc;

    use super::*;

    // -- derive_slug --

    #[test]
    fn slug_from_page_bundle() {
        let path = Path::new("content/posts/foo/bar/index.md");
        assert_eq!(derive_slug(path).unwrap(), "bar");
    }

    #[test]
    fn slug_from_standalone_file() {
        let path = Path::new("content/posts/hello-world.md");
        assert_eq!(derive_slug(path).unwrap(), "hello-world");
    }

    #[test]
    fn slug_empty_for_bare_index() {
        assert!(derive_slug(Path::new("index.md")).is_none());
    }

    #[test]
    fn slug_explicit_overrides_filename() {
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

    // -- extract_summary --

    #[test]
    fn summary_extraction() {
        let body = indoc! {r"
            This is the summary.

            <!--more-->

            Full content here.
        "};
        assert_eq!(extract_summary(body).unwrap(), "This is the summary.");
    }

    #[test]
    fn summary_absent() {
        let body = "No summary separator in this content.";
        assert!(extract_summary(body).is_none());
    }

    #[test]
    fn summary_empty_before_separator() {
        let body = "<!--more-->\nContent after.";
        assert!(extract_summary(body).is_none());
    }

    // -- strip_posts_prefix --

    #[test]
    fn strip_posts() {
        assert_eq!(
            strip_posts_prefix(Path::new("posts/foo/bar/index.md")),
            PathBuf::from("foo/bar/index.md")
        );
    }

    #[test]
    fn strip_posts_non_post() {
        assert_eq!(
            strip_posts_prefix(Path::new("example/index.md")),
            PathBuf::from("example/index.md")
        );
    }

    // -- from_content / from_file --

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
    fn from_file_integration() {
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

    // -- output_path --

    #[test]
    fn output_path_post() {
        let page = Page {
            frontmatter: Frontmatter::default(),
            raw_content: String::new(),
            slug: "bar".into(),
            summary: None,
            source_path: PathBuf::from("/site/content/posts/foo/bar/index.md"),
        };
        let out = page.output_path(Path::new("/site/content")).unwrap();
        assert_eq!(out, PathBuf::from("foo/bar/index.html"));
    }

    #[test]
    fn output_path_non_post() {
        let page = Page {
            frontmatter: Frontmatter::default(),
            raw_content: String::new(),
            slug: "example".into(),
            summary: None,
            source_path: PathBuf::from("/site/content/example/index.md"),
        };
        let out = page.output_path(Path::new("/site/content")).unwrap();
        assert_eq!(out, PathBuf::from("example/index.html"));
    }

    #[test]
    fn output_path_outside_content_dir_errors() {
        let page = Page {
            frontmatter: Frontmatter::default(),
            raw_content: String::new(),
            slug: "test".into(),
            summary: None,
            source_path: PathBuf::from("/other/dir/test.md"),
        };
        assert!(page.output_path(Path::new("/site/content")).is_err());
    }
}
