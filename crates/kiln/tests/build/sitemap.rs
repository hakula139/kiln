use std::fs;

use indoc::indoc;

use kiln::build::{BuildOptions, build};

use super::support::{copy_templates, write_page};

// ── build: sitemap + robots.txt ──

#[test]
fn build_generates_sitemap_and_robots() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("config.toml"),
        indoc! {r#"
            base_url = "https://example.com"
        "#},
    )
    .unwrap();
    copy_templates(&root.path().join("templates"));

    write_page(
        root.path(),
        "posts/hello",
        indoc! {r#"
            +++
            title = "Hello"
            date = "2026-01-15T00:00:00Z"
            +++
            Body
        "#},
    );

    build(root.path(), BuildOptions::default()).unwrap();

    let output_dir = root.path().join("public");

    let sitemap = fs::read_to_string(output_dir.join("sitemap.xml")).unwrap();
    assert!(
        sitemap.contains("<loc>https://example.com/</loc>"),
        "sitemap should contain home URL, xml:\n{sitemap}"
    );
    assert!(
        sitemap.contains("<loc>https://example.com/posts/hello/</loc>"),
        "sitemap should contain post URL, xml:\n{sitemap}"
    );
    assert!(
        sitemap.contains("<lastmod>"),
        "sitemap should have lastmod for dated page, xml:\n{sitemap}"
    );

    let robots = fs::read_to_string(output_dir.join("robots.txt")).unwrap();
    assert!(
        robots.contains("Sitemap: https://example.com/sitemap.xml"),
        "robots.txt should reference sitemap, txt:\n{robots}"
    );
}
