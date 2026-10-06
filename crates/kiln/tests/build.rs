use std::fs;
use std::path::Path;

use indoc::{formatdoc, indoc};
use sha2::{Digest, Sha256};

use kiln::build::{BuildOptions, build};

#[path = "support/fixtures.rs"]
mod fixtures;

use fixtures::{PermissionGuard, copy_templates, template_dir, write_test_file};

// ── build ──

#[test]
fn build_end_to_end() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("config.toml"),
        indoc! {r#"
            base_url = "https://example.com"
            title = "Test Site"
        "#},
    )
    .unwrap();
    copy_templates(&root.path().join("templates"));

    write_page(
        root.path(),
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

    build(root.path(), BuildOptions::default()).unwrap();

    let output = root
        .path()
        .join("public")
        .join("posts")
        .join("hello")
        .join("index.html");
    assert!(output.exists(), "output file should exist");

    let html = fs::read_to_string(&output).unwrap();

    assert!(
        html.contains("<title>Hello World - Test Site</title>"),
        "should have title, html:\n{html}"
    );
    assert!(
        html.contains(r#"<meta name="description" content="A test post">"#),
        "should have meta description, html:\n{html}"
    );
    assert!(
        html.contains(r#"<link rel="canonical" href="https://example.com/posts/hello/">"#),
        "should have canonical URL, html:\n{html}"
    );

    assert!(
        html.contains("<h1>Hello World</h1>"),
        "should have title heading, html:\n{html}"
    );
    assert!(
        html.contains("2026-02-24T12:34:56Z"),
        "should have date, html:\n{html}"
    );
    assert!(
        html.contains(r##"<a href="#first">First</a>"##),
        "should have ToC with links to headings, html:\n{html}"
    );
    assert!(
        html.contains("<p>This is a test <strong>post</strong>.</p>"),
        "should have rendered content, html:\n{html}"
    );
}

#[test]
fn build_with_minify_shrinks_html() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("config.toml"),
        indoc! {r#"
            base_url = "https://example.com"
            title = "Test Site"
        "#},
    )
    .unwrap();
    copy_templates(&root.path().join("templates"));

    write_page(
        root.path(),
        "posts/hello",
        indoc! {r#"
            +++
            title = "Hello World"
            date = "2026-02-24T12:34:56Z"
            +++

            Body paragraph with **markup**.
        "#},
    );

    let output = root
        .path()
        .join("public")
        .join("posts")
        .join("hello")
        .join("index.html");

    build(root.path(), BuildOptions::default()).unwrap();
    let plain_size = fs::metadata(&output).unwrap().len();

    build(
        root.path(),
        BuildOptions {
            minify: true,
            ..Default::default()
        },
    )
    .unwrap();
    let minified_size = fs::metadata(&output).unwrap().len();

    assert!(
        minified_size < plain_size,
        "minify should shrink HTML: {plain_size} → {minified_size}",
    );

    let html = fs::read_to_string(&output).unwrap();
    assert!(
        html.contains("Hello World"),
        "visible content should survive minify, html:\n{html}",
    );
    assert!(
        !html.contains("  "),
        "inner whitespace should collapse, html:\n{html}",
    );
}

#[test]
fn build_output_dir_override_preserves_configured_output() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("config.toml"), "").unwrap();
    copy_templates(&root.path().join("templates"));
    let configured_output = root.path().join("public");
    fs::create_dir(&configured_output).unwrap();
    fs::write(configured_output.join("sentinel.txt"), "Keep me").unwrap();
    let output = tempfile::tempdir().unwrap();
    fs::write(output.path().join("stale.html"), "Stale page").unwrap();
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

    build(
        root.path(),
        BuildOptions {
            output_dir_override: Some(output.path()),
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(
        fs::read_to_string(configured_output.join("sentinel.txt")).unwrap(),
        "Keep me"
    );
    assert!(!configured_output.join("index.html").exists());
    assert!(!output.path().join("stale.html").exists());
    let html = fs::read_to_string(output.path().join("posts/hello/index.html")).unwrap();
    assert!(html.contains("Hello"));
    assert!(output.path().join("index.html").is_file());
}

#[test]
fn build_base_url_override() {
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
            +++
            Body
        "#},
    );

    build(
        root.path(),
        BuildOptions {
            base_url_override: Some("http://localhost:5456"),
            ..Default::default()
        },
    )
    .unwrap();

    let html = fs::read_to_string(
        root.path()
            .join("public")
            .join("posts")
            .join("hello")
            .join("index.html"),
    )
    .unwrap();
    assert!(
        html.contains("http://localhost:5456/posts/hello/"),
        "canonical URL should use overridden base_url, html:\n{html}"
    );
    assert!(
        !html.contains("https://example.com"),
        "should NOT use config base_url when overridden, html:\n{html}"
    );
}

#[test]
fn build_copies_static_files() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("config.toml"), "").unwrap();
    copy_templates(&root.path().join("templates"));

    let static_dir = root.path().join("static");
    fs::create_dir_all(static_dir.join("images")).unwrap();
    fs::write(static_dir.join("favicon.ico"), "icon").unwrap();
    fs::write(static_dir.join("images").join("logo.png"), "logo").unwrap();

    build(root.path(), BuildOptions::default()).unwrap();

    let output_dir = root.path().join("public");
    assert_eq!(
        fs::read_to_string(output_dir.join("favicon.ico")).unwrap(),
        "icon"
    );
    assert_eq!(
        fs::read_to_string(output_dir.join("images").join("logo.png")).unwrap(),
        "logo"
    );
}

