use std::fs;

use indoc::{formatdoc, indoc};
use scraper::{Html, Selector};

use super::support::{TestSite, listing_site};

// ── build: listings ──

#[test]
fn build_orders_listings_and_overviews() {
    let site = listing_site();

    site.build().unwrap();

    for (path, expected) in [
        (
            "index.html",
            vec![
                r#"<a href="https://example.com/posts/note/older/">Post A</a>"#,
                r#"<a href="https://example.com/posts/review/latest/">Post C</a>"#,
                r#"<a href="https://example.com/posts/note/newer/">Post B</a>"#,
            ],
        ),
        (
            "posts/index.html",
            vec![
                r#"<a href="https://example.com/posts/review/latest/">Post C</a>"#,
                r#"<a href="https://example.com/posts/note/newer/">Post B</a>"#,
                r#"<a href="https://example.com/posts/note/older/">Post A</a>"#,
            ],
        ),
        (
            "posts/note/index.html",
            vec![
                r#"<a href="https://example.com/posts/note/newer/">Post B</a>"#,
                r#"<a href="https://example.com/posts/note/older/">Post A</a>"#,
            ],
        ),
        (
            "tags/rust/index.html",
            vec![
                r#"<a href="https://example.com/about/">About</a>"#,
                r#"<a href="https://example.com/posts/review/latest/">Post C</a>"#,
                r#"<a href="https://example.com/posts/note/older/">Post A</a>"#,
            ],
        ),
        (
            "sections/index.html",
            vec![
                r#"<a href="https://example.com/posts/note/">Note</a>"#,
                r#"<a href="https://example.com/posts/note/newer/">Post B</a>"#,
                r#"<a href="https://example.com/posts/note/older/">Post A</a>"#,
                r#"<a href="https://example.com/posts/review/">Review</a>"#,
                r#"<a href="https://example.com/posts/review/latest/">Post C</a>"#,
            ],
        ),
        (
            "tags/index.html",
            vec![
                r#"<a href="https://example.com/tags/rust/">rust</a>"#,
                r#"<a href="https://example.com/about/">About</a>"#,
                r#"<a href="https://example.com/posts/review/latest/">Post C</a>"#,
                r#"<a href="https://example.com/posts/note/older/">Post A</a>"#,
                r#"<a href="https://example.com/tags/testing/">testing</a>"#,
                r#"<a href="https://example.com/posts/note/newer/">Post B</a>"#,
            ],
        ),
    ] {
        assert_eq!(listing_links(&site.read(path)), expected, "{path}");
    }
    assert!(site.read("posts/note/index.html").contains("<h1>Note</h1>"));
}

