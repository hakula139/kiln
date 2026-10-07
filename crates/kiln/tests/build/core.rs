use std::fs;

#[cfg(unix)]
use indoc::formatdoc;
use indoc::indoc;
use scraper::{Html, Selector};
use sha2::{Digest, Sha256};

use kiln::build::{BuildOptions, build};

use super::support::{copy_templates, listing_links, write_page, write_test_file};

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
    let plain = fs::read_to_string(&output).unwrap();
    let plain_size = plain.len();

    build(
        root.path(),
        BuildOptions {
            minify: true,
            ..Default::default()
        },
    )
    .unwrap();
    let html = fs::read_to_string(&output).unwrap();
    assert!(html.len() < plain_size);

    let content = Selector::parse(".content").unwrap();
    let strong = Selector::parse("strong").unwrap();
    let title = Selector::parse("title").unwrap();
    let heading = Selector::parse("h1").unwrap();
    for rendered in [&plain, &html] {
        let document = Html::parse_document(rendered);
        assert_eq!(
            document
                .select(&title)
                .next()
                .unwrap()
                .text()
                .collect::<String>(),
            "Hello World - Test Site"
        );
        assert_eq!(
            document
                .select(&heading)
                .next()
                .unwrap()
                .text()
                .collect::<String>(),
            "Hello World"
        );
        let body = document.select(&content).next().unwrap();
        assert_eq!(
            body.text().collect::<String>().trim(),
            "Body paragraph with markup."
        );
        assert_eq!(
            body.select(&strong)
                .next()
                .unwrap()
                .text()
                .collect::<String>(),
            "markup"
        );
    }
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
    write_test_file(
        root.path(),
        "templates/post.html",
        r#"<a href="{{ config.base_url | safe }}/">Home</a><link rel="canonical" href="{{ url | safe }}">"#,
    );

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
            base_url_override: Some("http://localhost:5456/"),
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
    assert!(html.contains(r#"href="http://localhost:5456/""#));
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

#[cfg(unix)]
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
    fs::write(
        external.path().join("vendor.css"),
        ".imported { background: url(nested/image.svg); }",
    )
    .unwrap();
    std::os::unix::fs::symlink(external.path(), bundle.join("shared")).unwrap();
    std::os::unix::fs::symlink(
        external.path().join("caption.txt"),
        bundle.join("caption.txt"),
    )
    .unwrap();

    fs::create_dir_all(bundle.join("_cache")).unwrap();
    std::os::unix::fs::symlink(
        external.path().join("missing"),
        bundle.join("_cache/broken"),
    )
    .unwrap();
    std::os::unix::fs::symlink(external.path(), bundle.join("_private")).unwrap();
    write_test_file(root.path(), "static/icon.svg", "static image");
    write_test_file(
        root.path(),
        "content/posts/example/assets/css/_src/style.css",
        indoc! {r#"
            @import "../../../shared/vendor.css";
            .image { background: url(../../../shared/nested/image.svg); }
            .caption { background: url(../../../caption.txt); }
            .static { background: url(../../../../../../static/icon.svg); }
        "#},
    );

    for processor in ["plain", "tailwind"] {
        fs::write(
            root.path().join("config.toml"),
            formatdoc! {r#"
                [css]
                processor = "{processor}"
            "#},
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
        assert!(!output.join("_cache").exists());
        assert!(!output.join("_private").exists());
        let css = fs::read_to_string(output.join("assets/css/page.css")).unwrap();
        assert!(css.contains("../../shared/nested/image.svg"), "{css}");
        assert!(css.contains("../../caption.txt"), "{css}");
        assert!(css.contains(".imported"), "{css}");
        assert!(css.contains("../../../../icon.svg"), "{css}");
    }
}

#[test]
fn build_fingerprints_bundle_assets_after_static_collisions() {
    let root = tempfile::tempdir().unwrap();
    copy_templates(&root.path().join("templates"));
    write_page(
        root.path(),
        "example",
        indoc! {r#"
            +++
            title = "Example"
            +++
            Content.
        "#},
    );
    for (name, bundle, shared) in [
        ("app.js", "console.log('bundle');", "console.log('static');"),
        ("image.svg", "bundle-image", "static-image"),
    ] {
        write_test_file(
            root.path(),
            &format!("content/example/assets/{name}"),
            bundle,
        );
        write_test_file(
            root.path(),
            &format!("static/example/assets/{name}"),
            shared,
        );
    }
    build(
        root.path(),
        BuildOptions {
            minify: true,
            ..Default::default()
        },
    )
    .unwrap();

    let public = root.path().join("public");
    assert_eq!(
        fs::read_to_string(public.join("example/assets/image.svg")).unwrap(),
        "bundle-image"
    );
    let js = fs::read_to_string(public.join("example/assets/app.js")).unwrap();
    assert!(js.contains("bundle") && !js.contains("static"), "{js}");
    let hash = hex::encode(Sha256::digest(js.as_bytes()));
    assert_eq!(
        fs::read_to_string(public.join(format!("example/assets/app.{}.js", &hash[..12]))).unwrap(),
        js
    );
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
fn build_empty_site_generates_listings_feed_and_error_page() {
    let root = tempfile::tempdir().unwrap();
    write_test_file(root.path(), "config.toml", "");
    copy_templates(&root.path().join("templates"));

    build(root.path(), BuildOptions::default()).unwrap();

    let output = root.path().join("public");
    for path in [
        "index.html",
        "posts/index.html",
        "sections/index.html",
        "tags/index.html",
    ] {
        let html = fs::read_to_string(output.join(path)).unwrap();
        assert!(listing_links(&html).is_empty(), "{path}: {html}");
    }
    let feed = fs::read_to_string(output.join("index.xml")).unwrap();
    assert!(!feed.contains("<item>"), "{feed}");
    assert!(!feed.contains("<lastBuildDate>"), "{feed}");
    let error = fs::read_to_string(output.join("404.html")).unwrap();
    assert!(error.contains("404 Not Found"), "{error}");
}

#[test]
fn build_publishes_assets_with_owner_precedence_and_private_sources() {
    let root = tempfile::tempdir().unwrap();
    write_test_file(root.path(), "config.toml", r#"theme = "example""#);
    write_test_file(root.path(), "themes/example/theme.toml", "");
    fs::create_dir_all(root.path().join("themes/example/templates")).unwrap();
    copy_templates(&root.path().join("templates"));
    for (path, value) in [
        ("themes/example/assets/theme.txt", "theme resource"),
        ("themes/example/assets/overlay.txt", "theme asset"),
        (
            "themes/example/static/assets/theme-overlay.txt",
            "theme overlay",
        ),
        ("themes/example/assets/theme-overlay.txt", "theme asset"),
        ("themes/example/assets/_secret.txt", "private theme"),
        ("themes/example/assets/layers.txt", "theme asset"),
        ("themes/example/static/assets/layers.txt", "theme overlay"),
        ("assets/layers.txt", "site asset"),
        ("assets/overlay.txt", "site asset"),
        ("static/assets/overlay.txt", "site overlay"),
        ("assets/_secret.txt", "private"),
        ("assets/nested/_cache/secret.txt", "private"),
        ("static/_headers", "root headers"),
    ] {
        write_test_file(root.path(), path, value);
    }
    build(root.path(), BuildOptions::default()).unwrap();

    let public = root.path().join("public");
    for (path, value) in [
        ("assets/theme.txt", "theme resource"),
        ("assets/theme-overlay.txt", "theme overlay"),
        ("assets/layers.txt", "site asset"),
        ("assets/overlay.txt", "site overlay"),
        ("_headers", "root headers"),
    ] {
        assert_eq!(fs::read_to_string(public.join(path)).unwrap(), value);
    }
    assert!(!public.join("assets/_secret.txt").exists());
    assert!(!public.join("assets/nested/_cache").exists());
}

#[test]
fn build_reads_image_dimensions_from_published_theme_and_site_assets() {
    let root = tempfile::tempdir().unwrap();
    write_test_file(root.path(), "config.toml", r#"theme = "example""#);
    write_test_file(root.path(), "themes/example/theme.toml", "");
    fs::create_dir_all(root.path().join("themes/example/templates")).unwrap();
    copy_templates(&root.path().join("templates"));
    let theme_images = root.path().join("themes/example/assets/images");
    fs::create_dir_all(&theme_images).unwrap();
    image::RgbaImage::from_pixel(3, 5, image::Rgba([0, 0, 0, 255]))
        .save(theme_images.join("theme.png"))
        .unwrap();
    image::RgbaImage::from_pixel(7, 9, image::Rgba([0, 0, 0, 255]))
        .save(theme_images.join("override.png"))
        .unwrap();
    let site_images = root.path().join("assets/images");
    fs::create_dir_all(&site_images).unwrap();
    image::RgbaImage::from_pixel(11, 13, image::Rgba([0, 0, 0, 255]))
        .save(site_images.join("override.png"))
        .unwrap();
    write_page(
        root.path(),
        "example",
        indoc! {r#"
            +++
            title = "Example"
            +++
            ![Theme](/assets/images/theme.png)

            ![Site](/assets/images/override.png)
        "#},
    );
    build(root.path(), BuildOptions::default()).unwrap();

    let html = fs::read_to_string(root.path().join("public/example/index.html")).unwrap();
    let document = Html::parse_document(&html);
    let selector = Selector::parse("img").unwrap();
    let dimensions: Vec<_> = document
        .select(&selector)
        .map(|image| (image.value().attr("width"), image.value().attr("height")))
        .collect();
    assert_eq!(
        dimensions,
        vec![(Some("3"), Some("5")), (Some("11"), Some("13"))]
    );
}
