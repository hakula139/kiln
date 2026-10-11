use std::fs;

use indoc::indoc;
use scraper::{Html, Selector};

use kiln::build::BuildOptions;

use super::support::TestSite;

// ── build: options ──

#[test]
fn build_renders_post_with_and_without_minification() {
    let site = TestSite::new(indoc! {r#"
        base_url = "https://example.com"
        title = "Test Site"
    "#});
    site.page(
        "posts/hello",
        indoc! {r#"
            +++
            title = "Hello World"
            description = "A test post"
            date = "2026-02-24T12:34:56Z"
            +++

            ## First

            This is a test **post**.

            ## Second

            More content.
        "#},
    );

    site.build().unwrap();

    let plain = site.read("posts/hello/index.html");
    for expected in [
        "<title>Hello World - Test Site</title>",
        r#"<meta name="description" content="A test post">"#,
        r#"<link rel="canonical" href="https://example.com/posts/hello/">"#,
        "<h1>Hello World</h1>",
        r#"<time datetime="2026-02-24T12:34:56+00:00">"#,
        r##"<a href="#first">First</a>"##,
        "<p>This is a test <strong>post</strong>.</p>",
    ] {
        assert!(
            plain.contains(expected),
            "missing {expected}, html:\n{plain}"
        );
    }

    site.build_with(&BuildOptions {
        minify: true,
        ..Default::default()
    })
    .unwrap();

    let minified = site.read("posts/hello/index.html");
    assert!(minified.len() < plain.len());
    let text = |html: &str, selector: &str| {
        let selector = Selector::parse(selector).unwrap();
        Html::parse_document(html)
            .select(&selector)
            .map(|element| element.text().collect::<String>())
            .collect::<Vec<_>>()
    };
    for selector in ["title", "h1", "h2", ".content p", "strong"] {
        assert_eq!(
            text(&minified, selector),
            text(&plain, selector),
            "{selector}"
        );
    }
}

#[test]
fn build_output_dir_override_preserves_configured_output() {
    let site = TestSite::new("");
    site.titled_page("posts/hello", "Hello")
        .write("public/sentinel.txt", "Keep me");
    let output = tempfile::tempdir().unwrap();
    fs::write(output.path().join("stale.html"), "Stale page").unwrap();

    site.build_with(&BuildOptions {
        output_dir_override: Some(output.path()),
        ..Default::default()
    })
    .unwrap();

    assert_eq!(site.read("sentinel.txt"), "Keep me");
    assert!(!site.output("index.html").exists());
    assert!(!output.path().join("stale.html").exists());
    let html = fs::read_to_string(output.path().join("posts/hello/index.html")).unwrap();
    assert!(html.contains("<h1>Hello</h1>"), "{html}");
    assert!(output.path().join("index.html").is_file());
}

#[test]
fn build_base_url_override() {
    let site = TestSite::new(r#"base_url = "https://example.com""#);
    site.write(
        "templates/post.html",
        r#"<a href="{{ config.base_url }}/">Home</a><link rel="canonical" href="{{ url }}">"#,
    )
    .titled_page("posts/hello", "Hello");

    site.build_with(&BuildOptions {
        base_url_override: Some("http://localhost:5456/"),
        ..Default::default()
    })
    .unwrap();

    assert_eq!(
        site.read("posts/hello/index.html"),
        r#"<a href="http://localhost:5456/">Home</a><link rel="canonical" href="http://localhost:5456/posts/hello/">"#
    );
}
