use std::fs;
use std::path::{Path, PathBuf};

use indoc::{formatdoc, indoc};
use scraper::{Html, Selector};

#[path = "../support/fixtures.rs"]
mod fixtures;

#[cfg(unix)]
pub(super) use fixtures::PermissionGuard;
pub(super) use fixtures::{copy_templates, write_test_file};

// ── Site fixtures ──

pub(super) fn setup_site_with_page(root: &Path) {
    fs::write(root.join("config.toml"), "").unwrap();
    copy_templates(&root.join("templates"));
    write_page(
        root,
        "posts/hello",
        indoc! {r#"
            +++
            title = "Hello"
            +++
            Body
        "#},
    );
}

pub(super) fn setup_theme(root: &Path, theme_name: &str) {
    let theme_dir = root.join("themes").join(theme_name);
    let tmpl_dir = theme_dir.join("templates");
    fs::create_dir_all(&tmpl_dir).unwrap();
    copy_templates(&tmpl_dir);
    fs::write(theme_dir.join("theme.toml"), "").unwrap();
}

pub(super) fn write_paginated_posts(root: &Path, post_dir: &str, tag: Option<&str>) {
    let tags = tag.map_or_else(String::new, |tag| format!(r#"tags = ["{tag}"]"#));
    for i in 1..=3 {
        write_page(
            root,
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

pub(super) fn write_listing_site(root: &Path) {
    fs::write(
        root.join("config.toml"),
        indoc! {r#"
            base_url = "https://example.com"
            title = "Test Site"
        "#},
    )
    .unwrap();
    copy_templates(&root.join("templates"));

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
        write_page(
            root,
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
}

pub(super) fn write_page(root: &Path, rel_path: &str, content: &str) {
    write_test_file(root, &format!("content/{rel_path}/index.md"), content);
}

pub(super) fn copy_templates_except(dest: &Path, exclude: &[&str]) {
    copy_templates(dest);
    for name in exclude {
        fs::remove_file(dest.join(name)).unwrap();
    }
}

// ── Listing assertions ──

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

pub(super) fn listing_links(html: &str) -> Vec<&str> {
    html.split("<li>")
        .skip(1)
        .map(|item| {
            let start = item.find("<a ").unwrap();
            let end = item.find("</a>").unwrap() + "</a>".len();
            &item[start..end]
        })
        .collect()
}

// ── Stylesheet assertions ──

pub(super) fn stylesheet_url(html: &str) -> String {
    let document = Html::parse_document(html);
    let selector = Selector::parse(r#"link[rel="stylesheet"]"#).unwrap();
    document
        .select(&selector)
        .next()
        .unwrap()
        .value()
        .attr("href")
        .unwrap()
        .to_owned()
}

pub(super) fn published_path(root: &Path, url: &str) -> PathBuf {
    root.join(url.strip_prefix("/subsite/").unwrap())
}
