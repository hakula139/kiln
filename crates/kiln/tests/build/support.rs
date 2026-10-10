use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use indoc::{formatdoc, indoc};
use scraper::{Html, Selector};
use sha2::{Digest, Sha256};
use tempfile::TempDir;

use kiln::build::{BuildOptions, build};

use super::fixtures::{copy_templates, write_test_file};

// ── Site fixtures ──

/// Temporary site root whose files are removed on drop.
pub(super) struct TestSite {
    dir: TempDir,
}

impl TestSite {
    /// Creates a site with `config` as `config.toml` and a copy of the fixture templates.
    pub(super) fn new(config: &str) -> Self {
        let site = Self::empty();
        site.write("config.toml", config).templates("templates");
        site
    }

    /// Creates a site without configuration or templates.
    pub(super) fn empty() -> Self {
        Self {
            dir: tempfile::tempdir().unwrap(),
        }
    }

    /// Creates an empty site whose root directory name starts with `prefix`.
    pub(super) fn with_prefix(prefix: &str) -> Self {
        Self {
            dir: tempfile::Builder::new().prefix(prefix).tempdir().unwrap(),
        }
    }

    /// Selects theme `example` configured by `theme_toml`, replacing `config.toml`.
    pub(super) fn theme(&self, theme_toml: &str) -> &Self {
        fs::create_dir_all(self.path("themes/example/templates")).unwrap();
        self.write("config.toml", r#"theme = "example""#)
            .write("themes/example/theme.toml", theme_toml)
    }

    /// Copies the fixture templates into `rel_dir`.
    pub(super) fn templates(&self, rel_dir: &str) -> &Self {
        copy_templates(&self.path(rel_dir));
        self
    }

    pub(super) fn write(&self, rel_path: &str, content: &str) -> &Self {
        write_test_file(self.root(), rel_path, content);
        self
    }

    /// Writes a page bundle at `content/{rel_path}/index.md`.
    pub(super) fn page(&self, rel_path: &str, source: &str) -> &Self {
        self.write(&format!("content/{rel_path}/index.md"), source)
    }

    /// Writes a page bundle with `title` and a one-word body.
    pub(super) fn titled_page(&self, rel_path: &str, title: &str) -> &Self {
        self.page(
            rel_path,
            &formatdoc! {r#"
                +++
                title = "{title}"
                +++
                Body
            "#},
        )
    }

    /// Writes a solid black PNG, so images of different sizes have different bytes.
    pub(super) fn write_png(&self, rel_path: &str, width: u32, height: u32) -> &Self {
        let path = self.path(rel_path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        image::RgbaImage::from_pixel(width, height, image::Rgba([0, 0, 0, 255]))
            .save(path)
            .unwrap();
        self
    }

    /// Writes an executable script and returns its absolute path.
    #[cfg(unix)]
    pub(super) fn write_executable(&self, rel_path: &str, content: &str) -> PathBuf {
        self.write(rel_path, content);
        let path = self.path(rel_path);
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    pub(super) fn remove(&self, rel_path: &str) -> &Self {
        fs::remove_file(self.path(rel_path)).unwrap();
        self
    }

    pub(super) fn build(&self) -> anyhow::Result<()> {
        self.build_with(BuildOptions::default())
    }

    pub(super) fn build_with(&self, options: BuildOptions<'_>) -> anyhow::Result<()> {
        build(self.root(), options)
    }

    pub(super) fn root(&self) -> &Path {
        self.dir.path()
    }

    pub(super) fn path(&self, rel_path: &str) -> PathBuf {
        self.root().join(rel_path)
    }

    pub(super) fn output(&self, rel_path: &str) -> PathBuf {
        self.root().join("public").join(rel_path)
    }

    /// Reads a published file below `public/`.
    pub(super) fn read(&self, rel_path: &str) -> String {
        fs::read_to_string(self.output(rel_path)).unwrap()
    }
}

// ── CLI ──

/// Returns a `kiln` command isolated from the caller's base URL and log filter.
pub(super) fn kiln() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_kiln"));
    command.env_remove("KILN_BASE_URL").env_remove("RUST_LOG");
    command
}

// ── Listing fixtures ──

/// Creates a site with posts in two sections and a tagged standalone page, where the oldest
/// post is pinned by weight.
pub(super) fn listing_site() -> TestSite {
    let site = TestSite::new(indoc! {r#"
        base_url = "https://example.com"
        title = "Test Site"
    "#});
    for (path, title, tags, date, weight) in [
        (
            "posts/note/older",
            "Post A",
            "rust",
            "2026-01-01",
            "weight = 1",
        ),
        ("posts/note/newer", "Post B", "testing", "2026-01-02", ""),
        ("posts/review/latest", "Post C", "rust", "2026-01-03", ""),
        ("about", "About", "rust", "2026-01-04", ""),
    ] {
        site.page(
            path,
            &formatdoc! {r#"
                +++
                title = "{title}"
                tags = ["{tags}"]
                date = "{date}T00:00:00Z"
                {weight}
                +++
                Body
            "#},
        );
    }
    site
}

/// Writes posts 1 to 3 under `post_dir` on consecutive days, each tagged with `tag` if given.
pub(super) fn write_paginated_posts(site: &TestSite, post_dir: &str, tag: Option<&str>) {
    let tags = tag.map_or_else(String::new, |tag| format!(r#"tags = ["{tag}"]"#));
    for i in 1..=3 {
        site.page(
            &format!("{post_dir}/post-{i}"),
            &formatdoc! {r#"
                +++
                title = "Post {i}"
                date = "2026-01-0{i}T00:00:00Z"
                {tags}
                +++
                Body
            "#},
        );
    }
}

pub(super) fn assert_paginated_listing(
    output_dir: &Path,
    page1_links: &[&str],
    page2_links: &[&str],
    navigation_urls: Option<(&str, &str)>,
) {
    let html1 = fs::read_to_string(output_dir.join("index.html")).unwrap();
    let html2 = fs::read_to_string(output_dir.join("page/2/index.html")).unwrap();

    assert_eq!(listing_links(&html1), page1_links);
    assert_eq!(listing_links(&html2), page2_links);
    assert!(html1.contains("Page 1 / 2"));
    assert!(html2.contains("Page 2 / 2"));
    assert!(!output_dir.join("page/3/index.html").exists());
    if let Some((next_url, prev_url)) = navigation_urls {
        assert!(html1.contains(&format!(r#"<a href="{next_url}">Next →</a>"#)));
        assert!(html2.contains(&format!(r#"<a href="{prev_url}">← Prev</a>"#)));
        assert!(!html1.contains("← Prev"));
        assert!(!html2.contains("Next →"));
    }
}

// ── Output inspection ──

/// Returns the content fingerprint kiln embeds in published asset names.
pub(super) fn fingerprint(bytes: impl AsRef<[u8]>) -> String {
    hex::encode(Sha256::digest(bytes))[..12].to_owned()
}

/// Returns every link directly inside a list item, serialized in document order.
pub(super) fn listing_links(html: &str) -> Vec<String> {
    let selector = Selector::parse("li > a").unwrap();
    Html::parse_document(html)
        .select(&selector)
        .map(|link| link.html())
        .collect()
}

/// Returns the URL of the only stylesheet linked from `html`.
pub(super) fn stylesheet_url(html: &str) -> String {
    let [url] = <[String; 1]>::try_from(stylesheet_urls(html)).unwrap();
    url
}

/// Returns the URLs of every stylesheet linked from `html`, in document order.
pub(super) fn stylesheet_urls(html: &str) -> Vec<String> {
    let selector = Selector::parse(r#"link[rel="stylesheet"]"#).unwrap();
    Html::parse_document(html)
        .select(&selector)
        .map(|link| link.value().attr("href").unwrap().to_owned())
        .collect()
}

/// Returns the text of every `<tag>` element in feed or sitemap XML, in document order.
pub(super) fn xml_texts<'a>(xml: &'a str, tag: &str) -> Vec<&'a str> {
    let close = format!("</{tag}>");
    xml.split(&format!("<{tag}>"))
        .skip(1)
        .map(|element| element.split_once(&close).unwrap().0)
        .collect()
}
