use std::fs;
use std::path::Path;

use indoc::{formatdoc, indoc};

use kiln::build::{BuildOptions, build};

use super::support::write_test_file;

// ── build ──

#[test]
fn build_routes_share_prefix_metadata_and_sitemap() {
    let root = setup_site();
    write_post(root.path(), "content/posts/one.md", "One");
    write_post(root.path(), "content/posts/two.md", "Two");
    write_test_file(
        root.path(),
        "content/tags/rust/_index.md",
        indoc! {r#"
            +++
            title = "Rust language"
            +++
        "#},
    );
    build(root.path(), BuildOptions::default()).unwrap();
    let output = root.path().join("public");
    let page = fs::read_to_string(output.join("posts/one/index.html")).unwrap();
    assert!(page.contains("2025-01-02T00:00:00Z"));
    assert_eq!(page.matches("Rust language").count(), 1);
    assert!(page.contains("https://example.com/blog/tags/rust/"));
    let home = fs::read_to_string(output.join("page/2/index.html")).unwrap();
    assert!(home.contains("https://example.com/blog/page/2/"));
    assert!(home.contains(r#"href="/blog/""#));
    let archive = fs::read_to_string(output.join("posts/page/2/index.html")).unwrap();
    assert!(archive.contains("https://example.com/blog/posts/page/2/"));
    assert!(archive.contains(r#"href="/blog/posts/""#));
    let overview = fs::read_to_string(output.join("tags/index.html")).unwrap();
    assert!(overview.contains("https://example.com/blog/tags/rust/"));
    let sitemap = fs::read_to_string(output.join("sitemap.xml")).unwrap();
    let locations: Vec<_> = sitemap
        .split("<loc>")
        .skip(1)
        .map(|entry| entry.split("</loc>").next().unwrap())
        .collect();
    assert_eq!(
        locations,
        [
            "https://example.com/blog/",
            "https://example.com/blog/page/2/",
            "https://example.com/blog/posts/",
            "https://example.com/blog/posts/one/",
            "https://example.com/blog/posts/page/2/",
            "https://example.com/blog/posts/two/",
            "https://example.com/blog/sections/",
            "https://example.com/blog/tags/",
            "https://example.com/blog/tags/rust/",
            "https://example.com/blog/tags/rust/page/2/",
        ]
    );
    assert!(sitemap.contains("<lastmod>2025-01-02T00:00:00Z</lastmod>"));
    let feed = fs::read_to_string(output.join("index.xml")).unwrap();
    assert!(feed.contains("<pubDate>Mon, 01 Jan 2024 00:00:00 +0000</pubDate>"));
}

#[test]
fn build_sitemap_only_contains_emitted_html() {
    let root = setup_site();
    for template in ["home", "archive", "overview"] {
        fs::remove_file(root.path().join(format!("templates/{template}.html"))).unwrap();
    }
    write_post(root.path(), "content/index.md", "Root page");
    build(root.path(), BuildOptions::default()).unwrap();
    let xml = fs::read_to_string(root.path().join("public/sitemap.xml")).unwrap();
    assert_eq!(xml.matches("<loc>").count(), 1);
    assert!(xml.contains("<loc>https://example.com/blog/</loc>"));
    fs::remove_file(root.path().join("content/index.md")).unwrap();
    build(root.path(), BuildOptions::default()).unwrap();
    let xml = fs::read_to_string(root.path().join("public/sitemap.xml")).unwrap();
    assert!(!xml.contains("<loc>"));
    assert!(!root.path().join("public/index.html").exists());
}

#[test]
fn build_conflicting_content_generated_and_static_routes_returns_error() {
    for destination in [
        "content/index.md",
        "content/posts/index.md",
        "content/tags/index.md",
        "content/page/2/index.md",
        "static/posts/index.html",
        "static/sitemap.xml",
        "content/posts/one/index.md",
    ] {
        let root = setup_site();
        write_post(root.path(), "content/posts/one.md", "One");
        write_post(root.path(), "content/posts/two.md", "Two");
        write_post(root.path(), destination, "Conflicting");
        let error = build(root.path(), BuildOptions::default()).unwrap_err();
        assert!(
            format!("{error:#}").contains("collision"),
            "{destination}: {error:#}"
        );
    }
}

fn setup_site() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    write_test_file(
        root.path(),
        "config.toml",
        indoc! {r#"
            base_url = "https://example.com/blog/"
            title = "Example"
            [params]
            paginate = 1
        "#},
    );
    for template in ["post", "home", "archive", "overview"] {
        write_test_file(
            root.path(),
            &format!("templates/{template}.html"),
            indoc! {r#"
                <title>{{ title }}</title><link href="{{ url | safe }}">
                {{ description }} {{ updated }} {{ license }}
                {% for tag in tags %}<a href="{{ tag.url | safe }}">{{ tag.name }}</a>{% endfor %}
                {% for page in pages %}<a href="{{ page.url | safe }}">{{ page.title }}</a>{% endfor %}
                {% for bucket in buckets %}<a href="{{ bucket.url | safe }}">{{ bucket.name }}</a>{% endfor %}
                {% if pagination %}{{ pagination.current_page }}/{{ pagination.total_pages }}{% endif %}
                {% if pagination %}{% for item in pagination.items %}<a href="{{ item.url | safe }}">{{ item.number }}</a>{% endfor %}{% endif %}
            "#},
        );
    }
    root
}

fn write_post(root: &Path, path: &str, title: &str) {
    write_test_file(
        root,
        path,
        &formatdoc! {r#"
            +++
            title = "{title}"
            date = "2024-01-01T00:00:00Z"
            updated = "2025-01-02T00:00:00Z"
            tags = [" Rust ", "rust", ""]
            +++
            Summary.
        "#},
    );
}
