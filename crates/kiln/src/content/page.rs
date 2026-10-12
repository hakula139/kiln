use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use itertools::Itertools;
use pulldown_cmark::{Event, Parser, Tag, TagEnd};

use super::frontmatter::{self, Frontmatter};
use super::{is_markdown, is_public, is_single_component};
use crate::markdown::{code_ranges, markdown_options, push_plain_text};
use crate::output::walk_directory;

const SUMMARY_SEPARATOR: &str = "<!--more-->";

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
    /// Markdown following the frontmatter block.
    pub body: String,
    pub kind: PageKind,
    pub summary: Option<String>,
    pub source_path: PathBuf,
    /// Directory the page renders into, relative to the build output directory.
    pub output_dir: PathBuf,
    /// Public co-located non-markdown files for page bundles. Empty for standalone pages.
    pub assets: Vec<PathBuf>,
}

impl Page {
    /// Loads the page whose source at `path` under `content_dir` holds `content`, deriving its
    /// kind, output directory and summary and discovering its bundle assets on disk.
    ///
    /// # Errors
    ///
    /// Returns an error if the frontmatter or slug is invalid, a section name is not UTF-8, or
    /// bundle assets cannot be read.
    pub(super) fn load(content: &str, path: &Path, content_dir: &Path) -> Result<Self> {
        let relative = path
            .strip_prefix(content_dir)
            .expect("content discovery only yields paths under the content directory");
        let (frontmatter, body) = frontmatter::parse(content)
            .with_context(|| format!("invalid frontmatter in {}", path.display()))?;
        let output_dir = output_dir(relative, frontmatter.slug.as_deref())
            .with_context(|| format!("invalid slug in {}", path.display()))?;
        let kind = page_kind(relative)
            .with_context(|| format!("invalid section in {}", path.display()))?;

        let mut page = Self {
            summary: extract_summary(body),
            body: body.to_owned(),
            frontmatter,
            kind,
            source_path: path.to_owned(),
            output_dir,
            assets: Vec::new(),
        };
        if let Some(dir) = page.bundle_dir() {
            let assets = discover_assets(dir)
                .with_context(|| format!("failed to read assets in {}", dir.display()))?;
            page.assets = assets;
        }
        Ok(page)
    }

    /// Directory owning the co-located assets of a page bundle, or `None` for a standalone page.
    #[must_use]
    pub fn bundle_dir(&self) -> Option<&Path> {
        self.source_path
            .parent()
            .filter(|_| is_page_bundle(&self.source_path))
    }

    /// Output file path relative to the build output directory.
    #[must_use]
    pub fn output_path(&self) -> PathBuf {
        self.output_dir.join("index.html")
    }
}

/// Derives the output directory from a content-relative source path and an optional slug.
///
/// - `posts/foo/bar/index.md` → `posts/foo/bar`
/// - `posts/hello-world.md` → `posts/hello-world`
/// - `index.md` → the output root
///
/// A slug replaces the final component.
fn output_dir(relative: &Path, slug: Option<&str>) -> Result<PathBuf> {
    let mut directory = if is_page_bundle(relative) {
        relative.parent().unwrap_or(Path::new("")).to_owned()
    } else {
        relative.with_extension("")
    };
    if let Some(slug) = slug {
        ensure!(
            is_single_component(slug),
            "slug must be a single nonempty path component"
        );
        directory.set_file_name(slug);
    }
    Ok(directory)
}

/// Derives the page kind from its content-relative path.
///
/// Pages under `posts/` are posts. The first directory after `posts/` is the section when the
/// remaining path is deep enough:
///
/// - Bundles need 3+ components: `posts/note/slug/index.md` → section "note"
/// - Non-bundles need 2+: `posts/note/hello.md` → section "note"
///
/// Shallower paths (`posts/slug/index.md`, `posts/hello.md`) are orphan posts. Everything outside
/// `posts/` is a standalone page.
fn page_kind(relative: &Path) -> Result<PageKind> {
    let Ok(after_posts) = relative.strip_prefix("posts") else {
        return Ok(PageKind::Page);
    };

    let min_depth = if is_page_bundle(relative) { 3 } else { 2 };
    let section = match after_posts.iter().next() {
        Some(name) if after_posts.iter().count() >= min_depth => Some(
            name.to_str()
                .with_context(|| format!("section name {} is not UTF-8", name.display()))?
                .to_owned(),
        ),
        _ => None,
    };
    Ok(PageKind::Post { section })
}