#[test]
fn build_copies_colocated_assets() {
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
    let bundle = root.path().join("content").join("posts").join("hello");
    fs::create_dir_all(bundle.join("assets")).unwrap();
    fs::write(bundle.join("cover.webp"), "cover-data").unwrap();
    fs::write(bundle.join("assets").join("diagram.svg"), "svg-data").unwrap();

    build(root.path(), BuildOptions::default()).unwrap();

    let output_dir = root.path().join("public").join("posts").join("hello");
    assert_eq!(
        fs::read_to_string(output_dir.join("cover.webp")).unwrap(),
        "cover-data"
    );
    assert_eq!(
        fs::read_to_string(output_dir.join("assets").join("diagram.svg")).unwrap(),
        "svg-data"
    );
}

#[test]
fn build_materializes_external_bundle_symlinks() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("config.toml"), "").unwrap();
    copy_templates(&root.path().join("templates"));
    write_page(
        root.path(),
        "posts/example",
        indoc! {r#"
            +++
            title = "Post A"
            +++
            Body
        "#},
    );
    let bundle = root.path().join("content/posts/example");
    let external = tempfile::tempdir().unwrap();
    fs::create_dir(external.path().join("nested")).unwrap();
    fs::write(external.path().join("nested/image.svg"), "linked image").unwrap();
    fs::write(external.path().join("caption.txt"), "linked caption").unwrap();
    fs::write(external.path().join("notes.md"), "not a bundle asset").unwrap();
    std::os::unix::fs::symlink(external.path(), bundle.join("shared")).unwrap();
    std::os::unix::fs::symlink(
        external.path().join("caption.txt"),
        bundle.join("caption.txt"),
    )
    .unwrap();

    build(root.path(), BuildOptions::default()).unwrap();

    let output = root.path().join("public/posts/example");
    for (path, expected) in [
        ("caption.txt", "linked caption"),
        ("shared/caption.txt", "linked caption"),
        ("shared/nested/image.svg", "linked image"),
    ] {
        let asset = output.join(path);
        assert_eq!(fs::read_to_string(&asset).unwrap(), expected);
        assert!(fs::symlink_metadata(asset).unwrap().file_type().is_file());
    }
    assert!(
        fs::symlink_metadata(output.join("shared"))
            .unwrap()
            .is_dir()
    );
    assert!(!output.join("shared/notes.md").exists());
}

#[test]
fn build_cleans_stale_output() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("config.toml"), "").unwrap();
    copy_templates(&root.path().join("templates"));

    let output_dir = root.path().join("public");
    fs::create_dir_all(output_dir.join("old")).unwrap();
    fs::write(output_dir.join("old").join("stale.html"), "stale").unwrap();

    build(root.path(), BuildOptions::default()).unwrap();

    assert!(
        !output_dir.join("old").exists(),
        "stale output should be removed"
    );
}

