use std::fs;

use indoc::{formatdoc, indoc};

use kiln::build::{BuildOptions, build};

use super::support::{
    assert_paginated_listing, copy_templates, copy_templates_except, listing_links,
    write_listing_site, write_page, write_paginated_posts, write_test_file,
};

// ── build: home page ──

#[test]
fn build_generates_home_page() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("config.toml"), "").unwrap();
    copy_templates(&root.path().join("templates"));

    write_page(
        root.path(),
        "posts/note/hello",
        indoc! {r#"
            +++
            title = "Hello"
            date = "2026-01-01T00:00:00Z"
            +++
            Body
        "#},
    );

    build(root.path(), BuildOptions::default()).unwrap();

    let home = root.path().join("public").join("index.html");
    assert!(home.exists(), "should generate home page /index.html");
    let html = fs::read_to_string(&home).unwrap();
    assert!(
        html.contains("Hello"),
        "home page should list posts, html:\n{html}"
    );
    assert!(
        html.contains(r#"<a href="http://localhost:5456/posts/note/hello/">Hello</a>"#),
        "home page should link to the post under /posts/, html:\n{html}"
    );
}

#[test]
fn build_home_orders_by_date_descending() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("config.toml"), "").unwrap();
    copy_templates(&root.path().join("templates"));

    write_page(
        root.path(),
        "posts/aaa-old",
        indoc! {r#"
            +++
            title = "Old Post"
            date = "2025-01-01T00:00:00Z"
            +++
            Body
        "#},
    );
    write_page(
        root.path(),
        "posts/zzz-new",
        indoc! {r#"
            +++
            title = "New Post"
            date = "2026-06-01T00:00:00Z"
            +++
            Body
        "#},
    );

    build(root.path(), BuildOptions::default()).unwrap();

    let html = fs::read_to_string(root.path().join("public").join("index.html")).unwrap();
    let new_pos = html.find("New Post").expect("should list New Post");
    let old_pos = html.find("Old Post").expect("should list Old Post");
    assert!(
        new_pos < old_pos,
        "newer post should appear before older post on home page, html:\n{html}"
    );
}

#[test]
fn build_pins_home_without_reordering_archives() {
    let root = tempfile::tempdir().unwrap();
    write_listing_site(root.path());

    build(root.path(), BuildOptions::default()).unwrap();

    let output_dir = root.path().join("public");
    let home = fs::read_to_string(output_dir.join("index.html")).unwrap();
    assert_eq!(
        listing_links(&home),
        [
            r#"<a href="https://example.com/posts/note/older/">Post A</a>"#,
            r#"<a href="https://example.com/posts/review/latest/">Post C</a>"#,
            r#"<a href="https://example.com/posts/note/newer/">Post B</a>"#,
        ]
    );
    for (path, expected) in [
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
    ] {
        let html = fs::read_to_string(output_dir.join(path)).unwrap();
        assert_eq!(listing_links(&html), expected, "incorrect order in {path}");
    }
}

#[test]
fn build_orphan_posts_on_home_not_in_sections() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("config.toml"), "").unwrap();
    copy_templates(&root.path().join("templates"));

    write_page(
        root.path(),
        "posts/note/sectioned",
        indoc! {r#"
            +++
            title = "Sectioned Post"
            date = "2026-01-01T00:00:00Z"
            +++
            Body
        "#},
    );
    write_page(
        root.path(),
        "posts/orphan",
        indoc! {r#"
            +++
            title = "Orphan Post"
            date = "2026-01-02T00:00:00Z"
            +++
            Body
        "#},
    );

    build(root.path(), BuildOptions::default()).unwrap();

    let home_html = fs::read_to_string(root.path().join("public").join("index.html")).unwrap();
    assert!(
        home_html.contains("Sectioned Post"),
        "sectioned post should also appear on home page, html:\n{home_html}"
    );
    assert!(
        home_html.contains("Orphan Post"),
        "orphan post should appear on home page, html:\n{home_html}"
    );

    let note_html = fs::read_to_string(
        root.path()
            .join("public")
            .join("posts")
            .join("note")
            .join("index.html"),
    )
    .unwrap();
    assert!(
        note_html.contains("Sectioned Post"),
        "sectioned post should appear in section page, html:\n{note_html}"
    );
    assert!(
        !note_html.contains("Orphan Post"),
        "orphan post should NOT appear in section page, html:\n{note_html}"
    );
}

#[test]
fn build_home_pagination() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("config.toml"),
        indoc! {r#"
            base_url = "https://example.com"

            [params.home]
            paginate = 2
        "#},
    )
    .unwrap();
    copy_templates(&root.path().join("templates"));

    write_paginated_posts(root.path(), "posts/note", None);

    write_page(
        root.path(),
        "about",
        indoc! {r#"
            +++
            title = "About"
            tags = ["rust"]
            +++
            Body
        "#},
    );

    build(root.path(), BuildOptions::default()).unwrap();

    assert_paginated_listing(
        &root.path().join("public"),
        &[
            r#"<a href="https://example.com/posts/note/post-3/">Post 3</a>"#,
            r#"<a href="https://example.com/posts/note/post-2/">Post 2</a>"#,
        ],
        &[r#"<a href="https://example.com/posts/note/post-1/">Post 1</a>"#],
        None,
    );
}

#[test]
fn build_standalone_excluded_from_home() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("config.toml"), "").unwrap();
    copy_templates(&root.path().join("templates"));

    write_page(
        root.path(),
        "posts/note/hello",
        indoc! {r#"
            +++
            title = "Hello Post"
            date = "2026-01-01T00:00:00Z"
            +++
            Body
        "#},
    );
    write_page(
        root.path(),
        "about-me",
        indoc! {r#"
            +++
            title = "About Me"
            +++
            Bio
        "#},
    );

    build(root.path(), BuildOptions::default()).unwrap();

    let html = fs::read_to_string(root.path().join("public").join("index.html")).unwrap();
    assert!(
        html.contains("Hello Post"),
        "home page should list posts, html:\n{html}"
    );
    assert!(
        !html.contains("About Me"),
        "home page should NOT list standalone pages, html:\n{html}"
    );
}

#[test]
fn build_skips_home_without_template() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("config.toml"), "").unwrap();
    copy_templates_except(&root.path().join("templates"), &["home.html"]);

    write_page(
        root.path(),
        "posts/note/hello",
        indoc! {r#"
            +++
            title = "Hello"
            +++
            Body
        "#},
    );

    build(root.path(), BuildOptions::default()).unwrap();

    let home = root.path().join("public").join("index.html");
    assert!(
        !home.exists(),
        "should NOT generate home page without home.html template"
    );
}

// ── build: posts index ──

#[test]
fn build_generates_posts_index() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("config.toml"), "").unwrap();
    copy_templates(&root.path().join("templates"));

    write_page(
        root.path(),
        "posts/note/post-a",
        indoc! {r#"
            +++
            title = "Post A"
            date = "2026-01-01T00:00:00Z"
            +++
            Body
        "#},
    );
    write_page(
        root.path(),
        "posts/essay/post-b",
        indoc! {r#"
            +++
            title = "Post B"
            date = "2026-01-02T00:00:00Z"
            +++
            Body
        "#},
    );

    build(root.path(), BuildOptions::default()).unwrap();

    let posts_index = root.path().join("public").join("posts").join("index.html");
    assert!(posts_index.exists(), "should generate /posts/index.html");
    let html = fs::read_to_string(&posts_index).unwrap();
    assert!(
        html.contains("Post A") && html.contains("Post B"),
        "posts index should list all posts across sections, html:\n{html}"
    );
}

#[test]
fn build_posts_index_uses_index_title() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("config.toml"), "").unwrap();
    copy_templates(&root.path().join("templates"));

    let posts_dir = root.path().join("content").join("posts");
    fs::create_dir_all(&posts_dir).unwrap();
    fs::write(
        posts_dir.join("_index.md"),
        indoc! {r#"
            +++
            title = "文章"
            +++
        "#},
    )
    .unwrap();

    write_page(
        root.path(),
        "posts/note/hello",
        indoc! {r#"
            +++
            title = "Hello"
            date = "2026-01-01T00:00:00Z"
            +++
            Body
        "#},
    );

    build(root.path(), BuildOptions::default()).unwrap();

    let html =
        fs::read_to_string(root.path().join("public").join("posts").join("index.html")).unwrap();
    assert!(
        html.contains("文章"),
        "should use _index.md title for posts index, html:\n{html}"
    );
}

#[test]
fn build_posts_index_pagination() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("config.toml"),
        indoc! {r#"
            base_url = "https://example.com"

            [params.section]
            paginate = 2
        "#},
    )
    .unwrap();
    copy_templates(&root.path().join("templates"));

    write_paginated_posts(root.path(), "posts/note", None);

    write_page(
        root.path(),
        "about",
        indoc! {r#"
            +++
            title = "About"
            tags = ["rust"]
            +++
            Body
        "#},
    );

    build(root.path(), BuildOptions::default()).unwrap();

    assert_paginated_listing(
        &root.path().join("public/posts"),
        &[
            r#"<a href="https://example.com/posts/note/post-3/">Post 3</a>"#,
            r#"<a href="https://example.com/posts/note/post-2/">Post 2</a>"#,
        ],
        &[r#"<a href="https://example.com/posts/note/post-1/">Post 1</a>"#],
        Some(("/posts/page/2/", "/posts/")),
    );
}

#[test]
fn build_posts_index_generated_even_when_empty() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("config.toml"), "").unwrap();
    copy_templates(&root.path().join("templates"));

    write_page(
        root.path(),
        "about-me",
        indoc! {r#"
            +++
            title = "About Me"
            +++
            Bio
        "#},
    );

    build(root.path(), BuildOptions::default()).unwrap();

    let posts_index = root.path().join("public").join("posts").join("index.html");
    assert!(
        posts_index.exists(),
        "should generate /posts/index.html even with no posts"
    );
}

// ── build: section pages ──

#[test]
fn build_generates_section_pages() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("config.toml"), "").unwrap();
    copy_templates(&root.path().join("templates"));

    for (section, name) in [("note", "post-a"), ("note", "post-b"), ("essay", "hello")] {
        write_page(
            root.path(),
            &format!("posts/{section}/{name}"),
            &format!(
                indoc! {r#"
                    +++
                    title = "{name}"
                    date = "2026-01-01T00:00:00Z"
                    +++
                    Body
                "#},
                name = name,
            ),
        );
    }

    build(root.path(), BuildOptions::default()).unwrap();

    let output_dir = root.path().join("public");
    let note_index = output_dir.join("posts").join("note").join("index.html");
    assert!(
        note_index.exists(),
        "should generate /posts/note/index.html"
    );
    let html = fs::read_to_string(&note_index).unwrap();
    assert!(
        html.contains("Note"),
        "should have section title, html:\n{html}"
    );
    assert!(
        html.contains("post-a") && html.contains("post-b"),
        "should list section posts, html:\n{html}"
    );
    assert!(
        html.contains(r#"<a href="http://localhost:5456/posts/note/post-a/">post-a</a>"#),
        "section page should link to posts under /posts/, html:\n{html}"
    );

    let essay_index = output_dir.join("posts").join("essay").join("index.html");
    assert!(
        essay_index.exists(),
        "should generate /posts/essay/index.html"
    );
}

#[test]
fn build_section_uses_index_title() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("config.toml"), "").unwrap();
    copy_templates(&root.path().join("templates"));

    let section_dir = root.path().join("content").join("posts").join("note");
    fs::create_dir_all(&section_dir).unwrap();
    fs::write(
        section_dir.join("_index.md"),
        indoc! {r#"
            +++
            title = "笔记"
            +++
        "#},
    )
    .unwrap();

    write_page(
        root.path(),
        "posts/note/my-post",
        indoc! {r#"
            +++
            title = "My Post"
            +++
            Body
        "#},
    );

    build(root.path(), BuildOptions::default()).unwrap();

    let html = fs::read_to_string(
        root.path()
            .join("public")
            .join("posts")
            .join("note")
            .join("index.html"),
    )
    .unwrap();
    assert!(
        html.contains("笔记"),
        "should use _index.md title, html:\n{html}"
    );
}

#[test]
fn build_section_pagination() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("config.toml"),
        indoc! {r#"
            base_url = "https://example.com"

            [params.section]
            paginate = 2
        "#},
    )
    .unwrap();
    copy_templates(&root.path().join("templates"));

    write_paginated_posts(root.path(), "posts/note", None);

    write_page(
        root.path(),
        "about",
        indoc! {r#"
            +++
            title = "About"
            tags = ["rust"]
            +++
            Body
        "#},
    );

    write_page(
        root.path(),
        "posts/review/other",
        indoc! {r#"
            +++
            title = "Other Post"
            date = "2026-01-04T00:00:00Z"
            tags = ["rust"]
            +++
            Body
        "#},
    );

    build(root.path(), BuildOptions::default()).unwrap();

    assert_paginated_listing(
        &root.path().join("public/posts/note"),
        &[
            r#"<a href="https://example.com/posts/note/post-3/">Post 3</a>"#,
            r#"<a href="https://example.com/posts/note/post-2/">Post 2</a>"#,
        ],
        &[r#"<a href="https://example.com/posts/note/post-1/">Post 1</a>"#],
        Some(("/posts/note/page/2/", "/posts/note/")),
    );
}

#[test]
fn build_skips_archives_without_template() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("config.toml"), "").unwrap();
    copy_templates_except(&root.path().join("templates"), &["archive.html"]);

    write_page(
        root.path(),
        "posts/note/my-post",
        indoc! {r#"
            +++
            title = "My Post"
            +++
            Body
        "#},
    );

    build(root.path(), BuildOptions::default()).unwrap();

    let section_index = root
        .path()
        .join("public")
        .join("posts")
        .join("note")
        .join("index.html");
    assert!(
        !section_index.exists(),
        "should NOT generate archive pages without archive.html template"
    );
}

// ── build: sections index ──

#[test]
fn build_sections_index_generates_page() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("config.toml"), "").unwrap();
    copy_templates(&root.path().join("templates"));

    write_page(
        root.path(),
        "posts/note/post-a",
        indoc! {r#"
            +++
            title = "Post A"
            date = "2026-01-01T00:00:00Z"
            +++
            Body
        "#},
    );
    write_page(
        root.path(),
        "posts/essay/post-b",
        indoc! {r#"
            +++
            title = "Post B"
            date = "2026-01-02T00:00:00Z"
            +++
            Body
        "#},
    );

    build(root.path(), BuildOptions::default()).unwrap();

    let sections_index = root
        .path()
        .join("public")
        .join("sections")
        .join("index.html");
    assert!(
        sections_index.exists(),
        "should generate /sections/index.html"
    );
    let html = fs::read_to_string(&sections_index).unwrap();
    assert!(
        html.contains("Essay") && html.contains("Note"),
        "should list section names, html:\n{html}"
    );
    assert!(
        html.contains("Post A") && html.contains("Post B"),
        "should list section posts, html:\n{html}"
    );
}

#[test]
fn build_sections_index_skipped_without_overview_template() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("config.toml"), "").unwrap();
    copy_templates_except(&root.path().join("templates"), &["overview.html"]);

    write_page(
        root.path(),
        "posts/note/post-a",
        indoc! {r#"
            +++
            title = "Post A"
            +++
            Body
        "#},
    );

    build(root.path(), BuildOptions::default()).unwrap();

    let sections_index = root
        .path()
        .join("public")
        .join("sections")
        .join("index.html");
    assert!(
        !sections_index.exists(),
        "should NOT generate sections index without overview.html"
    );
}

// ── build: taxonomies ──

#[test]
fn build_generates_taxonomy_index_pages() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("config.toml"), "").unwrap();
    copy_templates(&root.path().join("templates"));

    write_page(
        root.path(),
        "posts/hello",
        indoc! {r#"
            +++
            title = "Hello"
            tags = ["rust", "web"]
            +++
            Body
        "#},
    );

    build(root.path(), BuildOptions::default()).unwrap();

    let output_dir = root.path().join("public");
    let tags_index = output_dir.join("tags").join("index.html");
    assert!(tags_index.exists(), "should generate /tags/index.html");
    let html = fs::read_to_string(&tags_index).unwrap();
    assert!(
        html.contains("rust") && html.contains("web"),
        "tags index should list terms, html:\n{html}"
    );
}