/// Returns `true` if the file is a page bundle entry point (`index.md`).
fn is_page_bundle(path: &Path) -> bool {
    path.file_stem().and_then(|s| s.to_str()) == Some("index")
}

/// Returns sorted public assets owned by this bundle, pruning nested bundle subtrees.
fn discover_assets(dir: &Path) -> Result<Vec<PathBuf>> {
    let include = |path: &Path| is_public(path) && !(path.is_dir() && contains_bundle(path));
    let mut assets = Vec::new();
    for entry in walk_directory(dir, &include) {
        let entry = entry.with_context(|| format!("failed to read entry in {}", dir.display()))?;
        if entry.file_type().is_file() && !is_markdown(entry.path()) {
            assets.push(entry.into_path());
        }
    }
    assets.sort();
    Ok(assets)
}

/// Unreadable directories count as bundle-free, so the walk descends and reports their error.
fn contains_bundle(dir: &Path) -> bool {
    fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .any(|entry| {
            let path = entry.path();
            is_markdown(&path) && is_page_bundle(&path) && path.is_file()
        })
}

/// Extracts summary text before the first `<!--more-->` outside code, stripped to plain text.
///
/// Parses the full body so reference link definitions after the separator still resolve.
fn extract_summary(body: &str) -> Option<String> {
    let mut candidates = body
        .match_indices(SUMMARY_SEPARATOR)
        .map(|(offset, _)| offset)
        .peekable();
    candidates.peek()?;
    let code = code_ranges(body);
    let separator_offset =
        candidates.find(|offset| !code.iter().any(|range| range.contains(offset)))?;
    let plain = strip_markdown(body, separator_offset);
    (!plain.is_empty()).then_some(plain)
}