#[test]
fn build_lists_orphan_posts_on_home_but_not_in_sections() {
    let site = TestSite::new("");
    for (path, title, day) in [
        ("posts/note/sectioned", "Sectioned Post", 1),
        ("posts/orphan", "Orphan Post", 2),
    ] {
        site.page(
            path,
            &formatdoc! {r#"
                +++
                title = "{title}"
                date = "2026-01-0{day}T00:00:00Z"
                +++
                Body
            "#},
        );
    }

    site.build().unwrap();

    let sectioned = r#"<a href="http://localhost:5456/posts/note/sectioned/">Sectioned Post</a>"#;
    let orphan = r#"<a href="http://localhost:5456/posts/orphan/">Orphan Post</a>"#;
    assert_eq!(listing_links(&site.read("index.html")), [orphan, sectioned]);
    assert_eq!(
        listing_links(&site.read("posts/index.html")),
        [orphan, sectioned]
    );
    assert_eq!(
        listing_links(&site.read("posts/note/index.html")),
        [sectioned]
    );
}

#[test]
fn build_tag_archive_excludes_untagged_standalone_pages() {
    let site = TestSite::new("");
    site.titled_page("about-me", "About Me").page(
        "posts/note/hello",
        indoc! {r#"
            +++
            title = "Hello Post"
            tags = ["rust"]
            date = "2026-01-01T00:00:00Z"
            +++
            Body
        "#},
    );

    site.build().unwrap();

    assert_eq!(
        listing_links(&site.read("tags/rust/index.html")),
        [r#"<a href="http://localhost:5456/posts/note/hello/">Hello Post</a>"#]
    );
}

#[test]
fn build_index_files_title_archives() {
    let site = TestSite::new("");
    for (path, title) in [("posts", "文章"), ("posts/note", "笔记")] {
        site.write(
            &format!("content/{path}/_index.md"),
            &formatdoc! {r#"
                +++
                title = "{title}"
                +++
            "#},
        );
    }
    site.titled_page("posts/note/hello", "Hello");

    site.build().unwrap();

    for (path, title) in [("posts", "文章"), ("posts/note", "笔记")] {
        let html = site.read(&format!("{path}/index.html"));
        assert!(
            html.contains(&format!("<h1>{title}</h1>")),
            "{path}:\n{html}"
        );
    }
}

#[test]
fn build_empty_listing_states_generate_listings_feed_and_error_page() {
    for content in [None, Some("about"), Some("posts/hello")] {
        let site = TestSite::new("");
        site.write("i18n/en.toml", r#"error_404_title = "Page Missing""#);
        if let Some(path) = content {
            site.titled_page(path, "Hello");
        }

        site.build().unwrap();

        for path in [
            "index.html",
            "posts/index.html",
            "sections/index.html",
            "tags/index.html",
        ] {
            let expected = if content == Some("posts/hello")
                && matches!(path, "index.html" | "posts/index.html")
            {
                vec![r#"<a href="http://localhost:5456/posts/hello/">Hello</a>"#]
            } else {
                Vec::new()
            };
            assert_eq!(
                listing_links(&site.read(path)),
                expected,
                "{content:?}: {path}"
            );
        }
        let feed = site.read("index.xml");
        assert_eq!(
            feed.matches("<item>").count(),
            usize::from(content == Some("posts/hello"))
        );
        assert!(!feed.contains("<lastBuildDate>"), "{feed}");
        let error = site.read("404.html");
        assert!(error.contains("<h1>Page Missing</h1>"), "{error}");
    }
}

// ── build: pagination ──

#[test]
fn build_home_pagination() {
    let site = TestSite::new(indoc! {r"
        [params.home]
        paginate = 2
    "});
    write_paginated_posts(&site, "posts/note", None);
    site.titled_page("about", "About");

    site.build().unwrap();

    assert_paginated_listing(
        &site,
        "",
        &[
            r#"<a href="http://localhost:5456/posts/note/post-3/">Post 3</a>"#,
            r#"<a href="http://localhost:5456/posts/note/post-2/">Post 2</a>"#,
        ],
        &[r#"<a href="http://localhost:5456/posts/note/post-1/">Post 1</a>"#],
        None,
    );
}

#[test]
fn build_posts_index_and_section_pagination() {
    let site = TestSite::new(indoc! {r#"
        base_url = "https://example.com"

        [params.section]
        paginate = 2
    "#});
    write_paginated_posts(&site, "posts/note", None);
    site.titled_page("about", "About").page(
        "posts/review/other",
        indoc! {r#"
            +++
            title = "Other Post"
            date = "2026-01-04T00:00:00Z"
            +++
            Body
        "#},
    );

    site.build().unwrap();

    assert_paginated_listing(
        &site,
        "posts",
        &[
            r#"<a href="https://example.com/posts/review/other/">Other Post</a>"#,
            r#"<a href="https://example.com/posts/note/post-3/">Post 3</a>"#,
        ],
        &[
            r#"<a href="https://example.com/posts/note/post-2/">Post 2</a>"#,
            r#"<a href="https://example.com/posts/note/post-1/">Post 1</a>"#,
        ],
        Some(("/posts/page/2/", "/posts/")),
    );
    assert_paginated_listing(
        &site,
        "posts/note",
        &[
            r#"<a href="https://example.com/posts/note/post-3/">Post 3</a>"#,
            r#"<a href="https://example.com/posts/note/post-2/">Post 2</a>"#,
        ],
        &[r#"<a href="https://example.com/posts/note/post-1/">Post 1</a>"#],
        Some(("/posts/note/page/2/", "/posts/note/")),
    );
}

#[test]
fn build_tag_archive_pagination() {
    let site = TestSite::new(indoc! {r#"
        base_url = "https://example.com"

        [params]
        paginate = 2
    "#});
    write_paginated_posts(&site, "posts", Some("rust"));
    site.page(
        "about",
        indoc! {r#"
            +++
            title = "About"
            tags = ["rust"]
            +++
            Body
        "#},
    )
    .page(
        "posts/other",
        indoc! {r#"
            +++
            title = "Other Post"
            date = "2026-01-04T00:00:00Z"
            tags = ["web"]
            +++
            Body
        "#},
    );

    site.build().unwrap();

    assert_paginated_listing(
        &site,
        "tags/rust",
        &[
            r#"<a href="https://example.com/posts/post-3/">Post 3</a>"#,
            r#"<a href="https://example.com/posts/post-2/">Post 2</a>"#,
        ],
        &[
            r#"<a href="https://example.com/posts/post-1/">Post 1</a>"#,
            r#"<a href="https://example.com/about/">About</a>"#,
        ],
        Some(("/tags/rust/page/2/", "/tags/rust/")),
    );
}

/// Writes posts 1 to 3 under `post_dir` on consecutive days, each tagged with `tag` if given.
fn write_paginated_posts(site: &TestSite, post_dir: &str, tag: Option<&str>) {
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

/// Asserts that the listing published under `dir` has exactly two pages with the given links,
/// and, when given, next / previous links between them.
fn assert_paginated_listing(
    site: &TestSite,
    dir: &str,
    page1_links: &[&str],
    page2_links: &[&str],
    navigation_urls: Option<(&str, &str)>,
) {
    let output = site.output(dir);
    let html1 = fs::read_to_string(output.join("index.html")).unwrap();
    let html2 = fs::read_to_string(output.join("page/2/index.html")).unwrap();

    assert_eq!(listing_links(&html1), page1_links, "{dir}");
    assert_eq!(listing_links(&html2), page2_links, "{dir}");
    assert!(html1.contains("Page 1 / 2"));
    assert!(html2.contains("Page 2 / 2"));
    assert!(!output.join("page/3/index.html").exists());
    if let Some((next_url, prev_url)) = navigation_urls {
        assert!(html1.contains(&format!(r#"<a href="{next_url}">Next →</a>"#)));
        assert!(html2.contains(&format!(r#"<a href="{prev_url}">← Prev</a>"#)));
        assert!(!html1.contains("← Prev"));
        assert!(!html2.contains("Next →"));
    }
}

/// Returns every link directly inside a list item, serialized in document order.
fn listing_links(html: &str) -> Vec<String> {
    let selector = Selector::parse("li > a").unwrap();
    Html::parse_document(html)
        .select(&selector)
        .map(|link| link.html())
        .collect()
}