#[test]
fn build_no_content() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("config.toml"), "").unwrap();
    copy_templates(&root.path().join("templates"));

    build(root.path(), BuildOptions::default()).unwrap();

    let output_dir = root.path().join("public");
    assert!(output_dir.exists(), "output directory should exist");
    assert!(
        output_dir.join("tags").join("index.html").exists(),
        "should generate empty tags index"
    );
}

// ── build: theme ──

#[test]
fn build_with_theme() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("config.toml"),
        indoc! {r#"
            base_url = "https://example.com"
            title = "Test"
            theme = "my-theme"
        "#},
    )
    .unwrap();
    setup_theme(root.path(), "my-theme");

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

    let output = root
        .path()
        .join("public")
        .join("posts")
        .join("hello")
        .join("index.html");
    assert!(output.exists(), "output file should exist");
    let html = fs::read_to_string(&output).unwrap();
    assert!(
        html.contains("<h1>Hello</h1>"),
        "should render with theme templates, html:\n{html}"
    );
}

#[test]
fn build_theme_static_files_with_site_override() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("config.toml"), r#"theme = "my-theme""#).unwrap();
    setup_theme(root.path(), "my-theme");

    let theme_static = root.path().join("themes/my-theme/static");
    fs::create_dir_all(&theme_static).unwrap();
    fs::write(theme_static.join("theme.css"), "theme-default").unwrap();
    fs::write(theme_static.join("shared.css"), "from-theme").unwrap();

    let site_static = root.path().join("static");
    fs::create_dir_all(&site_static).unwrap();
    fs::write(site_static.join("shared.css"), "from-site").unwrap();

    build(root.path(), BuildOptions::default()).unwrap();

    let output_dir = root.path().join("public");
    assert_eq!(
        fs::read_to_string(output_dir.join("theme.css")).unwrap(),
        "theme-default",
        "theme-only static file should be copied"
    );
    assert_eq!(
        fs::read_to_string(output_dir.join("shared.css")).unwrap(),
        "from-site",
        "site static file should override theme"
    );
}

#[test]
fn build_fingerprints_merged_static_assets_after_minification() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("config.toml"), r#"theme = "my-theme""#).unwrap();
    setup_theme(root.path(), "my-theme");

    let theme_dir = root.path().join("themes/my-theme");
    fs::write(
        theme_dir.join("templates/base.html"),
        indoc! {r#"
            <!DOCTYPE html>
            <html>
            <head>
              <link rel="stylesheet" href="{{ asset_url('/style.css') | safe }}">
              <script src="{{ asset_url('/app.js') | safe }}"></script>
            </head>
            <body>{% block body %}{% endblock %}</body>
            </html>
        "#},
    )
    .unwrap();
    fs::create_dir_all(theme_dir.join("static")).unwrap();
    fs::write(
        theme_dir.join("static/style.css"),
        ".theme { color: blue; }\n",
    )
    .unwrap();
    fs::write(
        theme_dir.join("static/app.js"),
        "const value = 1 + 2; console.log(value);\n",
    )
    .unwrap();

    let site_static = root.path().join("static");
    fs::create_dir_all(&site_static).unwrap();
    fs::write(
        site_static.join("style.css"),
        ".site { color: #ff0000; margin: 0px; }\n",
    )
    .unwrap();

    build(
        root.path(),
        BuildOptions {
            minify: true,
            ..Default::default()
        },
    )
    .unwrap();

    let output_dir = root.path().join("public");
    let html = fs::read_to_string(output_dir.join("index.html")).unwrap();
    for name in ["style.css", "app.js"] {
        let bytes = fs::read(output_dir.join(name)).unwrap();
        let digest = hex::encode(Sha256::digest(&bytes));
        let (stem, extension) = name.split_once('.').unwrap();
        let fingerprinted = format!("{stem}.{}.{extension}", &digest[..12]);

        assert!(output_dir.join(&fingerprinted).is_file());
        assert!(
            html.contains(&format!(r"/{fingerprinted}")),
            "html should reference {fingerprinted}, got:\n{html}"
        );
    }
    assert!(
        fs::read_to_string(output_dir.join("style.css"))
            .unwrap()
            .contains(".site"),
        "the site override should supply the fingerprinted stylesheet"
    );
}

// ── build: page template ──

