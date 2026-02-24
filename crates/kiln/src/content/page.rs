use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use walkdir::WalkDir;

use crate::content::frontmatter::{self, Frontmatter};

/// A content page with parsed frontmatter, body, and derived metadata.
#[derive(Debug)]
pub struct Page {
    pub frontmatter: Frontmatter,
    pub raw_content: String,
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
            slug,
            summary,
            source_path: path.to_owned(),
            assets: Vec::new(),
        })
    }

    /// Computes the output path relative to the build output directory.
    ///
    /// Strips the `content/` prefix and the `posts/` segment to match a
    /// site-specific permalink layout (Hugo `:sections[2:]`). Standalone
    /// files get pretty URLs (`slug/index.html` instead of `slug.html`).
    ///
    /// - `content/posts/foo/bar/index.md` → `foo/bar/index.html`
    /// - `content/posts/hello-world.md` → `hello-world/index.html`
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

        // Page bundles (index.md) keep their directory structure.
        // Standalone files get pretty URLs: slug.md → slug/index.html.
        let stem = stripped.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        if stem == "index" {
            Ok(stripped.with_extension("html"))
        } else {
            Ok(stripped.with_extension("").join("index.html"))
        }
    }
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
    use crate::test_utils::PermissionGuard;

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
        let body = "<!--more-->\nContent after.";
        assert!(extract_summary(body).is_none());
    }

    // -- strip_posts_prefix --

    #[test]
    fn strip_posts_prefix_basic() {
        assert_eq!(
            strip_posts_prefix(Path::new("posts/foo/bar/index.md")),
            PathBuf::from("foo/bar/index.md")
        );
    }

    #[test]
    fn strip_posts_prefix_non_post() {
        assert_eq!(
            strip_posts_prefix(Path::new("example/index.md")),
            PathBuf::from("example/index.md")
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
    fn from_file_standalone_has_no_assets() {
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

    // -- output_path --

    #[test]
    fn output_path_post() {
        let page = Page {
            frontmatter: Frontmatter::default(),
            raw_content: String::new(),
            slug: "bar".into(),
            summary: None,
            source_path: PathBuf::from("/site/content/posts/foo/bar/index.md"),
            assets: Vec::new(),
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
            assets: Vec::new(),
        };
        let out = page.output_path(Path::new("/site/content")).unwrap();
        assert_eq!(out, PathBuf::from("example/index.html"));
    }

    #[test]
    fn output_path_standalone_pretty_url() {
        let page = Page {
            frontmatter: Frontmatter::default(),
            raw_content: String::new(),
            slug: "hello-world".into(),
            summary: None,
            source_path: PathBuf::from("/site/content/posts/hello-world.md"),
            assets: Vec::new(),
        };
        let out = page.output_path(Path::new("/site/content")).unwrap();
        assert_eq!(out, PathBuf::from("hello-world/index.html"));
    }

    #[test]
    fn output_path_outside_content_dir_returns_error() {
        let page = Page {
            frontmatter: Frontmatter::default(),
            raw_content: String::new(),
            slug: "test".into(),
            summary: None,
            source_path: PathBuf::from("/other/dir/test.md"),
            assets: Vec::new(),
        };
        assert!(page.output_path(Path::new("/site/content")).is_err());
    }
}