/// Strips markdown to plain text from the region before `summary_end`.
fn strip_markdown(full_text: &str, summary_end: usize) -> String {
    let parser = Parser::new_ext(full_text, markdown_options()).into_offset_iter();

    let mut plain = String::with_capacity(summary_end);
    let mut in_image = false;

    for (event, range) in parser {
        if range.start >= summary_end {
            continue;
        }

        match event {
            Event::Start(Tag::Image { .. }) => in_image = true,
            Event::End(TagEnd::Image) => in_image = false,
            Event::Start(Tag::Paragraph) if !plain.is_empty() => plain.push('\n'),
            _ if !in_image => push_plain_text(&mut plain, &event),
            _ => {}
        }
    }

    plain
        .lines()
        .map(|line| line.split_whitespace().join(" "))
        .filter(|line| !line.is_empty())
        .join("\n")
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::*;
    #[cfg(unix)]
    use crate::test_utils::PermissionGuard;
    use crate::test_utils::write_test_file;

    const MINIMAL: &str = indoc! {r#"
        +++
        title = "Example"
        +++
        Body
    "#};

    // ── Page::load ──

    #[test]
    fn bundle_derives_metadata_and_assets() {
        let root = tempfile::tempdir().unwrap();
        let bundle = root.path().join("content/posts/note/hello");
        for (path, contents) in [
            ("cover.webp", "fake-webp"),
            ("assets/screenshot.webp", "fake-webp"),
            ("assets/data.json", "{}"),
            ("notes.md", "other markdown"),
            ("assets/notes.md", "nested markdown"),
            ("_private.txt", "private"),
            ("nested/index.md", "nested bundle"),
            ("nested/photo.webp", "nested bundle asset"),
        ] {
            write_test_file(&bundle, path, contents);
        }

        let page = load_at(
            root.path(),
            "posts/note/hello/index.md",
            indoc! {r#"
                +++
                title = "Hello"
                +++
                Summary here.

                <!--more-->

                Full content here.
            "#},
        )
        .unwrap();

        assert_eq!(page.frontmatter.title, "Hello");
        assert_eq!(
            page.body,
            "Summary here.\n\n<!--more-->\n\nFull content here.\n"
        );
        assert_eq!(page.summary.as_deref(), Some("Summary here."));
        assert_eq!(
            page.kind,
            PageKind::Post {
                section: Some("note".into())
            }
        );
        assert_eq!(page.output_dir, Path::new("posts/note/hello"));
        assert_eq!(page.output_path(), Path::new("posts/note/hello/index.html"));
        let relative_paths: Vec<_> = page
            .assets
            .iter()
            .map(|path| path.strip_prefix(&bundle).unwrap())
            .collect();
        assert_eq!(
            relative_paths,
            ["assets/data.json", "assets/screenshot.webp", "cover.webp"].map(Path::new)
        );
    }

    #[test]
    fn standalone_file_has_no_assets() {
        let root = tempfile::tempdir().unwrap();
        write_test_file(root.path(), "content/image.png", "fake-png");

        let page = load_at(root.path(), "standalone.md", MINIMAL).unwrap();

        assert_eq!(page.kind, PageKind::Page);
        assert_eq!(page.output_dir, Path::new("standalone"));
        assert_eq!(page.assets, Vec::<PathBuf>::new());
    }

    #[test]
    fn reports_invalid_frontmatter_and_slug() {
        for (content, expected) in [
            (
                indoc! {"
                    +++
                    not valid {{{
                    +++
                "},
                "invalid frontmatter",
            ),
            (
                indoc! {r#"
                    +++
                    slug = "../other"
                    +++
                "#},
                "invalid slug",
            ),
        ] {
            let root = tempfile::tempdir().unwrap();
            let error = load_at(root.path(), "bad.md", content).unwrap_err();
            assert!(
                error.to_string().starts_with(expected),
                "{expected}: {error:#}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn skips_private_and_nested_bundle_symlinks() {
        let root = tempfile::tempdir().unwrap();
        let bundle = root.path().join("content/example");
        write_test_file(&bundle, "visible.txt", "published");
        write_test_file(&bundle, "nested/index.md", MINIMAL);
        std::os::unix::fs::symlink(bundle.join("missing"), bundle.join("_broken")).unwrap();
        std::os::unix::fs::symlink(&bundle, bundle.join("_cycle")).unwrap();
        std::os::unix::fs::symlink(bundle.join("missing"), bundle.join("nested/broken")).unwrap();

        let page = load_at(root.path(), "example/index.md", MINIMAL).unwrap();

        assert_eq!(page.assets, [bundle.join("visible.txt")]);
    }

    #[cfg(unix)]
    #[test]
    fn reports_non_utf8_section() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let root = tempfile::tempdir().unwrap();
        let content_dir = root.path().join("content");
        let path = content_dir
            .join("posts")
            .join(OsStr::from_bytes(b"section\xff"))
            .join("post.md");

        let error = Page::load(MINIMAL, &path, &content_dir).unwrap_err();
        assert_eq!(
            error.to_string(),
            format!("invalid section in {}", path.display())
        );
    }

    #[cfg(unix)]
    #[test]
    fn reports_unreadable_bundle() {
        for unreadable in ["", "broken"] {
            let root = tempfile::tempdir().unwrap();
            let bundle = root.path().join("content/hello");
            write_test_file(&bundle, "index.md", MINIMAL);
            write_test_file(&bundle, "broken/file.txt", "content");
            let directory = bundle.join(unreadable);

            // Keep execute on the bundle itself so the entry point stays readable by path.
            let mode = if unreadable.is_empty() { 0o111 } else { 0o000 };
            let _guard = PermissionGuard::restrict(&directory, mode);

            let error = Page::load(
                MINIMAL,
                &bundle.join("index.md"),
                &root.path().join("content"),
            )
            .unwrap_err();
            assert!(
                error.to_string().contains("failed to read assets"),
                "{unreadable}: {error:#}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn reports_broken_and_cyclic_asset_symlinks() {
        for cycle in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let assets = root.path().join("content/example/assets");
            std::fs::create_dir_all(&assets).unwrap();
            let target = if cycle {
                assets.clone()
            } else {
                assets.join("missing")
            };
            let link = assets.join("asset");
            std::os::unix::fs::symlink(&target, &link).unwrap();

            let error = load_at(root.path(), "example/index.md", MINIMAL).unwrap_err();

            assert!(error.to_string().contains("failed to read assets"));
            let walk_error = error.downcast_ref::<walkdir::Error>().unwrap();
            assert_eq!(walk_error.path(), Some(link.as_path()));
            if cycle {
                assert_eq!(walk_error.loop_ancestor(), Some(assets.as_path()));
            } else {
                assert_eq!(
                    walk_error.io_error().unwrap().kind(),
                    std::io::ErrorKind::NotFound
                );
            }
        }
    }

    // ── output_dir ──

    #[test]
    fn uses_source_location_and_slug() {
        for (source, slug, expected) in [
            ("posts/foo/bar/index.md", None, "posts/foo/bar"),
            ("example/index.md", None, "example"),
            ("posts/hello-world.md", None, "posts/hello-world"),
            ("index.md", None, ""),
            (
                "posts/note/original/index.md",
                Some("custom"),
                "posts/note/custom",
            ),
            (
                "posts/note/original.md",
                Some("custom"),
                "posts/note/custom",
            ),
            ("index.md", Some("custom"), "custom"),
        ] {
            assert_eq!(
                output_dir(Path::new(source), slug).unwrap(),
                Path::new(expected),
                "{source}, {slug:?}"
            );
        }
    }

    #[test]
    fn rejects_slugs_other_than_one_component() {
        for slug in [
            "",
            ".",
            "..",
            "../other",
            "one/two",
            r"one\two",
            "c:drive",
            "/absolute",
        ] {
            let error = output_dir(Path::new("example/index.md"), Some(slug)).unwrap_err();
            assert_eq!(
                error.to_string(),
                "slug must be a single nonempty path component",
                "{slug}"
            );
        }
    }

    // ── page_kind ──

    #[test]
    fn classifies_content_paths() {
        let note = PageKind::Post {
            section: Some("note".into()),
        };
        let orphan = PageKind::Post { section: None };
        for (path, expected) in [
            ("posts/note/deep/nested/index.md", &note),
            ("posts/note/my-post/index.md", &note),
            ("posts/note/hello.md", &note),
            ("posts/hello/index.md", &orphan),
            ("posts/hello.md", &orphan),
            ("posts.md", &PageKind::Page),
            ("about-me/index.md", &PageKind::Page),
        ] {
            assert_eq!(&page_kind(Path::new(path)).unwrap(), expected, "{path}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn rejects_non_utf8_section() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let path = Path::new("posts")
            .join(OsStr::from_bytes(&[0xff]))
            .join("hello.md");
        let error = page_kind(&path).unwrap_err();
        assert!(error.to_string().ends_with("is not UTF-8"), "{error}");
    }

    // ── extract_summary ──

    #[test]
    fn extracts_text_before_separator() {
        let body = indoc! {r"
            This is the summary.

            <!--more-->

            Full content here.
        "};
        assert_eq!(extract_summary(body).unwrap(), "This is the summary.");
    }

    #[test]
    fn reduces_markup_to_plain_text() {
        for (summary, expected) in [
            (
                indoc! {r"
                    See [the docs][docs-ref] and the [home page].

                    [docs-ref]: https://example.com/docs
                    [home page]: https://example.com
                "},
                "See the docs and the home page.",
            ),
            (
                "A **bold** and *italic* intro with [a link](https://example.com).",
                "A bold and italic intro with a link.",
            ),
            (
                "Text before ![an image](photo.jpg) and after.",
                "Text before and after.",
            ),
            (
                "Use `strip_markdown` to clean text.",
                "Use strip_markdown to clean text.",
            ),
            (
                "Inline $x^2$ and display $$y$$ math.",
                "Inline x^2 and display y math.",
            ),
            (
                r#""Quoted" text -- with 'marks'..."#,
                r#""Quoted" text -- with 'marks'..."#,
            ),
        ] {
            let body = format!("{summary}\n\n<!--more-->\n");
            assert_eq!(extract_summary(&body).unwrap(), expected, "{summary}");
        }
    }

    #[test]
    fn resolves_reference_links_after_separator() {
        let body = indoc! {r"
            See [the project][project-ref] for details.

            <!--more-->

            Full content here.

            [project-ref]: https://example.com/project
        "};
        assert_eq!(
            extract_summary(body).unwrap(),
            "See the project for details."
        );
    }

    #[test]
    fn preserves_paragraph_breaks() {
        let body = indoc! {r"
            First
            paragraph.

            Second\
            paragraph.

            <!--more-->
        "};
        assert_eq!(
            extract_summary(body).unwrap(),
            "First paragraph.\nSecond paragraph."
        );
    }

    #[test]
    fn skips_separator_inside_code() {
        let body = indoc! {r"
            Write `<!--more-->` to split.

            <!--more-->
        "};
        assert_eq!(
            extract_summary(body).unwrap(),
            "Write <!--more--> to split."
        );
    }

    #[test]
    fn none_without_separator_or_summary_text() {
        for body in [
            "No summary separator in this content.",
            indoc! {r"
                Intro.

                ```html
                <!--more-->
                ```
            "},
            indoc! {r"
                <!--more-->

                Content after.
            "},
        ] {
            assert_eq!(extract_summary(body), None, "{body}");
        }
    }

    /// Writes `content` to `relative` under `<root>/content` and parses it.
    fn load_at(root: &Path, relative: &str, content: &str) -> Result<Page> {
        write_test_file(root, &format!("content/{relative}"), content);
        let content_dir = root.join("content");
        Page::load(content, &content_dir.join(relative), &content_dir)
    }
}