#[test]
fn build_uses_page_template_for_standalone() {
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
            Hello world.
        "#},
    );

    build(root.path(), BuildOptions::default()).unwrap();

    let output = root
        .path()
        .join("public")
        .join("about-me")
        .join("index.html");
    assert!(output.exists(), "should generate about-me page");
    let html = fs::read_to_string(&output).unwrap();
    assert!(
        html.contains(r#"<article class="page">"#),
        "should use page.html template, html:\n{html}"
    );
}

#[test]
fn build_renders_dates_in_configured_timezone() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("config.toml"),
        indoc! {r#"
            timezone = "Asia/Shanghai"
        "#},
    )
    .unwrap();
    copy_templates(&root.path().join("templates"));

    write_page(
        root.path(),
        "posts/note/hello",
        indoc! {r#"
            +++
            title = "Hello"
            date = "2026-03-13T09:36:00Z"
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
            .join("hello")
            .join("index.html"),
    )
    .unwrap();
    assert!(
        html.contains("2026-03-13T17:36:00+08:00"),
        "should render the configured time zone offset, html:\n{html}"
    );
    assert!(
        !html.contains("2026-03-13T09:36:00Z"),
        "should not leave the date in UTC, html:\n{html}"
    );
}

#[test]
fn build_exposes_updated_and_linked_tags_without_git() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("config.toml"),
        indoc! {r#"
            base_url = "https://example.com"
            timezone = "Asia/Shanghai"
        "#},
    )
    .unwrap();
    copy_templates(&root.path().join("templates"));
    fs::write(
        root.path().join("templates/post.html"),
        indoc! {r#"
            {% if updated %}<time datetime="{{ updated }}">{{ updated[:10] }}</time>{% endif %}
            {% for tag in tags %}<a href="{{ tag.url | safe }}">{{ tag.name }}</a>{% endfor %}
        "#},
    )
    .unwrap();
    write_page(
        root.path(),
        "posts/note/hello",
        indoc! {r#"
            +++
            title = "Hello"
            updated = "2026-03-13T22:36:00Z"
            tags = ["C++", "<script>"]
            +++
            Body
        "#},
    );

    build(root.path(), BuildOptions::default()).unwrap();

    let html = fs::read_to_string(root.path().join("public/posts/note/hello/index.html")).unwrap();
    assert!(html.contains(r#"<time datetime="2026-03-14T06:36:00+08:00">2026-03-14</time>"#));
    assert!(
        html.contains(r#"<a href="https://example.com/tags/c++/">C++</a>"#),
        "html:\n{html}"
    );
    assert!(html.contains("&lt;script&gt;</a>"));
    assert!(!html.contains("<script>"));
}

#[test]
fn build_heading_numbering_is_per_page() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("config.toml"),
        indoc! {r#"
            base_url = "https://example.com"
            title = "Test Site"

            [params]
            heading_numbering = true
        "#},
    )
    .unwrap();
    copy_templates(&root.path().join("templates"));
    let pages = [
        (
            "numbered",
            "heading_numbering = true",
            "",
            "",
            Some(("1", "1.1")),
        ),
        (
            "continued",
            "heading_numbering = true",
            " {numbering-start=2}",
            "",
            Some(("2", "2.1")),
        ),
        (
            "child",
            "heading_numbering = true",
            " {numbering-start=2}",
            " {numbering-start=0}",
            Some(("2", "2.0")),
        ),
        (
            "another",
            "heading_numbering = true",
            "",
            "",
            Some(("1", "1.1")),
        ),
        (
            "disabled",
            "heading_numbering = false",
            " {numbering-start=bad}",
            "",
            None,
        ),
        ("default", "", " {numbering-start=bad}", "", None),
    ];
    for (slug, setting, root_attribute, child_attribute, _) in pages {
        write_page(
            root.path(),
            &format!("posts/{slug}"),
            &formatdoc! {r#"
                +++
                title = "Post"
                {setting}
                +++
                ## Section{root_attribute}
                ### Detail{child_attribute}
            "#},
        );
    }
    build(root.path(), BuildOptions::default()).unwrap();
    for (slug, _, _, _, numbers) in pages {
        let html = fs::read_to_string(root.path().join(format!("public/posts/{slug}/index.html")))
            .unwrap();
        if let Some((root, child)) = numbers {
            for (id, number, title, level) in [
                ("section", root, "Section", 2),
                ("detail", child, "Detail", 3),
            ] {
                assert!(html.contains(&format!(
                    r#"<h{level} id="{id}"><span class="heading-number">{number}</span> {title}</h{level}>"#
                )));
                assert!(html.contains(&format!(
                    r##"href="#{id}"><span class="heading-number">{number}</span> {title}</a>"##
                )));
            }
        } else {
            assert!(html.contains(r#"<h2 id="section">Section</h2>"#));
            assert!(!html.contains("heading-number"));
        }
    }
}

// ── build: page CSS ──

#[test]
fn build_injects_page_css_link() {
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
    let bundle = root.path().join("content").join("posts").join("hello");
    fs::write(bundle.join("style.css"), ".custom { color: red; }").unwrap();

    build(root.path(), BuildOptions::default()).unwrap();

    let html = fs::read_to_string(
        root.path()
            .join("public")
            .join("posts")
            .join("hello")
            .join("index.html"),
    )
    .unwrap();
    assert!(
        html.contains(r#"<link rel="stylesheet" href="/posts/hello/style.css">"#),
        "should inject per-page CSS link, html:\n{html}"
    );
}

#[test]
fn build_omits_page_css_without_style() {
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

    let html = fs::read_to_string(
        root.path()
            .join("public")
            .join("posts")
            .join("hello")
            .join("index.html"),
    )
    .unwrap();
    assert!(
        !html.contains("style.css"),
        "should NOT inject page CSS link when no style.css, html:\n{html}"
    );
}

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

    build(root.path(), BuildOptions::default()).unwrap();

    let output_dir = root.path().join("public");
    let page1 = output_dir.join("index.html");
    assert!(page1.exists(), "should generate home page 1");
    let html1 = fs::read_to_string(&page1).unwrap();
    assert!(
        html1.contains("Page 1 / 2"),
        "should show pagination, html:\n{html1}"
    );

    assert_eq!(
        listing_links(&html1),
        [
            r#"<a href="https://example.com/posts/note/post-3/">Post 3</a>"#,
            r#"<a href="https://example.com/posts/note/post-2/">Post 2</a>"#,
        ]
    );

    let page2 = output_dir.join("page").join("2").join("index.html");
    assert!(page2.exists(), "should generate home page 2");

    let html2 = fs::read_to_string(&page2).unwrap();
    assert_eq!(
        listing_links(&html2),
        [r#"<a href="https://example.com/posts/note/post-1/">Post 1</a>"#]
    );
    assert!(html2.contains("Page 2 / 2"));
    assert!(!output_dir.join("page/3/index.html").exists());
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
fn build_empty_home_page() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("config.toml"), "").unwrap();
    copy_templates(&root.path().join("templates"));

    build(root.path(), BuildOptions::default()).unwrap();

    let home = root.path().join("public").join("index.html");
    assert!(
        home.exists(),
        "should generate home page even with zero posts"
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

    build(root.path(), BuildOptions::default()).unwrap();

    let output_dir = root.path().join("public");
    let page1 = output_dir.join("posts").join("index.html");
    assert!(page1.exists(), "should generate /posts/ page 1");
    let html1 = fs::read_to_string(&page1).unwrap();
    assert!(
        html1.contains("Page 1 / 2"),
        "should show pagination on /posts/, html:\n{html1}"
    );

    assert_eq!(
        listing_links(&html1),
        [
            r#"<a href="https://example.com/posts/note/post-3/">Post 3</a>"#,
            r#"<a href="https://example.com/posts/note/post-2/">Post 2</a>"#,
        ]
    );

    let page2 = output_dir
        .join("posts")
        .join("page")
        .join("2")
        .join("index.html");
    assert!(page2.exists(), "should generate /posts/ page 2");

    let html2 = fs::read_to_string(&page2).unwrap();
    assert_eq!(
        listing_links(&html2),
        [r#"<a href="https://example.com/posts/note/post-1/">Post 1</a>"#]
    );
    assert!(html2.contains("Page 2 / 2"));
    assert!(!output_dir.join("posts/page/3/index.html").exists());
    assert!(html1.contains(r#"<a href="/posts/page/2/">Next →</a>"#));
    assert!(html2.contains(r#"<a href="/posts/">← Prev</a>"#));
    assert!(!html1.contains("← Prev"));
    assert!(!html2.contains("Next →"));
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

    let output_dir = root.path().join("public");
    let page1 = output_dir.join("posts").join("note").join("index.html");
    assert!(page1.exists(), "should generate section page 1");
    let html1 = fs::read_to_string(&page1).unwrap();
    assert!(
        html1.contains("Page 1 / 2"),
        "should show pagination, html:\n{html1}"
    );

    assert_eq!(
        listing_links(&html1),
        [
            r#"<a href="https://example.com/posts/note/post-3/">Post 3</a>"#,
            r#"<a href="https://example.com/posts/note/post-2/">Post 2</a>"#,
        ]
    );

    let page2 = output_dir
        .join("posts")
        .join("note")
        .join("page")
        .join("2")
        .join("index.html");
    assert!(page2.exists(), "should generate section page 2");

    let html2 = fs::read_to_string(&page2).unwrap();
    assert_eq!(
        listing_links(&html2),
        [r#"<a href="https://example.com/posts/note/post-1/">Post 1</a>"#]
    );
    assert!(html2.contains("Page 2 / 2"));
    assert!(!output_dir.join("posts/note/page/3/index.html").exists());
    assert!(html1.contains(r#"<a href="/posts/note/page/2/">Next →</a>"#));
    assert!(html2.contains(r#"<a href="/posts/note/">← Prev</a>"#));
    assert!(!html1.contains("← Prev"));
    assert!(!html2.contains("Next →"));
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

    let output_dir = root.path().join("public");

    let page1 = output_dir.join("tags").join("rust").join("index.html");
    assert!(page1.exists(), "should generate page 1");
    let html1 = fs::read_to_string(&page1).unwrap();
    assert!(
        html1.contains("Page 1 / 2"),
        "should show pagination, html:\n{html1}"
    );

    assert_eq!(
        listing_links(&html1),
        [
            r#"<a href="https://example.com/posts/post-3/">Post 3</a>"#,
            r#"<a href="https://example.com/posts/post-2/">Post 2</a>"#,
        ]
    );

    let page2 = output_dir
        .join("tags")
        .join("rust")
        .join("page")
        .join("2")
        .join("index.html");
    assert!(page2.exists(), "should generate page 2");
    let html2 = fs::read_to_string(&page2).unwrap();
    assert!(
        html2.contains("Page 2 / 2"),
        "should show page 2, html:\n{html2}"
    );

    assert_eq!(
        listing_links(&html2),
        [
            r#"<a href="https://example.com/posts/post-1/">Post 1</a>"#,
            r#"<a href="https://example.com/about/">About</a>"#,
        ]
    );
    assert!(!output_dir.join("tags/rust/page/3/index.html").exists());
    assert!(html1.contains(r#"<a href="/tags/rust/page/2/">Next →</a>"#));
    assert!(html2.contains(r#"<a href="/tags/rust/">← Prev</a>"#));
    assert!(!html1.contains("← Prev"));
    assert!(!html2.contains("Next →"));
}

#[test]
fn build_tag_archive_correct_with_standalone_pages() {
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
    assert!(
        html.contains("Hello Post"),
        "tag archive should list the tagged post, html:\n{html}"
    );
    assert!(
        !html.contains("About Me"),
        "tag archive should NOT list standalone pages, html:\n{html}"
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
    fs::write(
        root.path().join("config.toml"),
        indoc! {r"
            [params]
            paginate = 0

            [params.home]
            paginate = 0

            [params.section]
            paginate = 0
        "},
    )
    .unwrap();
    copy_templates(&root.path().join("templates"));

    write_page(
        root.path(),
        "posts/note/hello",
        indoc! {r#"
            +++
            title = "Hello"
            tags = ["rust"]
            date = "2026-01-01T00:00:00Z"
            +++
            Body
        "#},
    );

    build(root.path(), BuildOptions::default()).unwrap();

    let output_dir = root.path().join("public");
    assert!(
        output_dir.join("index.html").exists(),
        "should build home page"
    );
    assert!(
        output_dir.join("posts").join("index.html").exists(),
        "should build posts index"
    );
    assert!(
        output_dir
            .join("posts")
            .join("note")
            .join("index.html")
            .exists(),
        "should build section page"
    );
    assert!(
        output_dir
            .join("tags")
            .join("rust")
            .join("index.html")
            .exists(),
        "should build tag archive page"
    );
}

// ── build: RSS feeds ──

#[test]
fn build_generates_rss_feeds() {
    let root = tempfile::tempdir().unwrap();
    write_listing_site(root.path());

    build(root.path(), BuildOptions::default()).unwrap();

    let output_dir = root.path().join("public");
    for (path, expected) in [
        (
            "index.xml",
            vec![
                "posts/review/latest",
                "posts/note/newer",
                "posts/note/older",
            ],
        ),
        (
            "posts/index.xml",
            vec![
                "posts/review/latest",
                "posts/note/newer",
                "posts/note/older",
            ],
        ),
        (
            "posts/note/index.xml",
            vec!["posts/note/newer", "posts/note/older"],
        ),
        ("posts/review/index.xml", vec!["posts/review/latest"]),
        (
            "tags/rust/index.xml",
            vec!["about", "posts/review/latest", "posts/note/older"],
        ),
        ("tags/testing/index.xml", vec!["posts/note/newer"]),
    ] {
        let xml = fs::read_to_string(output_dir.join(path)).unwrap();
        let links: Vec<_> = xml
            .split("<item>")
            .skip(1)
            .map(|item| {
                item.split_once("<link>")
                    .unwrap()
                    .1
                    .split_once("</link>")
                    .unwrap()
                    .0
            })
            .collect();
        let expected: Vec<_> = expected
            .iter()
            .map(|path| format!("https://example.com/{path}/"))
            .collect();
        assert_eq!(links, expected, "incorrect membership or order in {path}");
    }

    let main_feed = fs::read_to_string(output_dir.join("index.xml")).unwrap();
    assert!(main_feed.contains("<title>Test Site</title>"));
}

#[test]
fn build_rss_feed_empty_site() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("config.toml"), "").unwrap();
    copy_templates(&root.path().join("templates"));

    build(root.path(), BuildOptions::default()).unwrap();

    let output_dir = root.path().join("public");
    let main_feed = fs::read_to_string(output_dir.join("index.xml")).unwrap();
    assert!(
        !main_feed.contains("<item>"),
        "empty site should have no items, xml:\n{main_feed}"
    );
    assert!(
        !main_feed.contains("<lastBuildDate>"),
        "empty site should have no lastBuildDate, xml:\n{main_feed}"
    );
}

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

// ── build: 404 page ──

#[test]
fn build_generates_404_page() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("config.toml"), "").unwrap();
    copy_templates(&root.path().join("templates"));

    build(root.path(), BuildOptions::default()).unwrap();

    let output_dir = root.path().join("public");
    let html = fs::read_to_string(output_dir.join("404.html")).unwrap();
    assert!(
        html.contains("404 Not Found"),
        "should contain error message, html:\n{html}"
    );
}

#[test]
fn build_skips_404_without_template() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("config.toml"), "").unwrap();

    let templates = root.path().join("templates");
    copy_templates(&templates);
    fs::remove_file(templates.join("404.html")).unwrap();

    build(root.path(), BuildOptions::default()).unwrap();

    let output_dir = root.path().join("public");
    assert!(
        !output_dir.join("404.html").exists(),
        "should not generate 404.html without template"
    );
}

// ── build: errors ──

#[test]
fn build_invalid_config_returns_error() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("config.toml"), "{{invalid toml").unwrap();

    let err = format!(
        "{:#}",
        build(root.path(), BuildOptions::default()).unwrap_err()
    );
    assert!(
        err.contains("failed to load config"),
        "should report config failure, got: {err}"
    );
}

#[test]
fn build_invalid_timezone_returns_error() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("config.toml"), r#"timezone = "Mars/Base""#).unwrap();

    let err = build(root.path(), BuildOptions::default()).unwrap_err();
    let chain: Vec<String> = err.chain().map(ToString::to_string).collect();
    assert!(
        chain
            .iter()
            .any(|message| message.contains("invalid timezone `Mars/Base` in config.toml")),
        "should report invalid timezone, got: {chain:?}"
    );
}

#[test]
fn build_missing_templates_returns_error() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("config.toml"), "").unwrap();

    let err = build(root.path(), BuildOptions::default())
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("failed to initialize template engine"),
        "should report template engine failure, got: {err}"
    );
}

#[test]
fn build_broken_post_template_returns_error() {
    assert_broken_template_fails("post.html");
}

#[test]
fn build_broken_archive_template_returns_error() {
    assert_broken_template_fails("archive.html");
}

#[test]
fn build_broken_overview_template_returns_error() {
    assert_broken_template_fails("overview.html");
}

#[test]
fn build_broken_directive_template_returns_error() {
    let root = tempfile::tempdir().unwrap();
    setup_site_with_page(root.path());

    let directives = root.path().join("templates").join("directives");
    fs::create_dir_all(&directives).unwrap();
    fs::write(
        directives.join("broken.html"),
        "{% for k, v in name | items %}{{ k }}{% endfor %}",
    )
    .unwrap();

    write_page(
        root.path(),
        "posts/hello",
        indoc! {r#"
            +++
            title = "Hello"
            +++
            ::: broken
            Body
            :::
        "#},
    );

    let err = build(root.path(), BuildOptions::default())
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("failed to render"),
        "should report render failure, got: {err}"
    );
}

#[test]
fn build_output_cleanup_permission_denied_returns_error() {
    let root = tempfile::tempdir().unwrap();
    setup_site_with_page(root.path());

    build(root.path(), BuildOptions::default()).unwrap();
    let output_dir = root.path().join("public");
    let _guard = PermissionGuard::restrict(&output_dir, 0o555);

    let err = build(root.path(), BuildOptions::default())
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("failed to clean output directory"),
        "should report output cleanup failure, got: {err}"
    );
}

#[test]
fn build_asset_copy_permission_denied_returns_error() {
    let root = tempfile::tempdir().unwrap();
    setup_site_with_page(root.path());

    let page_dir = root.path().join("content").join("posts").join("hello");
    let asset = page_dir.join("image.png");
    fs::write(&asset, "img-data").unwrap();
    let _guard = PermissionGuard::restrict(&asset, 0o000);

    let err = build(root.path(), BuildOptions::default())
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("failed to copy asset"),
        "should report asset copy failure, got: {err}"
    );
}

fn assert_broken_template_fails(template_name: &str) {
    let root = tempfile::tempdir().unwrap();
    setup_site_with_page(root.path());

    fs::write(
        root.path().join("templates").join(template_name),
        "{% invalid %}",
    )
    .unwrap();

    let err = build(root.path(), BuildOptions::default())
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("failed to render"),
        "should report render failure for {template_name}, got: {err}"
    );
}

fn setup_site_with_page(root: &Path) {
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

fn write_paginated_posts(root: &Path, post_dir: &str, tag: Option<&str>) {
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

    write_page(
        root,
        "about",
        indoc! {r#"
            +++
            title = "About"
            tags = ["rust"]
            +++
            Body
        "#},
    );
}

fn write_listing_site(root: &Path) {
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

fn write_page(root: &Path, rel_path: &str, content: &str) {
    write_test_file(root, &format!("content/{rel_path}/index.md"), content);
}

fn copy_templates_except(dest: &Path, exclude: &[&str]) {
    let src = template_dir();
    fs::create_dir_all(dest).unwrap();
    for entry in fs::read_dir(&src).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name();
        if !name.to_str().is_some_and(|n| exclude.contains(&n)) {
            fs::copy(entry.path(), dest.join(&name)).unwrap();
        }
    }
}

fn setup_theme(root: &Path, theme_name: &str) {
    let theme_dir = root.join("themes").join(theme_name);
    let tmpl_dir = theme_dir.join("templates");
    fs::create_dir_all(&tmpl_dir).unwrap();
    copy_templates(&tmpl_dir);
    fs::write(theme_dir.join("theme.toml"), "").unwrap();
}

fn listing_links(html: &str) -> Vec<&str> {
    html.split("<li>")
        .skip(1)
        .map(|item| {
            let start = item.find("<a ").unwrap();
            let end = item.find("</a>").unwrap() + "</a>".len();
            &item[start..end]
        })
        .collect()
}
