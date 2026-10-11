use indoc::{formatdoc, indoc};

use super::support::{TestSite, xml_texts};

// ── build: routes ──

#[test]
fn build_routes_share_prefix_metadata_and_sitemap() {
    let site = route_site();
    write_post(&site, "content/posts/one.md", "One");
    write_post(&site, "content/posts/two.md", "Two");
    site.write(
        "content/tags/rust/_index.md",
        indoc! {r#"
            +++
            title = "Rust language"
            +++
        "#},
    );

    site.build().unwrap();

    let page = site.read("posts/one/index.html");
    assert!(page.contains("2025-01-02T00:00:00+00:00"));
    assert_eq!(page.matches("Rust language").count(), 1);
    assert!(page.contains("https://example.com/blog/tags/rust/"));
    let home = site.read("page/2/index.html");
    assert!(home.contains("https://example.com/blog/page/2/"));
    assert!(home.contains(r#"href="/blog/""#));
    let archive = site.read("posts/page/2/index.html");
    assert!(archive.contains("https://example.com/blog/posts/page/2/"));
    assert!(archive.contains(r#"href="/blog/posts/""#));
    let overview = site.read("tags/index.html");
    assert!(overview.contains("https://example.com/blog/tags/rust/"));
    let sitemap = site.read("sitemap.xml");
    assert_eq!(
        xml_texts(&sitemap, "loc"),
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
    let robots = site.read("robots.txt");
    assert!(robots.contains("Sitemap: https://example.com/blog/sitemap.xml"));
    let feed = site.read("index.xml");
    assert!(feed.contains("<pubDate>Mon, 01 Jan 2024 00:00:00 +0000</pubDate>"));
}

#[test]
fn build_sitemap_only_contains_emitted_html() {
    let site = route_site();
    for template in ["home", "archive", "overview"] {
        site.remove(&format!("templates/{template}.html"));
    }
    write_post(&site, "content/index.md", "Root page");
    write_post(&site, "content/posts/note/item.md", "Section post");

    site.build().unwrap();

    assert_eq!(
        xml_texts(&site.read("sitemap.xml"), "loc"),
        [
            "https://example.com/blog/",
            "https://example.com/blog/posts/note/item/",
        ]
    );
    for path in [
        "posts/index.html",
        "posts/note/index.html",
        "tags/rust/index.html",
        "sections/index.html",
        "tags/index.html",
    ] {
        assert!(!site.output(path).exists(), "{path}");
    }

    site.remove("content/index.md")
        .remove("content/posts/note/item.md");
    site.build().unwrap();

    assert_eq!(
        xml_texts(&site.read("sitemap.xml"), "loc"),
        Vec::<&str>::new()
    );
    assert!(!site.output("index.html").exists());
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
        let site = route_site();
        write_post(&site, "content/posts/one.md", "One");
        write_post(&site, "content/posts/two.md", "Two");
        write_post(&site, destination, "Conflicting");

        let error = site.build().unwrap_err();

        assert!(
            format!("{error:#}").contains("collision"),
            "{destination}: {error:#}"
        );
    }
}

/// Creates a site under a base URL path with one post per page and templates that print every
/// route-dependent variable.
fn route_site() -> TestSite {
    let site = TestSite::empty();
    site.write(
        "config.toml",
        indoc! {r#"
            base_url = "https://example.com/blog/"
            title = "Example"
            [params]
            paginate = 1
        "#},
    );
    for template in ["post", "home", "archive", "overview"] {
        site.write(
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
    site
}

fn write_post(site: &TestSite, path: &str, title: &str) {
    site.write(
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