#[test]
fn build_generates_tag_archive_pages() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("config.toml"), "").unwrap();
    copy_templates(&root.path().join("templates"));

    for (name, tag) in [("post-1", "rust"), ("post-2", "rust"), ("post-3", "web")] {
        write_page(
            root.path(),
            &format!("posts/{name}"),
            &format!(
                indoc! {r#"
                    +++
                    title = "{name}"
                    tags = ["{tag}"]
                    +++
                    Body
                "#},
                name = name,
                tag = tag,
            ),
        );
    }

    build(root.path(), BuildOptions::default()).unwrap();

    let output_dir = root.path().join("public");
    let rust_page = output_dir.join("tags").join("rust").join("index.html");
    assert!(rust_page.exists(), "should generate /tags/rust/index.html");
    let html = fs::read_to_string(&rust_page).unwrap();
    assert!(
        html.contains("post-1") && html.contains("post-2"),
        "tag archive should list posts, html:\n{html}"
    );
    assert!(
        !html.contains("post-3"),
        "tag archive should not include unrelated posts, html:\n{html}"
    );
}

#[test]
fn build_generates_paginated_tag_archive_pages() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("config.toml"),
        indoc! {r#"
            base_url = "https://example.com"

            [params]
            paginate = 2
        "#},
    )
    .unwrap();
    copy_templates(&root.path().join("templates"));

    write_paginated_posts(root.path(), "posts", Some("rust"));

    write_page(
        root.path(),
        "about",
        indoc! {r#"
            +++
            title = "About"
            tags = ["rust"]
            +++
            Body
        "#},
    );

    write_page(
        root.path(),
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

    build(root.path(), BuildOptions::default()).unwrap();

    assert_paginated_listing(
        &root.path().join("public/tags/rust"),
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

#[test]
fn build_tag_archive_excludes_untagged_standalone_pages() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("config.toml"), "").unwrap();
    copy_templates(&root.path().join("templates"));

    write_page(
        root.path(),
        "about-me",
        indoc! {r#"
            +++
            title = "About Me"
            +++
            Bio
        "#},
    );
    write_page(
        root.path(),
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

    build(root.path(), BuildOptions::default()).unwrap();

    let term_page = root
        .path()
        .join("public")
        .join("tags")
        .join("rust")
        .join("index.html");
    assert!(term_page.exists(), "should generate /tags/rust/index.html");
    let html = fs::read_to_string(&term_page).unwrap();
    assert_eq!(
        listing_links(&html),
        [r#"<a href="http://localhost:5456/posts/note/hello/">Hello Post</a>"#]
    );
}

#[test]
fn build_generates_tags_index_without_tags() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("config.toml"), "").unwrap();
    copy_templates(&root.path().join("templates"));

    write_page(
        root.path(),
        "posts/hello",
        indoc! {r#"
            +++
            title = "Hello"
            +++
            Body
        "#},
    );

    build(root.path(), BuildOptions::default()).unwrap();

    let output_dir = root.path().join("public");
    let tags_index = output_dir.join("tags").join("index.html");
    assert!(
        tags_index.exists(),
        "should generate /tags/index.html even with no tags"
    );
}

// ── build: pagination config ──

#[test]
fn build_zero_paginate_falls_back_to_defaults() {
    let root = tempfile::tempdir().unwrap();
    write_test_file(
        root.path(),
        "config.toml",
        indoc! {r#"
            base_url = "https://example.com"

            [params]
            paginate = 0

            [params.home]
            paginate = 0

            [params.section]
            paginate = 0
        "#},
    );
    copy_templates(&root.path().join("templates"));
    for i in 1..=11 {
        write_page(
            root.path(),
            &format!("posts/note/post-{i:02}"),
            &formatdoc! {r#"
                +++
                title = "Post {i}"
                tags = ["example"]
                date = "2026-01-{i:02}T00:00:00Z"
                +++
                Body
            "#},
        );
    }

    build(root.path(), BuildOptions::default()).unwrap();

    let page1: Vec<_> = (2..=11)
        .rev()
        .map(|i| format!(r#"<a href="https://example.com/posts/note/post-{i:02}/">Post {i}</a>"#))
        .collect();
    let page1: Vec<_> = page1.iter().map(String::as_str).collect();
    let page2 = [r#"<a href="https://example.com/posts/note/post-01/">Post 1</a>"#];
    for (path, navigation) in [
        ("", None),
        ("posts", Some(("/posts/page/2/", "/posts/"))),
        ("posts/note", Some(("/posts/note/page/2/", "/posts/note/"))),
        (
            "tags/example",
            Some(("/tags/example/page/2/", "/tags/example/")),
        ),
    ] {
        assert_paginated_listing(
            &root.path().join("public").join(path),
            &page1,
            &page2,
            navigation,
        );
    }
}
